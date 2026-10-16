/**
 * 書き込みを共有の daemon に任せたとき、daemon のコアが Undo を記録したかを、拡張の Undo の記録に伝える。
 * コアと拡張は同じ `undo.json` に書くので、どちらか一方だけが記録する。vscode に依存しない
 */
import { AsyncLocalStorage } from 'node:async_hooks';

const store = new AsyncLocalStorage<{ recorded: boolean }>();

/** fn を実行し、その中で daemon が Undo を記録したかを返す */
export async function recordedElsewhere<T>(fn: () => Promise<T>): Promise<{ result: T; recorded: boolean }> {
  const state = { recorded: false };
  const result = await store.run(state, fn);
  return { result, recorded: state.recorded };
}

/** daemon のコアが Undo を記録した（daemon に任せた書き込みから呼ぶ） */
export function markRecorded(): void {
  const state = store.getStore();
  if (state) {
    state.recorded = true;
  }
}
