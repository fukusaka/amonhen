/**
 * rebase の確認に添える前と後の絵（アプリの RewriteDrawingView と同じ中身）。左が今、右が rebase の後。History と
 * 同じく新しいものが上で、線と点は SVG、件名とブランチの札は HTML で描く。載せ直すコミットは左で薄く、右で強調
 * （A → A′）。一緒に動くブランチの札は右で新しいコミットへ移り、残るブランチと `origin/…` は元のところに残る
 */
import type { DrawnCommit, RewritePreview } from '../history/rewritePreview.ts';
import { el } from './api.ts';

const ROW = 22;
const LANE = 14;
const SVG_NS = 'http://www.w3.org/2000/svg';

interface Label {
  name: string;
  /** `local`・`remote`・`moved`（一緒に動いた） */
  kind: string;
  current?: boolean;
}

interface Row {
  commit: DrawnCommit;
  lane: number;
  dim?: boolean;
  highlight?: boolean;
  labels: Label[];
}

/** 省いたところ（`lane` の線の、上から `at` 行目。そこだけ点線にして「ほか n」と書く） */
interface Gap {
  lane: number;
  at: number;
  count: number;
}

/** 1 列。行は上から順に、省いたところは行の間に 1 行ぶん空ける。base は一番下で、右の線はそこへ合流する */
interface Column {
  heading: string;
  rows: (Row | Gap)[];
  base?: Row;
  baseNote?: string;
}

const isGap = (r: Row | Gap): r is Gap => 'count' in r;

function svg<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number>): SVGElementTagNameMap[K] {
  const e = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
  return e;
}

function column(c: Column): HTMLElement {
  const root = el('div', 'drawing-column');
  root.append(el('div', 'drawing-heading', c.heading));
  const all: (Row | Gap)[] = [...c.rows, ...(c.base ? [c.base] : [])];
  const lanes = Math.max(1, ...all.map((r) => r.lane));
  const laneX = (lane: number) => 8 + lane * LANE;
  const center = (i: number) => i * ROW + ROW / 2;
  const textX = laneX(lanes) + 14;
  const body = el('div', 'drawing-body');
  body.style.height = `${all.length * ROW}px`;
  const graph = svg('svg', { width: textX, height: all.length * ROW, class: 'drawing-graph' });
  const baseAt = c.base ? all.length - 1 : undefined;

  // 線。省いたところ（前の行から次の行まで）だけ点線
  const segment = (lane: number, from: number, to: number) => {
    const gaps = all
      .map((r, i) => (isGap(r) && r.lane === lane ? i : -1))
      .filter((i) => i > from && i < to);
    let y = center(from);
    for (const g of gaps) {
      graph.append(svg('line', { x1: laneX(lane), y1: y, x2: laneX(lane), y2: center(g - 1), class: 'drawing-line' }));
      graph.append(svg('line', { x1: laneX(lane), y1: center(g - 1), x2: laneX(lane), y2: center(g + 1), class: 'drawing-line dashed' }));
      y = center(g + 1);
    }
    graph.append(svg('line', { x1: laneX(lane), y1: y, x2: laneX(lane), y2: center(to), class: 'drawing-line' }));
  };
  for (let lane = 0; lane <= lanes; lane++) {
    const at = all.map((r, i) => (r.lane === lane && (lane > 0 || i !== baseAt) ? i : -1)).filter((i) => i >= 0);
    const first = at[0];
    const last = at.at(-1);
    if (first === undefined || last === undefined) continue;
    if (lane === 0) {
      segment(0, first, baseAt ?? last);
    } else {
      segment(lane, first, last);
      // まっすぐ下りて、分かれたところのすぐ上で左の線へ曲がって合流する
      if (baseAt !== undefined) {
        const [x1, y1, x0, y0] = [laneX(lane), center(last), laneX(0), center(baseAt)];
        const bend = Math.max(y1, y0 - ROW);
        graph.append(
          svg('path', { d: `M${x1},${y1} L${x1},${bend} C${x1},${y0 - 4} ${x0},${bend + 4} ${x0},${y0}`, class: 'drawing-line' }),
        );
      }
    }
  }

  all.forEach((r, i) => {
    const line = el('div', 'drawing-row');
    line.style.top = `${i * ROW}px`;
    line.style.left = `${textX}px`;
    if (isGap(r)) {
      line.classList.add('gap');
      line.textContent = `and ${r.count} more`;
      body.append(line);
      return;
    }
    const dot = svg('circle', { cx: laneX(r.lane), cy: center(i), r: r.highlight ? 5 : 4, class: 'drawing-dot' });
    if (r.highlight) dot.classList.add('highlight');
    if (r.dim) dot.classList.add('dim');
    graph.append(dot);
    for (const l of r.labels) {
      const pill = el('span', `ref ${l.kind}`, l.name);
      if (l.current) pill.classList.add('current');
      line.append(pill);
    }
    const subject = el('span', 'drawing-subject', r.commit.subject);
    if (r.dim) subject.classList.add('dim');
    if (r.highlight) subject.classList.add('highlight');
    line.append(subject);
    if (i === baseAt && c.baseNote) line.append(el('span', 'drawing-note', `— ${c.baseNote}`));
    body.append(line);
  });
  body.prepend(graph);
  root.append(body);
  return root;
}

