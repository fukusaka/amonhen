/**
 * メインビューの Blame 画面。ファイルの各行を最後に変えたコミットを、同じコミットの行をまとめて出す（アプリと同じ）。
 * まとまりの 1 行目に ID・作者・日時、2 行目に件名。行をダブルクリックすると、そのコミットを History で選ぶ。
 * 行が多くても重くならないよう、見えている行だけを描く
 */
import type { BlameCommit } from '../blame/protocol.ts';
import type { HostMessage } from '../view/protocol.ts';
import { el, formatDate, post, type View } from './api.ts';

const ROW_HEIGHT = 20;
/** 見えている範囲の上下に余分に描く行 */
const OVERSCAN = 20;

const committed = (c: BlameCommit) => !/^0+$/.test(c.oid);

export function createBlameView(): View {
  const root = el('div', 'view blame');
  const toolbar = el('div', 'toolbar');
  const title = el('span', 'title', '');
  const status = el('span', 'status');
  toolbar.append(title, status);
  const list = el('div', 'list blame-lines');
  list.tabIndex = 0;
  const spacer = el('div', 'spacer');
  list.append(spacer);
  root.append(toolbar, list);

  let path = '';
  let commits: BlameCommit[] = [];
  let lines: { c: number; t: string }[] = [];
  /** 行ごとの、まとまりの番号（背景を交互に変える）と、まとまりの中で何行目か */
  let blocks: { index: number; offset: number }[] = [];
  /** 選んだ行（同じまとまりを強く出す） */
  let selected = -1;
  let frame = 0;

  function renderVisible(): void {
    const first = Math.max(0, Math.floor(list.scrollTop / ROW_HEIGHT) - OVERSCAN);
    const last = Math.min(lines.length, Math.ceil((list.scrollTop + list.clientHeight) / ROW_HEIGHT) + OVERSCAN);
    const block = blocks[selected]?.index;
    const rows: HTMLElement[] = [];
    for (let i = first; i < last; i++) rows.push(renderRow(i, block));
    spacer.replaceChildren(...rows);
  }

  function renderRow(i: number, selectedBlock: number | undefined): HTMLElement {
    const line = lines[i]!;
    const commit = commits[line.c]!;
    const { index, offset } = blocks[i]!;
    const row = el('div', `row blame-line${index % 2 ? ' odd' : ''}${index === selectedBlock ? ' selected' : ''}`);
    row.style.top = `${i * ROW_HEIGHT}px`;
    row.dataset.index = String(i);
    const info = el('span', `info${offset === 0 ? ' first' : ''}`);
    if (!committed(commit)) {
      info.textContent = offset === 0 ? 'Not committed yet' : '';
    } else if (offset === 0) {
      info.textContent = `${commit.oid.slice(0, 8)}  ${commit.author}  ${formatDate(commit.time)}`;
    } else if (offset === 1) {
      info.textContent = commit.summary;
    }
    if (committed(commit)) {
      info.title = `${commit.summary}\n${commit.oid}\nDouble-click to show in History.`;
      // Webview の右クリックメニュー（package.json の webview/context）に渡す情報
      row.dataset.vscodeContext = JSON.stringify({
        webviewSection: 'blame',
        oid: commit.oid,
        path,
        subject: commit.summary || undefined,
        preventDefaultContextMenuItems: true,
      });
    }
    row.append(info, el('span', 'number', String(i + 1)), el('span', 'code', line.t));
    return row;
  }

  function lineIndex(event: Event): number | undefined {
    const row = (event.target as HTMLElement).closest<HTMLElement>('.row');
    return row ? Number(row.dataset.index) : undefined;
  }

  list.addEventListener('scroll', () => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(renderVisible);
  });
  list.addEventListener('mousedown', (e) => {
    const index = lineIndex(e);
    if (index === undefined) return;
    selected = index;
    renderVisible();
  });
  list.addEventListener('dblclick', (e) => {
    const index = lineIndex(e);
    const commit = index === undefined ? undefined : commits[lines[index]!.c];
    if (commit && committed(commit)) post({ type: 'blameReveal', oid: commit.oid });
  });

  function onBlame(message: Extract<HostMessage, { type: 'blame' }>): void {
    const same = message.path === path;
    path = message.path;
    commits = message.commits;
    lines = message.lines;
    let index = -1;
    let previous = -1;
    let offset = 0;
    blocks = lines.map((line) => {
      if (line.c !== previous) {
        index++;
        offset = 0;
        previous = line.c;
      } else {
        offset++;
      }
      return { index, offset };
    });
    title.textContent = `${path}${message.rev ? ` @ ${message.rev.slice(0, 8)}` : ''}`;
    title.title = message.rev ? `${path} as of ${message.rev}` : `${path} in the working tree`;
    status.textContent =
      message.error ?? (message.loading ? 'Loading…' : `${lines.length} line${lines.length === 1 ? '' : 's'}`);
    status.classList.toggle('error', !!message.error);
    spacer.style.height = `${lines.length * ROW_HEIGHT}px`;
    if (!same) {
      selected = -1;
      list.scrollTop = 0;
    }
    renderVisible();
  }

  function onMessage(message: HostMessage): void {
    if (message.type === 'blame') onBlame(message);
  }

  return {
    root,
    onMessage,
    onShow() {
      post({ type: 'blameLoad' });
      list.focus();
    },
  };
}
