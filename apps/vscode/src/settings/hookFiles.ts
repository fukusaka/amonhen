/** フックのファイルの置き場所（拡張ホストだけ。node:path を使う）。vscode に依存しない */
import { isAbsolute, join } from 'node:path';

/**
 * フックの置き場所（`core.hooksPath`。なければ git dir の hooks）。相対パスは作業ツリーから、`~` は `home` から
 */
export function hooksDirectory(hooksPath: string | null | undefined, workdir: string, commonDir: string, home: string): string {
  if (!hooksPath) return join(commonDir, 'hooks');
  if (hooksPath.startsWith('~')) return join(home, hooksPath.slice(1));
  return isAbsolute(hooksPath) ? hooksPath : join(workdir, hooksPath);
}
