import { readFile, stat } from 'node:fs/promises';
import { join } from 'node:path';
import * as vscode from 'vscode';
import { applySelection, diffLines, splitLines } from '../diff/lineDiff.ts';
import { GitError } from '../git/exec.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { predictStage, predictUnstage } from '../state/predict.ts';
import type { Repository } from '../state/repository.ts';
import { activeDiff } from './diff.ts';
import type { Change, Node } from './sidebar.ts';

export function registerStageCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.stage', (node?: Node, nodes?: Node[]) =>
      stageFiles(manager.current, nodes ?? (node ? [node] : []), 'stage'),
    ),
    vscode.commands.registerCommand('amonhen.unstage', (node?: Node, nodes?: Node[]) =>
      stageFiles(manager.current, nodes ?? (node ? [node] : []), 'unstage'),
    ),
    vscode.commands.registerCommand('amonhen.stageSelection', () => stageSelection(manager, 'stage')),
    vscode.commands.registerCommand('amonhen.unstageSelection', () => stageSelection(manager, 'unstage')),
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

/**
 * diff エディタで選んだ行（カーソルだけなら、その行を含む hunk）を stage / unstage する。
 * 新しい index の内容をこちらで作って書き込むので、パッチの当たり方に左右されない。
 */
async function stageSelection(manager: RepositoryManager, op: 'stage' | 'unstage'): Promise<void> {
  const diff = activeDiff();
  const editor = vscode.window.activeTextEditor;
  if (!diff || !editor || diff.kind !== (op === 'stage' ? 'unstaged' : 'staged')) {
    return;
  }
  const repo = manager.get(diff.root);
  if (!repo) {
    return;
  }
  const uri = editor.document.uri.toString();
  const onLeft = uri === diff.original.toString();
  if (!onLeft && uri !== diff.modified.toString()) {
    return;
  }
  // unstaged は 左 = index（base）、staged は 右 = index（base）
  const side = (diff.kind === 'unstaged') === onLeft ? 'base' : 'target';
  if (editor.document.isDirty && !(await editor.document.save())) {
    return;
  }

  try {
    const indexObj = await repo.backend.object(`:${diff.path}`);
    const base = indexObj?.data ?? Buffer.alloc(0);
    const target =
      diff.kind === 'unstaged'
        ? await readWorktree(join(diff.root, diff.path))
        : (await repo.backend.object(`HEAD:${diff.headPath}`))?.data;
    if (isBinary(base) || (target && isBinary(target))) {
      void vscode.window.showWarningMessage('バイナリファイルは行単位で扱えません');
      return;
    }
    const baseLines = splitLines(base.toString('latin1'));
    const targetLines = splitLines((target ?? Buffer.alloc(0)).toString('latin1'));
    const changes = diffLines(baseLines, targetLines);
    const collapsed = editor.selections.every((s) => s.isEmpty);
    const ranges = editor.selections.map((s) => ({
      start: s.start.line,
      // 行頭で終わる選択は、その行を含めない
      end: s.end.line > s.start.line && s.end.character === 0 ? s.end.line - 1 : s.end.line,
    }));
    const next = applySelection(baseLines, targetLines, changes, { side, ranges }, collapsed);
    if (next === base.toString('latin1')) {
      void vscode.window.setStatusBarMessage('選択範囲に変更がありません', 3000);
      return;
    }
    // 中身が空になったら、index のエントリごと戻す（新規ファイルの unstage、削除の stage）
    if (next === '' && target === undefined) {
      await (op === 'stage' ? repo.backend.stage([diff.path]) : repo.backend.unstage([diff.path]));
    } else {
      const mode = await indexMode(repo, diff.path, join(diff.root, diff.path));
      await repo.backend.writeIndex(diff.path, mode, Buffer.from(next, 'latin1'));
    }
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}

async function readWorktree(path: string): Promise<Buffer | undefined> {
  try {
    return await readFile(path);
  } catch {
    return undefined;
  }
}

function isBinary(data: Buffer): boolean {
  return data.subarray(0, 8000).includes(0);
}

/** index にあればそのモード、無ければ実ファイルの実行ビットから決める */
async function indexMode(repo: Repository, path: string, abs: string): Promise<string> {
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
