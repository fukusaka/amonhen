/** Working Copy から .gitignore に足す。vscode に依存しない */

/** untracked のファイルに出す候補: そのパスと、拡張子のパターン */
export function ignorePatterns(path: string): string[] {
  const name = path.slice(path.lastIndexOf('/') + 1);
  const dot = name.lastIndexOf('.');
  return dot > 0 && dot < name.length - 1 ? [path, `*${name.slice(dot)}`] : [path];
}

/** `pattern` を最後に足した中身。もうあれば undefined */
export function withIgnorePattern(text: string, pattern: string): string | undefined {
  if (text.split(/\r?\n/).some((line) => line.trim() === pattern)) return undefined;
  return `${text}${text && !text.endsWith('\n') ? '\n' : ''}${pattern}\n`;
}
