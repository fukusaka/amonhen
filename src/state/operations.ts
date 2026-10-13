/**
 * ブランチを書き換える操作。どれも Undo できるように記録する。
 * 対象のブランチが checkout されていなければ、fast-forward で済むときは update-ref だけで処理し、
 * それ以外は checkout してから実行する。
 */
import type { MergeResult, RepositoryBackend } from '../git/backend.ts';
import { splitRemoteRef } from '../git/refs.ts';
import type { UndoLog } from './undo.ts';

export type OperationResult = MergeResult | 'up-to-date';

export function shortName(ref: string): string {
  return ref.replace(/^refs\/(heads|remotes|tags)\//, '');
}

/** source（任意の ref）を target（`refs/heads/...`）に merge する */
export async function mergeInto(
  backend: RepositoryBackend,
  undo: UndoLog,
  source: string,
  target: string,
): Promise<OperationResult> {
  const plan = await planUpdate(backend, source, target);
  if (plan.kind === 'up-to-date') {
    return 'up-to-date';
  }
  const label =
    plan.kind === 'fast-forward'
      ? `Fast-forward ${shortName(target)} to ${shortName(source)}`
      : `Merge ${shortName(source)} into ${shortName(target)}`;
  return undo.record(
    label,
    [target],
    'keep',
    async () => {
      if (plan.kind === 'fast-forward' && !plan.checkedOut) {
        await backend.updateRef(target, plan.sourceOid, plan.targetOid);
        return 'done';
      }
      if (!plan.checkedOut) {
        await backend.switchTo({ branch: target, oid: plan.targetOid });
      }
      return backend.merge(source);
    },
    { skip: (r) => r === 'conflict' },
  );
}

/** target（`refs/heads/...`）を onto（任意の ref）の上に rebase する */
export async function rebaseOnto(
  backend: RepositoryBackend,
  undo: UndoLog,
  target: string,
  onto: string,
): Promise<OperationResult> {
  const plan = await planUpdate(backend, onto, target);
  if (plan.kind === 'up-to-date') {
    return 'up-to-date';
  }
  if (plan.kind === 'fast-forward') {
    // target に固有のコミットが無ければ、rebase は fast-forward と同じ
    return mergeInto(backend, undo, onto, target);
  }
  return undo.record(
    `Rebase ${shortName(target)} onto ${shortName(onto)}`,
    [target],
    'keep',
    () => backend.rebase(onto, plan.checkedOut ? undefined : shortName(target)),
    { skip: (r) => r === 'conflict' },
  );
}

type Plan =
  | { kind: 'up-to-date' }
  | { kind: 'fast-forward' | 'diverged'; checkedOut: boolean; sourceOid: string; targetOid: string };

async function planUpdate(backend: RepositoryBackend, source: string, target: string): Promise<Plan> {
  const [head, values] = await Promise.all([backend.head(), backend.resolveRefs([source, target])]);
  const sourceOid = values.get(source);
  const targetOid = values.get(target);
  if (!sourceOid || !targetOid) {
    throw new Error(`${!sourceOid ? source : target} が見つかりません`);
  }
  if (sourceOid === targetOid || (await backend.isAncestor(sourceOid, targetOid))) {
    return { kind: 'up-to-date' };
  }
  const kind = (await backend.isAncestor(targetOid, sourceOid)) ? 'fast-forward' : 'diverged';
  return { kind, checkedOut: head.branch === target, sourceOid, targetOid };
}

/** 現在のブランチの upstream（`refs/remotes/...`）。無ければ undefined */
async function upstreamOf(backend: RepositoryBackend, branch: string): Promise<string | undefined> {
  const [ref] = (await backend.refs([branch])).filter((r) => r.name === branch);
  return ref?.upstream;
}

/**
 * fetch してから、現在のブランチに upstream を merge する（fast-forward で済めばそうなる）。
 * upstream が無ければ no-upstream
 */
export async function pullCurrent(backend: RepositoryBackend, undo: UndoLog): Promise<OperationResult | 'no-upstream'> {
  const branch = (await backend.head()).branch;
  const upstream = branch && (await upstreamOf(backend, branch));
  if (!branch || !upstream) {
    return 'no-upstream';
  }
  await backend.fetch();
  return mergeInto(backend, undo, upstream, branch);
}

/**
 * 現在のブランチを push する。upstream があればそこへ、無ければ remote（1 つだけなら、それか origin）へ同じ名前で push し、
 * upstream に設定する。送り先を決められなければ no-remote
 */
export async function pushCurrent(
  backend: RepositoryBackend,
  opts: { force?: boolean } = {},
): Promise<{ remote: string; branch: string } | 'no-remote' | 'detached'> {
  const branch = (await backend.head()).branch;
  if (!branch) {
    return 'detached';
  }
  const name = shortName(branch);
  const remotes = (await backend.remotes()).map((r) => r.name);
  const upstream = await upstreamOf(backend, branch);
  const split = upstream ? splitRemoteRef(upstream, remotes) : undefined;
  if (split) {
    await backend.push(split.remote, name, split.branch, { force: opts.force });
    return { remote: split.remote, branch: split.branch };
  }
  const remote = remotes.length === 1 ? remotes[0] : remotes.find((r) => r === 'origin');
  if (!remote) {
    return 'no-remote';
  }
  await backend.push(remote, name, name, { force: opts.force, setUpstream: true });
  return { remote, branch: name };
}
