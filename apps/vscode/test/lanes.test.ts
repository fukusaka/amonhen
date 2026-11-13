import assert from 'node:assert/strict';
import { test } from 'node:test';
import { GraphBuilder, type GraphRow } from '../src/history/graph.ts';
import { graphRows } from '../src/daemon/history.ts';
import { type LaneMetrics, lanePositions } from '../src/history/lanes.ts';

const M: LaneMetrics = { full: 14, thin: 3, minFull: 7, minThin: 0, available: 10_000 };

/** 中心の位置から、レーンごとの幅に戻す */
function widths(centers: number[]): number[] {
  let x = 0;
  return centers.map((c) => {
    const w = (c - x) * 2;
    x += w;
    return w;
  });
}

/**
 * `n` 本のブランチが並ぶ履歴。先端 t0…t(n-1) が先に並び、それぞれの親 b0…b(n-1) が続き、どれも根 r に至る。
 * b の行では、まだ来ていない b のレーンがすべて横切る
 */
function wide(n: number): GraphRow[] {
  const g = new GraphBuilder();
  const rows: GraphRow[] = [];
  for (let i = 0; i < n; i++) rows.push(g.add(`t${i}`, [`b${i}`]));
  for (let i = 0; i < n; i++) rows.push(g.add(`b${i}`, ['r']));
  rows.push(g.add('r', []));
  return rows;
}

test('lanes with commits in view are wide, lanes with only lines are thin, others take no space', () => {
  const rows = wide(10);
  // b3…b5 の行: 点は 3〜5 のレーン、線は b6…b9 を待つレーンと、r へ集まる 0 のレーン。1・2 は 0 に合流して空いた
  const view = rows.slice(13, 16);
  assert.deepEqual(
    view.map((r) => r.col),
    [3, 4, 5],
  );
  const { centers, width } = lanePositions(view, M);
  const w = widths(centers);
  assert.deepEqual(w, [14, 0, 0, 14, 14, 14, 3, 3, 3, 3]);
  assert.equal(width, w.reduce((a, b) => a + b, 0));
  // 中心は、左のレーンの幅の合計 + 自分の幅の半分
  assert.equal(centers[3], 14 + 7);
});

test('a lane nothing reaches in view takes no space', () => {
  const rows = wide(10);
  // b8・b9 の行: b0…b7 は r を待つ 0 のレーンに合流して、1〜7 のレーンは空いている
  const { centers, width } = lanePositions(rows.slice(18, 20), M);
  assert.deepEqual(widths(centers), [14, 0, 0, 0, 0, 0, 0, 0, 14, 14]);
  assert.equal(width, 42);
  // まだ届いていない行は数えない
  assert.equal(lanePositions([undefined, undefined], M).width, 14);
});

test('squeezes lines first, then commits, to fit the column', () => {
  const rows = wide(70);
  const view = rows.slice(100, 110);
  const all = widths(lanePositions(view, M).centers);
  const commits = all.filter((w) => w === 14).length;
  assert.equal(commits, 11, '10 commits and the first lane');
  assert.ok(all.reduce((a, b) => a + b, 0) > 116, 'the lines alone do not fit');

  // 点のレーンは収まるが線のレーンは収まらない幅: 線のレーンだけ細くなる
  const narrow = { ...M, available: commits * 14 + 10 };
  const fit = lanePositions(view, narrow);
  assert.ok(Math.abs(fit.width - narrow.available) < 0.01, String(fit.width));
  assert.equal(widths(fit.centers).filter((w) => w === 14).length, commits);

  // 点のレーンも収まらない幅: 点のレーンも詰めるが、最小の幅より狭くはしない
  const tiny = lanePositions(view, { ...M, available: 116 });
  const w = widths(tiny.centers);
  assert.ok(w.every((x) => Math.abs(x) < 1e-9 || x >= 7 - 1e-9), String(w));
  assert.ok(tiny.width <= 116 + 1e-9, String(tiny.width));
});

test('rows not laid out yet take no lane and draw no commit', () => {
  // daemon が開いた直後の、まだレイアウトしていない行（`col` は 0 だが使わない）
  const rows: GraphRow[] = [
    { col: 2, color: 1, segs: [], width: 3 },
    { col: 0, color: 0, segs: [], width: 0, pending: true },
  ];
  const { centers } = lanePositions(rows, M);
  // 幹線（0）は必ず広く取り、2 は点のあるレーン。1 は何もないので幅を取らない
  assert.deepEqual(widths(centers), [14, 0, 14]);
  assert.deepEqual(widths(lanePositions([rows[1]!], M).centers), [14]);
});

test('a row the daemon has not laid out yet is marked pending', () => {
  // 点 2 つ（row u32・lane u16・color u16・flags u16・空き u16）。2 つ目は `Node::PENDING`（1 << 4）と MERGE
  const nodes = new Uint16Array([0, 0, 1, 7, 0, 0, 1, 0, 0, 0, (1 << 4) | 1, 0]);
  const [laid, pending] = graphRows(new Uint32Array([0, 1]), nodes, new Uint16Array(), new Uint32Array());
  assert.equal(laid!.pending, undefined);
  assert.equal(laid!.col, 1);
  assert.equal(pending!.pending, true);
  assert.equal(pending!.width, 0);
});
