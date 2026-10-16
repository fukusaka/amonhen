/**
 * daemon の History（`historyRows` の答え）を、メインビューの行（`RowData`）にする。
 * 点と線はコア（macOS のアプリと同じ）のレイアウトで、ここでは行ごとの線分（`GraphRow.segs`）に分けるだけ。vscode に依存しない
 */
import type { GraphRow } from '../history/graph.ts';
import type { RefLabel, RowData } from '../history/protocol.ts';
import type { Reply } from './client.ts';

/** 点: row u32・lane u16・color u16・flags u16・空き u16（12 バイト） */
const NODE_WORDS = 6;
/** 線: rowStart u32・rowEnd u32・laneStart u16・laneEnd u16・color u16・kind u16・child u32・parent u32（24 バイト） */
const EDGE_WORDS = 6;

export interface DaemonRow {
  row: number;
  oid: string;
  subject: string;
  author: string;
  /** unix 秒 */
  date: number;
  graph: GraphRow;
  /** 親の行 */
  parents: number[];
  /** 親の ID */
  parentOids: string[];
}

const hex = Array.from({ length: 256 }, (_, i) => i.toString(16).padStart(2, '0'));
const decoder = new TextDecoder();

/** 20 バイトずつ並んだ ID の i 番目 */
function oidAt(ids: Uint8Array, i: number): string {
  let oid = '';
  for (let b = 0; b < 20; b++) {
    oid += hex[ids[i * 20 + b]!];
  }
  return oid;
}

/** `historyRows`（start・count で頼んだもの）の答えを行にする */
export function readRows(reply: Reply): DaemonRow[] {
  const numbers = reply.u32('rowNumbers');
  const nodes = reply.u16('nodes');
  const ids = reply.bytes('ids');
  const times = reply.i64('times');
  const spans = reply.u32('spans');
  const text = reply.bytes('text');
  const parentStarts = reply.u32('parentStarts');
  const parents = reply.u32('parents');
  const parentIds = reply.bytes('parentIds');
  const graphs = graphRows(numbers, nodes, reply.u16('edges'), reply.u32('edges'));
  const rows: DaemonRow[] = [];
  for (let i = 0; i < numbers.length; i++) {
    const oid = oidAt(ids, i);
    const from = parentStarts[i]!;
    const to = parentStarts[i + 1]!;
    const s = spans[i * 4]!;
    const a = spans[i * 4 + 2]!;
    rows.push({
      row: numbers[i]!,
      oid,
      subject: decoder.decode(text.subarray(s, s + spans[i * 4 + 1]!)),
      author: decoder.decode(text.subarray(a, a + spans[i * 4 + 3]!)),
      date: Number(times[i]),
      graph: graphs[i]!,
      parents: Array.from(parents.subarray(from, to)),
      parentOids: Array.from({ length: to - from }, (_, k) => oidAt(parentIds, from + k)),
    });
  }
  return rows;
}

/**
 * 行ごとの線分。y は 0 = 行の上端、1 = 点、2 = 行の下端（`GraphBuilder` と同じ）。
 * 縦線は点から点まで、曲線は隣り合う 2 行の点をつなぐ（上の行で斜めに下端まで、下の行で点まで）
 */
export function graphRows(
  numbers: Uint32Array,
  nodes: Uint16Array,
  edges16: Uint16Array,
  edges32: Uint32Array,
): GraphRow[] {
  const first = numbers[0] ?? 0;
  const rows: GraphRow[] = Array.from(numbers, (_, i) => {
    const col = nodes[i * NODE_WORDS + 2]!;
    return { col, color: nodes[i * NODE_WORDS + 3]!, segs: [], width: col + 1 };
  });
  const count = edges32.length / EDGE_WORDS;
  for (let e = 0; e < count; e++) {
    const rowStart = edges32[e * EDGE_WORDS]!;
    const rowEnd = edges32[e * EDGE_WORDS + 1]!;
    const laneStart = edges16[e * EDGE_WORDS * 2 + 4]!;
    const laneEnd = edges16[e * EDGE_WORDS * 2 + 5]!;
    const color = edges16[e * EDGE_WORDS * 2 + 6]!;
    const from = Math.max(rowStart, first);
    const to = Math.min(rowEnd, first + rows.length - 1);
    for (let r = from; r <= to; r++) {
      const row = rows[r - first]!;
      if (laneStart === laneEnd) {
        row.segs.push(laneStart, r === rowStart ? 1 : 0, laneStart, r === rowEnd ? 1 : 2, color);
      } else if (r === rowStart) {
        row.segs.push(laneStart, 1, laneEnd, 2, color);
      } else {
        row.segs.push(laneEnd, 0, laneEnd, r === rowEnd ? 1 : 2, color);
      }
      row.width = Math.max(row.width, laneStart + 1, laneEnd + 1);
    }
  }
  return rows;
}

/** メインビューの行に（ラベルと、編集できる範囲は拡張が持つものを使う） */
export function toRowData(
  rows: readonly DaemonRow[],
  labels: ReadonlyMap<string, RefLabel[]>,
  chain: ReadonlyMap<string, number>,
): RowData[] {
  return rows.map((r) => {
    const position = chain.get(r.oid);
    return {
      oid: r.oid,
      subject: r.subject,
      author: r.author,
      date: r.date,
      refs: labels.get(r.oid) ?? [],
      graph: r.graph,
      editable: position !== undefined,
      chain: position,
      canSquash: position !== undefined && chain.size > position + 1,
    };
  });
}
