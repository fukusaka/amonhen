/**
 * Merge の取り込み方を選ばせる（アプリの Merge の確認の「取り込み方」）。選ぶことが確認になる。覚えず、毎回 git の
 * 設定のとおりから始める。メインビューが開いていて絵を描ければ、アプリと同じくシートで、git の設定のとおりのときの前と後の絵と
 * コンフリクトの見込みを添える（ほかの取り込み方では絵を出さない）
 */
import * as vscode from 'vscode';
import type { MergeMode } from '../git/backend.ts';
import {
  mergeDrawing,
  octopusDrawing,
  octopusSubject,
  type Drawing,
  type MergePreview,
  type OctopusPreview,
} from '../history/drawing.ts';
import { shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import { MainPanel } from './mainPanel.ts';

interface ModeItem extends vscode.QuickPickItem {
  mode: MergeMode;
}

const MODES: readonly ModeItem[] = [
  {
    label: '$(git-merge) Follow git’s Settings',
    detail: 'Uses merge.ff, as git merge in a terminal does: fast-forward when possible, otherwise a merge commit.',
    mode: 'git',
  },
  {
    label: '$(arrow-up) Fast-Forward Only',
    detail: 'Only moves the branch forward; stops if the branches have diverged (git merge --ff-only).',
    mode: 'fastForwardOnly',
  },
  {
    label: '$(git-commit) Always Create a Merge Commit',
    detail: 'Makes a merge commit even when the branch could move forward (git merge --no-ff).',
    mode: 'noFastForward',
  },
  {
    label: '$(fold) Squash into One Commit',
    detail: 'Makes one ordinary commit with all the changes (git merge --squash, then commit). The branch isn’t marked as merged.',
    mode: 'squash',
  },
];

/** 絵を描く材料: 取り込むもの（theirs。いくつもなら 1 つのコミットにまとめる octopus）と取り込む先のブランチ（ours。`refs/heads/...`） */
export interface MergeTarget {
  repo: Repository;
  theirs: string | readonly string[];
  ours: string;
}

/** 前と後の絵と、コンフリクトの見込みのファイル */
async function mergePicture(t: MergeTarget): Promise<[Drawing | undefined, string[]]> {
  const ours = shortName(t.ours);
  if (typeof t.theirs !== 'string') {
    const preview = await t.repo.backend.graphPreview<OctopusPreview>({ op: 'octopusPreview', theirs: [...t.theirs] });
    return [octopusDrawing(preview, ours, t.theirs, octopusSubject(t.theirs, ours)), []];
  }
  const [preview, steps] = await Promise.all([
    t.repo.backend.graphPreview<MergePreview>({ op: 'mergePreview', theirs: t.theirs, ours: t.ours }),
    t.repo.backend.predictMerge([t.theirs], t.ours).catch(() => undefined),
  ]);
  return [mergeDrawing(preview, ours), steps?.[0] ?? []];
}

/**
 * `title`（「Merge a into b?」）で取り込み方を選ばせる。Fast-Forward Only は 1 つを取り込むときだけ。やめたら
 * undefined。target があれば、シートに前と後の絵を添える
 */
export async function pickMergeMode(title: string, single: boolean, target?: MergeTarget): Promise<MergeMode | undefined> {
  const items = MODES.filter((m) => single || m.mode !== 'fastForwardOnly');
  const panel = MainPanel.active;
  const [drawing, conflicts] = panel?.canAsk && target ? await mergePicture(target) : [undefined, []];
  if (panel && drawing) {
    const answer = await panel.ask({
      title,
      lines: ['You can undo this.'],
      warnings: conflicts.length > 0 ? [`Conflicts expected: ${conflicts.join(', ')}`] : [],
      choices: items.map((m) => ({
        label: m.label.replace(/^\$\([^)]*\) /, ''),
        lines: [m.detail ?? ''],
        // 絵は git の設定のとおりのときの形
        ...(m.mode === 'git' ? { drawing } : {}),
      })),
      buttons: ['Merge'],
    });
    return answer && items[answer.choice]?.mode;
  }
  const picked = await vscode.window.showQuickPick(items, {
    title,
    placeHolder: 'Choose how to merge (Enter merges). You can undo this.',
  });
  return picked?.mode;
}
