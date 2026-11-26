/** Stash の保存・適用・pop・削除と、中身の表示 */
import { basename, join } from 'node:path';
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
  // op は失敗の見出しにする操作（failureTitle）
  const withRepo = (fn: (repo: Repository) => Promise<void>, op?: string) => async () => {
    const repo = manager.current;
    if (!repo) return;
    try {
      await fn(repo);
    } catch (err) {
      showError(err, { op });
    } finally {
      await repo.refresh();
    }
  };
  const onStash = (id: string, fn: (repo: Repository, node: StashNode) => Promise<void>, op?: string) =>
    vscode.commands.registerCommand(id, (node?: Node) =>
      node?.kind === 'stash' ? withRepo((repo) => fn(repo, node), op)() : undefined,
    );

  return vscode.Disposable.from(
    vscode.commands.registerCommand(
      'amonhen.stash',
      withRepo(async (repo) => {
        // エンジンがあれば、アプリと同じく stage したものだけ（git 2.35 から）と、stage した変更を残すのも選べる
        const engine = !!(await daemonFor(repo.root).catch(() => undefined));
        const title = 'Stash Changes';
        const options = await askStashOptions(title, engine, false);
        if (!options) return;
        const message = await vscode.window.showInputBox({ title, prompt: 'Message (optional)' });
        if (message === undefined) return;
        if (options.staged || options.keepIndex) {
          await runEngine(repo, { op: 'stashPush', message: message.trim() || null, ...options }, 'Stash');
          return;
        }
        await repo.backend.stashPush({ message: message.trim() || undefined, includeUntracked: options.includeUntracked });
      }, 'stashPush'),
    ),
    // Working Copy のファイルの右クリック: そのファイルの変更だけを stash する（エンジンだけ）
    // いくつも選んでいれば（paths）、選んだファイルすべて。選び方はアプリと同じシート（untracked があれば最初から含める）
    vscode.commands.registerCommand('amonhen.stash.files', async (ctx?: { path?: string; paths?: string[] }) => {
      const repo = manager.current;
      const paths = ctx?.paths?.length ? ctx.paths : ctx?.path ? [ctx.path] : [];
      if (!repo || paths.length === 0) return;
      const what = paths.length === 1 ? paths[0]! : `${paths.length} files`;
      const title = paths.length === 1 ? `Stash Changes in ${basename(paths[0]!)}` : `Stash Changes in ${paths.length} Files`;
      const untracked = (repo.current.status?.entries ?? []).some((e) => paths.includes(e.path) && e.kind === 'untracked');
      const options = await askStashOptions(title, true, untracked);
      if (!options) return;
      const message = await vscode.window.showInputBox({ title, prompt: 'Message (optional)' });
      if (message === undefined) return;
      try {
        await runEngine(repo, { op: 'stashPush', message: message.trim() || null, ...options, paths }, `Stash ${what}`);
      } catch (err) {
        showError(err, { op: 'stashPush' });
      } finally {
        await repo.refresh();
      }
    }),
    onStash('amonhen.stash.apply', (repo, node) => apply(repo, node, false), 'stashApply'),
    onStash('amonhen.stash.pop', (repo, node) => apply(repo, node, true), 'stashApply'),
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
    // アプリの Stashes の行の Copy Name（stash@{0}）
    vscode.commands.registerCommand('amonhen.stash.copyName', async (node?: Node) => {
      if (node?.kind !== 'stash') return;
      await vscode.env.clipboard.writeText(node.stash.name);
      vscode.window.setStatusBarMessage(`$(copy) ${node.stash.name}`, 3000);
    }),
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
      }, 'stashApply'),
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

interface StashOptions {
  staged: boolean;
  keepIndex: boolean;
  includeUntracked: boolean;
}

/**
 * 何を stash するか（すべての変更・stage した変更だけ）と、stage した変更を残すか・untracked を含めるか（アプリの
 * Stash のシートと同じ。後の 2 つは組み合わせられる）。stage した変更だけのときは、ほかの選び方は git が受けない。
 * エンジンがなければ untracked を含めるかだけ。やめたら undefined
 */
async function askStashOptions(title: string, engine: boolean, untrackedChosen: boolean): Promise<StashOptions | undefined> {
  if (engine) {
    const what = await vscode.window.showQuickPick(
      [
        { label: 'All changes', staged: false },
        { label: 'Only the staged changes', detail: 'git 2.35 and later', staged: true },
      ],
      { title, placeHolder: 'Changes to stash' },
    );
    if (!what) return undefined;
    if (what.staged) return { staged: true, keepIndex: false, includeUntracked: false };
  }
  const items = [
    ...(engine ? [{ label: 'Keep the staged changes in the working tree (--keep-index)', key: 'keepIndex' as const, picked: false }] : []),
    { label: 'Include untracked files', key: 'includeUntracked' as const, picked: untrackedChosen },
  ];
  const picked = await vscode.window.showQuickPick(items, { title, placeHolder: 'Options (none for git’s default)', canPickMany: true });
  if (!picked) return undefined;
  const on = new Set(picked.map((i) => i.key));
  return { staged: false, keepIndex: on.has('keepIndex'), includeUntracked: on.has('includeUntracked') };
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
