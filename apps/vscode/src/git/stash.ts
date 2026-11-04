/** stash の行に出すこと（アプリの Stashes の行と同じ）。vscode に依存しない */

/**
 * stash を作ったときのブランチ。git の stash のメッセージは `WIP on <branch>: …`（メッセージなし）か
 * `On <branch>: …`（メッセージあり）。detached HEAD なら `(no branch)` なので undefined
 */
export function stashBranch(subject: string): string | undefined {
  const m = /^(?:WIP on|On) (.+?): /.exec(subject);
  return m && m[1] !== '(no branch)' ? m[1] : undefined;
}

/** 土台（作ったときの HEAD）と今の HEAD の離れ具合の一言。同じなら undefined */
export function stashBaseText(ahead: number, behind: number): string | undefined {
  if (ahead === 0 && behind === 0) return undefined;
  const parts: string[] = [];
  if (ahead > 0) parts.push(`HEAD has ${ahead} commit${ahead === 1 ? '' : 's'} since`);
  if (behind > 0) parts.push(`${behind} commit${behind === 1 ? ' is' : 's are'} not on HEAD`);
  return parts.join(', ');
}
