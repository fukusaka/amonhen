/** 独自スキーム `amonhen:` の URI。パス部分は表示用の絶対パスで、言語やアイコンの判定に使われる */
import { join } from 'node:path';
import * as vscode from 'vscode';

export const SCHEME = 'amonhen';

/**
 * どこの内容か。rev は任意のコミット（rev パラメーターで指定）。
 * empty は存在しない側（追加・削除されたファイル）に使う。
 */
export type Side = 'HEAD' | 'index' | 'rev' | 'empty';

export interface GitUri {
  root: string;
  /** リポジトリのルートからの相対パス */
  path: string;
  side: Side;
  /** side が rev のときのコミット */
  rev?: string;
}

export function gitUri(root: string, path: string, side: Side, rev?: string): vscode.Uri {
  const params: Record<string, string> = { root, path, side };
  if (rev) params.rev = rev;
  return vscode.Uri.file(join(root, path)).with({ scheme: SCHEME, query: new URLSearchParams(params).toString() });
}

export function parseGitUri(uri: vscode.Uri): GitUri | undefined {
  if (uri.scheme !== SCHEME) {
    return undefined;
  }
  const q = new URLSearchParams(uri.query);
  const root = q.get('root');
  const path = q.get('path');
  const side = q.get('side');
  if (root === null || path === null || (side !== 'HEAD' && side !== 'index' && side !== 'rev' && side !== 'empty')) {
    return undefined;
  }
  const rev = q.get('rev');
  return rev ? { root, path, side, rev } : { root, path, side };
}

/** サイドバーの変更ファイルに付けるバッジ用の URI。ファイルアイコンはパスから決まる */
export function decorationUri(path: string, letter: string): vscode.Uri {
  return vscode.Uri.file(path).with({ scheme: SCHEME, query: new URLSearchParams({ decoration: letter }).toString() });
}

export function parseDecorationUri(uri: vscode.Uri): string | null {
  return uri.scheme === SCHEME ? new URLSearchParams(uri.query).get('decoration') : null;
}
