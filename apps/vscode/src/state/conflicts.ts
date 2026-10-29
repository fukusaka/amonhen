/**
 * コンフリクトのファイルに残っているマーカーと、git の rerere が前の解決を当てたか。
 * コアの `conflictFiles` と同じ見方（作業ツリーのファイルと `.git/MERGE_RR` を読むだけ、git は呼ばない）。vscode に依存しない
 */
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import type { StatusEntry } from '../git/status.ts';

export interface ConflictState {
  /** 作業ツリーに残っているコンフリクトのマーカーの数。0 なら、中身はもう解決されているかもしれない */
  markers: number;
  /** git の rerere が前の解決を当てた */
  rerere: boolean;
}

/** マーカーの行か（`<<<<<<<` などの後が、空白か行の終わり） */
function isMarker(line: string, marker: string): boolean {
  if (!line.startsWith(marker)) return false;
  const next = line[7];
  return next === undefined || next === ' ' || next === '\r';
}

/** 閉じたコンフリクト（`<<<<<<<`・`=======`・`>>>>>>>`）の数。閉じていないものは数えない */
export function countMarkers(text: string): number {
  const lines = text.split('\n');
  let count = 0;
  let i = 0;
  while (i < lines.length) {
    if (!isMarker(lines[i]!, '<<<<<<<')) {
      i++;
      continue;
    }
    let sep = false;
    let end = -1;
    let j = i + 1;
    for (; j < lines.length; j++) {
      const line = lines[j]!;
      if (isMarker(line, '=======') && !sep) {
        sep = true;
      } else if (isMarker(line, '>>>>>>>') && sep) {
        end = j;
        break;
      } else if (isMarker(line, '<<<<<<<')) {
        break;
      }
    }
    if (end >= 0) {
      count++;
      i = end + 1;
    } else {
      i = j;
    }
  }
  return count;
}

/** rerere が見張っている（まだ解決を当てていない）パス（`<id>\t<path>` を NUL で区切ったもの） */
export function rerereTracked(data: string): Set<string> {
  const paths = new Set<string>();
  for (const entry of data.split('\0')) {
    const tab = entry.indexOf('\t');
    if (tab >= 0) paths.add(entry.slice(tab + 1));
  }
  return paths;
}

/**
 * 内容のコンフリクト（両側にファイルがある UU・AA）それぞれの状態。片側で消えたコンフリクトなど、
 * マーカーで表せないものは入れない
 */
export async function conflictStates(
  root: string,
  gitDir: string,
  entries: readonly StatusEntry[],
): Promise<Map<string, ConflictState>> {
  const states = new Map<string, ConflictState>();
  const targets = entries.filter((e) => e.kind === 'unmerged' && ['UU', 'AA'].includes(e.index + e.worktree));
  if (targets.length === 0) return states;
  const tracked = await readFile(join(gitDir, 'MERGE_RR'), 'utf8').then(rerereTracked, () => undefined);
  for (const e of targets) {
    const text = await readFile(join(root, e.path), 'utf8').catch(() => '');
    const markers = countMarkers(text);
    states.set(e.path, { markers, rerere: markers === 0 && tracked !== undefined && !tracked.has(e.path) });
  }
  return states;
}
