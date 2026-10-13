/**
 * コミットグラフのレーン計算。トポロジカル順のコミットを 1 つずつ与えると、その行の描画情報を返す。
 * 分割して読み込んだページをまたいで状態を引き継ぐ。
 *
 * レーンは「次にそのレーンに来るはずのコミット」を持つ。空いたレーンは詰めずに、次に使うときに再利用する。
 */

export interface GraphRow {
  /** ノードの列 */
  col: number;
  /** ノードの色番号 */
  color: number;
  /**
   * 線分を [x1, y1, x2, y2, color] の順に並べたもの。x は列、y は 0 = 行の上端、1 = ノード、2 = 行の下端。
   * Webview にそのまま送るので数値の配列にしている。
   */
  segs: number[];
  /** この行で使っている列の数 */
  width: number;
}

export class GraphBuilder {
  private lanes: (string | null)[] = [];
  private colors: number[] = [];
  private nextColor = 0;

  add(oid: string, parents: readonly string[]): GraphRow {
    const lanes = this.lanes;
    const segs: number[] = [];

    let col = lanes.indexOf(oid);
    let color: number;
    if (col < 0) {
      // ブランチの先端。空いているレーンに置く
      col = this.freeLane();
      color = this.newColor();
      this.colors[col] = color;
    } else {
      color = this.colors[col]!;
    }

    // 上半分: このコミットに来るレーンはノードへ、それ以外はまっすぐ下へ
    for (let j = 0; j < lanes.length; j++) {
      const expected = lanes[j];
      if (expected === null || expected === undefined) {
        continue;
      }
      if (expected === oid) {
        segs.push(j, 0, col, 1, this.colors[j]!);
        lanes[j] = null;
      } else {
        segs.push(j, 0, j, 2, this.colors[j]!);
      }
    }

    // 下半分: 親へ。親がすでに別のレーンで待たれていれば、そのレーンに合流する
    lanes[col] = null;
    parents.forEach((parent, i) => {
      let lane = lanes.indexOf(parent);
      if (lane < 0) {
        if (i === 0) {
          lane = col;
          this.colors[lane] = color;
        } else {
          lane = this.freeLane();
          this.colors[lane] = this.newColor();
        }
        lanes[lane] = parent;
      }
      segs.push(col, 1, lane, 2, this.colors[lane]!);
    });

    while (lanes.length > 0 && lanes[lanes.length - 1] === null) {
      lanes.pop();
    }
    let width = col + 1;
    for (let i = 0; i < segs.length; i += 5) {
      width = Math.max(width, segs[i]! + 1, segs[i + 2]! + 1);
    }
    return { col, color, segs, width };
  }

  private freeLane(): number {
    const free = this.lanes.indexOf(null);
    if (free >= 0) {
      return free;
    }
    this.lanes.push(null);
    return this.lanes.length - 1;
  }

  private newColor(): number {
    return this.nextColor++;
  }
}
