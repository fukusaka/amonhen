/**
 * アプリのメニューにあって、VSCode ではボタンや右クリックの中にしかなかった操作を、コマンドパレットから使えるように
 * する（共有のコマンドの表 `crates/commands` の Commit・Commit and Push・Stage All・Set Upstream…・Rebase…・
 * Go to HEAD・Find in History…）。キーはどれにも割り当てられる
 */
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import { groupChanges } from '../state/changes.ts';
import { shortName } from '../state/operations.ts';
import type { ViewName } from '../view/protocol.ts';
import { rebaseAction, runAction } from './dragDrop.ts';
import { MainPanel } from './mainPanel.ts';
import { refusedMidSequence } from './sequenceCommands.ts';
import { showError, stageChanges } from './stageCommands.ts';

interface RefItem extends vscode.QuickPickItem {
  ref: string;
}

export function registerMenuCommands(manager: RepositoryManager, show: (view?: ViewName) => MainPanel): vscode.Disposable {
  const withRepo = (fn: (repo: Repository) => Promise<void> | void) => async () => {
    const repo = manager.current;
    if (repo) await fn(repo);
  };
  // コミット欄に書いた中身でコミットする。Working Copy を出し、押せなければそのわけを知らせる
  const commit = (push: boolean) => () => {
    if (!manager.current) return;
    const panel = show('workingCopy');
    panel.whenReady(() => panel.post({ type: 'wcCommitNow', push }));
  };
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.commit', commit(false)),
    vscode.commands.registerCommand('amonhen.commitAndPush', commit(true)),
    vscode.commands.registerCommand(
      'amonhen.stageAll',
      withRepo(async (repo) => {
        const changes = groupChanges(repo.current.status?.entries ?? []).flatMap(([group, list]) => (group === 'unstaged' ? list : []));
        if (changes.length === 0) {
          vscode.window.setStatusBarMessage('Nothing to stage', 3000);
          return;
        }
        await stageChanges(repo, changes, 'stage');
      }),
    ),
    vscode.commands.registerCommand('amonhen.setUpstream', withRepo(setUpstream)),
    vscode.commands.registerCommand('amonhen.rebase', withRepo(rebase)),
    vscode.commands.registerCommand('amonhen.history.goToHead', () => {
      if (!manager.current) return;
      const panel = show('history');
      panel.whenReady(() => void panel.history.goTo('head').catch(showError));
    }),
    vscode.commands.registerCommand('amonhen.history.find', () => {
      if (!manager.current) return;
      const panel = show('history');
      panel.whenReady(() => panel.post({ type: 'focusSearch' }));
    }),
  );
}

/** 今のブランチの上流をリモートのブランチから選ぶ（`git branch --set-upstream-to`） */
async function setUpstream(repo: Repository): Promise<void> {
  const head = (await repo.backend.head()).branch;
  if (!head) {
    void vscode.window.showWarningMessage('Set Upstream… needs a branch checked out (HEAD is detached).');
    return;
  }
  const name = shortName(head);
  const current = (repo.current.branches ?? []).find((b) => b.name === head)?.upstream;
  const items: RefItem[] = (repo.current.remoteBranches ?? [])
    .filter((r) => !r.symref)
    .map((r) => ({
      label: `$(cloud) ${shortName(r.name)}`,
      description: r.name === current ? 'current upstream' : undefined,
      detail: r.subject,
      ref: r.name,
    }));
  if (items.length === 0) {
    void vscode.window.showWarningMessage('There are no remote branches. Fetch first, or Publish the branch.');
    return;
  }
  const picked = await vscode.window.showQuickPick(items, {
    title: `Set Upstream of ${name}`,
    placeHolder: 'Choose the remote branch that Pull and Push use',
    matchOnDetail: true,
  });
  if (!picked || picked.ref === current) return;
  try {
    await repo.backend.setUpstream(name, shortName(picked.ref));
    vscode.window.setStatusBarMessage(`$(check) ${name} tracks ${shortName(picked.ref)}`, 5000);
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}

/** 今のブランチを、選んだブランチ・タグ・History で選んでいるコミットの上に rebase する */
async function rebase(repo: Repository): Promise<void> {
  if (await refusedMidSequence(repo)) return;
  const head = (await repo.backend.head()).branch;
  if (!head) {
    void vscode.window.showWarningMessage('Rebase… needs a branch checked out (HEAD is detached).');
    return;
  }
  const selected = MainPanel.active?.history.selectedCommit;
  const tags = repo.current.tags ?? (await repo.backend.refs(['refs/tags']));
  const refs = [
    ...(repo.current.branches ?? []).filter((r) => r.name !== head),
    ...(repo.current.remoteBranches ?? []).filter((r) => !r.symref),
    ...tags,
  ];
  const icon = (ref: string) => (ref.startsWith('refs/tags/') ? '$(tag)' : ref.startsWith('refs/remotes/') ? '$(cloud)' : '$(git-branch)');
  const items: RefItem[] = [
    ...(selected ? [{ label: `$(git-commit) ${selected.slice(0, 8)}`, description: 'selected in History', ref: selected }] : []),
    ...refs.map((r) => ({ label: `${icon(r.name)} ${shortName(r.name)}`, detail: r.subject, ref: r.name })),
  ];
  const picked = await vscode.window.showQuickPick(items, {
    title: `Rebase ${shortName(head)}`,
    placeHolder: `Choose what to rebase ${shortName(head)} onto`,
    matchOnDetail: true,
  });
  if (!picked) return;
  await runAction(repo, rebaseAction(repo, head, picked.ref));
}
