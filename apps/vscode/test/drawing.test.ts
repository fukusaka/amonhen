import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  mergeDrawing,
  moveDrawing,
  octopusDrawing,
  pickDrawing,
  rebaseDrawing,
  splitNote,
  type DrawnColumn,
  type DrawnItem,
  type DrawnRow,
} from '../src/history/drawing.ts';

const c = (oid: string, subject = oid) => ({ oid, subject });
const rows = (col: DrawnColumn | undefined) =>
  (col?.items ?? []).map((i: DrawnItem) => ('subject' in i ? `${i.lane}:${i.subject}` : 'more' in i ? `${i.lane}:+${i.more}` : `+${i.moreBranches} branches`));
const labels = (row: DrawnItem | DrawnRow | undefined) =>
  row && 'labels' in row ? row.labels.map((l) => `${l.filled ? '*' : ''}${l.name}`) : [];

test('moveDrawing moves the ref to the destination and hollows the commits taken off', () => {
  const d = moveDrawing(
    {
      gainedCount: 1,
      lostCount: 2,
      files: 3,
      upstreamHas: 0,
      drawing: {
        gained: [c('g')],
        gainedMore: 0,
        lost: [c('l1'), c('l2'), c('l3')],
        lostMore: 4,
        lostMoreAt: 2,
        base: { ...c('b'), time: 1 },
        labels: [
          { oid: 'l1', name: 'origin/main', kind: 'remote' },
          { oid: 'g', name: 'main', kind: 'local' },
        ],
      },
    },
    { moving: 'origin/main', kind: 'remote', afterTitle: 'After the force push', lostNote: 'removed from origin/main', tone: 'red' },
  );
  assert.ok(d);
  const [now, after] = d.columns;
  assert.deepEqual(rows(now), ['0:g', '1:l1', '1:l2', '1:+4', '1:l3']);
  assert.deepEqual(labels(now?.items[1]), ['*origin/main']);
  assert.match(now?.base?.note ?? '', /^split off /);
  assert.equal(after?.heading, 'After the force push');
  assert.deepEqual(labels(after?.items[0]), ['*origin/main', 'main']);
  const lost = after?.items[1] as DrawnRow;
  assert.equal(lost.hollow, true);
  assert.equal(lost.tone, 'red');
  assert.equal(lost.note, 'removed from origin/main');
  assert.equal(after?.base?.note, undefined);
});

test('moveDrawing keeps one lane when nothing is gained and draws no ref after deleting', () => {
  const d = moveDrawing(
    {
      gainedCount: 0,
      lostCount: 1,
      files: 0,
      upstreamHas: 0,
      drawing: { gained: [], gainedMore: 0, lost: [c('x')], lostMore: 0, base: c('b'), labels: [{ oid: 'x', name: 'topic', kind: 'local' }] },
    },
    { moving: 'topic', kind: 'local', afterTitle: 'After deleting', lostNote: 'goes with the branch', tone: 'red', removes: true },
  );
  assert.deepEqual(rows(d?.columns[0]), ['0:x']);
  assert.deepEqual(labels(d?.columns[1]?.items[0]), []);
  assert.deepEqual(labels(d?.columns[1]?.base), []);
  assert.equal(moveDrawing({ gainedCount: 0, lostCount: 0, files: 0, upstreamHas: 0, drawing: null }, { moving: 'a', kind: 'local', afterTitle: '', lostNote: '' }), undefined);
});

test('moveDrawing marks commits only Reflog has', () => {
  const d = moveDrawing(
    {
      gainedCount: 1,
      lostCount: 0,
      files: 1,
      upstreamHas: 0,
      drawing: { gained: [{ ...c('h'), hidden: true }], gainedMore: 0, lost: [], lostMore: 0, base: c('b'), labels: [] },
    },
    { moving: 'main', kind: 'local', afterTitle: 'After the undo', lostNote: 'taken off' },
  );
  const now = d?.columns[0]?.items[0] as DrawnRow;
  assert.equal(now.note, 'only in Reflog');
  assert.equal(now.hollow, true);
  assert.equal((d?.columns[1]?.items[0] as DrawnRow).highlight, true);
});