/** 分かれたところに添える文（「split off 3 months ago」）。日時がなければ undefined */
function splitNote(base: DrawnCommit | null | undefined): string | undefined {
  if (!base?.time) return undefined;
  const days = (base.time * 1000 - Date.now()) / 86_400_000;
  const f = new Intl.RelativeTimeFormat('en', { numeric: 'auto', style: 'short' });
  const text =
    Math.abs(days) >= 365
      ? f.format(Math.round(days / 365), 'year')
      : Math.abs(days) >= 30
        ? f.format(Math.round(days / 30), 'month')
        : Math.abs(days) >= 1
          ? f.format(Math.round(days), 'day')
          : f.format(Math.round(days * 24), 'hour');
  return `split off ${text}`;
}

/** 前と後の絵。描く材料がなければ undefined */
export function rewriteDrawing(p: RewritePreview): HTMLElement | undefined {
  const d = p.drawing;
  if (!d || d.picks.length === 0) return undefined;
  const moving = new Set(p.moving.map((b) => b.name));
  const labelsOf = (oid: string) => d.labels.filter((l) => l.oid === oid);
  const picks = [...d.picks].reverse();
  const picksMore = d.picksMore ?? 0;
  const picksAt = picksMore > 0 ? (d.picksMoreAt ?? picks.length) : -1;
  const ontoAt = d.ontoMore > 0 ? (d.ontoMoreAt ?? d.onto.length) : -1;

  // 載せ直すコミットと載せ先で進んだコミットを、省いたところを挟んで並べる
  const layout = (pick: (c: DrawnCommit, i: number) => Row, onto: (c: DrawnCommit) => Row, pickLane: number) => {
    const rows: (Row | Gap)[] = [];
    picks.forEach((c, i) => {
      if (i === picksAt) rows.push({ lane: pickLane, at: rows.length, count: picksMore });
      rows.push(pick(c, i));
    });
    if (picksAt === picks.length) rows.push({ lane: pickLane, at: rows.length, count: picksMore });
    d.onto.forEach((c, i) => {
      if (i === ontoAt) rows.push({ lane: 0, at: rows.length, count: d.ontoMore });
      rows.push(onto(c));
    });
    if (ontoAt === d.onto.length) rows.push({ lane: 0, at: rows.length, count: d.ontoMore });
    return rows;
  };

  // 左: 今。上に載せ直すコミット（右の線）、その下に載せ先で進んだコミット（左の線）。札はすべて今のところに
  const nowLabels = (oid: string): Label[] => labelsOf(oid).map((l) => ({ ...l, current: l.name === p.current }));
  const now: Column = {
    heading: 'Now',
    rows: layout(
      (c) => ({ commit: c, lane: 1, dim: true, labels: nowLabels(c.oid) }),
      (c) => ({ commit: c, lane: 0, labels: nowLabels(c.oid) }),
      1,
    ),
    base: d.base ? { commit: d.base, lane: 0, dim: true, labels: nowLabels(d.base.oid) } : undefined,
    baseNote: splitNote(d.base),
  };

  // 右: rebase の後。載せ直したコミット（A′）が載せ先の上に 1 本で続く。今のブランチは先頭の A′ へ、一緒に動く
  // ブランチは元の先頭に対応する A′ へ。載せ先の札はそのまま
  const stays = (oid: string): Label[] =>
    labelsOf(oid)
      .filter((l) => l.name !== p.current && !moving.has(l.name))
      .map((l) => ({ ...l }));
  const after: Column = {
    heading: 'After the rebase',
    rows: layout(
      (c, i) => ({
        commit: { oid: c.oid, subject: `${c.subject}′` },
        lane: 0,
        highlight: true,
        labels: [
          ...(i === 0 && p.current ? [{ name: p.current, kind: 'local', current: true }] : []),
          ...labelsOf(c.oid)
            .filter((l) => l.kind === 'local' && moving.has(l.name))
            .map((l) => ({ name: l.name, kind: 'moved' })),
        ],
      }),
      (c) => ({ commit: c, lane: 0, dim: true, labels: stays(c.oid) }),
      0,
    ),
    base: d.base ? { commit: d.base, lane: 0, dim: true, labels: stays(d.base.oid) } : undefined,
  };

  const root = el('div', 'rewrite-drawing');
  root.append(column(now), el('div', 'drawing-arrow', '→'), column(after));
  return root;
}
