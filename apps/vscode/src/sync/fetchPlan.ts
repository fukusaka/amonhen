/**
 * Fetch の確認（docs/ux.md の 5 の Fetch。アプリの SyncSheets.swift と同じ）の決まり。片づける（prune）かは git と
 * 同じく、リモートの `remote.<name>.prune`、なければ `fetch.prune`。vscode に依存しない
 */

/** git の真偽の値として true か */
function truthy(value: string | undefined): boolean {
  return ['true', 'yes', 'on', '1'].includes((value ?? '').trim().toLowerCase());
}

/** remote を片づける設定の名前（片づけないなら undefined）。`config` は設定の名前 → 値 */
export function pruneKey(remote: string, config: ReadonlyMap<string, string>): string | undefined {
  const own = `remote.${remote}.prune`;
  const key = config.has(own) ? own : 'fetch.prune';
  return truthy(config.get(key)) ? key : undefined;
}

/** chosen から fetch するときに、どれが片づけるかを言う文（どれも片づけなければ空） */
export function pruneLine(chosen: readonly string[], config: ReadonlyMap<string, string>): string {
  const names = chosen.filter((remote) => pruneKey(remote, config));
  if (names.length === 0) return '';
  const keys = [...new Set(names.map((remote) => pruneKey(remote, config)!))].join(', ');
  return names.length === chosen.length
    ? `Remote branches deleted there are removed here too (${keys}).`
    : `Remote branches deleted on ${names.join(', ')} are removed here too (${keys}).`;
}
