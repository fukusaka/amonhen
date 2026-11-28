/**
 * 並べた差分（いくつかのファイルを選んだとき）で選んだ行。行は並べた差分全体で選べ、ファイルをまたいでもよい
 * （アプリと同じ）。キーはファイルのパスと、そのファイルの中の行のキー（`old:12`・`new:7`）をつないだもの。
 * 選んだ行の操作は、ファイルごとに分けて、そのファイルの差分の版で頼む（GTK4 の split_multi_rows と同じ考え）。
 * vscode と DOM に依存しない
 */

/** パスと行のキーの区切り（パスにも行のキーにも出ない） */
const SEP = '\u0000';

/** 並べた差分の行のキー */
export function stackLineKey(path: string, line: string): string {
  return `${path}${SEP}${line}`;
}

/** 並べた差分の行のキーを、パスとファイルの中の行のキーに戻す */
export function parseStackLineKey(key: string): { path: string; line: string } {
  const at = key.lastIndexOf(SEP);
  return { path: key.slice(0, at), line: key.slice(at + 1) };
}

/** path のファイルの、選んだ行のキー（ファイルの中のもの） */
export function linesOf(keys: Iterable<string>, path: string): Set<string> {
  const out = new Set<string>();
  for (const key of keys) {
    const { path: p, line } = parseStackLineKey(key);
    if (p === path) out.add(line);
  }
  return out;
}

/**
 * 選んだ行を、ファイルごとに分ける。ファイルは並べた順（order）、ファイルの中は選んだ順。order にないファイルの
 * 行は落とし、行のないファイルは出さない
 */
export function splitByFile(keys: Iterable<string>, order: readonly string[]): { path: string; lines: string[] }[] {
  const byPath = new Map<string, string[]>();
  for (const key of keys) {
    const { path, line } = parseStackLineKey(key);
    let lines = byPath.get(path);
    if (!lines) byPath.set(path, (lines = []));
    if (!lines.includes(line)) lines.push(line);
  }
  return order.flatMap((path) => {
    const lines = byPath.get(path);
    return lines && lines.length > 0 ? [{ path, lines }] : [];
  });
}

/**
 * J・K で選ぶ変更のかたまり（blocks は上から順の、かたまりごとの行のキー）。選んだ行があれば、それを含む最後の
 * かたまりの次（K なら最初のかたまりの前）。範囲の外なら -1 か blocks.length。選んだ行がなければ undefined
 * （見えている所から探す）
 */
export function nextBlock(blocks: readonly (readonly string[])[], picked: ReadonlySet<string>, forward: boolean): number | undefined {
  const current = blocks.flatMap((block, i) => (block.some((k) => picked.has(k)) ? [i] : []));
  if (current.length === 0) return undefined;
  return forward ? Math.max(...current) + 1 : Math.min(...current) - 1;
}
