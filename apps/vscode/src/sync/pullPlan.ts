/**
 * Pull の確認（docs/ux.md の 5 の Pull。アプリの SyncSheets.swift と同じ）の決まり。取り込み方は git の設定に従うか、
 * この pull だけ Merge・Rebase・Fast-Forward Only を選ぶ。今の状態で、選んだ取り込み方が何をするかを 1 行で言い、
 * できないとき（分かれていて Fast-Forward Only、取り込み方を何も決めていなくて git が断る）は押せない。vscode に依存しない
 */

export type PullMode = 'git' | 'merge' | 'rebase' | 'fastForwardOnly';

export const PULL_MODES: readonly PullMode[] = ['git', 'merge', 'rebase', 'fastForwardOnly'];

export const PULL_TITLES: Record<PullMode, string> = {
  git: 'Follow git’s Settings',
  merge: 'Merge',
  rebase: 'Rebase',
  fastForwardOnly: 'Fast-Forward Only',
};

export const PULL_TOOLTIPS: Record<PullMode, string> = {
  git: 'Uses this repository’s pull.rebase and pull.ff, as git pull in a terminal does.',
  merge: 'Brings the upstream commits in. When both sides have new commits, a merge commit joins them.',
  rebase: 'Puts your local commits on top of the upstream commits, keeping the History in one line.',
  fastForwardOnly: 'Only moves the branch forward. Stops without changing anything if you have local commits too.',
};

function rebaseValue(value: string | undefined): boolean | undefined {
  if (value === undefined) return undefined;
  const v = value.trim().toLowerCase();
  // true・merges・interactive（i）は rebase、false・no・0・off は merge
  return !['false', 'no', '0', 'off'].includes(v);
}

/**
 * git の設定が決める取り込み方（`branch.<name>.rebase`、`pull.rebase`、`pull.ff=only` の順）。`unset` はどれも
 * 決めていない（git 2.33.1 からは、分かれたブランチの pull を断る）
 */
export function gitPullMode(branch: string, config: ReadonlyMap<string, string>): { mode: Exclude<PullMode, 'git'>; unset: boolean } {
  const unset = ![`branch.${branch}.rebase`, 'pull.rebase', 'pull.ff'].some((key) => config.has(key));
  const rebase = rebaseValue(config.get(`branch.${branch}.rebase`)) ?? rebaseValue(config.get('pull.rebase'));
  if (rebase) return { mode: 'rebase', unset };
  if (config.get('pull.ff')?.trim().toLowerCase() === 'only') return { mode: 'fastForwardOnly', unset };
  return { mode: 'merge', unset };
}

export interface PullSituation {
  branch: string;
  /** 上流（`origin/main`）。なければ source を上流にしてから取り込む */
  upstream?: string;
  source?: string;
  /** 最後の Fetch の時点で、取り込むコミットと手元だけのコミットの数 */
  incoming: number;
  outgoing: number;
  /** 作業中の変更のあるファイルの数（untracked を除く） */
  changed: number;
  /** git の設定が決める取り込み方 */
  byGit: Exclude<PullMode, 'git'>;
  /** 取り込み方を何も決めていなくて、分かれていれば git が断る */
  gitRefusesDiverged: boolean;
}

/** git の設定に従うとき、実際に行う取り込み方 */
export function methodOf(mode: PullMode, s: PullSituation): Exclude<PullMode, 'git'> {
  return mode === 'git' ? s.byGit : mode;
}

/** 選んだ取り込み方で、何が起きるか（行ごと）と、押せないか */
export function describePull(mode: PullMode, s: PullSituation): { lines: string[]; blocked: boolean } {
  const lines: string[] = [];
  let blocked = false;
  if (!s.upstream) {
    lines.push(`Sets ${s.source ?? 'the remote branch'} as the upstream, then pulls.`);
  } else if (mode === 'git' && s.gitRefusesDiverged && s.incoming > 0 && s.outgoing > 0) {
    lines.push(
      '⚠ git won’t pull: the branches have diverged, and neither pull.rebase nor pull.ff says how to reconcile them. Choose Merge or Rebase for this pull (your settings stay as they are).',
    );
    blocked = true;
  } else if (s.incoming === 0) {
    lines.push('Nothing new as of the last Fetch (Pull fetches first).');
  } else if (s.outgoing === 0) {
    lines.push(`Moves ${s.branch} forward (fast-forward).`);
  } else {
    switch (methodOf(mode, s)) {
      case 'merge':
        lines.push(`You have ${s.outgoing} local ${s.outgoing === 1 ? 'commit' : 'commits'}, so this makes a merge commit.`);
        break;
      case 'rebase':
        lines.push(`Your ${s.outgoing} local ${s.outgoing === 1 ? 'commit is' : 'commits are'} replayed on top (their IDs change).`);
        break;
      case 'fastForwardOnly':
        lines.push(
          `⚠ Fast-Forward Only can’t pull these: you have ${s.outgoing} local ${s.outgoing === 1 ? 'commit' : 'commits'}. Choose Merge or Rebase.`,
        );
        blocked = true;
        break;
    }
  }
  if (s.changed > 0) {
    lines.push(`${s.changed} ${s.changed === 1 ? 'file has' : 'files have'} uncommitted changes. Pull stops if the incoming changes touch them.`);
  }
  if (!blocked) lines.push('You can undo this.');
  return { lines, blocked };
}

/** 上流のないブランチの取り込み元の候補: 同じ名前のリモートのブランチを先に */
export function pullSources(branch: string, remotes: readonly string[]): string[] {
  const usable = remotes.filter((name) => !name.endsWith('/HEAD') && name.includes('/'));
  const same = (name: string) => name.slice(name.indexOf('/') + 1) === branch;
  return [...usable.filter(same), ...usable.filter((name) => !same(name))];
}
