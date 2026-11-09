/**
 * メインビューの Range Diff 画面（アプリと同じ）。左の上に比べる範囲（直して比べ直せる）、何と何を比べたか、組の数と
 * Changes Only、その下に組（同じ・変わった・消えた・足された）の一覧。右に、変わった組なら差分の差分、ほかの組なら
 * そのコミットの中身（消えたものは古い版、ほかは新しい版）
 */
import { rangeDiffFile, shortRange, type RangePair, type RangeStatus } from '../git/rangeDiff.ts';
import type { HostMessage } from '../view/protocol.ts';
import { el, formatDate, post, type View } from './api.ts';
import { attachSplitter, createDetailsPane } from './details.ts';
import { renderDiffBody } from './diffView.ts';

const ROW_HEIGHT = 40;

/** 組の印（git range-diff と同じ記号）と、どうなったか */
const MARKS: Record<RangeStatus, [string, string]> = {
  same: ['=', 'same'],
  changed: ['!', 'changed'],
  removed: ['<', 'removed'],
  added: ['>', 'added'],
};

const short = (oid: string | undefined) => (oid ? oid.slice(0, 7) : '–');

export function createRangeDiffView(): View {
  const root = el('div', 'view range-diff');
  const body = el('div', 'split');

  // 左: 比べる範囲・何と何を比べたか・数と Changes Only・組の一覧
  const side = el('div', 'range-side');
  const bar = el('div', 'range-bar');
  const top = el('div', 'range-top');
  const field = el('input', 'range-field');
  field.type = 'text';
  field.placeholder = 'old...new  or  base old new';
  field.title = 'The ranges to compare, written as for git range-diff';
  field.spellcheck = false;
  const compare = el('button', 'secondary', 'Compare');
  const close = el('button', 'icon-button close', '×');
  close.title = 'Close Range Diff';
  top.append(field, compare, close);
  const summary = el('div', 'range-summary');
  const second = el('div', 'range-second');
  const counts = el('span', 'range-counts');
  const changesOnlyLabel = el('label', 'range-changes-only');
  const changesOnly = el('input');
  changesOnly.type = 'checkbox';
  changesOnlyLabel.title = 'Hide the commits that are the same in both versions';
  changesOnlyLabel.append(changesOnly, 'Changes Only');
  second.append(counts, changesOnlyLabel);
  bar.append(top, summary, second);
  const list = el('div', 'list');
  list.tabIndex = 0;
  const spacer = el('div', 'spacer');
  list.append(spacer);
  side.append(bar, list);

  // 右: 変わった組の差分の差分か、コミットの中身
  const right = el('div', 'range-right');
  const pairPane = el('div', 'range-pair');
  const pairHead = el('div', 'range-pair-head');
  const pairBody = el('div', 'range-pair-body');
  pairPane.append(pairHead, pairBody);
  const details = createDetailsPane();
  right.append(pairPane, details.root);

  const splitter = el('div', 'splitter');
  splitter.setAttribute('role', 'separator');
  body.append(side, splitter, right);
  root.append(body);
  attachSplitter(body, splitter, right, () => undefined);

  let pairs: RangePair[] = [];
  /** 一覧に出している組（Changes Only なら同じものを除く） */
  let shown: RangePair[] = [];
  let selected = -1;
  let selectTimer = 0;
  /** 右に中身を出しているコミット */
  let wanted: string | undefined;

  function renderRow(pair: RangePair, index: number): HTMLElement {
    const [symbol, word] = MARKS[pair.status];
    const row = el('div', `row range-row ${pair.status}${index === selected ? ' selected' : ''}`);
    row.style.top = `${index * ROW_HEIGHT}px`;
    row.dataset.index = String(index);
    // Webview の右クリックメニュー（package.json の webview/context）に渡す情報
    row.dataset.vscodeContext = JSON.stringify({
      webviewSection: 'rangePair',
      oldOid: pair.old?.oid,
      newOid: pair.new?.oid,
      rangePairOld: !!pair.old,
      rangePairNew: !!pair.new,
      preventDefaultContextMenuItems: true,
    });
    // 1 行目: 印、古い番号 → 新しい番号、件名。2 行目: どうなったか、短い ID
    const first = el('div', 'range-line');
    first.append(
      el('span', 'mark', symbol),
      el('span', 'numbers', `${pair.old?.index ?? '–'} → ${pair.new?.index ?? '–'}`),
      el('span', 'subject', pair.subject),
    );
    first.title = pair.subject;
    const sub = el('div', 'range-line meta');
    sub.append(el('span', 'word', word), el('span', 'ids', `· ${short(pair.old?.oid)} → ${short(pair.new?.oid)}`));
    const lines = el('div', 'row-lines');
    lines.append(first, sub);
    row.append(lines);
    return row;
  }

  function render(): void {
    spacer.style.height = `${shown.length * ROW_HEIGHT}px`;
    spacer.replaceChildren(...shown.map((p, i) => renderRow(p, i)));
  }

  /** 右の欄に組を出す（なければ案内） */
  function showPair(pair: RangePair | undefined): void {
    clearTimeout(selectTimer);
    // 変わった組は差分の差分。ほかはコミットそのもの（消えたものは古い版、ほかは新しい版）
    if (pair && pair.status !== 'changed') {
      pairPane.hidden = true;
      details.root.hidden = false;
      const oid = (pair.new ?? pair.old)!.oid;
      if (oid !== wanted) {
        wanted = oid;
        selectTimer = window.setTimeout(() => post({ type: 'select', oid }), 60);
      }
      return;
    }
    wanted = undefined;
    pairPane.hidden = false;
    details.root.hidden = true;
    if (!pair) {
      pairHead.replaceChildren();
      pairBody.replaceChildren(el('div', 'empty', 'Choose two versions to compare'));
      return;
    }
    const note = el('div', 'range-pair-note', `${short(pair.old?.oid)} → ${short(pair.new?.oid)} · the colored lines changed between the two versions`);
    pairHead.replaceChildren(el('div', 'range-pair-subject', pair.subject), note);
    pairBody.replaceChildren(...renderDiffBody(rangeDiffFile(pair.diff)));
    pairBody.scrollTop = 0;
  }

  function select(index: number, reveal: boolean): void {
    if (index < 0 || index >= shown.length) return;
    selected = index;
    if (reveal) {
      const top = index * ROW_HEIGHT;
      if (top < list.scrollTop) list.scrollTop = top;
      else if (top + ROW_HEIGHT > list.scrollTop + list.clientHeight) list.scrollTop = top + ROW_HEIGHT - list.clientHeight;
    }
    render();
    showPair(shown[index]);
  }

  /** Changes Only に合わせて一覧を作り直す。選んでいた組を選んだままにし、なければ最初の変わった組を選ぶ */
  function filter(before = shown[selected]): void {
    shown = changesOnly.checked ? pairs.filter((p) => p.status !== 'same') : pairs;
    const same = (p: RangePair) => p.old?.oid === before?.old?.oid && p.new?.oid === before?.new?.oid;
    let index = before ? shown.findIndex(same) : -1;
    if (index < 0) index = shown.findIndex((p) => p.status !== 'same');
    if (index < 0) index = shown.length > 0 ? 0 : -1;
    selected = -1;
    if (index >= 0) select(index, true);
    else {
      render();
      showPair(undefined);
    }
  }

  function onRangeDiff(message: Extract<HostMessage, { type: 'rangeDiff' }>): void {
    // 書いている途中の範囲は消さない（結果が届いたときだけ入れ直す）
    if (document.activeElement !== field || message.pairs === null) field.value = message.ranges.map(shortRange).join(' ');
    summary.textContent = message.error ?? [message.summary, message.time ? formatDate(message.time) : ''].filter(Boolean).join(' · ');
    summary.title = message.ranges.length > 0 ? `git range-diff ${message.ranges.join(' ')}` : '';
    summary.classList.toggle('error', !!message.error);
    if (message.pairs === null) {
      counts.textContent = message.ranges.length > 0 ? 'Comparing…' : '';
      return;
    }
    const before = shown[selected];
    pairs = message.pairs;
    const tally = new Map<RangeStatus, number>();
    for (const p of pairs) tally.set(p.status, (tally.get(p.status) ?? 0) + 1);
    counts.textContent =
      pairs.length === 0
        ? message.ranges.length > 0 && !message.error
          ? 'No commits to compare'
          : ''
        : (['same', 'changed', 'removed', 'added'] as const).flatMap((s) => (tally.has(s) ? [`${tally.get(s)} ${s}`] : [])).join(' · ');
    filter(before);
  }

  function rowIndex(event: Event): number | undefined {
    const row = (event.target as HTMLElement).closest<HTMLElement>('.row');
    return row ? Number(row.dataset.index) : undefined;
  }

  list.addEventListener('mousedown', (e) => {
    const index = rowIndex(e);
    if (index !== undefined) select(index, false);
  });
  list.addEventListener('keydown', (e) => {
    const page = Math.max(1, Math.floor(list.clientHeight / ROW_HEIGHT) - 1);
    const moves: Record<string, number> = { ArrowDown: 1, ArrowUp: -1, PageDown: page, PageUp: -page };
    if (e.key in moves) {
      e.preventDefault();
      select(Math.min(shown.length - 1, Math.max(0, selected + moves[e.key]!)), true);
    }
  });
  const run = () => {
    const ranges = field.value.split(/\s+/).filter(Boolean);
    if (ranges.length > 0) post({ type: 'rangeDiffCompare', ranges });
  };
  compare.addEventListener('click', run);
  field.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      run();
    }
  });
  close.addEventListener('click', () => post({ type: 'rangeDiffClose' }));
  changesOnly.addEventListener('change', () => filter());

  showPair(undefined);
  return {
    root,
    onMessage(message: HostMessage) {
      switch (message.type) {
        case 'rangeDiff':
          onRangeDiff(message);
          break;
        case 'details':
          if (root.classList.contains('active') && message.details.oid === wanted) {
            details.show(message.details, undefined, message.viewed);
          }
          break;
        case 'commitDiff':
          if (root.classList.contains('active')) details.showDiff(message.oid, message.path, message.diff);
          break;
        case 'detailsExtras':
          if (root.classList.contains('active')) details.showExtras(message.extras);
          break;
      }
    },
    onShow() {
      // 見えていない間に届いた詳細は出していないので、選んでいる組の中身を求め直す
      details.forget();
      wanted = undefined;
      post({ type: 'rangeDiffLoad' });
      list.focus();
    },
  };
}
