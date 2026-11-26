import { isAbsolute, join, relative } from 'node:path';
import * as vscode from 'vscode';
import type { Sequence } from '../git/backend.ts';
import { conflictSides } from '../git/status.ts';
import type { Change } from '../state/changes.ts';
import { gitUri } from './uri.ts';

/** コンフリクトしたファイルを開く。マージエディタで開けなければ、ファイルをそのまま開く */
export async function openConflict(root: string, change: Change, sequence?: Sequence): Promise<void> {
  if (!(await openMergeEditor(root, change, sequence))) {
    // 片側が削除されたコンフリクトなど。Take Current File・Take Incoming File で解決する
    await vscode.commands.executeCommand('vscode.open', vscode.Uri.file(join(root, change.path)));
  }
}

/**
 * 標準の 3-way マージエディタで開く。base・current・incoming は index の stage 1・2・3。
 * マージエディタを開く公開 API は無いので、組み込みの Git 拡張と同じ内部コマンドを使う。
 * 開けなかったら false
 */
async function openMergeEditor(root: string, change: Change, sequence: Sequence | undefined): Promise<boolean> {
  const sides = conflictSides(change.conflict ?? 'UU');
  if (!sides.current || !sides.incoming) {
    return false;
  }
  // rebase では HEAD が rebase 先で、取り込む側が自分のコミットになる
  const [current, incoming] =
    sequence === 'rebase'
      ? ['Rebasing onto (HEAD)', 'Commit being applied']
      : ['Current branch (HEAD)', `Incoming (${sequence ?? 'merge'})`];
  try {
    await vscode.commands.executeCommand('_open.mergeEditor', {
      base: sides.base ? gitUri(root, change.path, 'rev', ':1') : gitUri(root, change.path, 'empty'),
      input1: { uri: gitUri(root, change.path, 'rev', ':2'), title: 'Current', description: current },
      input2: { uri: gitUri(root, change.path, 'rev', ':3'), title: 'Incoming', description: incoming },
      output: vscode.Uri.file(join(root, change.path)),
    });
    return true;
  } catch {
    return false;
  }
}

/**
 * root の中のファイルを開いているマージエディタを閉じる（途中の操作をやめたあと。アプリも Abort でマージエディタを
 * 閉じる）。保存していない Result は、保存せずにやめた後の中身へ戻してから閉じる（古い Result で上書きしない・保存を聞かない）
 */
export async function closeMergeEditors(root: string): Promise<void> {
  // root と fsPath は大文字小文字（Windows のドライブ名など）や区切りが違うことがあるので、同じ形にしてから比べる
  const base = vscode.Uri.file(root).fsPath;
  const fold = (p: string) => (process.platform === 'win32' || process.platform === 'darwin' ? p.toLowerCase() : p);
  const inside = (uri: vscode.Uri) => {
    if (uri.scheme !== 'file') return false;
    const rel = relative(fold(base), fold(uri.fsPath));
    return rel !== '' && !rel.startsWith('..') && !isAbsolute(rel);
  };
  const tabs = vscode.window.tabGroups.all
    .flatMap((g) => g.tabs)
    .filter((t) => {
      // TabInputTextMerge（base・input1・input2・result を持つ）
      const input = t.input as { result?: vscode.Uri; input1?: vscode.Uri } | undefined;
      return !!input?.result && !!input.input1 && inside(input.result);
    });
  if (tabs.length === 0) return;
  // マージエディタの Result は開いた時から保存していない扱い。保存せずにファイルの中身（やめた後のもの）へ戻してから
  // 閉じる（保存すると古い Result で上書きしうるうえ、閉じるときに保存を聞かれることがある）
  for (const result of new Set(tabs.map((t) => (t.input as { result: vscode.Uri }).result.toString()))) {
    await vscode.commands.executeCommand('workbench.action.files.revert', vscode.Uri.parse(result));
  }
  await vscode.window.tabGroups.close(tabs, true);
}
