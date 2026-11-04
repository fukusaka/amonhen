/**
 * subtree の操作（アプリのサイドバーの Subtrees と同じ）。Add Subtree…・Pull…・Push…・Split… を共有の daemon の
 * コアで行う。相手（URL・ref）と squash は、うまくいったらコアがこのリポジトリの設定に覚え、次の既定になる
 * （git subtree がない git やエンジンがなければ、メニューに出さない）
 */
import * as vscode from 'vscode';
import type { Json } from '../daemon/client.ts';
import { daemonFor } from '../daemon/connection.ts';
import { folderFromUrl } from '../git/submodules.ts';
import { defaultSquash, type Subtree } from '../git/subtrees.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { OperationResult } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import { runAction } from './dragDrop.ts';
import type { Node, SidebarProvider } from './sidebar.ts';
import { showError } from './stageCommands.ts';

/** コアの subtree の操作。Pull はコンフリクトすれば merge と同じく止まる */
async function subtree(repo: Repository, action: Json): Promise<OperationResult> {
  const client = await daemonFor(repo.root);
  if (!client) throw new Error('Subtrees need the Amon Hen engine (amonhen.daemon.enabled)');
  const reply = (await client.call({ op: 'run', command: { op: 'subtree', action } })).json;
  if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? 'git subtree failed'));
  return reply['outcome'] === 'conflict' || reply['outcome'] === 'stopped' ? 'conflict' : 'done';
}

/** 相手の URL と ref を聞く。既定は覚えているもの */
async function askSource(
  title: string,
  url: string | null | undefined,
  ref: string | null | undefined,
  refPrompt: string,
): Promise<{ url: string; ref: string } | undefined> {
  const u = (
    await vscode.window.showInputBox({
      title,
      prompt: 'Repository URL',
      value: url ?? '',
      placeHolder: 'https://example.com/team/library.git',
      validateInput: (v) => (v.trim() ? undefined : 'Enter a URL'),
    })
  )?.trim();
  if (!u) return undefined;
  const r = (
    await vscode.window.showInputBox({
      title,
      prompt: refPrompt,
      value: ref ?? '',
      placeHolder: 'main',
      validateInput: (v) => (v.trim() ? undefined : 'Enter a branch or tag'),
    })
  )?.trim();
  return r ? { url: u, ref: r } : undefined;
}

/** squash するかを聞く。既定（先に出す方）は前に選んだもの、なければ git の既定のオフ */
async function askSquash(title: string, previous: boolean): Promise<boolean | undefined> {
  const keep = { label: 'Keep the history', detail: 'Merge every commit of the branch (git’s default)', squash: false };
  const squash = { label: 'Squash', detail: 'Merge one commit with the branch’s changes (--squash)', squash: true };
  const picked = await vscode.window.showQuickPick(previous ? [squash, keep] : [keep, squash], { title });
  return picked?.squash;
}

export function registerSubtreeCommands(manager: RepositoryManager, sidebar: SidebarProvider): vscode.Disposable {
  const facts = sidebar.subtreeFacts;
  /** 行の subtree で fn を実行し、終わったら読み直す */
  const onRow = (id: string, fn: (repo: Repository, s: Subtree) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (node?: Node) => {
      const repo = manager.current;
      if (!repo || node?.kind !== 'subtree') return;
      try {
        await fn(repo, facts.get(node.subtree.dir) ?? node.subtree);
      } catch (err) {
        showError(err);
      } finally {
        await facts.reload();
      }
    });

  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.subtrees.add', async () => {
      const repo = manager.current;
      if (!repo) return;
      const title = 'Add Subtree';
      const source = await askSource(title, undefined, undefined, 'Branch or tag to import');
      if (!source) return;
      const dir = (
        await vscode.window.showInputBox({
          title,
          prompt: 'Folder in this repository',
          value: folderFromUrl(source.url),
          validateInput: (v) => (v.trim() ? undefined : 'Enter a folder'),
        })
      )?.trim();
      if (!dir) return;
      const squash = await askSquash(title, false);
      if (squash === undefined) return;
      await runAction(repo, {
        label: `Subtree Add ${dir}`,
        run: () => subtree(repo, { kind: 'add', dir, url: source.url, ref: source.ref, squash }),
      });
      await facts.reload();
    }),
    onRow('amonhen.subtree.pull', async (repo, s) => {
      const title = `Pull into ${s.dir}`;
      const source = await askSource(title, s.url, s.ref, 'Branch or tag to merge into the folder');
      if (!source) return;
      const squash = await askSquash(title, defaultSquash(s));
      if (squash === undefined) return;
      await runAction(repo, {
        label: `Subtree Pull ${s.dir}`,
        run: () => subtree(repo, { kind: 'pull', dir: s.dir, url: source.url, ref: source.ref, squash }),
      });
    }),
    onRow('amonhen.subtree.push', async (repo, s) => {
      const title = `Push ${s.dir}`;
      const source = await askSource(title, s.url, s.pushRef ?? s.ref, 'Branch to send the folder’s commits to');
      if (!source) return;
      await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title }, () =>
        subtree(repo, { kind: 'push', dir: s.dir, url: source.url, ref: source.ref }),
      );
      vscode.window.setStatusBarMessage(`$(check) Pushed ${s.dir} to ${source.ref}`, 5000);
    }),
    onRow('amonhen.subtree.split', async (repo, s) => {
      const branch = (
        await vscode.window.showInputBox({
          title: `Split ${s.dir}`,
          prompt: 'New branch with only the folder’s history',
          value: `${s.dir.split('/').pop()}-split`,
          validateInput: (v) => (v.trim() ? undefined : 'Enter a branch name'),
        })
      )?.trim();
      if (!branch) return;
      await runAction(repo, {
        label: `Subtree Split ${s.dir}`,
        run: () => subtree(repo, { kind: 'split', dir: s.dir, branch }),
      });
    }),
  );
}
