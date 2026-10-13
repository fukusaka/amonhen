/**
 * History の Webview。見えている行だけを描く仮想スクロールで、グラフは行ごとの canvas に描く。
 * 末尾に近づいたら次のページを要求する。
 */
import type { HostMessage, RowData, Scope, WebviewMessage } from '../history/protocol.ts';

declare function acquireVsCodeApi(): { postMessage(message: WebviewMessage): void };

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
    preventDefaultContextMenuItems: true,
  });

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

function select(index: number, reveal: boolean): void {
  if (index < 0 || index >= rows.length) {
    return;
  }
  selected = index;
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
      scopeSelect.value = message.scope;
      list.scrollTop = 0;
      break;
    case 'rows':
      rows.splice(message.start, rows.length - message.start, ...message.rows);
      done = message.done;
      requested = false;
      if (selected < 0 && rows.length > 0) selected = 0;
      break;
    case 'busy':
      document.body.classList.toggle('busy', message.busy);
      break;
    case 'editMessage':
      break;
  }
  scheduleRender();
});

resolveColors();
post({ type: 'ready' });
