/** 独自スキーム `amonhen:` の URI。パス部分は表示用の絶対パスで、言語やアイコンの判定に使われる */
import { join } from 'node:path';
import * as vscode from 'vscode';

export const SCHEME = 'amonhen';

/** HEAD・index のどちらの内容か。empty は存在しない側（追加・削除されたファイル）に使う */
export type Side = 'HEAD' | 'index' | 'empty';

export interface GitUri {
  root: string;
  /** リポジトリのルートからの相対パス */
  path: string;
  side: Side;
}

export function gitUri(root: string, path: string, side: Side): vscode.Uri {
  return vscode.Uri.file(join(root, path)).with({
    scheme: SCHEME,
    query: new URLSearchParams({ root, path, side }).toString(),
  });
}

export function parseGitUri(uri: vscode.Uri): GitUri | undefined {
  if (uri.scheme !== SCHEME) {
    return undefined;
  }
  const q = new URLSearchParams(uri.query);
  const root = q.get('root');
  const path = q.get('path');
  const side = q.get('side');
  if (root === null || path === null || (side !== 'HEAD' && side !== 'index' && side !== 'empty')) {
    return undefined;
  }
  return { root, path, side };
}

/** サイドバーの変更ファイルに付けるバッジ用の URI。ファイルアイコンはパスから決まる */
export function decorationUri(path: string, letter: string): vscode.Uri {
  return vscode.Uri.file(path).with({ scheme: SCHEME, query: new URLSearchParams({ decoration: letter }).toString() });
}

export function parseDecorationUri(uri: vscode.Uri): string | null {
  return uri.scheme === SCHEME ? new URLSearchParams(uri.query).get('decoration') : null;
}
