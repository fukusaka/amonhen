/** Range Diff: 同じ一連のコミットの 2 つの版（rebase の前と後など）を比べ、git range-diff の形の読み取り専用の文書に出す */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import { formatRangeDiff, type PreviousVersion, type RangePair } from '../git/rangeDiff.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import type { Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';
import { rangeDiffUri } from './uri.ts';

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

/** 文書の中身（`rangeDiffUri` の範囲で比べる。note は頭に書く一行） */
export async function rangeDiffDocument(repo: Repository, ranges: readonly string[], note?: string): Promise<string> {
  const text = formatRangeDiff(await rangeDiff(repo, ranges), ranges);
  return note ? `# ${note}\n${text}` : text;
}

async function open(repo: Repository, ranges: readonly string[], title: string, note?: string): Promise<void> {
  const doc = await vscode.workspace.openTextDocument(rangeDiffUri(repo.backend.root, ranges, title, note));
  await vscode.window.showTextDocument(doc, { preview: true });
}

/** 前の版と比べる。なければそう伝える */
async function compareWithPrevious(repo: Repository, branch?: string): Promise<void> {
  const previous = await previousVersion(repo, branch);
  const name = (branch ?? (await repo.backend.head()).branch ?? 'HEAD').replace(/^refs\/heads\//, '');
  if (!previous) {
    void vscode.window.showInformationMessage(`${name} hasn’t been rewritten (nothing in its reflog)`);
    return;
  }
  await open(repo, previous.ranges, name, `${previous.selector} → ${previous.branch}: ${previous.message}`);
}

export function registerRangeDiffCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.compareWithPrevious', async (target?: Node) => {
      const repo = manager.current;
      if (!repo) return;
      const branch = target && 'kind' in target && target.kind === 'branch' && !target.remote ? target.ref.name : undefined;
      await compareWithPrevious(repo, branch).catch(showError);
    }),
    vscode.commands.registerCommand('amonhen.rangeDiff', async () => {
      const repo = manager.current;
      if (!repo) return;
      // 今のブランチの前の版があれば、その範囲を入れておく
      const previous = await previousVersion(repo).catch(() => undefined);
      const value = await vscode.window.showInputBox({
        title: 'Range Diff',
        prompt: 'The ranges to compare, as for git range-diff (old...new, base old new, or old-range new-range)',
        value: previous?.ranges.join(' ') ?? '',
      });
      const ranges = value?.split(/\s+/).filter(Boolean);
      if (!ranges?.length) return;
      await open(repo, ranges, 'Range Diff').catch(showError);
    }),
  );
}
