/**
 * stage / unstage の結果を git を待たずに予測する（楽観的更新）。
 * 直後の refresh で正しい値に置き換わるので、細かいケースは近似でよい。
 */
import type { StatusEntry } from '../git/status.ts';

export function predictStage(entries: readonly StatusEntry[], paths: ReadonlySet<string>): StatusEntry[] {
  return entries.map((e) => {
    if (!paths.has(e.path)) {
      return e;
    }
    switch (e.kind) {
      case 'untracked':
        return { kind: 'changed', path: e.path, index: 'A', worktree: '.' };
      case 'unmerged':
        return { kind: 'changed', path: e.path, index: 'M', worktree: '.' };
      case 'changed':
      case 'renamed':
        return { ...e, index: e.index === '.' ? e.worktree : e.index, worktree: '.' };
      default:
        return e;
    }
  });
}

export function predictUnstage(entries: readonly StatusEntry[], paths: ReadonlySet<string>): StatusEntry[] {
  const result: StatusEntry[] = [];
  for (const e of entries) {
    // rename の unstage は 2 つのエントリに分かれるので予測しない
    if (!paths.has(e.path) || e.kind !== 'changed') {
      result.push(e);
      continue;
    }
    if (e.index === 'A') {
      if (e.worktree !== 'D') {
        result.push({ kind: 'untracked', path: e.path, index: '?', worktree: '?' });
      }
      continue;
    }
    result.push({ ...e, index: '.', worktree: e.worktree === '.' ? e.index : e.worktree });
  }
  return result;
}
