/**
 * `git range-diff`（同じ一連のコミットの 2 つの版の比べ合わせ）の結果の形と、読み取り・文字列にする処理。
 * コアの `rangeDiff`・`previousVersion` の答えと同じ形にする（daemon を使わないときは、拡張の git 実行層で作る）。
 * vscode に依存しない
 */

export type RangeStatus = 'same' | 'changed' | 'removed' | 'added';

export interface RangeCommit {
  /** 範囲の中の番号（1 始まり。古い順） */
  index: number;
  oid: string;
}

export interface RangePair {
  status: RangeStatus;
  old: RangeCommit | null;
  new: RangeCommit | null;
  subject: string;
  /** 差分の差分（changed のときだけ）。先頭の 1 文字が外側の記号（` `・`-`・`+`、見出しは `@`） */
  diff: string[];
}

/** ブランチの、最後に書き換える前の版（reflog から） */
export interface PreviousVersion {
  branch: string;
  /** 前の版の `<branch>@{n}` */
  selector: string;
  /** 書き換えた動きの reflog の行 */
  message: string;
  old: string;
  new: string;
  /** rebase の載せ先。わからなければ null */
  onto: string | null;
  /** range-diff に渡す範囲 */
  ranges: string[];
}

/** `git range-diff --no-color` の出力を組に分ける。見出しでない行は、前の組の差分の差分 */
export function parseRangeDiff(text: string): RangePair[] {
  const pairs: RangePair[] = [];
  for (const line of text.split('\n')) {
    if (line.startsWith('    ')) {
      pairs.at(-1)?.diff.push(line.slice(4));
      continue;
    }
    const pair = parseHeader(line);
    if (pair) pairs.push(pair);
  }
  return pairs;
}

const STATUS: Record<string, RangeStatus> = { '=': 'same', '!': 'changed', '<': 'removed', '>': 'added' };

/** `1:  abc1234 ! 1:  def5678 subject`・`-:  ------- > 2:  def5678 subject` */
function parseHeader(line: string): RangePair | undefined {
  const m = /^ *(\d+|-): +([0-9a-f]+|-+) ([=!<>]) +(\d+|-): +([0-9a-f]+|-+) ?(.*)$/.exec(line);
  if (!m) return undefined;
  const side = (index: string, oid: string): RangeCommit | null =>
    index === '-' ? null : { index: Number(index), oid };
  const status = STATUS[m[3]!]!;
  const old = side(m[1]!, m[2]!);
  const next = side(m[4]!, m[5]!);
  const ok =
    status === 'removed' ? old && !next : status === 'added' ? !old && next : old && next;
  return ok ? { status, old, new: next, subject: m[6]!, diff: [] } : undefined;
}

/** reflog の行（`rebase (finish): refs/heads/topic onto <oid>`）から、rebase の載せ先 */
export function rebaseOnto(message: string): string | undefined {
  if (!message.startsWith('rebase')) return undefined;
  const m = / onto ([0-9a-f]{7,})\s*$/.exec(message);
  return m?.[1];
}

/** 先を書き換えない reflog の行（コミット・作成・fast-forward など） */
export function reflogKeepsHistory(message: string): boolean {
  return ['commit:', 'commit (initial):', 'commit (merge):', 'branch: Created from', 'merge ', 'pull: Fast-forward', 'cherry-pick:', 'revert:'].some(
    (p) => message.startsWith(p),
  );
}

/** reflog の 1 行 */
export interface ReflogLine {
  old: string | null;
  new: string;
  message: string;
}

/**
 * 古い git（2.23 など）のふつうの rebase は、ブランチの reflog に HEAD 用の行（`rebase finished: returning to …`）を
 * 書き、載せ先を残さない。そのときは HEAD の reflog（新しい順）で同じ行を探し、それより前の
 * `rebase…: checkout <載せ先>` の行の移り先を載せ先とする
 */
export function rebaseOntoFromHead(head: readonly ReflogLine[], finished: ReflogLine): string | undefined {
  const at = head.findIndex((e) => e.new === finished.new && e.message === finished.message);
  if (at < 0) return undefined;
  for (const e of head.slice(at + 1)) {
    if (!e.message.startsWith('rebase')) return undefined;
    if (e.message.includes(': checkout ')) return e.new;
  }
  return undefined;
}

/** reflog の行（新しい順。`old` は前の行の `new`）から、最後に書き換える前の版を探す */
export async function findPreviousVersion(
  branch: string,
  entries: readonly ReflogLine[],
  isAncestor: (ancestor: string, descendant: string) => Promise<boolean>,
  resolve: (rev: string) => Promise<string | undefined>,
  headReflog?: () => Promise<readonly ReflogLine[]>,
): Promise<PreviousVersion | undefined> {
  const tip = entries[0]?.new;
  if (!tip) return undefined;
  for (const [i, e] of entries.entries()) {
    if (!e.old || e.old === e.new || reflogKeepsHistory(e.message)) continue;
    let kept: boolean;
    try {
      kept = await isAncestor(e.old, e.new);
    } catch {
      // 消えた（gc 済みの）コミットは比べられないので、そこで探すのをやめる
      break;
    }
    if (kept) continue;
    const short = rebaseOnto(e.message);
    let onto = short ? ((await resolve(short)) ?? null) : null;
    if (!short && e.message.startsWith('rebase') && headReflog) {
      onto = rebaseOntoFromHead(await headReflog(), e) ?? null;
    }
    return {
      branch,
      selector: `${branch}@{${i + 1}}`,
      message: e.message,
      old: e.old,
      new: tip,
      onto,
      ranges: onto ? [onto, e.old, tip] : [`${e.old}...${tip}`],
    };
  }
  return undefined;
}

/** git range-diff に近い形の文字列（ID は短く）。previous があれば、何と何を比べたかを頭に書く */
export function formatRangeDiff(pairs: readonly RangePair[], ranges: readonly string[], previous?: PreviousVersion): string {
  const width = String(Math.max(1, ...pairs.flatMap((p) => [p.old?.index ?? 0, p.new?.index ?? 0]))).length;
  const side = (c: RangeCommit | null) =>
    c ? `${String(c.index).padStart(width)}:  ${c.oid.slice(0, 7)}` : `${'-'.padStart(width)}:  -------`;
  const mark = { same: '=', changed: '!', removed: '<', added: '>' } as const;
  const lines: string[] = [];
  if (previous) {
    lines.push(`# ${previous.selector} → ${previous.branch}: ${previous.message}`);
  }
  lines.push(`# git range-diff ${ranges.join(' ')}`, '');
  if (pairs.length === 0) lines.push('# No commits to compare');
  for (const p of pairs) {
    lines.push(`${side(p.old)} ${mark[p.status]} ${side(p.new)} ${p.subject}`);
    for (const d of p.diff) lines.push(`    ${d}`);
  }
  return lines.join('\n') + '\n';
}
