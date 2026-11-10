/**
 * Merge の取り込み方を選ばせる（アプリの Merge の確認の「取り込み方」）。選ぶことが確認になる。覚えず、毎回 git の
 * 設定のとおりから始める
 */
import * as vscode from 'vscode';
import type { MergeMode } from '../git/backend.ts';

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

/** `title`（「Merge a into b?」）で取り込み方を選ばせる。Fast-Forward Only は 1 つを取り込むときだけ。やめたら undefined */
export async function pickMergeMode(title: string, single: boolean): Promise<MergeMode | undefined> {
  const items = MODES.filter((m) => single || m.mode !== 'fastForwardOnly');
  const picked = await vscode.window.showQuickPick(items, {
    title,
    placeHolder: 'Choose how to merge (Enter merges). You can undo this.',
  });
  return picked?.mode;
}
