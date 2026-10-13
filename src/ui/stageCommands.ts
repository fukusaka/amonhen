import * as vscode from 'vscode';
import { GitError } from '../git/exec.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { predictStage, predictUnstage } from '../state/predict.ts';
import type { Repository } from '../state/repository.ts';
import type { Change, Node } from './sidebar.ts';

export function registerStageCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.stage', (node?: Node, nodes?: Node[]) =>
      stageFiles(manager.current, nodes ?? (node ? [node] : []), 'stage'),
    ),
    vscode.commands.registerCommand('amonhen.unstage', (node?: Node, nodes?: Node[]) =>
      stageFiles(manager.current, nodes ?? (node ? [node] : []), 'unstage'),
    ),
  );
}

function changesOf(nodes: readonly Node[]): Change[] {
  return nodes.flatMap((n) => (n.kind === 'change' ? [n.change] : n.kind === 'group' ? n.changes : []));
}

async function stageFiles(repo: Repository | undefined, nodes: readonly Node[], op: 'stage' | 'unstage'): Promise<void> {
  const changes = changesOf(nodes);
  if (!repo || changes.length === 0) {
    return;
  }
  // rename を unstage するときは元のパスも戻す
  const paths = [...new Set(changes.flatMap((c) => (op === 'unstage' && c.origPath ? [c.path, c.origPath] : [c.path])))];
  const set = new Set(paths);
  repo.patchStatus((entries) => (op === 'stage' ? predictStage(entries, set) : predictUnstage(entries, set)));
  try {
    await (op === 'stage' ? repo.backend.stage(paths) : repo.backend.unstage(paths));
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}

export function showError(err: unknown): void {
  void vscode.window.showErrorMessage(err instanceof GitError || err instanceof Error ? err.message : String(err));
}
