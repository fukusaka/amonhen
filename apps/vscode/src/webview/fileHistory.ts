/**
 * メインビューの File History 画面。ファイルを変えたコミット（新しい順）を左に、選んだコミットの詳細（そのファイルの
 * 差分の位置）を右に出す。名前が変わっていれば、名前ごとにその時点のパスの見出しを挟む（アプリと同じ）
 */
import type { FileHistoryEntry } from '../fileHistory/protocol.ts';
import type { HostMessage } from '../view/protocol.ts';
import { el, formatDate, getState, post, updateState, type View } from './api.ts';
import { attachSplitter, createDetailsPane } from './details.ts';

type Row = { kind: 'path'; path: string; index: number } | { kind: 'entry'; index: number };

const ROW_HEIGHT = 40;
const PATH_HEIGHT = 24;

export function createFileHistoryView(): View {
  const root = el('div', 'view file-history');
  const toolbar = el('div', 'toolbar');
  const title = el('span', 'title', '');
  const merges = el('label', 'check');
  const mergesBox = el('input');
  mergesBox.type = 'checkbox';
  mergesBox.checked = getState().fileHistoryMerges === true;
  merges.title = 'Show merge commits too (git log shows them for a file only when it differs from every parent)';
  merges.append(mergesBox, ' Merge Commits');
  const status = el('span', 'status');
  toolbar.append(title, merges, status);
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

  let path = '';
  let entries: FileHistoryEntry[] = [];
  let rows: Row[] = [];
  /** entries の添字 */
  let selected = -1;
  let selectTimer = 0;

  /** 各行の上端（見出しは低い） */
  function tops(): number[] {
    let y = 0;
    return rows.map((r) => {
      const top = y;
      y += r.kind === 'path' ? PATH_HEIGHT : ROW_HEIGHT;
      return top;
    });
  }

  function render(): void {
    const at = tops();
    const last = rows.length > 0 ? at[rows.length - 1]! + (rows[rows.length - 1]!.kind === 'path' ? PATH_HEIGHT : ROW_HEIGHT) : 0;
    spacer.style.height = `${last}px`;
    spacer.replaceChildren(...rows.map((r, i) => renderRow(r, at[i]!)));
  }

  function renderRow(row: Row, top: number): HTMLElement {
    if (row.kind === 'path') {
      const head = el('div', 'path-head');
      head.style.top = `${top}px`;
      const slash = row.path.lastIndexOf('/');
      head.append(el('span', 'name', row.path.slice(slash + 1)));
      if (slash >= 0) head.append(el('span', 'dir', row.path.slice(0, slash)));
      const newer = entries[row.index - 1];
      head.title = newer ? `${row.path}\nRenamed or copied to ${newer.path} in ${newer.oid.slice(0, 8)}` : row.path;
      return head;
    }
    const entry = entries[row.index]!;
    const div = el('div', row.index === selected ? 'row two-line selected' : 'row two-line');
    div.style.top = `${top}px`;
    div.dataset.index = String(row.index);
    // Webview の右クリックメニュー（package.json の webview/context）に渡す情報
    div.dataset.vscodeContext = JSON.stringify({
      webviewSection: 'fileHistory',
      oid: entry.oid,
      path: entry.path,
      subject: entry.subject || undefined,
      preventDefaultContextMenuItems: true,
    });
    div.append(el('div', 'subject', entry.subject));
    div.append(el('div', 'meta', `${entry.author}  ·  ${formatDate(entry.time)}  ·  ${entry.oid.slice(0, 8)}`));
    div.title = entry.path;
    return div;
  }

  /** 名前が 1 つだけなら見出しは入れない（題にある） */
  function buildRows(): void {
    rows = [];
    const renamed = entries.some((e) => e.path !== entries[0]?.path);
    entries.forEach((e, i) => {
      if (renamed && (i === 0 || entries[i - 1]!.path !== e.path)) rows.push({ kind: 'path', path: e.path, index: i });
      rows.push({ kind: 'entry', index: i });
    });
  }

  function select(index: number, reveal: boolean): void {
    if (index < 0 || index >= entries.length) return;
    selected = index;
    const entry = entries[index]!;
    if (entry.oid !== details.shownOid) {
      clearTimeout(selectTimer);
      selectTimer = window.setTimeout(() => post({ type: 'select', oid: entry.oid }), 60);
    }
    if (reveal) {
      const at = tops();
      const r = rows.findIndex((x) => x.kind === 'entry' && x.index === index);
      const top = at[r] ?? 0;
      if (top < list.scrollTop) list.scrollTop = top;
      else if (top + ROW_HEIGHT > list.scrollTop + list.clientHeight) list.scrollTop = top + ROW_HEIGHT - list.clientHeight;
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
    if (index !== undefined) post({ type: 'fileHistoryReveal', oid: entries[index]!.oid });
  });
  list.addEventListener('keydown', (e) => {
    const page = Math.max(1, Math.floor(list.clientHeight / ROW_HEIGHT) - 1);
    const moves: Record<string, number> = { ArrowDown: 1, ArrowUp: -1, PageDown: page, PageUp: -page };
    if (e.key in moves) {
      e.preventDefault();
      select(Math.min(entries.length - 1, Math.max(0, selected + moves[e.key]!)), true);
    } else if (e.key === 'Enter' && entries[selected]) {
      post({ type: 'fileHistoryReveal', oid: entries[selected]!.oid });
    }
  });
  mergesBox.addEventListener('change', () => {
    updateState({ fileHistoryMerges: mergesBox.checked });
    post({ type: 'fileHistoryLoad', merges: mergesBox.checked });
  });

  function onFileHistory(message: Extract<HostMessage, { type: 'fileHistory' }>): void {
    // 選んでいたコミットを選んだままにする（同じファイルで、残りを読んだ・読み直したとき）
    const oid = message.path === path ? entries[selected]?.oid : undefined;
    path = message.path;
    entries = message.entries;
    mergesBox.checked = message.merges;
    title.textContent = path;
    title.title = path;
    status.textContent =
      message.error ??
      `${entries.length}${message.complete ? '' : '+'} commit${entries.length === 1 ? '' : 's'}${message.complete ? '' : ' · Loading…'}`;
    status.classList.toggle('error', !!message.error);
    buildRows();
    let index = oid ? entries.findIndex((e) => e.oid === oid) : -1;
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
      case 'fileHistory':
        onFileHistory(message);
        break;
      case 'details': {
        const entry = entries[selected];
        if (root.classList.contains('active') && message.details.oid === entry?.oid) {
          details.show(message.details, entry.path, message.viewed);
        }
        break;
      }
      case 'commitDiff':
        details.showDiff(message.oid, message.path, message.diff);
        break;
      case 'detailsExtras':
        details.showExtras(message.extras);
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
      post({ type: 'fileHistoryLoad', merges: mergesBox.checked });
      list.focus();
    },
  };
}