test('mergeDrawing puts a new merge commit on top that joins both sides, or a move when it fast-forwards', () => {
  const p = {
    drawing: {
      ours: [c('o')],
      oursMore: 0,
      theirs: [c('t1'), c('t2')],
      theirsMore: 0,
      base: c('b'),
      labels: [{ oid: 't1', name: 'topic', kind: 'local' }],
      subject: "Merge branch 'topic'",
    },
  };
  const d = mergeDrawing(p, 'main');
  assert.deepEqual(rows(d?.columns[0]), ['0:o', '1:t1', '1:t2']);
  assert.deepEqual(labels(d?.columns[0]?.items[0]), ['*main']);
  assert.deepEqual(rows(d?.columns[1]), ["0:Merge branch 'topic'", '0:o', '1:t1', '1:t2']);
  assert.equal(d?.columns[1]?.joinsAt, 0);
  assert.equal((d?.columns[1]?.items[0] as DrawnRow).isNew, true);
  const now = mergeDrawing(p, 'main', 'After the pull', 'Can’t fast-forward.');
  assert.equal(now?.columns[1]?.message, 'Can’t fast-forward.');
  const ff = mergeDrawing(
    {
      fastForward: true,
      move: { gainedCount: 1, lostCount: 0, files: 1, upstreamHas: 0, drawing: { gained: [c('t')], gainedMore: 0, lost: [], lostMore: 0, base: c('o'), labels: [] } },
    },
    'main',
  );
  assert.deepEqual(labels(ff?.columns[1]?.items[0]), ['*main']);
});

test('pickDrawing stacks the new commits on the tip and moves the branch to the top', () => {
  const p = { drawing: { head: [c('h'), c('p')], picks: [c('b', 'add b'), c('a', 'add a')], picksMore: 0, labels: [] } };
  const d = pickDrawing(p, 'main', 'cherryPick');
  assert.deepEqual(rows(d?.columns[0]), ['0:h', '0:p']);
  assert.deepEqual(rows(d?.columns[1]), ['0:add b′', '0:add a′', '0:h', '0:p']);
  assert.deepEqual(labels(d?.columns[1]?.items[0]), ['*main']);
  assert.equal(d?.columns[1]?.heading, 'After the cherry-pick');
  assert.deepEqual(rows(pickDrawing(p, 'main', 'revert')?.columns[1]).slice(0, 1), ['0:add b']);
});

test('octopusDrawing gives each side a lane in the chosen order, up to four', () => {
  const sides = ['a', 'b', 'c', 'd', 'e'].map((r) => ({ rev: r, tip: c(r), more: r === 'a' ? 2 : 0 }));
  const d = octopusDrawing(
    { drawing: { ours: [c('o')], oursMore: 0, sides: [...sides, { rev: 'done', tip: null, more: 0 }], base: c('b'), labels: [] } },
    'main',
    ['e', 'a', 'b', 'c', 'd', 'done'],
    'Merge branches',
  );
  assert.deepEqual(rows(d?.columns[0]), ['0:o', '1:e', '2:a', '2:+2', '3:b', '4:c', '+1 branches']);
  assert.deepEqual(labels(d?.columns[0]?.items[1]), ['e']);
});

test('rebaseDrawing moves the current branch to the top replayed commit', () => {
  const d = rebaseDrawing({
    moving: [],
    staying: [],
    current: 'topic',
    drawing: { picks: [c('a'), c('b')], onto: [c('m')], ontoMore: 0, base: c('base'), labels: [{ oid: 'b', name: 'topic', kind: 'local' }] },
  });
  assert.deepEqual(rows(d?.columns[0]), ['1:b', '1:a', '0:m']);
  assert.deepEqual(rows(d?.columns[1]), ['0:b′', '0:a′', '0:m']);
  assert.deepEqual(labels(d?.columns[1]?.items[0]), ['*topic']);
});

test('splitNote says how long ago the branches split', () => {
  const now = Date.UTC(2026, 10, 11);
  assert.equal(splitNote({ oid: 'b', subject: '', time: now / 1000 - 86_400 * 90 }, now), 'split off 3 mo. ago');
  assert.equal(splitNote({ oid: 'b', subject: '' }, now), undefined);
});
