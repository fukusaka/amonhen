/**
 * 履歴のない新しいブランチ（アプリの New Orphan Branch…）。空から始める（git switch --orphan）か、今のファイルを
 * stage したまま残す（git checkout --orphan）かを選ぶ。共有の daemon のコアで行う
 */
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import { runEngine } from './engine.ts';
import { showError } from './stageCommands.ts';

export function registerOrphanCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.branch.newOrphan', async () => {
    const repo = manager.current;
    if (!repo) return;
    const branches = new Set((repo.current.branches ?? []).map((b) => shortName(b.name)));
    const name = (
      await vscode.window.showInputBox({
        title: 'New Orphan Branch',
        prompt: 'A branch with no history. Its first commit has no parent',
        placeHolder: 'Branch name',
        validateInput: (v) => (!v.trim() ? 'Enter a branch name' : branches.has(v.trim()) ? `${v.trim()} already exists` : undefined),
      })
    )?.trim();
    if (!name) return;
    const start = await vscode.window.showQuickPick(
      [
        { label: 'Start Empty', detail: 'No files (git switch --orphan). Uncommitted changes must be committed or stashed first', keepFiles: false },
        { label: 'Keep the Files', detail: 'The current files stay, staged for the first commit (git checkout --orphan)', keepFiles: true },
      ],
      { title: `New Orphan Branch ${name}` },
    );
    if (!start) return;
    try {
      await runEngine(repo, { op: 'createOrphanBranch', name, keepFiles: start.keepFiles }, `New orphan branch ${name}`);
      vscode.window.setStatusBarMessage(`$(git-branch) Switched to the orphan branch ${name}`, 5000);
    } catch (err) {
      showError(err);
    } finally {
      await repo.refresh();
    }
  });
}
