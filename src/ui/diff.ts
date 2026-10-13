import { basename, join } from 'node:path';
import * as vscode from 'vscode';
import type { Sequence } from '../git/backend.ts';
import { conflictSides } from '../git/status.ts';
import type { Change } from '../state/changes.ts';
import { gitUri, parseGitUri } from './uri.ts';

/**
 * Working Copy の変更を標準の diff エディタで開く。
 * - Changes: index ↔ working tree（右側は実ファイルなので、そのまま編集できる）
 * - Staged: HEAD ↔ index
 * - Conflicts: 標準の 3-way マージエディタ
 */
export async function openChangeDiff(root: string, change: Change, sequence?: Sequence): Promise<void> {
  const file = vscode.Uri.file(join(root, change.path));
  const name = basename(change.path);
  const opts: vscode.TextDocumentShowOptions = { preview: true };
  switch (change.group) {
    case 'conflicts':
      if (!(await openMergeEditor(root, change, sequence))) {
        // 片側が削除されたコンフリクトなど。サイドバーの Accept Current / Incoming で解決する
        await vscode.commands.executeCommand('vscode.open', file);
      }
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
      ? ['rebase 先（HEAD）', '当てているコミット']
      : ['現在のブランチ（HEAD）', `${sequence ?? 'merge'} で取り込む側`];
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
  if ((left.side === 'index' || left.side === 'empty') && (input.modified.scheme === 'file' || right?.side === 'empty')) {
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
