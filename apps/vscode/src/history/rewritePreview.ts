/**
 * 書き換える前の見通し（コアの `rewritePreview`。docs/design.md の「Rewriting operations and stacked branches」）と、
 * 確認に出す文。vscode に依存しない
 */
import type { HistoryEdit } from './rebasePlan.ts';

export interface PreviewBranch {
  name: string;
  upstream?: string | null;
  /** 残る理由: `setting`（rebase.updateRefs がオフ）・`git`（git が古い）・`checkedOut`（ほかの worktree）・`off`（今回は動かさない） */
  reason?: string | null;
}

export interface RewritePreview {
  /** 一緒に動くブランチ */
  moving: PreviewBranch[];
  /** 書き換えるコミットに先頭があるのに残るブランチ */
  staying: PreviewBranch[];
  current?: string | null;
  /** 今のブランチの上流（あれば、書き換えた後の push には Force Push が要る） */
  currentUpstream?: string | null;
  /** コンフリクトしそうなコミットとファイル。調べられなければ null */
  conflicts?: { oid: string; files: string[] }[] | null;
  /** rebase.updateRefs の値（configured）と、git がそれをできるか（available、2.38 から） */
  updateRefs?: { configured: boolean; available: boolean } | null;
}

/** 何を書き換えるか: History の編集か、今のブランチを onto へ rebase すること */
export type PreviewTarget = { edit: HistoryEdit } | { onto: string };

const list = (names: readonly string[]): string =>
  names.length < 2 ? (names[0] ?? '') : `${names.slice(0, -1).join('、')}、${names.at(-1)}`;

/** 確認に出す行（ブランチ、push 済み、コンフリクト）。macOS のアプリと同じ内容 */
export function previewLines(p: RewritePreview): string[] {
  const out: string[] = [];
  const names = (bs: readonly PreviewBranch[]) => list(bs.map((b) => b.name));
  if (p.moving.length > 0) out.push(`${names(p.moving)} も一緒に動きます。`);
  const stay = (reason: string) => p.staying.filter((b) => b.reason === reason);
  const reasons: [string, string][] = [
    ['setting', 'rebase.updateRefs がオフ'],
    ['git', '動かすには git 2.38 が要ります'],
    ['checkedOut', 'ほかの worktree で checkout 中'],
    ['off', '今回は動かさない'],
  ];
  for (const [reason, why] of reasons) {
    if (stay(reason).length > 0) out.push(`${names(stay(reason))} は動きません（${why}）。`);
  }
  const pushed = [
    ...(p.currentUpstream && p.current ? [p.current] : []),
    ...p.moving.filter((b) => b.upstream).map((b) => b.name),
  ];
  if (pushed.length > 0) out.push(`${list(pushed)} は push 済みなので、あとの push には Force Push が要ります。`);
  if (p.conflicts) {
    const [first] = p.conflicts;
    if (first) {
      const files = first.files.slice(0, 3).join(', ') + (first.files.length > 3 ? ', …' : '');
      const at = first.oid.slice(0, 8);
      out.push(
        p.conflicts.length === 1
          ? `⚠︎ ${at} でコンフリクトする見込みです: ${files}`
          : `⚠︎ ${p.conflicts.length} コミットでコンフリクトする見込みです。最初は ${at}: ${files}`,
      );
    } else {
      out.push('コンフリクトしない見込みです。');
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
