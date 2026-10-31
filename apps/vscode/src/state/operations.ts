/**
 * ブランチを書き換える操作。どれも Undo できるように記録する。
 * 対象のブランチが checkout されていなければ、fast-forward で済むときは update-ref だけで処理し、
 * それ以外は checkout してから実行する。
 */
import type { MergeResult, OneByOneResult, RepositoryBackend } from '../git/backend.ts';
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
        // ほかの worktree でチェックアウト中なら断る（ref だけ動かすと、その作業ツリーとずれる）
        await backend.fastForward(target, plan.sourceOid, plan.targetOid);
        return 'done';
      }
      if (!plan.checkedOut) {
        await backend.switchTo({ branch: target, oid: plan.targetOid });
      }
      // 短い名前で渡すと、merge コミットのメッセージが git の通常どおり「Merge branch 'feature'」になる
      return backend.merge(shortName(source));
    },
    { skip: (r) => r === 'conflict' },
  );
}

/** いくつかの名前（`a`、`a and b`、`a, b and c`。git の merge のメッセージと同じ形） */
export function namesLabel(names: readonly string[]): string {
  return names.length < 2 ? (names[0] ?? '') : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;
}

/**
 * revs（任意の ref）を今のブランチに merge する。oneByOne でなければ 1 つのコミットにまとめる（octopus）。
 * 1 つずつなら、コンフリクトで止まったところまでを 1 つの記録にする。どちらも Undo 1 回で戻る
 */
export async function mergeSeveral(
  backend: RepositoryBackend,
  undo: UndoLog,
  revs: readonly string[],
  oneByOne: boolean,
): Promise<OneByOneResult> {
  const head = (await backend.head()).branch;
  const refs = head ? [head] : [];
  // 短い名前で渡すと、merge コミットのメッセージが git の通常どおりになる
  const names = revs.map(shortName);
  const label = `Merge ${namesLabel(names)}`;
  if (!oneByOne) {
    const result = await undo.record(label, refs, 'keep', () => backend.merge(names), {
      skip: (r) => r === 'conflict',
    });
    return { result, remaining: [] };
  }
  const result = await undo.record(label, refs, 'keep', () => backend.mergeOneByOne(names));
  if (result.error) {
    throw result.error;
  }
  return result;
}

/**
 * target（`refs/heads/...`）を onto（任意の ref）の上に rebase する。updateRefs は積み重ねたブランチを一緒に動かすかの、
 * この操作だけの指定（なければ git の `rebase.updateRefs`）
 */
export async function rebaseOnto(
  backend: RepositoryBackend,
  undo: UndoLog,
  target: string,
  onto: string,
  updateRefs?: boolean,
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
    () => backend.rebase(onto, plan.checkedOut ? undefined : shortName(target), updateRefs),
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
 * 現在のブランチを upstream へ push する。upstream が無い（リモートで消えたものも）なら unpublished を返す。
 * 初めて送るときは送り先を選んでから publishBranch で送る
 */
export async function pushCurrent(
  backend: RepositoryBackend,
  opts: { force?: boolean } = {},
): Promise<{ remote: string; branch: string } | 'unpublished' | 'detached'> {
  const branch = (await backend.head()).branch;
  if (!branch) {
    return 'detached';
  }
  const target = await pushTarget(backend, branch);
  if (!target) {
    return 'unpublished';
  }
  await backend.push(target.remote, shortName(branch), target.branch, { force: opts.force });
  return target;
}

/** ローカルブランチ（`refs/heads/...`）の upstream のリモートとそこでの名前。upstream が無いか消えていれば undefined */
export async function pushTarget(
  backend: RepositoryBackend,
  branch: string,
): Promise<{ remote: string; branch: string } | undefined> {
  const [ref] = (await backend.refs([branch])).filter((r) => r.name === branch);
  if (!ref?.upstream || ref.gone) {
    return undefined;
  }
  return splitRemoteRef(ref.upstream, (await backend.remotes()).map((r) => r.name));
}

/**
 * 初めて送るブランチ（`refs/heads/...`）の送り先の候補。git の push と同じく branch.<name>.pushRemote・remote.pushDefault
 * の順で、無ければリモートが 1 つならそれ、いくつもあれば origin。preferred は候補に無ければ undefined
 */
export async function publishChoices(
  backend: RepositoryBackend,
  branch: string,
): Promise<{ remotes: string[]; preferred: string | undefined }> {
  const name = shortName(branch);
  const [remotes, pushRemote, pushDefault] = await Promise.all([
    backend.remotes().then((rs) => rs.map((r) => r.name)),
    backend.config(`branch.${name}.pushRemote`),
    backend.config('remote.pushDefault'),
  ]);
  const preferred = [pushRemote, pushDefault, remotes.length === 1 ? remotes[0] : 'origin'].find(
    (r) => r !== undefined && remotes.includes(r),
  );
  return { remotes, preferred };
}

/** ブランチ（`refs/heads/...`）を remote の remoteBranch へ push し、それを upstream にする */
export async function publishBranch(
  backend: RepositoryBackend,
  branch: string,
  remote: string,
  remoteBranch: string,
): Promise<void> {
  await backend.push(remote, shortName(branch), remoteBranch, { setUpstream: true });
}

/**
 * チェックアウトしていないブランチ（`refs/heads/...`）を upstream まで fast-forward する（最後の Fetch の時点）。
 * チェックアウト中なら checked-out（Pull を使う）、分かれていれば diverged
 */
export async function fastForwardBranch(
  backend: RepositoryBackend,
  undo: UndoLog,
  branch: string,
): Promise<OperationResult | 'no-upstream' | 'checked-out' | 'diverged'> {
  const upstream = await upstreamOf(backend, branch);
  if (!upstream) {
    return 'no-upstream';
  }
  const plan = await planUpdate(backend, upstream, branch);
  if (plan.kind === 'up-to-date') {
    return 'up-to-date';
  }
  if (plan.checkedOut) {
    return 'checked-out';
  }
  if (plan.kind === 'diverged') {
    return 'diverged';
  }
  return undo.record(`Fast-forward ${shortName(branch)}`, [branch], 'keep', async () => {
    await backend.fastForward(branch, plan.sourceOid, plan.targetOid);
    return 'done' as const;
  });
}
