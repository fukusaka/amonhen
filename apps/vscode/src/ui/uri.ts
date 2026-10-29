/** 独自スキーム `amonhen:` の URI。パス部分は表示用の絶対パスで、言語やアイコンの判定に使われる */
import { join } from 'node:path';
import * as vscode from 'vscode';

export const SCHEME = 'amonhen';

/**
 * どこの内容か。rev は rev パラメーターで指定したもの（コミット、または `:1` のような index の stage）。
 * empty は存在しない側（追加・削除されたファイル）に使う。
 */
export type Side = 'rev' | 'empty' | 'rangeDiff';

export interface GitUri {
  root: string;
  /** リポジトリのルートからの相対パス */
  path: string;
  side: Side;
  /** side が rev のときの rev */
  rev?: string;
  /** side が rangeDiff のときの、比べる範囲（git range-diff と同じ形）と、頭に書く一行 */
  ranges?: string[];
  note?: string;
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
  if (root === null || path === null || (side !== 'rev' && side !== 'empty' && side !== 'rangeDiff')) {
    return undefined;
  }
  if (side === 'rangeDiff') {
    const note = q.get('note');
    return { root, path, side, ranges: q.getAll('range'), ...(note ? { note } : {}) };
  }
  const rev = q.get('rev');
  return rev ? { root, path, side, rev } : { root, path, side };
}

/** Range Diff の文書（読み取り専用）。名前は `<title>.range-diff.diff`（diff として色を付ける） */
export function rangeDiffUri(root: string, ranges: readonly string[], title: string, note?: string): vscode.Uri {
  const params = new URLSearchParams({ root, path: `${title.replace(/[\\/]/g, '-')}.range-diff.diff`, side: 'rangeDiff' });
  for (const r of ranges) params.append('range', r);
  if (note) params.set('note', note);
  return vscode.Uri.file(join(root, params.get('path')!)).with({ scheme: SCHEME, query: params.toString() });
}
