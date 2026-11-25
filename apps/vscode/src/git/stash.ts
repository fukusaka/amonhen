/** stash の行に出すこと（アプリの Stashes の行と同じ）。vscode に依存しない */

/**
 * stash を作ったときのブランチ。git の stash のメッセージは `WIP on <branch>: …`（メッセージなし）か
 * `On <branch>: …`（メッセージあり）。detached HEAD なら `(no branch)` なので undefined
 */
export function stashBranch(subject: string): string | undefined {
  const m = /^(?:WIP on|On) (.+?): /.exec(subject);
  return m && m[1] !== '(no branch)' ? m[1] : undefined;
}

/**
 * 行の見出し（アプリの Stashes の行と同じ）: `-m` のメッセージ。なければ WIP と、作ったときの HEAD の件名（短い ID は
 * 外す。土台は説明に出す）
 */
export function stashTitle(subject: string): string {
  const m = /^(WIP on|On) .+?: (.*)$/s.exec(subject);
  if (!m) return subject;
  if (m[1] === 'On') return m[2]!;
  const space = m[2]!.indexOf(' ');
  return `WIP · ${space >= 0 ? m[2]!.slice(space + 1) : ''}`;
}

/** 土台（作ったときの HEAD）と今の HEAD の離れ具合の一言。同じなら undefined */
export function stashBaseText(ahead: number, behind: number): string | undefined {
  if (ahead === 0 && behind === 0) return undefined;
  const parts: string[] = [];
  if (ahead > 0) parts.push(`HEAD has ${ahead} commit${ahead === 1 ? '' : 's'} since`);
  if (behind > 0) parts.push(`${behind} commit${behind === 1 ? ' is' : 's are'} not on HEAD`);
  return parts.join(', ');
}

/** 土台と今の HEAD の関係（アプリの Stashes の行と同じ一言）。HEAD の履歴になければ、戻すときに重なりやすい */
export function stashBaseRelation(ahead: number, behind: number): string {
  if (behind > 0) return 'not in HEAD’s history';
  return ahead === 0 ? 'at HEAD' : `${ahead} behind HEAD`;
}

/** いつ（「3 days ago」。アプリの短い相対表記） */
export function relativeTime(seconds: number, now = Date.now()): string {
  const diff = seconds - now / 1000;
  const f = new Intl.RelativeTimeFormat('en', { numeric: 'always', style: 'short' });
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ['year', 365 * 86_400],
    ['month', 30 * 86_400],
    ['week', 7 * 86_400],
    ['day', 86_400],
    ['hour', 3_600],
    ['minute', 60],
  ];
  for (const [unit, size] of units) {
    if (Math.abs(diff) >= size) return f.format(Math.trunc(diff / size), unit);
  }
  return f.format(Math.trunc(diff), 'second');
}

/**
 * stash の行の説明（アプリの Stashes の行の見出しと 2 行目と同じ並び）: 見出し・作ったブランチ・土台と HEAD の関係・いつ・
 * untracked を含むか・土台の短い ID。土台と untracked はエンジンがあるときだけ
 */
export function stashDescription(
  stash: { subject: string; date: number },
  fact?: { base?: string; ahead: number; behind: number; untracked: boolean },
  now = Date.now(),
): string {
  const branch = stashBranch(stash.subject);
  return [
    stashTitle(stash.subject),
    branch,
    fact?.base ? stashBaseRelation(fact.ahead, fact.behind) : undefined,
    stash.date ? relativeTime(stash.date, now) : undefined,
    fact?.untracked ? 'with untracked files' : undefined,
    fact?.base?.slice(0, 7),
  ]
    .filter(Boolean)
    .join(' · ');
}
