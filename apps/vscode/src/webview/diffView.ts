/** 差分の描画。Working Copy と History で共通に使う */
import type { DiffHunk, DiffLine, FileDiff } from '../diff/fileDiff.ts';
import type { DiffWhitespace } from '../history/protocol.ts';
import { wordHighlighter, type WordRange } from '../diff/words.ts';
import { el, post } from './api.ts';

/**
 * 長い差分もすべて描く（アプリと同じ。VSCode の検索もすべての行を探せる）。重くならないよう、初めの行だけ描いてから
 * 返し、残りは行の塊ごとに少しずつ足す。塊は画面の外にある間は配置と描画を省く（content-visibility）
 */
const FIRST_LINES = 2000;
/** 1 つの塊の行の数 */
const BLOCK_LINES = 500;
/** 後から足すとき、1 回に使う時間（ms）。それを超えたら画面に返す */
const SLICE_MS = 12;

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
  /** あれば hunk の見出しの左に印を置き、押すとファイルの前後の行を広げる */
  widen?: () => void;
  /**
   * 行の右クリックメニュー（package.json の webview/context）に渡す情報。右クリックしたときに、その行の分だけ作る
   * （行ごとに持たせると長い差分で重い）
   */
  lineContext?: (line: DiffLine) => Record<string, unknown>;
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
 * コードの 1 行。syntax（[始め, 終わり, 種類]）で色を付け、words（変わった部分。いくつあってもよい）を強調する。
 * どちらもなければ文字だけ
 */
