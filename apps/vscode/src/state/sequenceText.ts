/**
 * merge・rebase・cherry-pick・revert・am の途中を言う文（アプリの帯と同じ）。1 行目は何をどこへしている途中か（何手目
 * か）、2 行目はどこで・なぜ止まっていて、次に何をすればよいか。vscode に依存しない
 */

/** コアの `sequence` の info */
export interface SequenceInfo {
  kind: string;
  branch?: string;
  onto?: string;
  from?: string;
  merging?: string[];
  current?: number;
  total?: number;
  step?: { action?: string; subject?: string; oid?: string };
  editing?: boolean;
  threeWay?: boolean;
  retried?: boolean;
  empty?: boolean;
}

export function sequenceTitle(s: SequenceInfo): string {
  const from = s.merging && s.merging.length > 0 ? s.merging.join(', ') : s.from;
  const target = s.onto ?? from;
  let title: string;
  if (s.kind === 'rebase') title = s.branch && target ? `Rebasing ${s.branch} onto ${target}` : 'Rebase in progress';
  else if (s.kind === 'merge') title = s.branch && from ? `Merging ${from} into ${s.branch}` : 'Merge in progress';
  else if (s.kind === 'cherry-pick') title = s.branch ? `Cherry-picking into ${s.branch}` : 'Cherry-pick in progress';
  else if (s.kind === 'squash') title = s.branch && from ? `Squashing ${from} into ${s.branch}` : 'Squash in progress';
  else if (s.kind === 'am') title = s.branch ? `Applying patches to ${s.branch}` : 'Applying patches';
  else title = s.branch ? `Reverting in ${s.branch}` : 'Revert in progress';
  return s.current !== undefined && s.total !== undefined ? `${title} · step ${s.current} of ${s.total}` : title;
}

/** withoutMarkers は、コンフリクトのうちマーカーの残っていないファイルの数（アプリの帯と同じく数を添える） */
export function sequenceDetail(s: SequenceInfo, conflicts: number, withoutMarkers = 0): string {
  const subject = s.step?.subject ? `“${s.step.subject}”` : undefined;
  const files = (n: number) => `${n} ${n === 1 ? 'file has' : 'files have'} conflicts`;
  if (s.step?.action === 'exec' && conflicts === 0) return `The command ${subject ?? ''} failed. Fix the problem, then Continue.`;
  if (conflicts > 0) {
    const where = `${files(conflicts)}${subject ? ` at ${subject}` : ''}${withoutMarkersNote(withoutMarkers)}`;
    return `${where}. Resolve them in Working Copy, then Continue.`;
  }
  if (s.editing) return `Stopped to edit ${subject ?? 'the commit'}. Change files or amend the commit, then Continue.`;
  if (s.kind === 'am' && !s.retried) {
    const patch = subject ?? 'The patch';
    if (s.empty) return `${patch} has no changes (a cover letter?). Skip it.`;
    return s.threeWay
      ? `${patch} doesn’t apply. Apply it by hand and Continue, or Skip it.`
      : `${patch} doesn’t apply. Retry it with a 3-way merge to resolve conflicts, apply it by hand and Continue, or Skip it.`;
  }
  return 'Conflicts resolved. Continue to finish, or Abort to go back to where you started.';
}

/**
 * サイドバーの途中の行の説明の頭に付ける数（なければ空）。行の説明は長く末尾が切れるので、数は前に出す
 */
export function withoutMarkersLead(withoutMarkers: number): string {
  return withoutMarkers > 0 ? `${withoutMarkers} without markers · ` : '';
}

/** マーカーの残っていないコンフリクトの数の注記（なければ空） */
export function withoutMarkersNote(withoutMarkers: number): string {
  return withoutMarkers > 0 ? ` (${withoutMarkers} without markers)` : '';
}
