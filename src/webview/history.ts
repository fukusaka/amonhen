/**
 * History の Webview。左にコミットの一覧とグラフ、右に選んだコミットの詳細を出す。
 * 一覧は見えている行だけを描く仮想スクロールで、グラフは行ごとの canvas に描く。
 * 末尾に近づいたら次のページを要求する。
 */
import type { CommitDetails, HostMessage, MessageEdit, RowData, Scope, WebviewMessage } from '../history/protocol.ts';

interface State {
  detailsWidth?: number;
}

declare function acquireVsCodeApi(): {
  postMessage(message: WebviewMessage): void;
  getState(): State | undefined;
  setState(state: State): void;
};

const vscode = acquireVsCodeApi();
const ROW_HEIGHT = 24;
const LANE_WIDTH = 14;
/** 見えている範囲の外に余分に描く行数 */
const OVERSCAN = 8;
/** 末尾からこの行数以内に来たら次を読む */
const PREFETCH = 50;
const PALETTE = [
  '--vscode-charts-blue',
  '--vscode-charts-green',
  '--vscode-charts-orange',
  '--vscode-charts-purple',
  '--vscode-charts-red',
  '--vscode-charts-yellow',
  '--vscode-terminal-ansiCyan',
  '--vscode-terminal-ansiMagenta',
];

const list = document.getElementById('list')!;
const spacer = document.getElementById('spacer')!;
const scopeSelect = document.getElementById('scope') as HTMLSelectElement;
const status = document.getElementById('status')!;
const detailsPane = document.getElementById('details')!;
const splitter = document.getElementById('splitter')!;

let rows: RowData[] = [];
let done = false;
let requested = false;
let selected = -1;
let colors: string[] = [];
let frame = 0;

function post(message: WebviewMessage): void {
  vscode.postMessage(message);
}

function resolveColors(): void {
  const style = getComputedStyle(document.body);
  colors = PALETTE.map((v) => style.getPropertyValue(v).trim() || '#888');
}

function scheduleRender(): void {
  if (!frame) {
    frame = requestAnimationFrame(() => {
      frame = 0;
      render();
    });
  }
}

function render(): void {
  spacer.style.height = `${rows.length * ROW_HEIGHT}px`;
  const first = Math.max(0, Math.floor(list.scrollTop / ROW_HEIGHT) - OVERSCAN);
  const last = Math.min(rows.length, Math.ceil((list.scrollTop + list.clientHeight) / ROW_HEIGHT) + OVERSCAN);
  // グラフの幅は見えている範囲で一番広い行に合わせる
  let lanes = 1;
  for (let i = first; i < last; i++) {
    lanes = Math.max(lanes, rows[i]!.graph.width);
  }
  const graphWidth = lanes * LANE_WIDTH + 4;
  const elements: HTMLElement[] = [];
  for (let i = first; i < last; i++) {
    elements.push(renderRow(rows[i]!, i, graphWidth));
  }
  spacer.replaceChildren(...elements);

  if (!done && !requested && last >= rows.length - PREFETCH) {
    requested = true;
    post({ type: 'more' });
  }
  status.textContent = done ? `${rows.length} commits` : `${rows.length}+ commits`;
}

function renderRow(row: RowData, index: number, graphWidth: number): HTMLElement {
  const el = document.createElement('div');
  el.className = index === selected ? 'row selected' : 'row';
  el.style.top = `${index * ROW_HEIGHT}px`;
  el.dataset.index = String(index);
  // Webview の右クリックメニュー（package.json の webview/context）に渡す情報
  el.dataset.vscodeContext = JSON.stringify({
    webviewSection: 'commit',
    oid: row.oid,
    commitEditable: row.editable,
    commitCanSquash: row.canSquash,
    preventDefaultContextMenuItems: true,
  });
  // 編集できるコミットはドラッグで並べ替えられる
  if (row.editable) {
    el.draggable = true;
  }

  el.append(drawGraph(row, graphWidth));
  const text = document.createElement('div');
  text.className = 'text';
  for (const ref of row.refs) {
    const badge = document.createElement('span');
    badge.className = `ref ${ref.kind}${ref.current ? ' current' : ''}`;
    badge.textContent = ref.name;
    text.append(badge);
  }
  const subject = document.createElement('span');
  subject.className = 'subject';
  subject.textContent = row.subject;
  text.append(subject);
  el.append(text);

  for (const [cls, value] of [
    ['author', row.author],
    ['date', formatDate(row.date)],
    ['oid', row.oid.slice(0, 8)],
  ] as const) {
    const span = document.createElement('span');
    span.className = cls;
    span.textContent = value;
    el.append(span);
  }
  return el;
}

