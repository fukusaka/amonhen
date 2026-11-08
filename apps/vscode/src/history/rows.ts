/** History の行に付ける情報。vscode に依存しない */
import type { Worktree } from '../git/worktrees.ts';
import type { HeadState } from '../git/backend.ts';
import type { Commit } from '../git/log.ts';
import type { Ref } from '../git/refs.ts';
import type { RefLabel } from './protocol.ts';

/** コミットごとのラベル。checkout しているブランチ、ローカル、リモート、タグの順 */
export function refLabels(
  head: HeadState,
  branches: readonly Ref[],
  remoteBranches: readonly Ref[],
  tags: readonly Ref[],
): Map<string, RefLabel[]> {
  const map = new Map<string, RefLabel[]>();
  const add = (oid: string | undefined, label: RefLabel) => {
    if (!oid) return;
    let list = map.get(oid);
    if (!list) {
      list = [];
      map.set(oid, list);
    }
    list.push(label);
  };
  if (!head.branch) {
    add(head.oid, { name: 'HEAD', kind: 'head', current: true });
  }
  for (const b of branches) {
    const current = b.name === head.branch;
    add(b.oid, current ? { name: shorten(b.name), kind: 'branch', current } : { name: shorten(b.name), kind: 'branch' });
  }
  for (const r of remoteBranches) {
    if (!r.symref) add(r.oid, { name: shorten(r.name), kind: 'remote' });
  }
  for (const t of tags) {
    add(t.peeled ?? t.oid, { name: shorten(t.name), kind: 'tag' });
  }
  for (const list of map.values()) {
    list.sort((a, b) => Number(!!b.current) - Number(!!a.current));
  }
  return map;
}

function shorten(ref: string): string {
  return ref.replace(/^refs\/(heads|remotes|tags)\//, '');
}

/**
 * 非対話の rebase で編集できるコミットと、その位置（新しい順、HEAD が 0）。
 * HEAD から first-parent で辿り、最初の merge コミットの手前まで（merge をまたぐ rebase は扱わない）。
 */
export function editableChain(firstParentChain: readonly Commit[]): Map<string, number> {
  const result = new Map<string, number>();
  for (const c of firstParentChain) {
    if (c.parents.length > 1) {
      break;
    }
    result.set(c.oid, result.size);
  }
  return result;
}

/**
 * worktree ごとの HEAD の札（アプリと同じ印）。✎ 書き込む先、⌂ メインの作業ツリー、⑂ 追加した worktree。変更があれば
 * 数を添える。worktree が 1 つなら札はいらない
 */
export function worktreeLabels(
  worktrees: readonly Worktree[],
  changes: ReadonlyMap<string, number>,
  isEditing: (w: Worktree) => boolean,
): Map<string, RefLabel[]> {
  const map = new Map<string, RefLabel[]>();
  if (worktrees.length < 2) return map;
  for (const w of worktrees) {
    if (!w.head) continue;
    const editing = isEditing(w);
    const name = w.path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? w.path;
    const count = changes.get(w.path) ?? 0;
    const label: RefLabel = {
      name: `${editing ? '✎' : w.main ? '⌂' : '⑂'} ${name}${count > 0 ? ` · ${count}` : ''}`,
      kind: 'worktree',
      ...(editing ? { current: true } : {}),
    };
    map.set(w.head, [...(map.get(w.head) ?? []), label]);
  }
  return map;
}
