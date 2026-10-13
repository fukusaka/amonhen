import assert from 'node:assert/strict';
import { test } from 'node:test';
import { GraphBuilder, type GraphRow } from '../src/history/graph.ts';

/** 線分を読みやすい形にする: "x1,y1>x2,y2" */
function lines(row: GraphRow): string[] {
  const out: string[] = [];
  for (let i = 0; i < row.segs.length; i += 5) {
    const [x1, y1, x2, y2] = row.segs.slice(i, i + 4);
    out.push(`${x1},${y1}>${x2},${y2}`);
  }
  return out;
}

test('linear history stays in one column', () => {
  const g = new GraphBuilder();
  const rows = [g.add('c', ['b']), g.add('b', ['a']), g.add('a', [])];
  assert.deepEqual(
    rows.map((r) => r.col),
    [0, 0, 0],
  );
  assert.deepEqual(lines(rows[0]!), ['0,1>0,2']);
  assert.deepEqual(lines(rows[1]!), ['0,0>0,1', '0,1>0,2']);
  assert.deepEqual(lines(rows[2]!), ['0,0>0,1']);
  assert.equal(new Set(rows.map((r) => r.color)).size, 1);
});

test('a merge opens a second lane that joins back at the fork point', () => {
  //   m      (merge of b and f)
  //   | \
  //   b  f
  //   | /
  //   a
  const g = new GraphBuilder();
  const m = g.add('m', ['b', 'f']);
  const b = g.add('b', ['a']);
  const f = g.add('f', ['a']);
  const a = g.add('a', []);
  assert.deepEqual([m.col, b.col, f.col, a.col], [0, 0, 1, 0]);
  assert.deepEqual(lines(m), ['0,1>0,2', '0,1>1,2']);
  // b の行では、f のレーンがまっすぐ下へ通る
  assert.deepEqual(lines(b), ['0,0>0,1', '1,0>1,2', '0,1>0,2']);
  // f の親 a はすでにレーン 0 で待たれているので合流する
  assert.deepEqual(lines(f), ['0,0>0,2', '1,0>1,1', '1,1>0,2']);
  assert.deepEqual(lines(a), ['0,0>0,1']);
  assert.notEqual(m.color, f.color);
  assert.equal(f.width, 2);
});

test('two branch tips sharing a parent', () => {
  const g = new GraphBuilder();
  const x = g.add('x', ['a']);
  const y = g.add('y', ['a']);
  const a = g.add('a', []);
  assert.deepEqual([x.col, y.col, a.col], [0, 1, 0]);
  // y は自分の行の中で、a を待っているレーン 0 に合流する
  assert.deepEqual(lines(y), ['0,0>0,2', '1,1>0,2']);
  assert.deepEqual(lines(a), ['0,0>0,1']);
});

test('the first-parent line stays on the left when a branch on the right waits for the same parent', () => {
  //   m        (merge of b and f)
  //   | \
  //   |  f     f が先に並ぶので、a はまずレーン 1 で待たれる
  //   b  |
  //   | /
  //   a
  const g = new GraphBuilder();
  g.add('m', ['b', 'f']);
  g.add('f', ['a']);
  const b = g.add('b', ['a']);
  // b はレーン 1 を自分のレーン 0 に寄せる
  assert.deepEqual(lines(b), ['0,0>0,1', '1,0>0,2', '0,1>0,2']);
  assert.equal(g.add('a', ['z']).col, 0);
  // 空いたレーン 1 は再利用される
  assert.equal(g.add('t', ['z']).col, 1);
});
