/** メインビューの Working Copy から使う、stage・コンフリクト・エラー表示の共通処理 */
import { readFile, stat } from 'node:fs/promises';
import { join } from 'node:path';
import * as vscode from 'vscode';
import { GitError } from '../git/exec.ts';
import type { Change } from '../state/changes.ts';
import { predictStage, predictUnstage } from '../state/predict.ts';
import type { Repository } from '../state/repository.ts';

/** ファイル単位で stage / unstage する。git の結果を待たずに画面を先に更新する */
export async function stageChanges(
  repo: Repository | undefined,
  changes: readonly Change[],
  op: 'stage' | 'unstage',
): Promise<void> {
  if (!repo || changes.length === 0) {
    return;
  }
  if (op === 'stage' && !(await confirmMarkers(repo, changes))) {
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

const MARKER = /^(<{7}|>{7})( |$)/m;

/** コンフリクトを解決済みにするとき、マーカーが残っていれば確かめる */
async function confirmMarkers(repo: Repository, changes: readonly Change[]): Promise<boolean> {
  const withMarkers: string[] = [];
  for (const c of changes) {
    if (c.group !== 'conflicts') continue;
    const text = await readFile(join(repo.root, c.path), 'utf8').catch(() => '');
    if (MARKER.test(text)) withMarkers.push(c.path);
  }
  if (withMarkers.length === 0) {
    return true;
  }
  const choice = await vscode.window.showWarningMessage(
    `コンフリクトのマーカーが残っています: ${withMarkers.join(', ')}`,
    { modal: true, detail: 'このまま解決済みにしますか？' },
    'Mark as Resolved',
  );
  return choice === 'Mark as Resolved';
}

/** コンフリクトしたファイルを、現在の側か取り込む側の内容で解決する。確認してから行う */
export async function acceptConflicts(
  repo: Repository | undefined,
  changes: readonly Change[],
  side: 'current' | 'incoming',
): Promise<void> {
  const files = changes.flatMap((c) => (c.conflict ? [{ path: c.path, conflict: c.conflict }] : []));
  if (!repo || files.length === 0) {
    return;
  }
  const name = side === 'current' ? 'Current（HEAD 側）' : 'Incoming（取り込む側）';
  const target = files.length === 1 ? files[0]!.path : `${files.length} 個のファイル`;
  const choice = await vscode.window.showWarningMessage(
    `${target} を ${name} の内容で解決しますか？`,
    { modal: true, detail: 'もう一方の変更と、ファイルに加えた編集は失われます。' },
    'Accept',
  );
  if (choice !== 'Accept') {
    return;
  }
  try {
    await repo.backend.resolveConflicts(files, side);
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}

export function showError(err: unknown): void {
  void vscode.window.showErrorMessage(err instanceof GitError || err instanceof Error ? err.message : String(err));
}

/** index にあればそのモード、無ければ実ファイルの実行ビットから決める */
export async function indexMode(repo: Repository, path: string, abs: string): Promise<string> {
  const entry = repo.current.status?.entries.find((e) => e.path === path);
  if (entry?.modeIndex && entry.modeIndex !== '000000') {
    return entry.modeIndex;
  }
  try {
    return (await stat(abs)).mode & 0o111 ? '100755' : '100644';
  } catch {
    return '100644';
  }
}
