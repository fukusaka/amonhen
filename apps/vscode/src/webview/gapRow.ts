/**
 * Working Copy のたたんだ差分の隙間の行（変更のない所）。破線の上に、左に関数名、中央に札（「⋯ 56 unchanged lines」）、
 * マウスが乗ると札の右に開く・畳むのボタン、右端にその下の hunk の操作を置く（アプリと同じ）。置き方は
 * `placeGap` で決め、行の幅が変わったら置き直す
 */
import type { DiffHunk } from '../diff/fileDiff.ts';
import { gapActions, pillText, placeGap, type GapAction, type StageOp } from '../workingCopy/gaps.ts';
import { el } from './api.ts';

export interface GapRowSpec {
  hunk: DiffHunk;
  /** diff.hunks の位置 */
  index: number;
  /** この hunk で選んでいる変更の行の数（札が「n lines selected」になり、開く・畳むは出さない） */
  selected: number;
  /** 右端の hunk の操作（選んだ行か hunk の変更すべてに効く） */
  ops: { op: StageOp; label: string }[];
  onGap: (action: GapAction) => void;
  onOp: (op: StageOp) => void;
  /** 札を押したが、開く・畳むのボタンが入りきらない: メニューを開く（行と札を渡す） */
  onMenu: (row: HTMLElement, pill: HTMLElement) => void;
  /** 開く・畳むを出さない（All Files のファイル全体の差分。いつもすべて見せる。アプリと同じ） */
  fixed?: boolean;
}

/** 置き直すための、行の部品と文字 */
interface Parts {
  pill: HTMLButtonElement;
  text: string;
  short?: string;
  chips: HTMLButtonElement[];
  fn?: HTMLElement;
  ops: HTMLElement;
  overflows: boolean;
}

const parts = new WeakMap<HTMLElement, Parts>();

export function gapRow(spec: GapRowSpec): HTMLElement {
  const gap = spec.hunk.gap!;
  const row = el('div', 'gap-row');
  row.dataset.gap = String(gap.index);
  row.dataset.hunk = String(spec.index);
  const { text, short } = pillText(gap, spec.selected);
  const pill = el('button', spec.selected > 0 ? 'gap-pill selected' : 'gap-pill');
  pill.textContent = text;
  pill.hidden = text === '' || (spec.fixed === true && spec.selected === 0);
  const actions = spec.fixed ? [] : gapActions(gap, spec.selected > 0);
  const chips = actions.map(({ action, label }) => {
    const chip = el('button', 'gap-chip', label);
    chip.dataset.action = action;
    chip.addEventListener('click', (e) => {
      e.stopPropagation();
      spec.onGap(action);
    });
    return chip;
  });
  pill.addEventListener('click', (e) => {
    e.stopPropagation();
    if (spec.selected > 0 || spec.fixed) return;
    // 入りきらないボタンがあれば、出せる開く・畳むすべてをメニューで。でなければ、すべて開く（開いていれば畳む）
    if (parts.get(row)?.overflows && actions.length > 0) {
      spec.onMenu(row, pill);
    } else if (gap.hidden > 0) {
      spec.onGap('all');
    } else if (gap.shown > 0) {
      spec.onGap('collapse');
    }
  });
  pill.title = spec.selected > 0 ? 'The hunk’s actions act on the selected lines' : gap.hidden > 0 ? 'Show these lines' : 'Fold these lines again';
  const ops = el('span', 'gap-ops');
  for (const { op, label } of spec.ops) {
    const button = el('button', op === 'discard' ? 'gap-op discard' : 'gap-op', label);
    button.addEventListener('click', (e) => {
      e.stopPropagation();
      spec.onOp(op);
    });
    ops.append(button);
  }
  const fn = gap.function ? el('span', 'gap-fn', `ƒ ${gap.function}`) : undefined;
  if (fn) row.append(fn);
  row.append(pill, ...chips, ops);
  parts.set(row, { pill, text, ...(short ? { short } : {}), chips, ...(fn ? { fn } : {}), ops, overflows: false });
  return row;
}

/** 文字の幅を測る（置き直すたびに DOM を測らない） */
let measure: CanvasRenderingContext2D | null | undefined;

function textWidth(text: string, font: string): number {
  measure ??= document.createElement('canvas').getContext('2d');
  if (!measure) return text.length * 7;
  measure.font = font;
  return measure.measureText(text).width;
}

function fontOf(element: HTMLElement): string {
  const style = getComputedStyle(element);
  return `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
}

/**
 * container の中の隙間の行の札とボタンを置く（描いた後と、幅が変わったとき）。gutter は行番号の欄の幅（行の左端から
 * 文字の始まりまで）
 */
export function layoutGapRows(container: HTMLElement, gutter: number): void {
  const rows = container.querySelectorAll<HTMLElement>('.gap-row');
  if (rows.length === 0) return;
  let pillFont: string | undefined;
  let chipFont: string | undefined;
  for (const row of rows) {
    const p = parts.get(row);
    if (!p) continue;
    const width = row.clientWidth;
    if (width === 0) continue;
    pillFont ??= fontOf(p.pill);
    if (p.chips[0]) chipFont ??= fontOf(p.chips[0]);
    const rowLeft = row.getBoundingClientRect().left;
    const right = p.ops.childElementCount > 0 ? p.ops.getBoundingClientRect().left - rowLeft - 2 : width - 10;
    const left = gutter + 8;
    let pillX: number | undefined;
    if (p.text) {
      const pillWidth = textWidth(p.text, pillFont) + 20;
      const shortWidth = p.short ? textWidth(p.short, pillFont) + 20 : undefined;
      const chipWidths = p.chips.map((c) => textWidth(c.textContent ?? '', chipFont ?? pillFont!) + 16);
      const fit = placeGap({ left, center: gutter + (width - gutter) / 2, right, pill: pillWidth, ...(shortWidth ? { shortPill: shortWidth } : {}), chips: chipWidths });
      p.overflows = fit.overflows;
      pillX = fit.pillX;
      p.pill.hidden = pillX === undefined;
      if (pillX !== undefined) {
        p.pill.textContent = fit.short ? p.short! : p.text;
        p.pill.style.left = `${pillX}px`;
        p.pill.style.width = `${fit.short ? shortWidth! : pillWidth}px`;
      }
      const placed = new Map(fit.chips.map((c) => [c.index, c.x]));
      p.chips.forEach((chip, i) => {
        const x = placed.get(i);
        chip.classList.toggle('fit', x !== undefined);
        if (x !== undefined) {
          chip.style.left = `${x}px`;
          chip.style.width = `${chipWidths[i]}px`;
        }
      });
    }
    // 左の関数名（札にかかるなら出さない）
    if (p.fn) {
      const end = left + textWidth(p.fn.textContent ?? '', fontOf(p.fn));
      p.fn.hidden = !(end + 12 < (pillX ?? right));
      p.fn.style.left = `${left}px`;
    }
    row.dataset.overflows = String(p.overflows);
  }
}
