/**
 * 行単位の差分と、選んだ変更だけを当てる処理。
 * 文字列は latin1 で扱う。バイトと 1 対 1 に対応するので、どの文字コードのファイルでも内容を壊さない。
 */

/** 改行を含めたまま行に分ける。最終行に改行が無ければ、その行だけ改行なしになる */
export function splitLines(text: string): string[] {
  const lines: string[] = [];
  let start = 0;
  for (;;) {
    const nl = text.indexOf('\n', start);
    if (nl < 0) {
      break;
    }
    lines.push(text.slice(start, nl + 1));
    start = nl + 1;
  }
  if (start < text.length) {
    lines.push(text.slice(start));
  }
  return lines;
}

/** base の [baseStart, baseEnd) が target の [targetStart, targetEnd) に置き換わる。行番号は 0 始まり */
export interface LineChange {
  baseStart: number;
  baseEnd: number;
  targetStart: number;
  targetEnd: number;
}

/** Myers の O(ND) 差分。前後の共通部分は先に取り除く */
export function diffLines(base: readonly string[], target: readonly string[]): LineChange[] {
  let prefix = 0;
  while (prefix < base.length && prefix < target.length && base[prefix] === target[prefix]) {
    prefix++;
  }
  let suffix = 0;
  while (
    suffix < base.length - prefix &&
    suffix < target.length - prefix &&
    base[base.length - 1 - suffix] === target[target.length - 1 - suffix]
  ) {
    suffix++;
  }
  // 行を整数に置き換えて比較を速くする
  const ids = new Map<string, number>();
  const toIds = (lines: readonly string[], from: number, to: number) => {
    const out = new Int32Array(to - from);
    for (let i = from; i < to; i++) {
      const line = lines[i]!;
      let id = ids.get(line);
      if (id === undefined) {
        id = ids.size;
        ids.set(line, id);
      }
      out[i - from] = id;
    }
    return out;
  };
  const a = toIds(base, prefix, base.length - suffix);
  const b = toIds(target, prefix, target.length - suffix);
  return myers(a, b).map((c) => ({
    baseStart: c.baseStart + prefix,
    baseEnd: c.baseEnd + prefix,
    targetStart: c.targetStart + prefix,
    targetEnd: c.targetEnd + prefix,
  }));
}

/** これより編集が多いと時間とメモリがかかりすぎるので、探索を打ち切る */
const MAX_EDITS = 2000;

function myers(a: Int32Array, b: Int32Array): LineChange[] {
  const n = a.length;
  const m = b.length;
  if (n === 0 && m === 0) {
    return [];
  }
  const whole = [{ baseStart: 0, baseEnd: n, targetStart: 0, targetEnd: m }];
  if (n === 0 || m === 0) {
    return whole;
  }
  const max = Math.min(n + m, MAX_EDITS);
  const offset = max + 1;
  const v = new Int32Array(2 * max + 3);
  // trace[d] には d 回目の開始時点の v のうち、k ∈ [-d, d] の範囲だけを残す
  const trace: Int32Array[] = [];
  let found = false;
  for (let d = 0; d <= max && !found; d++) {
    trace.push(v.slice(offset - d - 1, offset + d + 2));
    for (let k = -d; k <= d; k += 2) {
      let x: number;
      if (k === -d || (k !== d && v[offset + k - 1]! < v[offset + k + 1]!)) {
        x = v[offset + k + 1]!;
      } else {
        x = v[offset + k - 1]! + 1;
      }
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) {
        x++;
        y++;
      }
      v[offset + k] = x;
      if (x >= n && y >= m) {
        found = true;
        break;
      }
    }
  }
  if (!found) {
    // 打ち切ったときは全体を 1 つの変更として扱う。行単位の stage はこのままでもできる
    return whole;
  }
  // 末尾から辿って編集を復元する
  const edits: { kind: 'del' | 'ins'; x: number; y: number }[] = [];
  let x = n;
  let y = m;
  for (let d = trace.length - 1; d > 0; d--) {
    const vd = trace[d]!;
    // vd[i] は k = i - d - 1 に対応する
    const at = (k: number) => vd[k + d + 1]!;
    const k = x - y;
    let prevK: number;
    if (k === -d || (k !== d && at(k - 1) < at(k + 1))) {
      prevK = k + 1;
    } else {
      prevK = k - 1;
    }
    const prevX = at(prevK);
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) {
      x--;
      y--;
    }
    if (x === prevX) {
      edits.push({ kind: 'ins', x: prevX, y: prevY });
    } else {
      edits.push({ kind: 'del', x: prevX, y: prevY });
    }
    x = prevX;
    y = prevY;
  }
  edits.reverse();
  // 連続する削除・挿入を 1 つの変更にまとめる
  const changes: LineChange[] = [];
  for (const e of edits) {
    const last = changes[changes.length - 1];
    const bx = e.x;
    const by = e.y;
    if (last && last.baseEnd === bx && last.targetEnd === by) {
      if (e.kind === 'del') last.baseEnd++;
      else last.targetEnd++;
    } else {
      changes.push(
        e.kind === 'del'
          ? { baseStart: bx, baseEnd: bx + 1, targetStart: by, targetEnd: by }
          : { baseStart: bx, baseEnd: bx, targetStart: by, targetEnd: by + 1 },
      );
    }
  }
  return changes;
}

