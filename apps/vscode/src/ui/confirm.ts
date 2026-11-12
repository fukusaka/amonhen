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
 * メインビューが開いていて extras の絵を描ければ、アプリと同じくその上に重ねたシートで、前と後の絵を添えて確かめ、
 * ok を押したら true。シートを出せなければ undefined（呼ぶ側がこれまでの確かめ方で）
 */
export async function confirmInSheet(title: string, lines: readonly string[], ok: string, extras: SheetExtras): Promise<boolean | undefined> {
  const panel = MainPanel.active;
  const drawing = panel?.canAsk ? await extras.drawing?.() : undefined;
  if (!panel || !drawing) return undefined;
  const answer = await panel.ask({ title, lines: lines.filter((l) => l), drawing, buttons: [ok], destructive: extras.destructive });
  return answer !== undefined;
}

/** title と lines（詳しい欄）を見せ、ok を押したら true。extras の絵を描ければシートで（confirmInSheet）、でなければ modal で */
export async function confirmOperation(title: string, lines: readonly string[], ok: string, extras?: SheetExtras): Promise<boolean> {
  const asked = extras && (await confirmInSheet(title, lines, ok, extras));
  if (asked !== undefined) return asked;
  const detail = lines.filter((l) => l).join('\n');
  const choice = await vscode.window.showInformationMessage(title, { modal: true, ...(detail ? { detail } : {}) }, ok);
  return choice === ok;
}

/** 作業中の変更（untracked を除く）を持って行くと言う行。なければ空 */
export function uncommittedLine(repo: Repository): string {
  const n = (repo.current.status?.entries ?? []).filter((e) => e.kind !== 'untracked' && e.kind !== 'ignored').length;
  return n === 0 ? '' : `${n} ${n === 1 ? 'file' : 'files'} with uncommitted changes come along.`;
}
