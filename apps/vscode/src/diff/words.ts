/**
 * 変わった行の中の、変わった単語（コアの `words.rs`・`diff.rs` の word_highlights と同じ考え）。続けて並ぶ削除行と
 * 追加行を 1 行ずつ組にし（数が違っても、組になる分だけ）、単語単位で比べる。1 行に変わったところがいくつあってもよい。
 * エンジンがなくても同じに強調できるよう、Webview で計算する。vscode と DOM に依存しない
 */
import { diffLines } from './lineDiff.ts';

/** 行の中の範囲 [始め, 終わり)（文字列の位置。UTF-16） */
export type WordRange = readonly [number, number];

/** これより長い行（UTF-8 のバイト数）は比べない（時間がかかり、強調しても読みにくい。コアと同じ） */
const MAX_LINE = 1000;

/** 文字の種類: 単語（英数字・`_`・ASCII でない文字）0、空白 1、それ以外の記号 2（コアと同じ） */
function charClass(code: number): number {
  if ((code >= 0x30 && code <= 0x39) || (code >= 0x41 && code <= 0x5a) || (code >= 0x61 && code <= 0x7a) || code === 0x5f || code >= 0x80) return 0;
  // Rust の is_ascii_whitespace（空白・タブ・改行・改ページ・復帰）
  if (code === 0x20 || code === 0x09 || code === 0x0a || code === 0x0c || code === 0x0d) return 1;
  return 2;
}

/** 単語の区切りの位置。記号は 1 文字ずつ、そのほかは同じ種類が続く間を 1 つにする（サロゲートの間では切らない） */
function boundaries(text: string): number[] {
  const cuts = [0];
  for (let i = 1; i < text.length; i++) {
    const a = charClass(text.charCodeAt(i - 1));
    if (a !== charClass(text.charCodeAt(i)) || a === 2) cuts.push(i);
  }
  if (text.length > 0) cuts.push(text.length);
  return cuts;
}

/** text の [from, to) の UTF-8 のバイト数（コアはバイトで数えるので、同じ数え方にする） */
function utf8Length(text: string, from = 0, to = text.length): number {
  let n = 0;
  for (let i = from; i < to; i++) {
    const c = text.charCodeAt(i);
    if (c < 0x80) n += 1;
    else if (c < 0x800) n += 2;
    // サロゲートの組は 4 バイト（2 つで 4）
    else if (c >= 0xd800 && c <= 0xdfff) n += 2;
    else n += 3;
  }
  return n;
}

/**
 * old と new の中で変わった部分。ほとんどが変わっていれば強調しない（行全体の色で足りる）。そのときと長すぎる行は
 * undefined
 */
export function changedWords(old: string, neu: string): [WordRange[], WordRange[]] | undefined {
  const oldBytes = utf8Length(old);
  const newBytes = utf8Length(neu);
  if (oldBytes > MAX_LINE || newBytes > MAX_LINE) return undefined;
  const oc = boundaries(old);
  const nc = boundaries(neu);
  const tokens = (text: string, cuts: number[]) => cuts.slice(1).map((end, i) => text.slice(cuts[i], end));
  const removed: WordRange[] = [];
  const added: WordRange[] = [];
  let changed = 0;
  for (const c of diffLines(tokens(old, oc), tokens(neu, nc))) {
    if (c.baseStart < c.baseEnd) {
      removed.push([oc[c.baseStart]!, oc[c.baseEnd]!]);
      changed += utf8Length(old, oc[c.baseStart]!, oc[c.baseEnd]!);
    }
    if (c.targetStart < c.targetEnd) {
      added.push([nc[c.targetStart]!, nc[c.targetEnd]!]);
      changed += utf8Length(neu, nc[c.targetStart]!, nc[c.targetEnd]!);
    }
  }
  // 変わっていない部分が少なければ、単語の強調は役に立たない
  if (changed * 2 > oldBytes + newBytes) return undefined;
  return [removed, added];
}

/**
 * 続けて並ぶ削除行と追加行の組（行の位置 → 組になる相手の位置）。削除行が 3 つ・追加行が 2 つなら、前から 2 組
 */
export function wordPairs(kinds: readonly string[]): Map<number, number> {
  const pairs = new Map<number, number>();
  let i = 0;
  while (i < kinds.length) {
    if (kinds[i] !== '-') {
      i++;
      continue;
    }
    const removedStart = i;
    while (i < kinds.length && kinds[i] === '-') i++;
    const addedStart = i;
    while (i < kinds.length && kinds[i] === '+') i++;
    for (let k = 0; removedStart + k < addedStart && addedStart + k < i; k++) {
      pairs.set(removedStart + k, addedStart + k);
      pairs.set(addedStart + k, removedStart + k);
    }
  }
  return pairs;
}

/**
 * 行の並びから、各行の変わった単語を引く（描く行だけ計算する。長い差分でも、描いていない行の分は計算しない）
 */
export function wordHighlighter(lines: readonly { kind: string; text: string }[]): (index: number) => readonly WordRange[] | undefined {
  const pairs = wordPairs(lines.map((l) => l.kind));
  const done = new Map<number, [WordRange[], WordRange[]] | undefined>();
  return (index) => {
    const other = pairs.get(index);
    if (other === undefined) return undefined;
    const removed = Math.min(index, other);
    if (!done.has(removed)) done.set(removed, changedWords(lines[removed]!.text, lines[Math.max(index, other)]!.text));
    const ranges = done.get(removed);
    return ranges?.[index === removed ? 0 : 1];
  };
}