function drawGraph(row: RowData, width: number): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  const ratio = window.devicePixelRatio || 1;
  canvas.width = width * ratio;
  canvas.height = ROW_HEIGHT * ratio;
  canvas.style.width = `${width}px`;
  canvas.style.height = `${ROW_HEIGHT}px`;
  const ctx = canvas.getContext('2d')!;
  ctx.scale(ratio, ratio);
  ctx.lineWidth = 1.5;
  const x = (col: number) => col * LANE_WIDTH + LANE_WIDTH / 2 + 2;
  const y = (pos: number) => (pos * ROW_HEIGHT) / 2;
  const { segs } = row.graph;
  for (let i = 0; i < segs.length; i += 5) {
    const [x1, y1, x2, y2, color] = segs.slice(i, i + 5) as [number, number, number, number, number];
    ctx.strokeStyle = colors[color % colors.length]!;
    ctx.beginPath();
    ctx.moveTo(x(x1), y(y1));
    if (x1 === x2) {
      ctx.lineTo(x(x2), y(y2));
    } else {
      // 列をまたぐ線はなめらかに曲げる
      const mid = (y(y1) + y(y2)) / 2;
      ctx.bezierCurveTo(x(x1), mid, x(x2), mid, x(x2), y(y2));
    }
    ctx.stroke();
  }
  ctx.fillStyle = colors[row.graph.color % colors.length]!;
  ctx.beginPath();
  ctx.arc(x(row.graph.col), y(1), 4, 0, Math.PI * 2);
  ctx.fill();
  return canvas;
}

function formatDate(seconds: number): string {
  const d = new Date(seconds * 1000);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

let selectTimer = 0;
let shownOid: string | undefined;

function select(index: number, reveal: boolean): void {
  if (index < 0 || index >= rows.length) {
    return;
  }
  selected = index;
  requestDetails();
  if (reveal) {
    const top = index * ROW_HEIGHT;
    if (top < list.scrollTop) {
      list.scrollTop = top;
    } else if (top + ROW_HEIGHT > list.scrollTop + list.clientHeight) {
      list.scrollTop = top + ROW_HEIGHT - list.clientHeight;
    }
  }
  scheduleRender();
}

/** メッセージの編集欄。保存は Ctrl/Cmd+Enter、取り消しは Esc */
function editMessage(oid: string, kind: MessageEdit, title: string, message: string): void {
  const overlay = document.createElement('div');
  overlay.className = 'overlay';
  const box = document.createElement('div');
  box.className = 'editor';
  const heading = document.createElement('div');
  heading.className = 'heading';
  heading.textContent = title;
  const textarea = document.createElement('textarea');
  textarea.value = message;
  textarea.spellcheck = false;
  const buttons = document.createElement('div');
  buttons.className = 'buttons';
  const save = document.createElement('button');
  save.textContent = 'Save';
  const cancel = document.createElement('button');
  cancel.textContent = 'Cancel';
  cancel.className = 'secondary';
  buttons.append(cancel, save);
  box.append(heading, textarea, buttons);
  overlay.append(box);
  document.body.append(overlay);
  textarea.focus();
  textarea.setSelectionRange(0, textarea.value.indexOf('\n') < 0 ? textarea.value.length : textarea.value.indexOf('\n'));

  const close = () => {
    overlay.remove();
    list.focus();
  };
  const submit = () => {
    if (!textarea.value.trim()) {
      textarea.focus();
      return;
    }
    post({ type: 'saveMessage', oid, kind, message: textarea.value });
    close();
  };
  save.addEventListener('click', submit);
  cancel.addEventListener('click', close);
  textarea.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      submit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  });
}

/** 選択が落ち着いてから詳細を求める。矢印キーを押し続けても要求が溜まらないように */
function requestDetails(): void {
  const oid = rows[selected]?.oid;
  if (!oid || oid === shownOid) {
    return;
  }
  clearTimeout(selectTimer);
  selectTimer = window.setTimeout(() => post({ type: 'select', oid }), 60);
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (className) e.className = className;
  if (text !== undefined) e.textContent = text;
  return e;
}

