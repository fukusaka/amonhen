/** History の行に付ける情報。vscode に依存しない */
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
 * 非対話の rebase で編集できるコミット。
 * HEAD から first-parent で辿り、最初の merge コミットの手前まで（merge をまたぐ rebase は扱わない）。
 */
export function editableOids(firstParentChain: readonly Commit[]): Set<string> {
  const result = new Set<string>();
  for (const c of firstParentChain) {
    if (c.parents.length > 1) {
      break;
    }
    result.add(c.oid);
  }
  return result;
}
