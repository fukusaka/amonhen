/**
 * submodule の操作（アプリのサイドバーの Submodules と同じ）。Update と別のウィンドウで開くは git 実行層でも、
 * Update…・URL・追うブランチ・外す・Update All・Update Submodules…・Sync URLs・Add Submodule… は共有の daemon の
 * コアで行う（エンジンがなければメニューに出さない）
 */
import { join } from 'node:path';
import * as vscode from 'vscode';
import type { Json } from '../daemon/client.ts';
import { daemonFor } from '../daemon/connection.ts';
import { folderFromUrl } from '../git/submodules.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import type { Node, SidebarProvider } from './sidebar.ts';
import { showError } from './stageCommands.ts';

/** コアの命令を実行する。うまくいかなければ投げる */
async function run(repo: Repository, command: Json, title: string): Promise<void> {
  const client = await daemonFor(repo.root);
  if (!client) throw new Error(`${title} needs the Amon Hen engine (amonhen.daemon.enabled)`);
  const reply = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title }, async () =>
    (await client.call({ op: 'run', command })).json,
  );
  if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `${title} failed`));
}

/** Update… のしかた（git の既定はどちらもしない） */
async function pickUpdateOptions(title: string): Promise<{ recursive: boolean; remote: boolean } | undefined> {
  const items = [
    { label: 'Also update the submodules inside', detail: 'git submodule update --recursive', key: 'recursive' },
    { label: 'Use the latest commit of the branch it follows', detail: 'git submodule update --remote', key: 'remote' },
  ];
  const picked = await vscode.window.showQuickPick(items, {
    title,
    canPickMany: true,
    placeHolder: 'Leave both off to check out the recorded commits (git’s default)',
  });
  if (!picked) return undefined;
  return { recursive: picked.some((p) => p.key === 'recursive'), remote: picked.some((p) => p.key === 'remote') };
}

