/**
 * 拡張の git 実行層の書き込みを、共有の daemon のコアの命令に置き換える（設定 `amonhen.daemon.path` があるとき）。
 * 読み取りと、拡張に固有の手順（untracked を消す破棄・index の書き込み・rebase の todo）は git 実行層のまま。
 * コアが Undo を記録する操作は、拡張の側では記録しない（`delegation.ts`）。vscode に依存しない
 */
import type { MergeResult, PushOptions, RebaseResult, RepositoryBackend, Sequence } from '../git/backend.ts';
import type { DaemonClient, Json } from './client.ts';
import { markRecorded } from './delegation.ts';

type Overrides = Partial<Record<keyof RepositoryBackend, (...args: never[]) => Promise<unknown>>>;

/** inner の書き込みを daemon（client が返すつながり）に任せる backend。client が undefined を返せば inner のまま */
export function withDaemonWrites(
  inner: RepositoryBackend,
  client: () => Promise<DaemonClient | undefined>,
): RepositoryBackend {
  /** コアの命令（`run`）を実行する。recorded ならコアが Undo を記録する操作 */
  const run = async (c: DaemonClient, command: Json, recorded: boolean): Promise<Json> => {
    const reply = (await c.call({ op: 'run', command })).json;
    if (reply['ok'] !== true) {
      throw new Error(String(reply['error'] ?? `${String(command['op'])} に失敗しました`));
    }
    if (recorded) {
      markRecorded();
    }
    return reply;
  };
  const outcome = (reply: Json): RebaseResult => {
    const o = String(reply['outcome'] ?? 'done').toLowerCase();
    return o === 'conflict' ? 'conflict' : o === 'stopped' ? 'stopped' : 'done';
  };
  // 名前ごとの置き換え。daemon を使えなければ inner を呼ぶ
  const via =
    <A extends unknown[], R>(name: keyof RepositoryBackend, fn: (c: DaemonClient, ...args: A) => Promise<R>) =>
    async (...args: A): Promise<R> => {
      const c = await client();
      if (!c) {
        return (inner[name] as unknown as (...a: A) => Promise<R>).apply(inner, args);
      }
      return fn(c, ...args);
    };
  const overrides: Overrides = {
    switchBranch: via('switchBranch', async (c, branch: string) => void (await run(c, { op: 'checkout', branch }, false))),
    trackBranch: via('trackBranch', async (c, remoteBranch: string) => void (await run(c, { op: 'track', remoteBranch }, false))),
    deleteBranch: via('deleteBranch', async (c, name: string, force: boolean) => void (await run(c, { op: 'deleteBranch', name, force }, true))),
    createBranch: via('createBranch', async (c, name: string, start: string, checkout: boolean) =>
      void (await run(c, { op: 'createBranch', name, start, checkout }, true)),
    ),
    renameBranch: via('renameBranch', async (c, from: string, to: string) => void (await run(c, { op: 'renameBranch', from, to }, true))),
    createTag: via('createTag', async (c, name: string, target: string, message?: string) =>
      void (await run(c, { op: 'createTag', name, target, message: message ?? null }, true)),
    ),
    cherryPick: via('cherryPick', async (c, oid: string, isMerge: boolean) => outcome(await run(c, { op: 'cherryPick', oid, isMerge }, true))),
    revert: via('revert', async (c, oid: string, isMerge: boolean) => outcome(await run(c, { op: 'revert', oid, isMerge }, true))),
    merge: via('merge', async (c, rev: string) => outcome(await run(c, { op: 'merge', rev }, true)) as MergeResult),
    rebase: async (onto: string, branch?: string): Promise<MergeResult> => {
      const c = await client();
      // ほかのブランチの rebase（checkout してから）は git 実行層で
      if (!c || branch) {
        return inner.rebase(onto, branch);
      }
      return outcome(await run(c, { op: 'rebase', onto }, true)) as MergeResult;
    },
    updateSubmodules: via('updateSubmodules', async (c, paths: readonly string[]) =>
      void (await run(c, { op: 'updateSubmodules', paths }, false)),
    ),
    stashPush: via('stashPush', async (c, opts: { message?: string; includeUntracked?: boolean }) =>
      void (await run(c, { op: 'stashPush', message: opts.message ?? null, includeUntracked: !!opts.includeUntracked }, false)),
    ),
    stashApply: via('stashApply', async (c, name: string, pop: boolean) => outcome(await run(c, { op: 'stashApply', name, pop }, false)) as MergeResult),
    stashDrop: via('stashDrop', async (c, name: string) => void (await run(c, { op: 'stashDrop', name }, true))),
    abort: via('abort', async (c, _sequence: Sequence) => void (await run(c, { op: 'abort' }, false))),
    continueSequence: via('continueSequence', async (c, _sequence: Sequence) => outcome(await run(c, { op: 'continue' }, false))),
    skipSequence: via('skipSequence', async (c, _sequence: Sequence) => outcome(await run(c, { op: 'skip' }, false))),
    fetch: via('fetch', async (c) => void (await run(c, { op: 'fetch' }, false))),
    push: via('push', async (c, remote: string, branch: string, remoteBranch: string, opts?: PushOptions) =>
      void (await run(c, { op: 'push', remote, branch, remoteBranch, force: !!opts?.force, setUpstream: !!opts?.setUpstream }, false)),
    ),
    commit: via('commit', async (c, message: string, opts?: { amend?: boolean }) => {
      await c.call({ op: 'commit', message, amend: !!opts?.amend });
      markRecorded();
    }),
    stage: via('stage', async (c, paths: readonly string[]) => void (await c.call({ op: 'stage', paths }))),
    unstage: via('unstage', async (c, paths: readonly string[]) => void (await c.call({ op: 'unstage', paths }))),
  };
  return new Proxy(inner, {
    get(target, prop, receiver) {
      const override = overrides[prop as keyof RepositoryBackend];
      if (override) {
        return override;
      }
      const value: unknown = Reflect.get(target, prop, receiver);
      return typeof value === 'function' ? (value as (...a: unknown[]) => unknown).bind(target) : value;
    },
  });
}
