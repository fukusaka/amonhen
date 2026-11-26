/** `commit.template` のファイル（拡張ホストだけ。node:path を使う）。vscode に依存しない */
import { join, resolve } from 'node:path';

/** `commit.template` のファイルの場所。相対パスはリポジトリから、`~` は `home` から（git と同じ） */
export function templateFilePath(path: string, root: string, home: string): string {
  return path === '~' || path.startsWith('~/') ? join(home, path.slice(1)) : resolve(root, path);
}

/** Commits のページに出すファイルのテンプレートの中身: 前後の空白を除き、12 行まで（アプリと同じ。超えたら …） */
export function templatePreview(text: string): string {
  const lines = text.trim().split(/\r?\n/);
  return lines.length > 12 ? `${lines.slice(0, 12).join('\n')}…` : lines.join('\n');
}
