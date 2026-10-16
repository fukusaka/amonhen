/** Stash の保存・適用・pop・削除と、中身の表示 */
import { join } from 'node:path';
import * as vscode from 'vscode';
import { parseCommitObject } from '../git/commitObject.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import { gitUri } from './uri.ts';
import type { Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';

type StashNode = Extract<Node, { kind: 'stash' }>;

export function registerStashCommands(manager: RepositoryManager): vscode.Disposable {
  const withRepo = (fn: (repo: Repository) => Promise<void>) => async () => {
    const repo = manager.current;
    if (!repo) return;
    try {
      await fn(repo);
    } catch (err) {
      showError(err);
    } finally {
      await repo.refresh();
    }
  };
  const onStash = (id: string, fn: (repo: Repository, node: StashNode) => Promise<void>) =>
    vscode.commands.registerCommand(id, (node?: Node) =>
      node?.kind === 'stash' ? withRepo((repo) => fn(repo, node))() : undefined,
    );

  return vscode.Disposable.from(
    vscode.commands.registerCommand(
      'amonhen.stash',
      withRepo(async (repo) => {
        const kind = await vscode.window.showQuickPick(
          [
            { label: 'Stash Changes', includeUntracked: false },
            { label: 'Stash Changes Including Untracked Files', includeUntracked: true },
          ],
          { placeHolder: 'stash する変更' },
        );
        if (!kind) return;
        const message = await vscode.window.showInputBox({ title: 'Stash', prompt: 'メッセージ（空でもよい）' });
        if (message === undefined) return;
        await repo.backend.stashPush({ message: message.trim() || undefined, includeUntracked: kind.includeUntracked });
      }),
    ),
    onStash('amonhen.stash.apply', (repo, node) => apply(repo, node, false)),
    onStash('amonhen.stash.pop', (repo, node) => apply(repo, node, true)),
    onStash('amonhen.stash.drop', async (repo, node) => {
      const choice = await vscode.window.showWarningMessage(
        `${node.stash.name}（${node.stash.subject}）を削除しますか？`,
        { modal: true, detail: 'Undo で戻せます。' },
        'Drop',
      );
      if (choice === 'Drop') {
        await repo.undo.recordStashDrop(node.stash.name, node.stash.oid, node.stash.subject);
      }
    }),
    onStash('amonhen.stash.show', (repo, node) => showStash(repo, node)),
  );
}

async function apply(repo: Repository, node: StashNode, pop: boolean): Promise<void> {
  const result = await repo.backend.stashApply(node.stash.name, pop);
  if (result === 'conflict') {
    const choice = await vscode.window.showWarningMessage(
      `${node.stash.name} を当てたところ、コンフリクトが発生しました。${pop ? 'stash は残っています。' : ''}`,
      'Show Working Copy',
    );
    if (choice) await vscode.commands.executeCommand('amonhen.showWorkingCopy');
  }
}

/**
 * stash の中身（作業中だった変更）を、複数ファイル diff で開く。
 * 追跡しているファイルは stash を作ったときの HEAD（1 つめの親）との差分、
 * untracked のファイル（`-u` で保存したもの。3 つめの親）は追加として出す
 */
async function showStash(repo: Repository, node: StashNode): Promise<void> {
  const obj = await repo.backend.object(node.stash.oid);
  if (!obj) return;
  const [base, , untracked] = parseCommitObject(obj.data.toString('utf8')).parents;
  const root = repo.root;
  const resources: vscode.Uri[][] = [];
  for (const f of await repo.backend.commitFiles(node.stash.oid, base)) {
    const original =
      f.status === 'A' || !base ? gitUri(root, f.path, 'empty') : gitUri(root, f.origPath ?? f.path, 'rev', base);
    const modified = f.status === 'D' ? gitUri(root, f.path, 'empty') : gitUri(root, f.path, 'rev', node.stash.oid);
    resources.push([vscode.Uri.file(join(root, f.path)), original, modified]);
  }
  if (untracked) {
    for (const f of await repo.backend.commitFiles(untracked, undefined)) {
      resources.push([
        vscode.Uri.file(join(root, f.path)),
        gitUri(root, f.path, 'empty'),
        gitUri(root, f.path, 'rev', untracked),
      ]);
    }
  }
  await vscode.commands.executeCommand('vscode.changes', `${node.stash.name} ${node.stash.subject}`, resources);
}
