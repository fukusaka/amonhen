/** Working Copy の変更を、Conflicts / Staged / Changes に振り分ける。vscode に依存しない */
import type { StatusEntry } from '../git/status.ts';

export type ChangeGroup = 'conflicts' | 'staged' | 'unstaged';

export interface Change {
  group: ChangeGroup;
  path: string;
  origPath?: string;
  /** バッジに出す 1 文字 */
  letter: string;
  /** コンフリクトしているときの XY（UU、DU など） */
  conflict?: string;
}

/** status のエントリを Conflicts / Staged / Changes に振り分ける。1 つのファイルが Staged と Changes の両方に出ることもある */
export function groupChanges(entries: readonly StatusEntry[]): [ChangeGroup, Change[]][] {
  const conflicts: Change[] = [];
  const staged: Change[] = [];
  const unstaged: Change[] = [];
  for (const e of entries) {
    switch (e.kind) {
      case 'unmerged':
        conflicts.push({ group: 'conflicts', path: e.path, letter: '!', conflict: e.index + e.worktree });
        break;
      case 'untracked':
        unstaged.push({ group: 'unstaged', path: e.path, letter: 'U' });
        break;
      case 'changed':
      case 'renamed':
        if (e.index !== '.') {
          staged.push({ group: 'staged', path: e.path, origPath: e.origPath, letter: e.index });
        }
        if (e.worktree !== '.') {
          unstaged.push({ group: 'unstaged', path: e.path, letter: e.worktree });
        }
        break;
    }
  }
  return [
    ['conflicts', conflicts],
    ['staged', staged],
    ['unstaged', unstaged],
  ];
}
