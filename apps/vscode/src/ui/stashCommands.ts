/** Stash の保存・適用・pop・削除と、中身の表示 */
import { join } from 'node:path';
import * as vscode from 'vscode';
import { confirmOperation } from './confirm.ts';
import { daemonFor } from '../daemon/connection.ts';
import { parseCommitObject } from '../git/commitObject.ts';
import { stashBranch } from '../git/stash.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import { runEngine } from './engine.ts';
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
        // エンジンがあれば、アプリと同じく stage したものだけ（git 2.35 から）と、stage した変更を残すのも選べる
        const engine = !!(await daemonFor(repo.root).catch(() => undefined));
        const kinds: { label: string; detail?: string; includeUntracked: boolean; staged?: boolean; keepIndex?: boolean }[] = [
          { label: 'Stash Changes', includeUntracked: false },
          { label: 'Stash Changes Including Untracked Files', includeUntracked: true },
        ];
        if (engine) {
          kinds.push(
            { label: 'Stash Only the Staged Changes', detail: 'git stash --staged (git 2.35 and later)', includeUntracked: false, staged: true },
            { label: 'Stash, Keeping the Staged Changes', detail: 'git stash --keep-index', includeUntracked: false, keepIndex: true },
          );
        }
        const kind = await vscode.window.showQuickPick(kinds, { placeHolder: 'Changes to stash' });
        if (!kind) return;
        const message = await vscode.window.showInputBox({ title: 'Stash', prompt: 'Message (optional)' });
        if (message === undefined) return;
        if (kind.staged || kind.keepIndex) {
          await runEngine(
            repo,
            { op: 'stashPush', message: message.trim() || null, includeUntracked: false, staged: !!kind.staged, keepIndex: !!kind.keepIndex },
            'Stash',
          );
          return;
        }
        await repo.backend.stashPush({ message: message.trim() || undefined, includeUntracked: kind.includeUntracked });
      }),
    ),
    // Working Copy のファイルの右クリック: そのファイルの変更だけを stash する（エンジンだけ）
    vscode.commands.registerCommand('amonhen.stash.files', async (ctx?: { path?: string }) => {
      const repo = manager.current;
      if (!repo || !ctx?.path) return;
      const message = await vscode.window.showInputBox({ title: `Stash ${ctx.path}`, prompt: 'Message (optional)' });
      if (message === undefined) return;
      const untracked = (repo.current.status?.entries ?? []).some((e) => e.path === ctx.path && e.kind === 'untracked');
      try {
        await runEngine(
          repo,
          { op: 'stashPush', message: message.trim() || null, includeUntracked: untracked, paths: [ctx.path] },
          `Stash ${ctx.path}`,
        );
      } catch (err) {
        showError(err);
      } finally {
        await repo.refresh();
      }
    }),
    onStash('amonhen.stash.apply', (repo, node) => apply(repo, node, false)),
    onStash('amonhen.stash.pop', (repo, node) => apply(repo, node, true)),
    onStash('amonhen.stash.drop', async (repo, node) => {
      const choice = await vscode.window.showWarningMessage(
        `Drop ${node.stash.name} (${node.stash.subject})?`,
        { modal: true, detail: 'You can undo this.' },
        'Drop',
      );
      if (choice === 'Drop') {
        await repo.undo.recordStashDrop(node.stash.name, node.stash.oid, node.stash.subject);
      }
    }),
    onStash('amonhen.stash.show', (repo, node) => showStash(repo, node)),
    // 新しい stash を先に確かめてから pop する（アプリの Pop。ツールバーの代わりに Stashes の見出しとコマンドパレット）
    vscode.commands.registerCommand(
      'amonhen.stash.popLatest',
      withRepo(async (repo) => {
        await repo.want('stashes');
        const latest = repo.current.stashes?.[0];
        if (!latest) {
          void vscode.window.showInformationMessage('There are no stashes');
          return;
        }
        // 確かめるのは apply（Stashes の Pop と同じ文）
        await apply(repo, { kind: 'stash', stash: latest }, true);
      }),
    ),
    // stash を作ったときの HEAD からブランチを作り、そこで stash を当てて消す（git stash branch。エンジンだけ）
    onStash('amonhen.stash.branch', async (repo, node) => {
      const client = await daemonFor(repo.root);
      if (!client) throw new Error('Branch from a stash needs the Amon Hen engine (amonhen.daemon.enabled)');
      const taken = new Set((repo.current.branches ?? []).map((b) => b.name.replace(/^refs\/heads\//, '')));
      const suggested = stashBranch(node.stash.subject);
      const branch = (
        await vscode.window.showInputBox({
          title: `New Branch from ${node.stash.name}`,
          prompt: 'Creates a branch at the commit the stash was made on, checks it out, and applies and drops the stash there',
          value: suggested && !taken.has(suggested) ? suggested : '',
          validateInput: (v) => (!v.trim() ? 'Enter a branch name' : taken.has(v.trim()) ? `${v.trim()} already exists` : undefined),
        })
      )?.trim();
      if (!branch) return;
      const reply = (await client.call({ op: 'run', command: { op: 'stashBranch', name: node.stash.name, branch } })).json;
      if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? 'Can’t create the branch'));
      vscode.window.setStatusBarMessage(`$(git-branch) ${branch}`, 5000);
    }),
  );
}

async function apply(repo: Repository, node: StashNode, pop: boolean): Promise<void> {
  const ok = await confirmOperation(
    `${pop ? 'Pop' : 'Apply'} ${node.stash.name} (${node.stash.subject})?`,
    [pop ? 'Applies it to the working tree and removes it from the list. If it conflicts, the stash stays.' : 'Applies it to the working tree; the stash stays.'],
    pop ? 'Pop' : 'Apply',
  );
  if (!ok) return;
  const result = await repo.backend.stashApply(node.stash.name, pop);
  if (result === 'conflict') {
    const choice = await vscode.window.showWarningMessage(
      `Applying ${node.stash.name} left conflicts.${pop ? ' The stash is kept.' : ''}`,
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
