/** History でいくつも選んだ行（⌘・Ctrl-クリック、⇧-クリック、⇧+矢印）。vscode に依存しない */

export interface RowSelection {
  /** 今の行（矢印で動く元） */
  focus: number;
  /** ⇧ で広げる元 */
  anchor: number;
  rows: ReadonlySet<number>;
}

export function singleRow(row: number): RowSelection {
  return { focus: row, anchor: row, rows: new Set([row]) };
}

/** 1 行を足す・外す。最後の 1 行は外さない */
export function toggleRow(selection: RowSelection, row: number): RowSelection {
  const rows = new Set(selection.rows);
  if (!rows.has(row)) {
    rows.add(row);
    return { focus: row, anchor: row, rows };
  }
  if (rows.size === 1) {
    return selection;
  }
  rows.delete(row);
  const focus = nearest(rows, row);
  return { focus, anchor: focus, rows };
}

/** anchor から row までを選ぶ（それまでの選択は置き換える） */
export function extendTo(selection: RowSelection, row: number): RowSelection {
  const rows = new Set<number>();
  const [from, to] = selection.anchor < row ? [selection.anchor, row] : [row, selection.anchor];
  for (let r = from; r <= to; r++) {
    rows.add(r);
  }
  return { focus: row, anchor: selection.anchor, rows };
}

/** 行が隙間なく続いているか */
export function isContiguous(rows: ReadonlySet<number>): boolean {
  let low = Infinity;
  let high = -Infinity;
  for (const r of rows) {
    low = Math.min(low, r);
    high = Math.max(high, r);
  }
  return high - low + 1 === rows.size;
}

function nearest(rows: ReadonlySet<number>, row: number): number {
  let best = row;
  let distance = Infinity;
  for (const r of rows) {
    if (Math.abs(r - row) < distance) {
      best = r;
      distance = Math.abs(r - row);
    }
  }
  return best;
}
