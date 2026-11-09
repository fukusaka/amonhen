/**
 * メインビューの Range Diff の拡張ホスト側。比べる範囲で組を求め、Webview に送る。範囲はコアの `previousVersion`
 * （前の版）か、History・Reflog・入力欄から。コアを使えなければ拡張の git 実行層で比べる
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { PreviousVersion, RangePair } from '../git/rangeDiff.ts';
import type { RangeDiffHostMessage, RangeDiffWebviewMessage } from '../rangeDiff/protocol.ts';
import type { Repository } from '../state/repository.ts';

/** 共有の daemon のコアの命令（使えなければ undefined） */
async function runInCore(repo: Repository, command: Record<string, unknown>): Promise<Record<string, unknown> | undefined> {
  const client = await daemonFor(repo.backend.root).catch(() => undefined);
  if (!client) return undefined;
  const reply = (await client.call({ op: 'run', command })).json;
  if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `${String(command['op'])} failed`));
  return (reply['info'] as Record<string, unknown> | undefined) ?? {};
}

/** branch（完全な名前。省略時は今のブランチ）の、最後に書き換える前の版 */
export async function previousVersion(repo: Repository, branch?: string): Promise<PreviousVersion | undefined> {
  const info = await runInCore(repo, { op: 'previousVersion', branch });
  if (info) return (info['previous'] as PreviousVersion | null) ?? undefined;
  return repo.backend.previousVersion(branch);
}

/** ranges（git range-diff と同じ形）で比べた組 */
export async function rangeDiff(repo: Repository, ranges: readonly string[]): Promise<RangePair[]> {
  const info = await runInCore(repo, { op: 'rangeDiff', ranges });
  if (info) return info['pairs'] as RangePair[];
  return repo.backend.rangeDiff(ranges);
}

/** Range Diff を開いている（サイドバーの行を出す）かが変わった */
const shownEmitter = new vscode.EventEmitter<boolean>();
export const onDidChangeRangeDiffShown = shownEmitter.event;

type State = Omit<Extract<RangeDiffHostMessage, { type: 'rangeDiff' }>, 'type'>;

/** メインビューの Range Diff の拡張ホスト側 */
export class RangeDiffController implements vscode.Disposable {
  private readonly post: (message: RangeDiffHostMessage) => void;
  /** 閉じたら History に戻る */
  private readonly closed: () => void;
  private repo: Repository | undefined;
  /** 開いていれば、今の結果 */
  private state: State | undefined;
  /** 比べるたびに増やし、古い答えを捨てる */
  private generation = 0;

  constructor(post: (message: RangeDiffHostMessage) => void, closed: () => void) {
    this.post = post;
    this.closed = closed;
  }

  get shown(): boolean {
    return this.state !== undefined;
  }

  setRepository(repo: Repository | undefined): void {
    // 別のリポジトリに移ったら閉じる（結果は前のリポジトリのもの）
    if (this.repo && repo?.root !== this.repo.root) this.close();
    this.repo = repo;
  }

  async handle(message: RangeDiffWebviewMessage): Promise<void> {
    switch (message.type) {
      case 'rangeDiffLoad':
        this.send();
        break;
      case 'rangeDiffCompare':
        await this.open(message.ranges);
        break;
      case 'rangeDiffClose':
        this.close();
        break;
    }
  }

  /** `ranges`（git range-diff と同じ形）で比べる */
  async open(ranges: readonly string[], summary = '', time?: number): Promise<void> {
    const generation = ++this.generation;
    this.update({ ranges: [...ranges], summary, ...(time ? { time } : {}), pairs: null });
    const repo = this.repo;
    if (!repo) return;
    try {
      const pairs = await rangeDiff(repo, ranges);
      if (generation === this.generation) this.update({ ...this.state!, pairs });
    } catch (err) {
      if (generation === this.generation) this.update({ ...this.state!, pairs: [], error: err instanceof Error ? err.message : String(err) });
    }
  }

  /** `branch`（完全な名前。省略時は今のブランチ）を、最後に書き換える前の版と比べる */
  async openPrevious(branch?: string): Promise<void> {
    const generation = ++this.generation;
    // 探している間は、前に比べた結果を残さない（別のブランチの結果に見える）
    this.update({ ranges: [], summary: 'Looking for the previous version…', pairs: null });
    const repo = this.repo;
    if (!repo) return;
    try {
      const previous = await previousVersion(repo, branch);
      if (generation !== this.generation) return;
      if (previous) {
        await this.open(previous.ranges, `${previous.selector} → ${previous.branch} · ${previous.message}`, previous.time);
      } else {
        const name = (branch ?? (await repo.backend.head()).branch ?? 'HEAD').replace(/^refs\/heads\//, '');
        this.update({ ranges: [], summary: `${name} hasn’t been rewritten (nothing in its reflog)`, pairs: [] });
      }
    } catch (err) {
      if (generation === this.generation) this.update({ ranges: [], summary: '', pairs: [], error: err instanceof Error ? err.message : String(err) });
    }
  }

  private update(state: State): void {
    const opened = !this.state;
    this.state = state;
    this.send();
    if (opened) shownEmitter.fire(true);
  }

  private send(): void {
    if (this.state) this.post({ type: 'rangeDiff', ...this.state });
  }

  /** 閉じる。サイドバーの行を消し、History に戻る */
  close(): void {
    if (!this.state) return;
    this.generation++;
    this.state = undefined;
    shownEmitter.fire(false);
    this.closed();
  }

  dispose(): void {
    if (this.state) {
      this.state = undefined;
      shownEmitter.fire(false);
    }
  }
}
