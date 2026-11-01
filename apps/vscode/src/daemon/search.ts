/**
 * daemon の History の検索（`historySearch`）。構文はコアのもの（`crates/core/src/query.rs`）。vscode に依存しない
 */
import type { DaemonClient } from './client.ts';

/** 途中の答えを読みに行く間隔（ミリ秒） */
const PARTIAL_INTERVAL = 300;

/** 問い合わせに `contents:`（`content:`）があるか（引用の中の語も数えるが、尋ねすぎるだけ） */
export function searchesContents(query: string): boolean {
  return /(^|[\s(])contents?:\S/i.test(query);
}

/** partial clone で、内容の検索の前に取る、手元にない中身の数。上限を超えたら `more`（`count` は上限まで） */
export async function contentsToFetch(
  client: DaemonClient,
  history: unknown,
  query: string,
  signal?: AbortSignal,
): Promise<{ count: number; more: boolean }> {
  const reply = await client.call({ op: 'historyContentsToFetch', history, query }, signal);
  return { count: Number(reply.json['count']), more: reply.json['more'] === true };
}

/**
 * `query` に合う行（昇順）。内容の検索は大きな履歴では数分かかるので、探しているあいだ、分かった行（最後の答えの
 * 一部）と探し終えた割合（0〜1）を、数か割合が変わるたびに `progress` に渡す
 */
export async function searchHistory(
  client: DaemonClient,
  history: unknown,
  query: string,
  signal: AbortSignal,
  progress: (rows: number[], done: number) => void,
  interval = PARTIAL_INTERVAL,
): Promise<number[]> {
  const search = client.call({ op: 'historySearch', history, query }, signal);
  let finished = false;
  let last = { count: -1, done: -1 };
  const timer = setInterval(() => {
    client
      .call({ op: 'historySearchPartial', history, query })
      .then((reply) => {
        const rows = reply.u32('rows');
        const done = Number(reply.json['done']);
        if (!finished && !signal.aborted && (rows.length !== last.count || done !== last.done)) {
          last = { count: rows.length, done };
          progress(Array.from(rows), done);
        }
      })
      .catch(() => undefined);
  }, interval);
  try {
    return Array.from((await search).u32('rows'));
  } finally {
    finished = true;
    clearInterval(timer);
  }
}
