import { basename, join } from 'node:path';
import * as vscode from 'vscode';
import type { Change } from './sidebar.ts';
import { gitUri, parseGitUri } from './uri.ts';

/**
 * Working Copy の変更を標準の diff エディタで開く。
 * - Changes: index ↔ working tree（右側は実ファイルなので、そのまま編集できる）
 * - Staged: HEAD ↔ index
 */
export async function openChangeDiff(root: string, change: Change): Promise<void> {
  const file = vscode.Uri.file(join(root, change.path));
  const name = basename(change.path);
  const opts: vscode.TextDocumentShowOptions = { preview: true };
  switch (change.group) {
    case 'conflicts':
      await vscode.commands.executeCommand('vscode.open', file);
      return;
    case 'unstaged': {
      const left = gitUri(root, change.path, change.letter === 'U' ? 'empty' : 'index');
      const right = change.letter === 'D' ? gitUri(root, change.path, 'empty') : file;
      await vscode.commands.executeCommand('vscode.diff', left, right, `${name} (Index ↔ Working Tree)`, opts);
      return;
    }
    case 'staged': {
      const left =
        change.letter === 'A' ? gitUri(root, change.path, 'empty') : gitUri(root, change.origPath ?? change.path, 'HEAD');
      const right = gitUri(root, change.path, change.letter === 'D' ? 'empty' : 'index');
      await vscode.commands.executeCommand('vscode.diff', left, right, `${name} (HEAD ↔ Index)`, opts);
      return;
    }
  }
}

/**
 * 開いている diff が、この拡張で開いたものならその種類を返す。
 * - unstaged: base（index）が左、target（working tree）が右
 * - staged: target（HEAD）が左、base（index）が右。unstage は index を HEAD に近づける操作になる
 */
export interface ActiveDiff {
  kind: 'unstaged' | 'staged';
  root: string;
  /** index 上のパス */
  path: string;
  /** staged のときの HEAD 側のパス（rename の元） */
  headPath: string;
  original: vscode.Uri;
  modified: vscode.Uri;
}

export function activeDiff(): ActiveDiff | undefined {
  const input = vscode.window.tabGroups.activeTabGroup.activeTab?.input;
  if (!(input instanceof vscode.TabInputTextDiff)) {
    return undefined;
  }
  const left = parseGitUri(input.original);
  if (!left) {
    return undefined;
  }
  const right = parseGitUri(input.modified);
  const uris = { original: input.original, modified: input.modified };
  // 左が index か empty（untracked）で、右が実ファイルか empty（削除）なら unstaged
  if (left.side !== 'HEAD' && (input.modified.scheme === 'file' || right?.side === 'empty')) {
    return { kind: 'unstaged', root: left.root, path: left.path, headPath: left.path, ...uris };
  }
  // 右が index か、左が HEAD で右が empty（削除を stage 済み）なら staged
  if (right && (right.side === 'index' || (right.side === 'empty' && left.side === 'HEAD'))) {
    return { kind: 'staged', root: right.root, path: right.path, headPath: left.path, ...uris };
  }
  return undefined;
}

/** editor/title のボタンとキーバインドの出し分けに使う */
export function trackActiveDiff(): vscode.Disposable {
  const update = () => void vscode.commands.executeCommand('setContext', 'amonhen.diffKind', activeDiff()?.kind);
  update();
  return vscode.Disposable.from(
    vscode.window.tabGroups.onDidChangeTabs(update),
    vscode.window.tabGroups.onDidChangeTabGroups(update),
    vscode.window.onDidChangeActiveTextEditor(update),
  );
}
