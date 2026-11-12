/**
 * 書き換える前の見通し（コアの `rewritePreview`。docs/design.md の「Rewriting operations and stacked branches」）と、
 * 確認に出す文。vscode に依存しない
 */
import type { HistoryEdit } from './rebasePlan.ts';

export interface PreviewBranch {
  name: string;
  upstream?: string | null;
  /** 上流が書き換えるコミットを持っている（書き換えた後の push には Force Push が要る）。moving だけ */
  pushed?: boolean | null;
  /** 残る理由: `setting`（rebase.updateRefs がオフ）・`git`（git が古い）・`checkedOut`（ほかの worktree）・`off`（今回は動かさない） */
  reason?: string | null;
}

export interface RewritePreview {
  /** 一緒に動くブランチ */
  moving: PreviewBranch[];
  /** 書き換えるコミットに先頭があるのに残るブランチ */
  staying: PreviewBranch[];
  current?: string | null;
  /** 今のブランチの上流 */
  currentUpstream?: string | null;
  /** 今のブランチの上流が書き換えるコミットを持っている（書き換えた後の push には Force Push が要る） */
  currentPushed?: boolean | null;
  /** コンフリクトしそうなコミットとファイル。調べられなければ null */
  conflicts?: { oid: string; files: string[] }[] | null;
  /** rebase.updateRefs の値（configured）と、git がそれをできるか（available、2.38 から） */
  updateRefs?: { configured: boolean; available: boolean } | null;
  /** 前と後を描く材料（rebase のとき） */
  drawing?: RewriteDrawing | null;
  /** History の編集の前と後を描く材料（merge を越える編集では描けないので null） */
  editDrawing?: EditDrawing | null;
}

/** History の編集の 1 つ。action は git の命令（`pick`・`reword`・`squash`・`fixup`・`drop`・`edit`） */
export interface EditStep {
  oid: string;
  subject: string;
  action: string;
  /** after だけ: まとめたコミットの数 */
  combined?: number | null;
}

/** before は書き換わるコミット、after はできるコミット（どちらも新しい順、長ければ真ん中を省く） */
export interface EditDrawing {
  before: EditStep[];
  beforeMore: number;
  beforeMoreAt?: number | null;
  after: EditStep[];
  afterMore: number;
  afterMoreAt?: number | null;
  base?: DrawnCommit | null;
  labels: { oid: string; name: string; kind: string }[];
}

export interface DrawnCommit {
  oid: string;
  subject: string;
  /** 分かれたところの author の日時（秒）。base だけ */
  time?: number | null;
}

export interface RewriteDrawing {
  /** 載せ直すコミット（古い順） */
  picks: DrawnCommit[];
  /** 省いた載せ直すコミットの数と、新しい順に並べたときの省いたところ */
  picksMore?: number | null;
  picksMoreAt?: number | null;
  /** 載せ先で進んだコミット（新しい順、先頭のいくつか）と、ほかにいくつあるか */
  onto: DrawnCommit[];
  ontoMore: number;
  /** 載せ先で進んだコミットを省いたところ */
  ontoMoreAt?: number | null;
  /** 分かれたところ */
  base?: DrawnCommit | null;
  /** コミットに付いているブランチ。kind は `local`・`remote` */
  labels: { oid: string; name: string; kind: string }[];
}

/** 何を書き換えるか: History の編集か、今のブランチを onto へ rebase すること */
export type PreviewTarget = { edit: HistoryEdit } | { onto: string };

const list = (names: readonly string[]): string =>
  names.length < 2 ? (names[0] ?? '') : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;

