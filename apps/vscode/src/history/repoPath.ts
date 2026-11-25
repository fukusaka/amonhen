/** リポジトリの中のパスの計算（vscode・DOM に依存しない） */

/** Markdown のファイル `from` からの相対パスを、リポジトリの根からのパスに（根の外へ出れば undefined） */
export function resolvePath(from: string, link: string): string | undefined {
  const parts = link.startsWith('/') ? [] : from.split('/').slice(0, -1);
  for (const piece of decodeURIComponent(link).split('/')) {
    if (piece === '' || piece === '.') continue;
    if (piece === '..') {
      if (parts.length === 0) return undefined;
      parts.pop();
    } else {
      parts.push(piece);
    }
  }
  return parts.length > 0 ? parts.join('/') : undefined;
}
