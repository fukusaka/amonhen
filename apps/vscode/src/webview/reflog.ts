/**
 * メインビューの Reflog 画面。HEAD が指してきたところ（新しい順）を左に、選んだ行のコミットの詳細を右に出す。
 * reset・rebase・amend で、どのブランチからも辿れなくなったコミットを見つけて取り戻すところ
 */
import type { ReflogEntry } from '../reflog/protocol.ts';
import type { HostMessage } from '../view/protocol.ts';
import { el, formatDate, post, type View } from './api.ts';
import { attachSplitter, createDetailsPane } from './details.ts';

const ROW_HEIGHT = 24;

export function createReflogView(): View {
  const root = el('div', 'view reflog');
  const toolbar = el('div', 'toolbar');
  const title = el('span', 'title', 'HEAD');
  title.title = 'Every place HEAD has been, newest first';
  const newBranch = el('button', 'secondary', 'New Branch…');
  newBranch.title = 'Create a branch at the selected entry’s commit and check it out';
  const checkout = el('button', 'secondary', 'Checkout');
  checkout.title = 'Check out the selected entry’s commit (detached HEAD)';
  const status = el('span', 'status');
  toolbar.append(title, newBranch, checkout, status);
  const body = el('div', 'split');
  const list = el('div', 'list');
  list.tabIndex = 0;
  const spacer = el('div', 'spacer');
  list.append(spacer);
  const splitter = el('div', 'splitter');
  splitter.setAttribute('role', 'separator');
  const details = createDetailsPane();
  body.append(list, splitter, details.root);
  root.append(toolbar, body);
  attachSplitter(body, splitter, details.root, () => undefined);

  let entries: ReflogEntry[] = [];
  let selected = -1;
  let selectTimer = 0;

  function render(): void {
    spacer.style.height = `${entries.length * ROW_HEIGHT}px`;
    spacer.replaceChildren(...entries.map((e, i) => renderRow(e, i)));
    const entry = entries[selected];
    newBranch.disabled = !entry;
    checkout.disabled = !entry;
  }

  function renderRow(entry: ReflogEntry, index: number): HTMLElement {
    const row = el('div', index === selected ? 'row selected' : 'row');
    row.style.top = `${index * ROW_HEIGHT}px`;
    row.dataset.index = String(index);
    // Webview の右クリックメニュー（package.json の webview/context）に渡す情報
    row.dataset.vscodeContext = JSON.stringify({
      webviewSection: 'reflog',
      oid: entry.oid,
      selector: entry.selector,
      subject: entry.subject || undefined,
      reflogReachable: entry.reachable,
      preventDefaultContextMenuItems: true,
    });
    row.append(el('span', 'selector', entry.selector));
    const text = el('div', 'text');
    text.append(el('span', 'message', entry.message));
    if (entry.subject && !entry.message.endsWith(entry.subject)) {
      text.append(el('span', 'subject', entry.subject));
    }
    text.title = entry.subject ? `${entry.message}\n${entry.subject}` : entry.message;
    row.append(text);
    if (!entry.reachable) {
      const lost = el('span', 'unreachable', 'not on any branch');
      lost.title = 'No branch or tag reaches this commit. Create a branch to keep it.';
      row.append(lost);
    }
    row.append(el('span', 'date', formatDate(entry.time)), el('span', 'oid', entry.oid.slice(0, 8)));
    return row;
  }

  function select(index: number, reveal: boolean): void {
    if (index < 0 || index >= entries.length) {
      return;
    }
    selected = index;
    const oid = entries[index]!.oid;
    if (oid !== details.shownOid) {
      clearTimeout(selectTimer);
      selectTimer = window.setTimeout(() => post({ type: 'select', oid }), 60);
    }
    if (reveal) {
      const top = index * ROW_HEIGHT;
      if (top < list.scrollTop) {
        list.scrollTop = top;
      } else if (top + ROW_HEIGHT > list.scrollTop + list.clientHeight) {
        list.scrollTop = top + ROW_HEIGHT - list.clientHeight;
      }
    }
    render();
  }

  function rowIndex(event: Event): number | undefined {
    const row = (event.target as HTMLElement).closest<HTMLElement>('.row');
    return row ? Number(row.dataset.index) : undefined;
  }

  list.addEventListener('mousedown', (e) => {
    const index = rowIndex(e);
    if (index !== undefined) select(index, false);
  });
  list.addEventListener('dblclick', (e) => {
    const index = rowIndex(e);
    if (index !== undefined) post({ type: 'open', oid: entries[index]!.oid });
  });
  list.addEventListener('keydown', (e) => {
    const page = Math.max(1, Math.floor(list.clientHeight / ROW_HEIGHT) - 1);
    const moves: Record<string, number> = { ArrowDown: 1, ArrowUp: -1, PageDown: page, PageUp: -page };
    if (e.key in moves) {
      e.preventDefault();
      select(Math.min(entries.length - 1, Math.max(0, selected + moves[e.key]!)), true);
    } else if (e.key === 'Enter' && entries[selected]) {
      post({ type: 'open', oid: entries[selected]!.oid });
    }
  });
  const action = (kind: 'newBranch' | 'checkout') => () => {
    const entry = entries[selected];
    if (entry) post({ type: 'reflogAction', action: kind, oid: entry.oid, selector: entry.selector });
  };
  newBranch.addEventListener('click', action('newBranch'));
  checkout.addEventListener('click', action('checkout'));

  function onReflog(next: ReflogEntry[], error: string | undefined): void {
    // 選んでいた行のコミットを選んだままにする（HEAD が動くと、行は 1 つずつ下がる）
    const oid = entries[selected]?.oid;
    const selector = entries[selected]?.selector;
    entries = next;
    status.textContent = error ?? `${entries.length} entr${entries.length === 1 ? 'y' : 'ies'}`;
    status.classList.toggle('error', !!error);
    let index = entries.findIndex((e) => e.selector === selector && e.oid === oid);
    if (index < 0) index = oid ? entries.findIndex((e) => e.oid === oid) : -1;
    if (index < 0) {
      index = entries.length > 0 ? 0 : -1;
      details.clear();
    }
    selected = -1;
    if (index >= 0) select(index, false);
    else render();
  }

  function onMessage(message: HostMessage): void {
    switch (message.type) {
      case 'reflog':
        onReflog(message.entries, message.error);
        break;
      case 'details':
        if (root.classList.contains('active') && message.details.oid === entries[selected]?.oid) {
          details.show(message.details);
        }
        break;
      case 'commitDiff':
        details.showDiff(message.oid, message.path, message.diff);
        break;
    }
  }

  render();
  return {
    root,
    onMessage,
    onShow() {
      // 見えていない間に届いた詳細は出していないので、選んでいる行の詳細を求め直す
      details.forget();
      post({ type: 'reflogLoad' });
      list.focus();
    },
  };
}
