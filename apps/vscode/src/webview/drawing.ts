/**
 * 確認のシートの前と後の絵を描く（中身は拡張が `history/drawing.ts` で作る）。History と同じく新しいものが上で、
 * 線と点は SVG、件名と札は HTML。右の線はいちばん下で分かれたところへ合流し、merge コミットからは右の線の先端へ
 * 下りる。省いたところは線を点線にして「and n more」
 */
import type { DrawnColumn, DrawnItem, DrawnRow, Drawing } from '../history/drawing.ts';
import { el } from './api.ts';

const ROW = 22;
const LANE = 14;
/** 札を並べる段の上限（アプリと同じ）。入らない札は、最後の段の「+n」の札にまとめる */
const MAX_LABEL_LINES = 3;
const SVG_NS = 'http://www.w3.org/2000/svg';

const isRow = (i: DrawnItem): i is DrawnRow => 'subject' in i;

function svg<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number>): SVGElementTagNameMap[K] {
  const e = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
  return e;
}

/** 札が段の上限を超えたら、超えた札と、「+n」の札が入るまで最後の段の札を、まとめる（幅が変われば数え直す） */
function capLabels(row: HTMLElement): void {
  row.querySelector(':scope > .ref.more')?.remove();
  const pills = [...row.querySelectorAll<HTMLElement>(':scope > .ref')];
  for (const p of pills) p.style.display = '';
  const tops = [...new Set(pills.map((p) => p.offsetTop))].sort((a, b) => a - b);
  if (tops.length <= MAX_LABEL_LINES) return;
  const limit = tops[MAX_LABEL_LINES - 1]!;
  const hidden = pills.filter((p) => p.offsetTop > limit);
  const shown = pills.filter((p) => p.offsetTop <= limit);
  for (const p of hidden) p.style.display = 'none';
  const more = el('span', 'ref more');
  shown.at(-1)?.after(more);
  const count = () => {
    more.textContent = `+${hidden.length}`;
    more.title = hidden.map((p) => p.textContent).join('\n');
  };
  count();
  while (more.offsetTop > limit && shown.length > 0) {
    const p = shown.pop()!;
    p.style.display = 'none';
    hidden.unshift(p);
    count();
  }
}

/** 描いた列。札を段に収める（layout）と、行の位置から線と点を描く（paint）を分けて呼べる */
interface Column {
  root: HTMLElement;
  body?: HTMLElement;
  /** 分かれたところの行（あれば） */
  base?: HTMLElement;
  layout(): void;
  paint(): void;
}

function column(c: DrawnColumn): Column {
  const root = el('div', 'drawing-column');
  root.append(el('div', 'drawing-heading', c.heading));
  if (c.message !== undefined) {
    root.append(el('div', 'drawing-message', c.message));
    return { root, layout: () => undefined, paint: () => undefined };
  }
  const all: DrawnItem[] = [...c.items, ...(c.base ? [c.base] : [])];
  const lanes = Math.max(1, ...all.map((r) => r.lane));
  const laneX = (lane: number) => 8 + lane * LANE;
  const textX = laneX(lanes) + 14;
  const body = el('div', 'drawing-body');
  body.style.paddingLeft = `${textX}px`;
  const graph = svg('svg', { width: textX, class: 'drawing-graph' });
  const baseAt = c.base ? all.length - 1 : undefined;

  // 件名と札の行は、ふつうに上から並べる（札が入らなければ件名の下へ折り返し、その行だけ伸びる）
  const rows = all.map((r) => {
    const row = el('div', 'drawing-row');
    if (!isRow(r)) {
      row.classList.add('gap');
      row.textContent = 'more' in r ? `and ${r.more} more` : `and ${r.moreBranches} more branches`;
      return row;
    }
    for (const l of r.labels) {
      const pill = el('span', `ref ${l.kind}`, l.name);
      if (l.filled) pill.classList.add('current');
      // 1 つの札だけで列に入らないときは末尾を省くので、全文はツールチップに
      pill.title = l.name;
      row.append(pill);
    }
    const text = el('span', 'drawing-text');
    const subject = el('span', 'drawing-subject', r.subject);
    if (r.dim) subject.classList.add('dim');
    if (r.highlight || r.isNew) subject.classList.add('highlight');
    text.append(subject);
    if (r.note) {
      const note = el('span', 'drawing-note', `— ${r.note}`);
      if (r.tone === 'red') note.classList.add('red');
      text.append(note);
    }
    // 切れても読めるよう、件名と印の全文
    text.title = r.note ? `${r.subject} — ${r.note}` : r.subject;
    row.append(text);
    return row;
  });
  body.append(graph, ...rows);

  // 線と点は、並んだ行の位置から描く（幅が変わって折り返しが変われば描き直す）
  const layout = () => {
    for (const row of rows) capLabels(row);
  };
  const paint = () => {
    const top = rows.map((row) => row.offsetTop);
    const center = (i: number) => top[i]! + ROW / 2;
    graph.replaceChildren();
    graph.setAttribute('height', String(body.offsetHeight));
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
      if (!isRow(r)) return;
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
    });
  };
  root.append(body);
  return { root, body, ...(baseAt !== undefined ? { base: rows[baseAt]! } : {}), layout, paint };
}

export function drawing(d: Drawing): HTMLElement {
  const root = el('div', 'rewrite-drawing');
  const columns = d.columns.map(column);
  columns.forEach((c, i) => {
    if (i > 0) root.append(el('div', 'drawing-arrow', '→'));
    root.append(c.root);
  });
  // 置かれて大きさが決まったとき（最初）と、幅が変わったときに描く。Edit では、下の書き換えないコミットを左右で
  // 同じ高さに（高い方の列に合わせて、低い方の列はその前を空ける。アプリと同じ）
  const paint = () => {
    for (const c of columns) c.layout();
    if (d.alignBase) {
      // 列の頭からの位置で比べる（狭くて縦に積んだときも、それぞれの列の中で同じ位置に）
      const bases = columns.flatMap((c) => (c.base ? [{ row: c.base, column: c.root }] : []));
      const at = ({ row, column }: (typeof bases)[number]) => row.getBoundingClientRect().top - column.getBoundingClientRect().top;
      for (const b of bases) b.row.style.marginTop = '';
      const lowest = Math.max(...bases.map(at));
      for (const b of bases) b.row.style.marginTop = `${lowest - at(b)}px`;
    }
    for (const c of columns) c.paint();
  };
  const observer = new ResizeObserver(paint);
  for (const c of columns) if (c.body) observer.observe(c.body);
  return root;
}