export function renderCode(text: string, syntax?: readonly (readonly [number, number, number])[], words?: readonly WordRange[]): HTMLElement {
  const code = el('span', 'code');
  if (!syntax?.length && !words?.length) {
    code.textContent = text;
    return code;
  }
  // 区切り（色の境と強調の境）ごとに切る
  const cuts = new Set<number>([0, text.length]);
  for (const [s, e] of syntax ?? []) {
    cuts.add(Math.max(0, Math.min(text.length, s)));
    cuts.add(Math.max(0, Math.min(text.length, e)));
  }
  for (const [s, e] of words ?? []) {
    cuts.add(Math.max(0, Math.min(text.length, s)));
    cuts.add(Math.max(0, Math.min(text.length, e)));
  }
  const points = [...cuts].sort((a, b) => a - b);
  for (let i = 0; i + 1 < points.length; i++) {
    const [from, to] = [points[i]!, points[i + 1]!];
    if (to <= from) continue;
    // 重なっていれば内側（後に来るもの）
    let span: readonly [number, number, number] | undefined;
    for (const candidate of syntax ?? []) if (candidate[0] <= from && to <= candidate[1]) span = candidate;
    const changed = words?.some(([s, e]) => s <= from && to <= e) === true;
    const classes = [span ? `syn-${SYNTAX_CLASSES[span[2]] ?? ''}` : '', changed ? 'word' : '']
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

/** 差分の本体（hunk の並び、またはバイナリなどの説明）。初めの行だけ描いて返し、残りは後から足す */
export function renderDiffBody(diff: FileDiff, opts: DiffOptions = {}): HTMLElement[] {
  if (diff.binary) return [el('div', 'empty', 'Binary file')];
  if (diff.tooLarge) return [el('div', 'empty', 'The file is too large to show its diff')];
  if (diff.hunks.length === 0) return [el('div', 'empty', 'No changes')];
  const later: Later = { budget: FIRST_LINES, queue: [] };
  const parts = diff.hunks.map((hunk) => renderHunk(hunk, opts, later));
  if (later.queue.length > 0) fillLater(later.queue);
  return parts;
}

/** 後から描く行の塊と、すぐ描ける残りの行の数 */
interface Later {
  budget: number;
  queue: { block: HTMLElement; fill: () => void }[];
}

/**
 * 後に回した塊を、少しずつ描く（1 回 SLICE_MS まで）。置かれる前に始まったら置かれるのを待ち、外されたら
 * （描き直した・別の差分を出した）やめる
 */
function fillLater(queue: Later['queue']): void {
  let next = 0;
  let placed = false;
  let waits = 0;
  const step = () => {
    const first = queue[next];
    if (!first) return;
    if (!first.block.isConnected) {
      if (placed || ++waits > 100) return;
      setTimeout(step, 16);
      return;
    }
    placed = true;
    const until = performance.now() + SLICE_MS;
    while (next < queue.length && performance.now() < until) {
      const { block, fill } = queue[next++]!;
      if (!block.isConnected) return;
      fill();
    }
    if (next < queue.length) setTimeout(step, 0);
  };
  setTimeout(step, 0);
}

function renderHunk(hunk: DiffHunk, opts: DiffOptions, later: Later): HTMLElement {
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
  if (opts.widen) {
    const widen = opts.widen;
    const more = el('button', 'widen', '⇕');
    more.title = 'Show 20 more lines around each change in this file';
    more.addEventListener('click', (e) => {
      e.stopPropagation();
      widen();
    });
    head.append(more);
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
  const words = wordHighlighter(hunk.lines);
  const renderLine = (line: DiffLine, i: number): HTMLElement => {
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
    row.append(renderCode(line.text, line.syntax, words(i)));
    return row;
  };
  // 行の塊。すぐ描ける分は描き、残りは高さだけ取っておいて後から描く
  for (let start = 0; start < hunk.lines.length; start += BLOCK_LINES) {
    const end = Math.min(hunk.lines.length, start + BLOCK_LINES);
    const block = el('div', 'line-block');
    // 画面の外にある間の高さ（折り返さなければ行の高さ × 行数）。一度描けばその高さを覚える
    block.style.containIntrinsicBlockSize = `auto ${(end - start) * 1.5}em`;
    const fill = () => {
      const rows: HTMLElement[] = [];
      for (let i = start; i < end; i++) rows.push(renderLine(hunk.lines[i]!, i));
      block.style.minHeight = '';
      block.append(...rows);
    };
    if (later.budget > 0) {
      fill();
      later.budget -= end - start;
    } else {
      block.style.minHeight = `${(end - start) * 1.5}em`;
      later.queue.push({ block, fill });
    }
    table.append(block);
  }
  const lineContext = opts.lineContext;
  if (lineContext) {
    // 右クリックした行の情報を、VSCode がメニューを出す前に（window で受ける前に）その行に置く
    table.addEventListener('contextmenu', (e) => {
      const row = (e.target as HTMLElement).closest<HTMLElement>('.line');
      const block = row?.parentElement;
      if (!row || !block) return;
      const index = [...table.children].indexOf(block) * BLOCK_LINES + [...block.children].indexOf(row);
      const line = hunk.lines[index];
      if (line) row.dataset.vscodeContext = JSON.stringify(lineContext(line));
    });
  }
  box.append(table);
  return box;
}

/** 長い行を折り返しているか（設定。ボタンの見た目に使う） */
let wrapping = false;

/** 長い行の折り返しを当てる（すべての差分とファイルの中身。帯の ↩︎ の見た目も合わせる） */
export function setWrapLines(wrap: boolean): void {
  wrapping = wrap;
  document.body.classList.toggle('wrap-lines', wrap);
  for (const button of document.querySelectorAll<HTMLElement>('.wrap-button')) updateWrapButton(button);
}

function updateWrapButton(button: HTMLElement): void {
  button.classList.toggle('on', wrapping);
  button.title = `${wrapping ? 'Wrapping long lines' : 'Wrap Long Lines'} (Alt+Z)\nWrap lines wider than the diff instead of scrolling sideways (in every diff)`;
}

/** 差分の帯の ↩︎（長い行の折り返しを切り替える。設定はすべての差分で 1 つ） */
const WHITESPACE: { value: DiffWhitespace; label: string; tip: string }[] = [
  { value: 'exact', label: 'Compare Whitespace', tip: 'Lines that differ only in whitespace are changes' },
  { value: 'at-eol', label: 'Ignore Whitespace at Line Ends', tip: 'Ignore whitespace at the end of lines (git diff --ignore-space-at-eol)' },
  { value: 'change', label: 'Ignore Changes in Whitespace', tip: 'Ignore changes in the amount of whitespace (git diff -b)' },
  { value: 'all', label: 'Ignore All Whitespace', tip: 'Ignore all whitespace when comparing lines (git diff -w)' },
];

/** 行を比べるときに無視する空白を選ぶ欄（History の詳細と Working Copy の差分の帯。無視している間は枠を強調） */
export function whitespaceSelect(current: DiffWhitespace, onChange: (whitespace: DiffWhitespace) => void): HTMLSelectElement {
  const select = el('select', 'whitespace');
  select.classList.toggle('on', current !== 'exact');
  for (const w of WHITESPACE) {
    const option = el('option', '', w.label);
    option.value = w.value;
    option.title = w.tip;
    option.selected = w.value === current;
    select.append(option);
  }
  select.title = 'Whitespace to ignore when comparing lines (as git diff does)';
  select.addEventListener('change', () => onChange(select.value as DiffWhitespace));
  return select;
}

export function wrapButton(): HTMLElement {
  const button = el('button', 'bar-button wrap-button', '↩︎');
  button.addEventListener('click', () => post({ type: 'toggleWrapLines' }));
  updateWrapButton(button);
  return button;
}
