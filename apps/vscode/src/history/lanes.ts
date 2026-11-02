/**
 * 見えている行に合わせたレーンの横位置（魚眼）。アプリ（コアの `crates/core/src/lanes.rs`）と同じ決め方を、
 * 行ごとのグラフ（`GraphRow`）から行う。
 *
 * git/git や linux のような履歴では、1 行を何十本・何百本の線が横切る。すべてのレーンを同じ幅で並べると、
 * グラフが一覧の幅を超えて文字を押し出す。そこで、見えている行に点があるレーンだけを広く取り、ほかの線は
 * 細い束に詰め、何もないレーンは幅を取らない。全体は `available` に収める。vscode に依存しない
 */
import type { GraphRow } from './graph.ts';

/** レーンの幅の決め方（CSS のピクセル） */
export interface LaneMetrics {
  /** 点のあるレーンの幅 */
  full: number;
  /** 線だけのレーンの幅 */
  thin: number;
  /** 収まらないときに、ここまで詰める */
  minFull: number;
  minThin: number;
  /** 使える幅 */
  available: number;
}

const EMPTY = 0;
const PASS = 1;
const ACTIVE = 2;

/**
 * `rows` を表示するときの、レーンごとの中心の横位置（左端を 0 とする）と、レーン全体の幅。何もないレーンは
 * 幅を取らず、次のレーンと同じ位置になる
 */
export function lanePositions(
  rows: Iterable<GraphRow | undefined>,
  m: LaneMetrics,
): { centers: number[]; width: number } {
  const activity: number[] = [ACTIVE];
  const mark = (lane: number, a: number) => {
    while (activity.length <= lane) activity.push(EMPTY);
    activity[lane] = Math.max(activity[lane]!, a);
  };
  for (const row of rows) {
    if (!row) continue;
    mark(row.col, ACTIVE);
    for (let i = 0; i < row.segs.length; i += 5) {
      mark(row.segs[i]!, PASS);
      mark(row.segs[i + 2]!, PASS);
    }
  }

  const active = activity.filter((a) => a === ACTIVE).length;
  const pass = activity.filter((a) => a === PASS).length;
  let { full, thin } = m;
  // 収まらなければ、まず線だけのレーンを、それでも足りなければ点のあるレーンを詰める
  if (active * full + pass * thin > m.available && pass > 0) {
    thin = clamp((m.available - active * full) / pass, m.minThin, m.thin);
  }
  if (active * full + pass * thin > m.available) {
    full = clamp((m.available - pass * thin) / active, m.minFull, m.full);
  }
  let x = 0;
  const centers = activity.map((a) => {
    const w = a === ACTIVE ? full : a === PASS ? thin : 0;
    const center = x + w / 2;
    x += w;
    return center;
  });
  return { centers, width: x };
}

function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}
