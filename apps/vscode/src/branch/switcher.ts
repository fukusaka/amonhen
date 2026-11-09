/**
 * ブランチの切り替え（docs/ux.md の 5 の Branch。アプリの BranchSwitcher.swift と同じ）の決まり。最近切り替えた
 * ブランチを上に、ほかは名前の順。リモートのブランチは、同じ名前のローカルブランチがなければ出す（選ぶと追跡する
 * ブランチを作る）。打った名前のブランチがなければ、作る行を足す。vscode に依存しない
 */

export type SwitchChoice =
  | { kind: 'local'; name: string }
  | { kind: 'remote'; name: string }
  | { kind: 'create'; name: string };

/** 覚えておく最近のブランチの数 */
export const RECENT_LIMIT = 10;

/**
 * 並べる候補。`locals`・`remotes` は短い名前（`main`、`origin/main`）、`head` は今のブランチ（出さない）、`recent` は
 * 最近切り替えたもの（新しい順）
 */
export function switchCandidates(
  locals: readonly string[],
  remotes: readonly string[],
  head: string | undefined,
  recent: readonly string[],
): SwitchChoice[] {
  const localSet = new Set(locals);
  const rank = new Map<string, number>();
  recent.forEach((name, i) => rank.has(name) || rank.set(name, i));
  const ordered = locals
    .filter((name) => name !== head)
    .sort((a, b) => (rank.get(a) ?? Infinity) - (rank.get(b) ?? Infinity) || (a < b ? -1 : a > b ? 1 : 0));
  const tracked = remotes.filter((name) => {
    if (name.endsWith('/HEAD')) return false;
    const slash = name.indexOf('/');
    return slash < 0 || !localSet.has(name.slice(slash + 1));
  });
  return [
    ...ordered.map((name): SwitchChoice => ({ kind: 'local', name })),
    ...tracked.map((name): SwitchChoice => ({ kind: 'remote', name })),
  ];
}

/** 打った名前で作れるなら、その名前（同じ名前のローカルブランチがある・空白を含む・`-` で始まるなら作らない） */
export function creatableName(query: string, locals: readonly string[]): string | undefined {
  const name = query.trim();
  if (!name || locals.includes(name) || /\s/.test(name) || name.startsWith('-')) return undefined;
  return name;
}

/** 切り替えたブランチを、最近のものの先頭に */
export function rememberRecent(recent: readonly string[], branch: string): string[] {
  if (recent[0] === branch) return [...recent];
  return [branch, ...recent.filter((name) => name !== branch)].slice(0, RECENT_LIMIT);
}