/** 選択範囲。行番号は 0 始まりで、両端を含む */
export interface LineRange {
  start: number;
  end: number;
}

export interface Selection {
  side: 'base' | 'target';
  ranges: readonly LineRange[];
}

/**
 * base に、選んだ部分の変更だけを当てた内容を返す。
 * - collapsed（カーソルだけで何も選んでいない）なら、カーソル行を含む変更（hunk）全体を当てる
 * - そうでなければ、選んだ行だけを当てる。base 側の選択は削除、target 側の選択は追加になる
 */
export function applySelection(
  base: readonly string[],
  target: readonly string[],
  changes: readonly LineChange[],
  selection: Selection,
  collapsed: boolean,
): string {
  const inRanges = (line: number) => selection.ranges.some((r) => r.start <= line && line <= r.end);
  const out: string[] = [];
  let pos = 0;
  for (const c of changes) {
    for (; pos < c.baseStart; pos++) {
      out.push(base[pos]!);
    }
    const whole = collapsed && touches(c, selection);
    const removed: boolean[] = [];
    for (let i = c.baseStart; i < c.baseEnd; i++) {
      removed.push(whole || (!collapsed && selection.side === 'base' && inRanges(i)));
    }
    // 削除しない行を先に残し、そのあとに追加する行を置く
    for (let i = c.baseStart; i < c.baseEnd; i++) {
      if (!removed[i - c.baseStart]) {
        out.push(base[i]!);
      }
    }
    for (let i = c.targetStart; i < c.targetEnd; i++) {
      if (whole || (!collapsed && selection.side === 'target' && inRanges(i))) {
        out.push(target[i]!);
      }
    }
    pos = c.baseEnd;
  }
  for (; pos < base.length; pos++) {
    out.push(base[pos]!);
  }
  return fixLastNewline(out);
}

/** 変更が選択範囲に触れているか。削除だけの変更は、その前後の行に触れていれば対象にする */
function touches(c: LineChange, selection: Selection): boolean {
  const [start, end] = selection.side === 'base' ? [c.baseStart, c.baseEnd] : [c.targetStart, c.targetEnd];
  return selection.ranges.some((r) => (start === end ? r.start <= start && start - 1 <= r.end : r.start < end && start <= r.end));
}

/** 途中に改行の無い行が来た場合（元の最終行のあとに行を足したとき）は改行を補う */
function fixLastNewline(lines: string[]): string {
  for (let i = 0; i < lines.length - 1; i++) {
    const line = lines[i]!;
    if (!line.endsWith('\n')) {
      lines[i] = line + (lines.find((l) => l.endsWith('\r\n')) ? '\r\n' : '\n');
    }
  }
  return lines.join('');
}
