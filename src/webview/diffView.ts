/** 差分の描画。Working Copy と History で共通に使う */
import type { DiffHunk, DiffLine, FileDiff } from '../diff/fileDiff.ts';
import { el } from './api.ts';

/** これより多い行は描かない（重くなるので） */
const MAX_LINES = 5000;

export interface DiffOptions {
  /** 選んでいる行（lineKey）。行を選べるときだけ渡す */
  picked?: ReadonlySet<string>;
  /** hunk の見出しに置くボタン */
  hunkAction?: { label: string; run: (hunk: DiffHunk) => void };
}

/** 行を表す文字列。削除行は古い側、追加行は新しい側の行番号で表す */
export function lineKey(line: DiffLine): string {
  return line.kind === '-' ? `old:${line.old}` : `new:${line.new}`;
}

/** 差分の本体（hunk の並び、またはバイナリなどの説明） */
export function renderDiffBody(diff: FileDiff, opts: DiffOptions = {}): HTMLElement[] {
  if (diff.binary) return [el('div', 'empty', 'バイナリファイルです')];
  if (diff.tooLarge) return [el('div', 'empty', 'ファイルが大きいので差分を出しません')];
  if (diff.hunks.length === 0) return [el('div', 'empty', '差分はありません')];
  const parts: HTMLElement[] = [];
  let budget = MAX_LINES;
  for (const hunk of diff.hunks) {
    if (budget <= 0) {
      parts.push(el('div', 'empty', '残りの差分は省略しました。エディタで開いて確認してください'));
      break;
    }
    parts.push(renderHunk(hunk, opts, budget));
    budget -= hunk.lines.length;
  }
  return parts;
}

function renderHunk(hunk: DiffHunk, opts: DiffOptions, budget: number): HTMLElement {
  const box = el('div', 'hunk');
  const head = el('div', 'hunk-head');
  head.append(el('span', 'range', hunk.header));
  if (opts.hunkAction) {
    const { label, run } = opts.hunkAction;
    const button = el('button', 'secondary', label);
    button.addEventListener('click', () => run(hunk));
    head.append(button);
  }
  box.append(head);

  const table = el('div', 'lines');
  const words = wordRanges(hunk.lines);
  hunk.lines.slice(0, budget).forEach((line, i) => {
    const row = el('div', `line ${line.kind === '+' ? 'add' : line.kind === '-' ? 'del' : 'ctx'}`);
    if (opts.picked && line.kind !== ' ') {
      const k = lineKey(line);
      row.dataset.key = k;
      row.classList.add('pickable');
      if (opts.picked.has(k)) row.classList.add('picked');
    }
    row.append(el('span', 'no', line.old ? String(line.old) : ''), el('span', 'no', line.new ? String(line.new) : ''));
    row.append(el('span', 'sign', line.kind === ' ' ? '' : line.kind));
    const text = el('span', 'code');
    const range = words.get(i);
    if (range) {
      text.append(
        line.text.slice(0, range[0]),
        el('span', 'word', line.text.slice(range[0], range[1])),
        line.text.slice(range[1]),
      );
    } else {
      text.textContent = line.text;
    }
    row.append(text);
    table.append(row);
  });
  box.append(table);
  return box;
}

/**
 * 削除行の並びと追加行の並びが同じ数で続くところでは、対応する行どうしで変わった部分を強調する。
 * 行の位置 → [変わった部分の始まり, 終わり]
 */
function wordRanges(lines: readonly DiffLine[]): Map<number, [number, number]> {
  const result = new Map<number, [number, number]>();
  let i = 0;
  while (i < lines.length) {
    if (lines[i]!.kind !== '-') {
      i++;
      continue;
    }
    let d = i;
    while (d < lines.length && lines[d]!.kind === '-') d++;
    let a = d;
    while (a < lines.length && lines[a]!.kind === '+') a++;
    if (a - d === d - i) {
      for (let k = 0; k < d - i; k++) {
        const x = lines[i + k]!.text;
        const y = lines[d + k]!.text;
        let p = 0;
        while (p < x.length && p < y.length && x[p] === y[p]) p++;
        let s = 0;
        while (s < x.length - p && s < y.length - p && x[x.length - 1 - s] === y[y.length - 1 - s]) s++;
        // ほとんど全部が変わっている行は強調しない
        if (p + s > 0) {
          result.set(i + k, [p, x.length - s]);
          result.set(d + k, [p, y.length - s]);
        }
      }
    }
    i = a;
  }
  return result;
}