function showDetails(d: CommitDetails): void {
  if (d.oid !== rows[selected]?.oid) {
    return;
  }
  shownOid = d.oid;
  const [subject = '', ...rest] = d.message.replace(/\n+$/, '').split('\n');
  const body = rest.join('\n').replace(/^\n+/, '');

  const head = el('div', 'subject');
  for (const ref of d.refs) {
    head.append(el('span', `ref ${ref.kind}${ref.current ? ' current' : ''}`, ref.name));
  }
  head.append(el('span', '', subject));

  const meta = el('dl', 'meta');
  const add = (label: string, value: Node | string) => {
    const dd = el('dd');
    dd.append(value);
    meta.append(el('dt', '', label), dd);
  };
  add('Author', `${d.author.name} <${d.author.email}>`);
  add('Date', formatDate(d.author.date));
  if (d.committer.name !== d.author.name || d.committer.date !== d.author.date) {
    add('Committer', `${d.committer.name}  ${formatDate(d.committer.date)}`);
  }
  add('Commit', el('code', '', d.oid));
  if (d.parents.length > 0) {
    const parents = el('span');
    for (const p of d.parents) {
      const link = el('a', 'parent', p.slice(0, 8));
      link.href = '#';
      link.title = p;
      link.addEventListener('click', (e) => {
        e.preventDefault();
        const index = rows.findIndex((r) => r.oid === p);
        if (index >= 0) select(index, true);
      });
      parents.append(link, ' ');
    }
    add(d.parents.length > 1 ? 'Parents' : 'Parent', parents);
  }

  const files = el('div', 'files');
  const filesHead = el('div', 'files-head');
  const count = d.files.length;
  filesHead.append(el('span', '', `${count} file${count === 1 ? '' : 's'} changed${d.parents.length > 1 ? '（最初の親との差分）' : ''}`));
  if (count > 0) {
    const all = el('button', 'secondary', 'Open All');
    all.addEventListener('click', () => post({ type: 'open', oid: d.oid }));
    filesHead.append(all);
  }
  files.append(filesHead);
  for (const f of d.files) {
    const row = el('div', 'file');
    row.title = f.origPath ? `${f.origPath} → ${f.path}` : f.path;
    row.append(el('span', `status s-${f.status}`, f.status));
    const slash = f.path.lastIndexOf('/');
    row.append(el('span', 'name', f.path.slice(slash + 1)));
    if (slash >= 0) row.append(el('span', 'dir', f.path.slice(0, slash)));
    row.addEventListener('click', () => {
      for (const other of files.querySelectorAll('.file.active')) other.classList.remove('active');
      row.classList.add('active');
      post({ type: 'openFile', oid: d.oid, path: f.path });
    });
    files.append(row);
  }

  const parts: Node[] = [head, meta];
  if (body) parts.push(el('pre', 'body', body));
  parts.push(files);
  detailsPane.replaceChildren(...parts);
}

function clearDetails(): void {
  shownOid = undefined;
  detailsPane.replaceChildren(el('div', 'empty', 'コミットを選ぶと、ここに詳細が出ます'));
}

// 左右の境目をドラッグして、詳細の幅を変える。幅は覚えておく
function setDetailsWidth(width: number): void {
  const max = Math.max(240, document.body.clientWidth - 240);
  const clamped = Math.min(max, Math.max(240, width));
  detailsPane.style.width = `${clamped}px`;
  vscode.setState({ ...vscode.getState(), detailsWidth: clamped });
}
splitter.addEventListener('mousedown', (e) => {
  e.preventDefault();
  const onMove = (m: MouseEvent) => setDetailsWidth(document.body.clientWidth - m.clientX);
  const onUp = () => {
    window.removeEventListener('mousemove', onMove);
    window.removeEventListener('mouseup', onUp);
    document.body.classList.remove('resizing');
    scheduleRender();
  };
  document.body.classList.add('resizing');
  window.addEventListener('mousemove', onMove);
  window.addEventListener('mouseup', onUp);
});

function rowIndex(event: Event): number | undefined {
  const row = (event.target as HTMLElement).closest<HTMLElement>('.row');
  return row ? Number(row.dataset.index) : undefined;
}

