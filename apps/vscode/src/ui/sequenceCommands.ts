import * as vscode from 'vscode';
import { inProgress } from '../git/backend.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import { showError } from './stageCommands.ts';

/**
 * 途中の操作（merge・rebase など）があれば断って true。アプリは Pull・Merge…・Rebase…・Apply Patch… をその間は押せなく
 * する（コマンドパレットでは出さない。ボタンやキーから来たときは、ここで何の途中かを言う）
 */
export async function refusedMidSequence(repo: Repository): Promise<boolean> {
  const sequence = await repo.backend.sequenceInProgress().catch(() => undefined);
  if (!sequence) return false;
  void vscode.window.showErrorMessage(inProgress(sequence).message);
  return true;
}

/**
 * 途中の操作がないのに Continue・Abort・Skip を（キーなどから）呼んだ。アプリはメニューを押せなくし、コマンドパレット
 * でも出さないが、来たら何もないと言う（コアと同じ文）
 */
function nothingInProgress(): void {
  void vscode.window.showErrorMessage('No operation in progress');
}

/** 途中で止まっている merge や rebase を続ける・中止する */
export function registerSequenceCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.continueSequence', () => continueSequence(manager)),
    vscode.commands.registerCommand('amonhen.abortSequence', () => abortSequence(manager)),
    vscode.commands.registerCommand('amonhen.skipSequence', () => skipSequence(manager)),
  );
}

async function continueSequence(manager: RepositoryManager): Promise<void> {
  const repo = manager.current;
  const sequence = repo?.current.sequence;
  if (!repo || !sequence) {
    if (repo) nothingInProgress();
    return;
  }
  try {
    const result = await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: `Continue ${sequence}` },
      () => repo.backend.continueSequence(sequence),
    );
    if (result === 'conflict') {
      void vscode.window.showWarningMessage(
        'There are conflicts left, or it can’t continue yet. Resolve and stage them, then Continue.',
      );
    } else if (result === 'done') {
      vscode.window.setStatusBarMessage(`$(check) ${sequence} finished`, 5000);
    }
  } catch (err) {
    showError(err, { op: 'continue' });
  } finally {
    await repo.refresh();
  }
}

/**
 * 今当てているコミットを飛ばして続ける。そのコミットは結果に残らない。アプリと同じく聞かずに行い、rebase・cherry-pick・
 * am だけ（revert には出さない）
 */
async function skipSequence(manager: RepositoryManager): Promise<void> {
  const repo = manager.current;
  const sequence = repo?.current.sequence;
  if (!repo || (sequence !== 'rebase' && sequence !== 'cherry-pick' && sequence !== 'am')) {
    if (repo && !sequence) nothingInProgress();
    return;
  }
  try {
    const result = await repo.backend.skipSequence(sequence);
    if (result === 'conflict') {
      void vscode.window.showWarningMessage('The next commit has conflicts too');
    }
  } catch (err) {
    showError(err, { op: 'skip' });
  } finally {
    await repo.refresh();
  }
}

/** 始める前に戻す。アプリと同じく聞かずに行う */
async function abortSequence(manager: RepositoryManager): Promise<void> {
  const repo = manager.current;
  const sequence = repo?.current.sequence;
  if (!repo || !sequence) {
    if (repo) nothingInProgress();
    return;
  }
  try {
    await repo.backend.abort(sequence);
  } catch (err) {
    showError(err, { op: 'abort' });
  } finally {
    await repo.refresh();
  }
}
