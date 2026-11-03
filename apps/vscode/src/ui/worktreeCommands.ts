/**
 * worktree の操作（アプリの Working Copy の下の worktree と同じ）。選んだ worktree を書き込む先（今のリポジトリ）に
 * する、新しい worktree を作る、消す、片付ける、フォルダを開く。一覧と作る・消すは共有の daemon のコアで行う
 * （エンジンがなければメニューに出さない）
 */
import { basename, dirname, join } from 'node:path';
import * as vscode from 'vscode';
import type { Json } from '../daemon/client.ts';
import { daemonFor } from '../daemon/connection.ts';
import { samePath, worktreeName, type Worktree } from '../git/worktrees.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import type { Node, SidebarProvider } from './sidebar.ts';
import { showError } from './stageCommands.ts';

/** コアの命令を実行する。うまくいかなければ投げる */
async function run(repo: Repository, command: Json, title: string): Promise<Json> {
  const client = await daemonFor(repo.root);
  if (!client) throw new Error(`${title} needs the Amon Hen engine (amonhen.daemon.enabled)`);
  const reply = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title }, async () =>
    (await client.call({ op: 'run', command })).json,
  );
  if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `${title} failed`));
  return reply;
}

/** 新しい worktree の起点: サイドバーのブランチ・タグ、History の行のコミット（data-vscode-context）、なければ HEAD */
function startOf(arg: unknown): { rev: string; label: string } {
  const node = arg as Node | undefined;
  if (node?.kind === 'branch' || node?.kind === 'tag') return { rev: shortName(node.ref.name), label: shortName(node.ref.name) };
  const oid = (arg as { oid?: unknown } | undefined)?.oid;
  if (typeof oid === 'string') return { rev: oid, label: oid.slice(0, 8) };
  return { rev: 'HEAD', label: 'HEAD' };
}

function worktreeOf(arg: unknown): Worktree | undefined {
  const node = arg as Node | undefined;
  return node?.kind === 'worktree' ? node.worktree : undefined;
}

export function registerWorktreeCommands(manager: RepositoryManager, sidebar: SidebarProvider): vscode.Disposable {
  /** worktree を書き込む先にする（Working Copy・History・操作がその worktree のものになる） */
  const edit = async (path: string): Promise<void> => {
    const repo = await manager.openPath(path);
    if (!repo) throw new Error(`Can’t open the worktree ${path}`);
    manager.select(repo);
    vscode.window.setStatusBarMessage(`$(edit) Editing the worktree ${basename(path)}`, 5000);
  };
  const command = (id: string, fn: (w: Worktree, repo: Repository) => Promise<void> | void) =>
    vscode.commands.registerCommand(id, async (arg?: unknown) => {
      const w = worktreeOf(arg);
      const repo = manager.current;
      if (!w || !repo) return;
      try {
        await fn(w, repo);
      } catch (err) {
        showError(err);
      }
    });

  return vscode.Disposable.from(
    command('amonhen.worktree.edit', (w) => edit(w.path)),
    command('amonhen.worktree.openInNewWindow', async (w) => {
      await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(w.path), { forceNewWindow: true });
    }),
    command('amonhen.worktree.reveal', async (w) => {
      await vscode.commands.executeCommand('revealFileInOS', vscode.Uri.file(w.path));
    }),
    command('amonhen.worktree.openInTerminal', (w) => {
      vscode.window.createTerminal({ name: worktreeName(w), cwd: w.path }).show();
    }),
    command('amonhen.worktree.copyPath', async (w) => {
      await vscode.env.clipboard.writeText(w.path);
      vscode.window.setStatusBarMessage(`$(copy) ${w.path}`, 3000);
    }),
    // 変更が残っていれば、それも捨てると確かめてから --force（ロックしたものも）。ブランチは残る
    command('amonhen.worktree.remove', async (w, repo) => {
      if (w.main) return;
      const changes = sidebar.worktrees.changes(w);
      const answer = await vscode.window.showWarningMessage(
        `Remove the worktree “${worktreeName(w)}”?`,
        {
          modal: true,
          detail:
            changes > 0
              ? `${w.path} has uncommitted changes in ${changes} files. They are lost with the folder. The branch stays. This can’t be undone.`
              : `Removes the folder ${w.path}. The branch stays.`,
        },
        'Remove Worktree',
      );
      if (answer !== 'Remove Worktree') return;
      // 書き込む先を消すなら、先にメインの作業ツリー（なければほかのリポジトリ）に戻る
      let from = repo;
      if (samePath(w.path, repo.root)) {
        const main = sidebar.worktrees.worktrees.find((m) => m.main);
        const back = (main && (await manager.openPath(main.path))) ?? manager.repositories.find((r) => r !== repo);
        if (!back) throw new Error('Open another worktree before removing this one');
        manager.select(back);
        from = back;
      }
      await run(from, { op: 'worktreeRemove', path: w.path, force: changes > 0 || w.locked !== null }, `Remove worktree ${worktreeName(w)}`);
      manager.forget(w.path);
      await sidebar.worktrees.reload();
    }),
    vscode.commands.registerCommand('amonhen.worktree.prune', async () => {
      const repo = manager.current;
      if (!repo) return;
      try {
        await run(repo, { op: 'worktreePrune' }, 'Prune worktrees');
        await sidebar.worktrees.reload();
      } catch (err) {
        showError(err);
      }
    }),
    // ブランチを作り、それを checkout した worktree を作って、書き込む先にする（アプリの New Worktree…）
    vscode.commands.registerCommand('amonhen.worktree.new', async (arg?: unknown) => {
      const repo = manager.current;
      if (!repo) return;
      const start = startOf(arg);
      try {
        const branches = new Set((repo.current.branches ?? []).map((b) => shortName(b.name)));
        const name = (
          await vscode.window.showInputBox({
            title: 'New Worktree',
            prompt: `Creates a branch from ${start.label} and checks it out in a new folder.`,
            placeHolder: 'Branch name',
            validateInput: (v) => (!v.trim() ? 'Enter a branch name' : branches.has(v.trim()) ? `${v.trim()} already exists` : undefined),
          })
        )?.trim();
        if (!name) return;
        // フォルダの既定は、メインの作業ツリーの隣の「<名前>-<ブランチ>」（git の worktree でよくある置き方）
        const main = sidebar.worktrees.worktrees.find((w) => w.main)?.path ?? repo.root;
        const suggested = join(dirname(main), `${basename(main)}-${name.replaceAll('/', '-')}`);
        const folder = (
          await vscode.window.showInputBox({
            title: `New Worktree ${name}`,
            prompt: 'Folder',
            value: suggested,
            validateInput: (v) => (v.trim() ? undefined : 'Enter a folder'),
          })
        )?.trim();
        if (!folder) return;
        await run(repo, { op: 'worktreeAdd', path: folder, newBranch: name, start: start.rev }, `New worktree ${name}`);
        await sidebar.worktrees.reload();
        await edit(folder);
      } catch (err) {
        showError(err);
      } finally {
        await repo.refresh();
      }
    }),
  );
}
