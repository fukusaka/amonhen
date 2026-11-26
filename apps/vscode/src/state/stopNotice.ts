/**
 * 操作がコンフリクトや途中（rebase の break・edit）で止まったことを、1 か所で知る（アプリの Actions の onStop と同じ）。
 * merge・rebase・cherry-pick・revert・stash の apply・am・パッチ・History の編集・Continue・Skip のどれで止まっても、
 * 呼び出し側によらず知らせる。vscode に依存しない
 */
import type { RepositoryBackend } from '../git/backend.ts';

/** 止まりうる書き込み（結果に conflict・stopped を返すもの） */
const STOPPABLE: ReadonlySet<keyof RepositoryBackend> = new Set<keyof RepositoryBackend>([
  'merge',
  'mergeOneByOne',
  'rebase',
  'rewrite',
  'rebaseTodo',
  'cherryPick',
  'revert',
  'stashApply',
  'continueSequence',
  'skipSequence',
  'am',
  'amThreeWay',
  'applyPatch',
]);

/** 操作の結果が、コンフリクトか途中で止まったことを言うか（1 つずつの merge は `{ result }`） */
export function isStopped(value: unknown): boolean {
  if (value === 'conflict' || value === 'stopped') return true;
  return typeof value === 'object' && value !== null && (value as { result?: unknown }).result === 'conflict';
}

/** inner の止まりうる書き込みが止まったら onStop を、途中の操作をやめたら（abort が済んだら）onAbort を呼ぶ backend */
export function withStopNotice(inner: RepositoryBackend, onStop: () => void, onAbort: () => void = () => {}): RepositoryBackend {
  return new Proxy(inner, {
    get(target, prop, receiver) {
      const value: unknown = Reflect.get(target, prop, receiver);
      if (typeof value !== 'function') return value;
      const fn = value as (...a: unknown[]) => unknown;
      if (prop === 'abort') {
        return async (...args: unknown[]) => {
          const result = await fn.apply(target, args);
          onAbort();
          return result;
        };
      }
      if (!STOPPABLE.has(prop as keyof RepositoryBackend)) return fn.bind(target);
      return async (...args: unknown[]) => {
        const result = await fn.apply(target, args);
        if (isStopped(result)) onStop();
        return result;
      };
    },
  });
}
