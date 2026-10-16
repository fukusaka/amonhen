/** History でのコミットの編集。非対話の rebase で行い、Undo できるように記録する */
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import type { RebaseResult, RepositoryBackend } from '../git/backend.ts';
import { planRebase, type HistoryEdit } from '../history/rebasePlan.ts';
import type { UndoLog } from './undo.ts';

/** 編集できる範囲を調べるときに辿るコミットの数 */
const CHAIN_LIMIT = 500;

export async function rewriteHistory(
  backend: RepositoryBackend,
  undo: UndoLog,
  label: string,
  edit: HistoryEdit,
): Promise<RebaseResult> {
  const head = await backend.head();
  if (!head.oid) {
    throw new Error('まだコミットがありません');
  }
  if (await backend.sequenceInProgress()) {
    throw new Error('merge や rebase の途中です。先に終わらせるか中止してください');
  }
  // HEAD から first-parent で辿り、最初の merge の手前まで
  const log = await backend.log(['--first-parent', 'HEAD'], 0, CHAIN_LIMIT);
  const end = log.findIndex((c) => c.parents.length > 1);
  const chain = end < 0 ? log : log.slice(0, end);

  const dir = join(backend.dirs.gitDir, 'amonhen');
  const plan = planRebase(chain, edit, (name) => join(dir, name));
  await mkdir(dir, { recursive: true });
  await Promise.all([...plan.messages].map(([name, message]) => writeFile(join(dir, name), message)));
  const todo = join(dir, 'rebase-todo');
  await writeFile(todo, plan.todo);

  return undo.record(label, head.branch ? [head.branch] : [], 'keep', () => backend.rebaseTodo(plan.base, todo), {
    skip: (r) => r === 'conflict',
    pending: (r) => r === 'stopped',
  });
}
