/** History の範囲のうち、ref で決まるもの。vscode に依存しない */
import type { Scope } from './protocol.ts';

/** いくつかの ref の範囲（名前は改行で区切る。ref の名前に改行は入らない） */
export function refsScope(names: readonly string[]): Scope {
  return names.length === 1 ? `ref:${names[0]}` : `refs:${names.join('\n')}`;
}

/** 範囲の ref。今のブランチやすべてのブランチなら undefined */
export function scopeRefs(scope: Scope): string[] | undefined {
  if (scope.startsWith('refs:')) return scope.slice('refs:'.length).split('\n');
  if (scope.startsWith('ref:')) return [scope.slice('ref:'.length)];
  return undefined;
}

/** 範囲の選択肢に出す名前 */
export function scopeTitle(scope: Scope): string {
  const short = (name: string) => name.replace(/^refs\/(heads|remotes|tags)\//, '');
  return (scopeRefs(scope) ?? []).map(short).join(', ');
}
