/** メインビューの Working Copy から使う、stage・コンフリクト・エラー表示の共通処理 */
import { readFile, stat } from 'node:fs/promises';
import { join } from 'node:path';
import * as vscode from 'vscode';
import { failureCause, hasMoreThan } from '../failure/cause.ts';
import { classifyFailure, failureAdvice, failureRemedies, failureSummary, failureTitle, operationOf, type Failure, type Remedy } from '../failure/kind.ts';
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
    `Conflict markers are left in ${withMarkers.join(', ')}`,
    { modal: true, detail: 'Mark it as resolved anyway?' },
    'Mark as Resolved',
  );
  return choice === 'Mark as Resolved';
}

/**
 * コンフリクトしたファイルを、現在の側か取り込む側のファイルで解決する（Take Current File・Take Incoming File）。
 * アプリと同じく、git checkout --ours・--theirs のようにすぐ行い、失うものがあるときだけ確かめる: エディタで保存して
 * いない変更（上書きされて失われる）と、マーカーの残っていないファイル（rerere やほかのエディタで解決した内容を片側で
 * 上書きする）
 */
export async function acceptConflicts(
  repo: Repository | undefined,
  changes: readonly Change[],
  side: 'current' | 'incoming',
): Promise<void> {
  const files = changes.flatMap((c) => (c.conflict ? [{ path: c.path, conflict: c.conflict }] : []));
  if (!repo || files.length === 0) {
    return;
  }
  const title = side === 'current' ? 'Take Current File' : 'Take Incoming File';
  const target = files.length === 1 ? files[0]!.path : `${files.length} files`;
  const paths = new Set(files.map((f) => vscode.Uri.file(join(repo.root, f.path)).fsPath));
  const unsaved = vscode.workspace.textDocuments.filter((d) => d.isDirty && d.uri.scheme === 'file' && paths.has(d.uri.fsPath));
  const ask =
    unsaved.length > 0
      ? {
          message: `Take the ${side === 'current' ? 'current' : 'incoming'} file without saving ${target}?`,
          detail: 'Your unsaved changes to the file are lost.',
        }
      : changes.some((c) => c.markers === 0)
        ? {
            message: `Replace the resolved content of ${target}?`,
            detail: 'No conflict markers are left, so it may already be resolved. It becomes one side’s whole file.',
          }
        : undefined;
  if (ask) {
    const choice = await vscode.window.showWarningMessage(ask.message, { modal: true, detail: ask.detail }, title);
    if (choice !== title) {
      return;
    }
  }
  try {
    await repo.backend.resolveConflicts(files, side);
    // 保存していなかった変更は捨てると答えた。エディタをファイルの今の中身に戻す（あとで保存して上書きしないように）
    for (const doc of unsaved) {
      await vscode.window.showTextDocument(doc, { preview: false, preserveFocus: false });
      await vscode.commands.executeCommand('workbench.action.files.revert');
    }
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}

/** 失敗の全文を見せるところ（Details） */
let failures: vscode.OutputChannel | undefined;

/**
 * 失敗を知らせる。原因の行（git の `fatal:`・`error:`）だけを出し、それで言い尽くさなければ Details で全文（git の
 * 命令も）を出力パネルに出す（アプリの帯の原因と Details）
 */
/** showError の付け足し */
export interface ShowErrorOptions {
  /** 「もう一度」で同じ操作をやり直す（つながらない・ロックのとき） */
  retry?: () => unknown;
  /** 端末を開く場所（認証・ホスト鍵のとき） */
  cwd?: string;
  /**
   * 失敗した操作（コアの命令の名前。`failureTitle`）。あればアプリの帯と同じく「Couldn’t Push」などを見出しにして
   * 原因を続ける。読み込みなど、操作でないものの失敗には渡さない
   */
  op?: string;
  /** 操作したブランチ（ほかの worktree で checkout しているとき、その名前を言う） */
  branch?: string;
}

const REMEDY_TITLES: Record<Remedy, string> = {
  openWorktree: 'Open Worktree',
  openTerminal: 'Open in Terminal',
  retry: 'Try Again',
  pull: 'Pull…',
  workingCopy: 'Open Working Copy',
  setUpstream: 'Set Upstream…',
};

/**
 * 失敗を知らせる。git の文から失敗の種類が分かれば、アプリの帯と同じく言い直し、次の一手のボタン（Pull…・Working
 * Copy・Set Upstream…・worktree・端末・もう一度）を添える。全文と助言は Details で
 */
export function showError(err: unknown, options: ShowErrorOptions = {}): void {
  const message = err instanceof GitError || err instanceof Error ? err.message : String(err);
  const cause = failureCause(message);
  const failure = classifyFailure(err instanceof GitError ? `${err.stderr}\n${message}` : message);
  const summary = failure && failureSummary(failure, options.branch);
  const remedies = failure ? failureRemedies(failure, options.retry !== undefined) : [];
  // 見出し（何が失敗したか）に原因を続ける（アプリの帯の見出しと要点）
  const op = options.op ?? operationOf(err);
  const headed = (text: string) => (op ? `${failureTitle(op)}: ${text}` : text);
  if (!summary && !hasMoreThan(cause, message) && !(err instanceof GitError)) {
    void vscode.window.showErrorMessage(headed(message));
    return;
  }
  const buttons = [...remedies.map((r) => REMEDY_TITLES[r]), 'Details'];
  void vscode.window.showErrorMessage(headed(summary ?? cause), ...buttons).then((choice) => {
    const remedy = remedies.find((r) => REMEDY_TITLES[r] === choice);
    if (remedy) {
      runRemedy(remedy, failure!, options);
      return;
    }
    if (choice !== 'Details') return;
    failures ??= vscode.window.createOutputChannel('Amon Hen Failures');
    const command = err instanceof GitError ? `git ${err.args.join(' ')}\n${err.exitCode === null ? '' : `exit code ${err.exitCode}\n`}` : '';
    const advice = failure && failureAdvice(failure);
    failures.appendLine(`[${new Date().toLocaleString()}] ${headed(summary ?? cause)}`);
    if (advice) failures.appendLine(advice);
    failures.appendLine(`${command}${(err instanceof GitError ? err.stderr : message).trimEnd()}`);
    failures.appendLine('');
    failures.show(true);
  });
}

function runRemedy(remedy: Remedy, failure: Failure, options: ShowErrorOptions): void {
  switch (remedy) {
    case 'retry':
      void Promise.resolve(options.retry?.()).catch((err) => showError(err, options));
      break;
    case 'pull':
      void vscode.commands.executeCommand('amonhen.pull');
      break;
    case 'workingCopy':
      void vscode.commands.executeCommand('amonhen.showWorkingCopy');
      break;
    case 'setUpstream':
      void vscode.commands.executeCommand('amonhen.setUpstream');
      break;
    case 'openWorktree':
      if (failure.path) void vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(failure.path), { forceNewWindow: true });
      break;
    case 'openTerminal':
      vscode.window.createTerminal({ name: 'Amon Hen', ...(options.cwd ? { cwd: options.cwd } : {}) }).show();
      break;
  }
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
