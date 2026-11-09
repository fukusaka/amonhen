/**
 * ブランチの切り替え（アプリのツールバーのブランチ・⌘⇧B）。QuickPick で選び、ローカルブランチは切り替え、リモートの
 * ブランチは追跡するブランチを作って切り替え、打った名前のブランチがなければ作って切り替える。ほかの worktree が
 * checkout しているブランチは、git と同じく押し通さず、その worktree を書き込む先にする。今のブランチが変わるたびに
 * 最近のものとして覚える
 */
import * as vscode from 'vscode';
import { creatableName, rememberRecent, type SwitchChoice, switchCandidates } from '../branch/switcher.ts';
import { samePath } from '../git/worktrees.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import { recentBranches, setRecentBranches } from './preferences.ts';
import type { SidebarProvider } from './sidebar.ts';
import { showError } from './stageCommands.ts';

interface Item extends vscode.QuickPickItem {
  choice: SwitchChoice;
}

async function switchBranch(repo: Repository, sidebar: SidebarProvider): Promise<void> {
  const state = repo.current;
  const head = state.status?.branch.head;
  const locals = (state.branches ?? []).map((r) => shortName(r.name));
  const remotes = (state.remoteBranches ?? []).filter((r) => !r.symref).map((r) => shortName(r.name));
  const recent = recentBranches(repo.root);
  // ほかの worktree が checkout しているブランチ → その worktree
  const inUse = new Map(
    sidebar.worktrees.worktrees.filter((w) => w.branch && !samePath(w.path, repo.root)).map((w) => [w.branch!, w]),
  );
  const itemOf = (choice: SwitchChoice): Item => {
    switch (choice.kind) {
      case 'local': {
        const worktree = inUse.get(choice.name);
        return {
          label: `$(${worktree ? 'folder' : 'git-branch'}) ${choice.name}`,
          description: worktree ? `in the worktree ${worktree.path}` : recent.includes(choice.name) ? 'recent' : undefined,
          choice,
        };
      }
      case 'remote':
        return { label: `$(cloud) ${choice.name}`, description: 'creates a tracking branch', choice };
      case 'create':
        return { label: `$(add) Create branch ${choice.name} and switch to it`, alwaysShow: true, choice };
    }
  };
  const base = switchCandidates(locals, remotes, head, recent).map(itemOf);
  const pick = vscode.window.createQuickPick<Item>();
  pick.title = head ? `Switch Branch (now on ${head})` : 'Switch Branch';
  pick.placeholder = 'Find a branch or type a new name';
  pick.matchOnDescription = false;
  pick.items = base;
  pick.onDidChangeValue((value) => {
    const name = creatableName(value, locals);
    pick.items = name ? [...base, itemOf({ kind: 'create', name })] : base;
  });
  const chosen = await new Promise<SwitchChoice | undefined>((resolve) => {
    pick.onDidAccept(() => {
      resolve(pick.selectedItems[0]?.choice);
      pick.hide();
    });
    pick.onDidHide(() => resolve(undefined));
    pick.show();
  });
  pick.dispose();
  if (!chosen) return;
  const worktree = chosen.kind === 'local' ? inUse.get(chosen.name) : undefined;
  if (worktree) {
    await vscode.commands.executeCommand('amonhen.worktree.edit', { kind: 'worktree', worktree });
    return;
  }
  try {
    await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: `Switching to ${chosen.name}` },
      async () => {
        if (chosen.kind === 'local') await repo.backend.switchBranch(chosen.name);
        else if (chosen.kind === 'remote') await repo.backend.trackBranch(chosen.name);
        else await repo.backend.createBranch(chosen.name, 'HEAD', true);
      },
    );
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}

export function registerBranchSwitcher(manager: RepositoryManager, sidebar: SidebarProvider): vscode.Disposable {
  // 今のブランチが変わるたびに覚える（拡張の外での checkout も）
  const last = new Map<string, string | undefined>();
  const watch = (repo: Repository | undefined) => {
    if (!repo) return undefined;
    const note = () => {
      const head = repo.current.status?.branch.head;
      if (!head || last.get(repo.root) === head) return;
      const first = !last.has(repo.root);
      last.set(repo.root, head);
      // 開いたときのブランチは、切り替えたものではないので、まだ何も覚えていなければ覚えない
      if (first && recentBranches(repo.root).length === 0) return;
      void setRecentBranches(repo.root, rememberRecent(recentBranches(repo.root), head));
    };
    note();
    return repo.onDidChange((sections) => sections.has('workingCopy') && note());
  };
  let listener = watch(manager.current);
  return vscode.Disposable.from(
    manager.onDidChangeCurrent(() => {
      listener?.dispose();
      listener = watch(manager.current);
    }),
    { dispose: () => listener?.dispose() },
    vscode.commands.registerCommand('amonhen.switchBranch', async () => {
      const repo = manager.current;
      if (repo) await switchBranch(repo, sidebar);
    }),
  );
}
