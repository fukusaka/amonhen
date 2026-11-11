/**
 * HEAD や作業ツリーを動かす操作の前の確認（docs/ux.md の 5: Checkout・Merge・Cherry-pick・Revert・Pop は、何が起きる
 * かを見せてから行う。アプリのシートを VSCode の modal で）
 */
import * as vscode from 'vscode';
import type { Drawing } from '../history/drawing.ts';
import type { Repository } from '../state/repository.ts';
import { MainPanel } from './mainPanel.ts';

/** シートで確かめるときに添えるもの */
export interface SheetExtras {
  /** 前と後の絵（読むのに時間がかかるので、シートを出すときだけ読む） */
  drawing?: () => Promise<Drawing | undefined>;
  /** ok が取り消せない操作 */
  destructive?: boolean;
}

/**
 * title と lines（詳しい欄）を見せ、ok を押したら true。extras があり、メインビューが開いていれば、アプリと同じく
 * その上に重ねたシートで、前と後の絵を添えて確かめる
 */
export async function confirmOperation(title: string, lines: readonly string[], ok: string, extras?: SheetExtras): Promise<boolean> {
  const panel = MainPanel.active;
  if (extras && panel?.canAsk) {
    const drawing = await extras.drawing?.();
    const answer = await panel.ask({
      title,
      lines: lines.filter((l) => l),
      ...(drawing ? { drawing } : {}),
      buttons: [ok],
      destructive: extras.destructive,
    });
    return answer !== undefined;
  }
  const detail = lines.filter((l) => l).join('\n');
  const choice = await vscode.window.showInformationMessage(title, { modal: true, ...(detail ? { detail } : {}) }, ok);
  return choice === ok;
}

/** 作業中の変更（untracked を除く）を持って行くと言う行。なければ空 */
export function uncommittedLine(repo: Repository): string {
  const n = (repo.current.status?.entries ?? []).filter((e) => e.kind !== 'untracked' && e.kind !== 'ignored').length;
  return n === 0 ? '' : `${n} ${n === 1 ? 'file' : 'files'} with uncommitted changes come along.`;
}