list.addEventListener('scroll', scheduleRender);
window.addEventListener('resize', scheduleRender);
list.addEventListener('mousedown', (e) => {
  const index = rowIndex(e);
  if (index !== undefined) select(index, false);
});
list.addEventListener('dblclick', (e) => {
  const index = rowIndex(e);
  if (index !== undefined) post({ type: 'open', oid: rows[index]!.oid });
});
list.addEventListener('keydown', (e) => {
  const page = Math.max(1, Math.floor(list.clientHeight / ROW_HEIGHT) - 1);
  const moves: Record<string, number> = { ArrowDown: 1, ArrowUp: -1, PageDown: page, PageUp: -page };
  if (e.key in moves) {
    e.preventDefault();
    select(Math.min(rows.length - 1, Math.max(0, selected + moves[e.key]!)), true);
  } else if (e.key === 'Enter' && rows[selected]) {
    post({ type: 'open', oid: rows[selected]!.oid });
  }
});
scopeSelect.addEventListener('change', () => post({ type: 'scope', scope: scopeSelect.value as Scope }));

// ドラッグでの並べ替え。編集できるコミットの上にだけ落とせる
let dragging: RowData | undefined;
let dropTarget: HTMLElement | undefined;
const clearDropTarget = () => {
  dropTarget?.classList.remove('drop-above', 'drop-below');
  dropTarget = undefined;
};
list.addEventListener('dragstart', (e) => {
  const index = rowIndex(e);
  dragging = index === undefined ? undefined : rows[index];
  if (!dragging?.editable || !e.dataTransfer) {
    e.preventDefault();
    return;
  }
  e.dataTransfer.effectAllowed = 'move';
  e.dataTransfer.setData('text/plain', dragging.oid);
});
list.addEventListener('dragover', (e) => {
  const index = rowIndex(e);
  const target = index === undefined ? undefined : rows[index];
  if (!dragging || target?.chain === undefined || target.oid === dragging.oid) {
    clearDropTarget();
    return;
  }
  e.preventDefault();
  const el = (e.target as HTMLElement).closest<HTMLElement>('.row')!;
  const above = isUpperHalf(e, el);
  clearDropTarget();
  dropTarget = el;
  el.classList.add(above ? 'drop-above' : 'drop-below');
});
list.addEventListener('dragleave', clearDropTarget);
list.addEventListener('drop', (e) => {
  e.preventDefault();
  const index = rowIndex(e);
  const target = index === undefined ? undefined : rows[index];
  const el = dropTarget;
  clearDropTarget();
  if (!dragging || dragging.chain === undefined || target?.chain === undefined || !el) {
    return;
  }
  // 上半分なら target の上（より新しい側）、下半分なら下に置く
  const above = isUpperHalf(e, el);
  let to = above ? target.chain : target.chain + 1;
  // 自分より下に動かすときは、自分が抜けた分だけ詰まる
  if (to > dragging.chain) to -= 1;
  if (to !== dragging.chain) {
    post({ type: 'move', oid: dragging.oid, to });
  }
  dragging = undefined;
});
list.addEventListener('dragend', () => {
  dragging = undefined;
  clearDropTarget();
});

function isUpperHalf(e: MouseEvent, el: HTMLElement): boolean {
  const rect = el.getBoundingClientRect();
  return e.clientY < rect.top + rect.height / 2;
}

// テーマが変わったら色を取り直す
new MutationObserver(() => {
  resolveColors();
  scheduleRender();
}).observe(document.body, { attributes: true, attributeFilter: ['class'] });

window.addEventListener('message', (event: MessageEvent<HostMessage>) => {
  const message = event.data;
  switch (message.type) {
    case 'reset':
      rows = [];
      done = false;
      requested = true;
      selected = -1;
      clearDetails();
      scopeSelect.value = message.scope;
      list.scrollTop = 0;
      break;
    case 'rows':
      rows.splice(message.start, rows.length - message.start, ...message.rows);
      done = message.done;
      requested = false;
      if (selected < 0 && rows.length > 0) {
        selected = 0;
        requestDetails();
      }
      break;
    case 'busy':
      document.body.classList.toggle('busy', message.busy);
      break;
    case 'details':
      showDetails(message.details);
      return;
    case 'editMessage':
      editMessage(message.oid, message.kind, message.title, message.message);
      return;
  }
  scheduleRender();
});

resolveColors();
clearDetails();
setDetailsWidth(vscode.getState()?.detailsWidth ?? Math.round(document.body.clientWidth * 0.4));
post({ type: 'ready' });