/** 確認に出す行（ブランチ、push 済み、コンフリクト）。macOS のアプリと同じ内容 */
export function previewLines(p: RewritePreview): string[] {
  const out: string[] = [];
  const names = (bs: readonly PreviewBranch[]) => list(bs.map((b) => b.name));
  if (p.moving.length > 0) out.push(`Also moves ${names(p.moving)}.`);
  const stay = (reason: string) => p.staying.filter((b) => b.reason === reason);
  const reasons: [string, string][] = [
    ['setting', 'rebase.updateRefs is off'],
    ['git', 'moving it needs git 2.38'],
    ['checkedOut', 'checked out in another worktree'],
    ['off', 'not moved this time'],
  ];
  for (const [reason, why] of reasons) {
    const staying = stay(reason);
    if (staying.length > 0) out.push(`${names(staying)} ${staying.length === 1 ? 'stays' : 'stay'} where ${staying.length === 1 ? 'it is' : 'they are'} (${why}).`);
  }
  const pushed = [
    ...(p.currentPushed && p.current ? [p.current] : []),
    ...p.moving.filter((b) => b.pushed).map((b) => b.name),
  ];
  if (pushed.length > 0) {
    out.push(`${list(pushed)} ${pushed.length === 1 ? 'is' : 'are'} pushed, so pushing afterwards needs Force Push.`);
  }
  if (p.conflicts) {
    const [first] = p.conflicts;
    if (first) {
      const files = first.files.slice(0, 3).join(', ') + (first.files.length > 3 ? ', …' : '');
      const at = first.oid.slice(0, 8);
      out.push(
        p.conflicts.length === 1
          ? `⚠︎ Conflicts expected at ${at}: ${files}`
          : `⚠︎ Conflicts expected at ${p.conflicts.length} commits, first at ${at}: ${files}`,
      );
    } else {
      out.push('No conflicts expected.');
    }
  }
  return out;
}

/**
 * この操作だけ、積み重ねたブランチの扱いを設定と逆にできるなら、その値（true: 動かす、false: 動かさない）。
 * git が古いか、扱いの変わるブランチがなければ undefined
 */
export function stackedAlternative(p: RewritePreview): boolean | undefined {
  if (!p.updateRefs?.available) return undefined;
  const affected = p.moving.length + p.staying.filter((b) => b.reason === 'setting' || b.reason === 'off').length;
  return affected > 0 ? !p.updateRefs.configured : undefined;
}

/**
 * rebase の前と後を文で描く（アプリの RewriteDrawingView と同じ中身）。○ は載せ直すコミット、● はそのまま。後では
 * 今のブランチと一緒に動くブランチの札が載せ直したコミット（′）に移り、載せ先の札はそのまま
 */
export function drawingLines(p: RewritePreview): string[] {
  const d = p.drawing;
  if (!d || d.picks.length === 0) return [];
  const moving = new Set(p.moving.map((b) => b.name));
  const labelsOf = (oid: string, keep: (name: string) => boolean = () => true) =>
    d.labels.filter((l) => l.oid === oid && keep(l.name)).map((l) => l.name);
  const row = (mark: string, subject: string, labels: readonly string[]) =>
    `  ${mark} ${subject}${labels.length > 0 ? `  [${labels.join(', ')}]` : ''}`;
  const picks = [...d.picks].reverse();
  const tail = (keep: (name: string) => boolean) => [
    ...d.onto.map((c) => row('●', c.subject, labelsOf(c.oid, keep))),
    ...(d.ontoMore > 0 ? [`  ⋮ ${d.ontoMore} more`] : []),
    ...(d.base ? [row('●', d.base.subject, labelsOf(d.base.oid, keep))] : []),
  ];
  const stays = (name: string) => name !== p.current && !moving.has(name);
  return [
    'Now:',
    ...picks.map((c) => row('○', c.subject, labelsOf(c.oid))),
    ...tail(() => true),
    'After the rebase:',
    ...picks.map((c, i) =>
      row('○', `${c.subject}′`, [
        ...(i === 0 && p.current ? [p.current] : []),
        ...d.labels.filter((l) => l.oid === c.oid && l.kind === 'local' && moving.has(l.name)).map((l) => l.name),
      ]),
    ),
    ...tail(stays),
  ];
}

/**
 * merge したときのコンフリクトの見込み（`predictMerge` の答え）を 1 行に（アプリの確認の見込みの行と同じ）。
 * `estimate` なら rebase のように 1 つずつ載せるので目安と添える
 */
export function forecastLine(steps: readonly (readonly string[])[] | undefined, estimate: boolean): string {
  if (!steps) return 'Can’t check for conflicts ahead (needs git 2.38 or newer).';
  const conflicts = [...new Set(steps.flat())].sort();
  const names = conflicts.slice(0, 3).join(', ') + (conflicts.length > 3 ? ', …' : '');
  const line = conflicts.length === 0 ? 'No conflicts expected.' : `⚠︎ Conflicts expected in ${conflicts.length} ${conflicts.length === 1 ? 'file' : 'files'}: ${names}`;
  return estimate ? `${line} Rebase applies the commits one at a time, so this is only an estimate.` : line;
}
