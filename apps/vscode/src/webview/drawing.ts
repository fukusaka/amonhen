/**
 * 確認のシートの前と後の絵を描く（中身は拡張が `history/drawing.ts` で作る）。History と同じく新しいものが上で、
 * 線と点は SVG、件名と札は HTML。右の線はいちばん下で分かれたところへ合流し、merge コミットからは右の線の先端へ
 * 下りる。省いたところは線を点線にして「and n more」
 */
import type { DrawnColumn, DrawnItem, DrawnRow, Drawing } from '../history/drawing.ts';
import { el } from './api.ts';

const ROW = 22;
const LANE = 14;
const SVG_NS = 'http://www.w3.org/2000/svg';

const isRow = (i: DrawnItem): i is DrawnRow => 'subject' in i;

function svg<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number>): SVGElementTagNameMap[K] {
  const e = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
  return e;
}

function column(c: DrawnColumn): HTMLElement {
  const root = el('div', 'drawing-column');
  root.append(el('div', 'drawing-heading', c.heading));
  if (c.message !== undefined) {
    root.append(el('div', 'drawing-message', c.message));
    return root;
  }
  const all: DrawnItem[] = [...c.items, ...(c.base ? [c.base] : [])];
  const lanes = Math.max(1, ...all.map((r) => r.lane));
  const laneX = (lane: number) => 8 + lane * LANE;
  const center = (i: number) => i * ROW + ROW / 2;
  const textX = laneX(lanes) + 14;
  const body = el('div', 'drawing-body');
  body.style.height = `${all.length * ROW}px`;
  const graph = svg('svg', { width: textX, height: all.length * ROW, class: 'drawing-graph' });
  const baseAt = c.base ? all.length - 1 : undefined;
  const line = (d: string, dashed = false) => graph.append(svg('path', { d, class: dashed ? 'drawing-line dashed' : 'drawing-line' }));

  // まっすぐな線。省いたところ（前の行から次の行まで）だけ点線
  const vertical = (lane: number, from: number, to: number) => {
    const x = laneX(lane);
    let y = center(from);
    for (let g = from + 1; g < to; g++) {
      const item = all[g]!;
      if (isRow(item) || item.lane !== lane || !('more' in item)) continue;
      line(`M${x},${y} L${x},${center(g - 1)}`);
      line(`M${x},${center(g - 1)} L${x},${center(g + 1)}`, true);
      y = center(g + 1);
    }
    line(`M${x},${y} L${x},${center(to)}`);
  };
  for (let lane = 1; lane <= lanes; lane++) {
    const at = all.map((r, i) => (r.lane === lane && (isRow(r) || 'more' in r) ? i : -1)).filter((i) => i >= 0);
    const first = at[0];
    const last = at.at(-1);
    if (first === undefined || last === undefined) continue;
    vertical(lane, first, last);
    // まっすぐ下りて、分かれたところのすぐ上で左の線へ曲がって合流する
    if (baseAt !== undefined) {
      const [x1, y1, x0, y0] = [laneX(lane), center(last), laneX(0), center(baseAt)];
      const bend = Math.max(y1, y0 - ROW);
      line(`M${x1},${y1} L${x1},${bend} C${x1},${y0 - 4} ${x0},${bend + 4} ${x0},${y0}`);
    }
    // merge コミットから、その線へ曲がり、取り込む側の先端まで下りる
    if (c.joinsAt !== undefined) {
      const [x0, y0, x1] = [laneX(0), center(c.joinsAt), laneX(lane)];
      line(`M${x0},${y0} C${x1},${y0} ${x1},${y0} ${x1},${y0 + ROW * 0.8} L${x1},${center(first)}`);
    }
  }
  const zero = all.map((r, i) => (r.lane === 0 && (isRow(r) || 'more' in r) ? i : -1)).filter((i) => i >= 0);
  if (zero.length > 0) vertical(0, zero[0]!, baseAt ?? zero.at(-1)!);

  all.forEach((r, i) => {
    const row = el('div', 'drawing-row');
    row.style.top = `${i * ROW}px`;
    row.style.left = `${textX}px`;
    if (!isRow(r)) {
      row.classList.add('gap');
      row.textContent = 'more' in r ? `and ${r.more} more` : `and ${r.moreBranches} more branches`;
      body.append(row);
      return;
    }
    const big = r.highlight || r.isNew;
    const dot = svg('circle', { cx: laneX(r.lane), cy: center(i), r: big ? 5 : 4, class: 'drawing-dot' });
    for (const [flag, name] of [
      [r.highlight, 'highlight'],
      [r.dim, 'dim'],
      [r.hollow, 'hollow'],
      [r.isNew, 'new'],
      [r.tone === 'red', 'red'],
    ] as const) {
      if (flag) dot.classList.add(name);
    }
    graph.append(dot);
    for (const l of r.labels) {
      const pill = el('span', `ref ${l.kind}`, l.name);
      if (l.filled) pill.classList.add('current');
      row.append(pill);
    }
    const subject = el('span', 'drawing-subject', r.subject);
    if (r.dim) subject.classList.add('dim');
    if (big) subject.classList.add('highlight');
    row.append(subject);
    if (r.note) {
      const note = el('span', 'drawing-note', `— ${r.note}`);
      if (r.tone === 'red') note.classList.add('red');
      row.append(note);
    }
    body.append(row);
  });
  body.prepend(graph);
  root.append(body);
  return root;
}

export function drawing(d: Drawing): HTMLElement {
  const root = el('div', 'rewrite-drawing');
  d.columns.forEach((c, i) => {
    if (i > 0) root.append(el('div', 'drawing-arrow', '→'));
    root.append(column(c));
  });
  return root;
}
