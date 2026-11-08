/** History の詳細に縦に並べるファイルの決まり（最初から畳むもの・行数・Viewed の記録）。vscode に依存しない */
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

/** Viewed にしたファイル。鍵はコミット ID（いくつも選んだときはその見せ方の鍵）。`order` は古い順 */
export interface ViewedStore {
  order: string[];
  files: Record<string, string[]>;
}

/** これより多くのコミットの Viewed は、古いものから忘れる（アプリと同じ） */
export const VIEWED_COMMITS = 500;

/** path の Viewed を変えた記録を返す（元は変えない）。空になった鍵は消し、新しく使った鍵は最後に回す */
export function withViewed(store: ViewedStore, key: string, path: string, viewed: boolean): ViewedStore {
  const set = new Set(store.files[key] ?? []);
  if (viewed) set.add(path);
  else set.delete(path);
  const files = { ...store.files };
  const order = store.order.filter((k) => k !== key);
  if (set.size > 0) {
    files[key] = [...set];
    order.push(key);
  } else {
    delete files[key];
  }
  for (const old of order.splice(0, Math.max(0, order.length - VIEWED_COMMITS))) {
    delete files[old];
  }
  return { order, files };
}
