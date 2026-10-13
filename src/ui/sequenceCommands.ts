import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { showError } from './stageCommands.ts';

/** 途中で止まっている merge や rebase を続ける・中止する */
export function registerSequenceCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.continueSequence', () => continueSequence(manager)),
    vscode.commands.registerCommand('amonhen.abortSequence', () => abortSequence(manager)),
  );
}

async function continueSequence(manager: RepositoryManager): Promise<void> {
  const repo = manager.current;
  const sequence = repo?.current.sequence;
  if (!repo || !sequence) {
    return;
  }
  try {
    const result = await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: `Continue ${sequence}` },
      () => repo.backend.continueSequence(sequence),
    );
    if (result === 'conflict') {
      void vscode.window.showWarningMessage(
        'コンフリクトが残っているか、続けられない状態です。解決して stage してから Continue してください',
      );
    } else if (result === 'done') {
      vscode.window.setStatusBarMessage(`$(check) ${sequence} finished`, 5000);
    }
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}

async function abortSequence(manager: RepositoryManager): Promise<void> {
  const repo = manager.current;
  const sequence = repo?.current.sequence;
  if (!repo || !sequence) {
    return;
  }
  const choice = await vscode.window.showWarningMessage(
    `${sequence} を中止して、始める前の状態に戻しますか？`,
    { modal: true },
    'Abort',
  );
  if (choice !== 'Abort') {
    return;
  }
  try {
    await repo.backend.abort(sequence);
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}
