/** 差分の描画。Working Copy と History で共通に使う */
import type { DiffHunk, DiffLine, FileDiff } from '../diff/fileDiff.ts';
import { el } from './api.ts';

/** これより多い行は描かない（重くなるので） */
const MAX_LINES = 5000;

export interface DiffOptions {
  /** 選んでいる行（lineKey）。行を選べるときだけ渡す */
  picked?: ReadonlySet<string>;
  /** hunk の見出しに置くボタン */
  hunkActions?: { label: string; run: (hunk: DiffHunk) => void; secondary?: boolean }[];
  /**
   * HEAD → 作業ツリーの差分（diff.combined）で、行と hunk の見出しにチェックを置く（次のコミットに入れるか）。
   * 押すと lines を stage（true）か外すか（false）で呼ぶ
   */
  check?: (lines: DiffLine[], stage: boolean) => void;
}

/** hunk の変更の行がどれだけ入っているか: all・some・none */
export function hunkCheck(hunk: DiffHunk): 'all' | 'some' | 'none' {
  const changed = hunk.lines.filter((l) => l.kind !== ' ');
  const staged = changed.filter((l) => l.staged).length;
  return staged === 0 ? 'none' : staged === changed.length ? 'all' : 'some';
}

const CHECK_MARKS = { all: '☑', some: '▣', none: '☐' } as const;

/** 構文の色の種類（コアの `SyntaxSpan` の kind）→ class の名前 */
const SYNTAX_CLASSES = [
  '',
  'keyword',
  'string',
  'comment',
  'number',
  'function',
  'type',
  'constant',
  'property',
  'attribute',
  'tag',
  'operator',
  'variable',
  'escape',
  'module',
];

/**
 * コードの 1 行。syntax（[始め, 終わり, 種類]）で色を付け、word（変わった部分）を強調する。どちらもなければ文字だけ
 */
export function renderCode(text: string, syntax?: readonly (readonly [number, number, number])[], word?: readonly [number, number]): HTMLElement {
  const code = el('span', 'code');
  if (!syntax?.length && !word) {
    code.textContent = text;
    return code;
  }
  // 区切り（色の境と強調の境）ごとに切る
  const cuts = new Set<number>([0, text.length]);
  for (const [s, e] of syntax ?? []) {
    cuts.add(Math.max(0, Math.min(text.length, s)));
    cuts.add(Math.max(0, Math.min(text.length, e)));
  }
  if (word) {
    cuts.add(word[0]);
    cuts.add(word[1]);
  }
  const points = [...cuts].sort((a, b) => a - b);
  for (let i = 0; i + 1 < points.length; i++) {
    const [from, to] = [points[i]!, points[i + 1]!];
    if (to <= from) continue;
    // 重なっていれば内側（後に来るもの）
    let span: readonly [number, number, number] | undefined;
    for (const candidate of syntax ?? []) if (candidate[0] <= from && to <= candidate[1]) span = candidate;
    const classes = [span ? `syn-${SYNTAX_CLASSES[span[2]] ?? ''}` : '', word && word[0] <= from && to <= word[1] ? 'word' : '']
      .filter(Boolean)
      .join(' ');
    code.append(classes ? el('span', classes, text.slice(from, to)) : text.slice(from, to));
  }
  return code;
}

/** 行を表す文字列。削除行は古い側、追加行は新しい側の行番号で表す */
export function lineKey(line: DiffLine): string {
  return line.kind === '-' ? `old:${line.old}` : `new:${line.new}`;
}

/** 差分の本体（hunk の並び、またはバイナリなどの説明） */
export function renderDiffBody(diff: FileDiff, opts: DiffOptions = {}): HTMLElement[] {
  if (diff.binary) return [el('div', 'empty', 'Binary file')];
  if (diff.tooLarge) return [el('div', 'empty', 'The file is too large to show its diff')];
  if (diff.hunks.length === 0) return [el('div', 'empty', 'No changes')];
  const parts: HTMLElement[] = [];
  let budget = MAX_LINES;
  for (const hunk of diff.hunks) {
    if (budget <= 0) {
      parts.push(el('div', 'empty', 'The rest of the diff is left out. Open it in the editor to see it all.'));
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
  const check = opts.check;
  if (check) {
    // 3 つの様子。すべて入っていれば外し、それ以外なら残りを入れる
    const state = hunkCheck(hunk);
    const box = el('button', `check ${state}`, CHECK_MARKS[state]);
    box.title = state === 'all' ? 'Unstage this hunk' : 'Stage this hunk';
    box.addEventListener('click', (e) => {
      e.stopPropagation();
      const changed = hunk.lines.filter((l) => l.kind !== ' ');
      if (state === 'all') check(changed, false);
      else check(changed.filter((l) => !l.staged), true);
    });
    head.append(box);
  }
  head.append(el('span', 'range', hunk.header));
  if (opts.hunkActions?.length) {
    const actions = el('span', 'actions');
    for (const { label, run } of opts.hunkActions) {
      const button = el('button', 'secondary', label);
      button.addEventListener('click', () => run(hunk));
      actions.append(button);
    }
    head.append(actions);
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
    if (check) {
      // 変更の行のチェック（次のコミットに入れるか）。押すとその行だけ入れる・外す
      if (line.kind === ' ') {
        row.append(el('span', 'check'));
      } else {
        const mark = el('button', `check ${line.staged ? 'all' : 'none'}`, line.staged ? CHECK_MARKS.all : CHECK_MARKS.none);
        mark.title = line.staged ? 'Unstage this line' : 'Stage this line';
        mark.addEventListener('click', (e) => {
          e.stopPropagation();
          check([line], !line.staged);
        });
        row.append(mark);
      }
    }
    row.append(el('span', 'no', line.old ? String(line.old) : ''), el('span', 'no', line.new ? String(line.new) : ''));
    row.append(el('span', 'sign', line.kind === ' ' ? '' : line.kind));
    row.append(renderCode(line.text, line.syntax, words.get(i)));
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
