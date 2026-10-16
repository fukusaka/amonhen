import { join } from 'node:path';
import * as vscode from 'vscode';
import type { Sequence } from '../git/backend.ts';
import { conflictSides } from '../git/status.ts';
import type { Change } from '../state/changes.ts';
import { gitUri } from './uri.ts';

/** コンフリクトしたファイルを開く。マージエディタで開けなければ、ファイルをそのまま開く */
export async function openConflict(root: string, change: Change, sequence?: Sequence): Promise<void> {
  if (!(await openMergeEditor(root, change, sequence))) {
    // 片側が削除されたコンフリクトなど。Accept Current / Incoming で解決する
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