export function registerSubmoduleCommands(manager: RepositoryManager, sidebar: SidebarProvider): vscode.Disposable {
  /** 行の submodule で fn を実行し、終わったら読み直す */
  const onRow = (id: string, fn: (repo: Repository, path: string) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (node?: Node) => {
      const repo = manager.current;
      if (!repo || node?.kind !== 'submodule') return;
      await settle(repo, () => fn(repo, node.submodule.path));
    });
  const onRepo = (id: string, fn: (repo: Repository) => Promise<void>) =>
    vscode.commands.registerCommand(id, async () => {
      const repo = manager.current;
      if (!repo) return;
      await settle(repo, () => fn(repo));
    });
  const settle = async (repo: Repository, fn: () => Promise<void>): Promise<void> => {
    try {
      await fn();
    } catch (err) {
      showError(err);
    } finally {
      await repo.refresh();
      await sidebar.submoduleFacts.reload();
    }
  };
  const fact = (path: string) => sidebar.submoduleFacts.get(path);

  return vscode.Disposable.from(
    onRow('amonhen.submodule.update', async (repo, path) => {
      await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: `Update ${path}` }, () =>
        repo.backend.updateSubmodules([path]),
      );
    }),
    onRow('amonhen.submodule.updateWith', async (repo, path) => {
      const options = await pickUpdateOptions(`Update ${path}`);
      if (!options) return;
      await run(repo, { op: 'updateSubmodules', paths: [path], ...options }, `Update ${path}`);
    }),
    vscode.commands.registerCommand('amonhen.submodule.open', async (node?: Node) => {
      const repo = manager.current;
      if (!repo || node?.kind !== 'submodule') return;
      await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(join(repo.root, node.submodule.path)), {
        forceNewWindow: true,
      });
    }),
    onRow('amonhen.submodule.editUrl', async (repo, path) => {
      const url = (
        await vscode.window.showInputBox({
          title: `URL of ${path}`,
          prompt: 'Changes .gitmodules and this repository’s settings (git submodule set-url)',
          value: fact(path)?.url ?? '',
          validateInput: (v) => (v.trim() ? undefined : 'Enter a URL'),
        })
      )?.trim();
      if (!url || url === fact(path)?.url) return;
      await run(repo, { op: 'changeSubmodule', change: { kind: 'setUrl', path, url } }, `Set the URL of ${path}`);
    }),
    onRow('amonhen.submodule.setBranch', async (repo, path) => {
      const current = fact(path)?.branch ?? '';
      const branch = await vscode.window.showInputBox({
        title: `Branch for ${path} to follow`,
        prompt: 'Update… with the latest commit moves it to this branch’s tip. Empty: follow the recorded commit (git’s default)',
        value: current,
      });
      if (branch === undefined || branch.trim() === current) return;
      await run(
        repo,
        { op: 'changeSubmodule', change: { kind: 'setBranch', path, branch: branch.trim() || null } },
        `Set the branch of ${path}`,
      );
    }),
    // .git/modules を残すかを選ぶ（残すのが git の既定。また足せば、push していないコミットも戻る）
    onRow('amonhen.submodule.remove', async (repo, path) => {
      const keep = 'Remove';
      const drop = 'Remove and Delete Its Data';
      const answer = await vscode.window.showWarningMessage(
        `Remove the submodule ${path}?`,
        {
          modal: true,
          detail:
            'Removes it from .gitmodules, the index and the working tree. Remove keeps its repository data in .git/modules, as git does, so adding it again brings back commits not pushed. Remove and Delete Its Data deletes that too.',
        },
        keep,
        drop,
      );
      if (answer !== keep && answer !== drop) return;
      // 開いていれば閉じる（常駐の cat-file と daemon へのつながり。Windows では、使っているフォルダーを消せない）
      await manager.forget(join(repo.root, path), true);
      await run(
        repo,
        { op: 'changeSubmodule', change: { kind: 'remove', path, deleteGitDir: answer === drop } },
        `Remove ${path}`,
      );
    }),
    // 中のリポジトリのデータを .git/modules に移す（git submodule absorbgitdirs）
    onRow('amonhen.submodule.absorb', (repo, path) =>
      run(repo, { op: 'changeSubmodule', change: { kind: 'absorb', paths: [path] } }, `Move the repository data of ${path}`),
    ),
    // それぞれの submodule の中でシェルの命令を動かす（git submodule foreach）。出力は出せない（コアは成否だけを返す）
    onRepo('amonhen.submodules.foreach', async (repo) => {
      const command = (
        await vscode.window.showInputBox({
          title: 'Run in Each Submodule',
          prompt: 'Shell command to run in each submodule’s folder (git submodule foreach)',
          placeHolder: 'git switch main',
          validateInput: (v) => (v.trim() ? undefined : 'Enter a command'),
        })
      )?.trim();
      if (!command) return;
      const depth = await vscode.window.showQuickPick(
        [
          { label: 'These Submodules', recursive: false },
          { label: 'Also the Submodules Inside', detail: '--recursive', recursive: true },
        ],
        { title: 'Run in Each Submodule' },
      );
      if (!depth) return;
      await run(repo, { op: 'changeSubmodule', change: { kind: 'foreach', command, recursive: depth.recursive } }, `Run ${command}`);
      vscode.window.setStatusBarMessage(`$(check) Ran ${command} in each submodule`, 5000);
    }),
    onRepo('amonhen.submodules.updateAll', (repo) => run(repo, { op: 'updateSubmodules', paths: [] }, 'Update all submodules')),
    onRepo('amonhen.submodules.update', async (repo) => {
      const paths = (repo.current.submodules ?? []).map((s) => s.path);
      if (paths.length === 0) {
        void vscode.window.showInformationMessage('This repository has no submodules');
        return;
      }
      const picked = await vscode.window.showQuickPick(
        paths.map((p) => ({ label: p, picked: true })),
        { title: 'Update Submodules', canPickMany: true, placeHolder: 'Submodules to update' },
      );
      if (!picked || picked.length === 0) return;
      const options = await pickUpdateOptions('Update Submodules');
      if (!options) return;
      await run(repo, { op: 'updateSubmodules', paths: picked.map((p) => p.label), ...options }, 'Update submodules');
    }),
    onRepo('amonhen.submodules.sync', (repo) =>
      run(repo, { op: 'changeSubmodule', change: { kind: 'sync', recursive: true } }, 'Sync submodule URLs'),
    ),
    onRepo('amonhen.submodules.add', async (repo) => {
      const url = (
        await vscode.window.showInputBox({
          title: 'Add Submodule',
          prompt: 'Repository URL',
          placeHolder: 'https://example.com/team/library.git',
          validateInput: (v) => (v.trim() ? undefined : 'Enter a URL'),
        })
      )?.trim();
      if (!url) return;
      const path = (
        await vscode.window.showInputBox({
          title: 'Add Submodule',
          prompt: 'Folder in this repository',
          value: folderFromUrl(url),
          validateInput: (v) => (v.trim() ? undefined : 'Enter a folder'),
        })
      )?.trim();
      if (!path) return;
      const branch = await vscode.window.showInputBox({
        title: 'Add Submodule',
        prompt: 'Branch to follow when updating to the latest commit (empty: none)',
      });
      if (branch === undefined) return;
      await run(
        repo,
        { op: 'changeSubmodule', change: { kind: 'add', url, path, branch: branch.trim() || null } },
        `Add submodule ${path}`,
      );
    }),
  );
}
