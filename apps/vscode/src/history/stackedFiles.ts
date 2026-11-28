/** History の詳細に縦に並べるファイルの決まり（すべて開くでも畳んだままのもの・行数）。vscode に依存しない */
import type { FileDiff } from '../diff/fileDiff.ts';

/** lockfile などの生成物（見ても仕方がない大きな差分）。最初から畳む。アプリと同じ並び */
const GENERATED = new Set([
  'Cargo.lock',
  'package-lock.json',
  'yarn.lock',
  'pnpm-lock.yaml',
  'Gemfile.lock',
  'poetry.lock',
  'composer.lock',
  'go.sum',
  'Package.resolved',
  'Podfile.lock',
  'bun.lockb',
  'uv.lock',
]);

export function isGenerated(path: string): boolean {
  const name = path.slice(path.lastIndexOf('/') + 1);
  return GENERATED.has(name) || name.endsWith('.min.js') || name.endsWith('.min.css');
}

/** 1 ファイルの変わった行（足した行と消した行の和）がこれを超えたら、最初は畳む */
export const COLLAPSE_ABOVE = 1000;

export interface LineCounts {
  added: number;
  removed: number;
}

export function lineCounts(diff: FileDiff): LineCounts {
  let added = 0;
  let removed = 0;
  for (const hunk of diff.hunks) {
    for (const line of hunk.lines) {
      if (line.kind === '+') added++;
      else if (line.kind === '-') removed++;
    }
  }
  return { added, removed };
}

/**
 * 名前を変えたファイルのパス。`git diff --stat` と同じく、前後で同じところをくくり出す（`src/{old.rs → new.rs}`）
 */
export function renamedPath(from: string, to: string): string {
  const a = from.split('/');
  const b = to.split('/');
  let head = 0;
  while (head < a.length - 1 && head < b.length - 1 && a[head] === b[head]) head++;
  let tail = 0;
  while (tail < a.length - head - 1 && tail < b.length - head - 1 && a[a.length - 1 - tail] === b[b.length - 1 - tail]) tail++;
  const prefix = a.slice(0, head).map((p) => `${p}/`).join('');
  const suffix = a.slice(a.length - tail).map((p) => `/${p}`).join('');
  const mid = (parts: string[]) => parts.slice(head, parts.length - tail).join('/');
  return `${prefix}{${mid(a)} → ${mid(b)}}${suffix}`;
}

/** 設定「Files to open together」（`amonhen.diff.filesToOpenTogether`）で選べる数と既定。アプリと同じ */
export const OPEN_TOGETHER_CHOICES = [5, 10, 20, 50, 100] as const;
export const OPEN_TOGETHER_DEFAULT = 20;

/** 設定の値を、選べる数に（ほかの値は既定） */
export function openTogether(value: unknown): number {
  return (OPEN_TOGETHER_CHOICES as readonly unknown[]).includes(value) ? (value as number) : OPEN_TOGETHER_DEFAULT;
}

/**
 * コミットを選んだときに開いておくファイル（並べた順の位置）: 先頭から limit 個のうち、生成物と変わった行の多い
 * もの（heavy）を除いたもの。除いたものも数に入る（アプリと同じく、先頭 limit 個の中から選ぶ）
 */
export function firstOpen(files: readonly { heavy: boolean }[], limit: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < Math.min(limit, files.length); i++) if (!files[i]!.heavy) out.push(i);
  return out;
}
