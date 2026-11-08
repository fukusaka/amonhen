/** 拡張の見た目の好み。設定（settings.json）に出すほどではないものを、VSCode の globalState に覚える */
import type * as vscode from 'vscode';
import type { HistoryItem, HistoryOrder } from '../history/protocol.ts';
import { type ViewedStore, withViewed } from '../history/stackedFiles.ts';
import type { MessageParts } from '../workingCopy/commitMessage.ts';
import type { FileLayout } from '../workingCopy/fileList.ts';

let store: vscode.Memento | undefined;

export function initPreferences(memento: vscode.Memento): void {
  store = memento;
}

export function fileLayout(): FileLayout {
  return store?.get<FileLayout>('workingCopy.fileLayout') ?? 'flat';
}

export async function setFileLayout(layout: FileLayout): Promise<void> {
  await store?.update('workingCopy.fileLayout', layout);
}

/** リポジトリごとのコミットメッセージの下書き */
export function commitDraft(root: string): MessageParts | undefined {
  return store?.get<MessageParts>(`workingCopy.draft:${root}`);
}

/** 下書きを覚える。件名も本文も空なら忘れる */
export async function setCommitDraft(root: string, draft: MessageParts | undefined): Promise<void> {
  const empty = !draft || (!draft.summary.trim() && !draft.body.trim());
  await store?.update(`workingCopy.draft:${root}`, empty ? undefined : draft);
}

/** History の並び順（既定は日付順） */
export function historyOrder(): HistoryOrder {
  return store?.get<HistoryOrder>('history.order') ?? 'date';
}

export async function setHistoryOrder(order: HistoryOrder): Promise<void> {
  await store?.update('history.order', order);
}

/** History の行に出さない項目 */
export function historyHidden(): HistoryItem[] {
  return store?.get<HistoryItem[]>('history.hidden') ?? [];
}

export async function setHistoryHidden(hidden: readonly HistoryItem[]): Promise<void> {
  await store?.update('history.hidden', hidden);
}

/** History で、merge が取り込んだコミットを畳むか */
export function historyFold(): boolean {
  return store?.get<boolean>('history.fold') ?? false;
}

export async function setHistoryFold(fold: boolean): Promise<void> {
  await store?.update('history.fold', fold);
}

/** History で、選んだコミットの祖先だけをはっきり出すか */
export function historyHighlight(): boolean {
  return store?.get<boolean>('history.highlight') ?? false;
}

export async function setHistoryHighlight(highlight: boolean): Promise<void> {
  await store?.update('history.highlight', highlight);
}

/** History の詳細で Viewed にしたファイル（コミットごと。新しい 500 コミットの分） */
export function viewedFiles(key: string): string[] {
  return store?.get<ViewedStore>('history.viewed')?.files[key] ?? [];
}

export async function setViewedFile(key: string, path: string, viewed: boolean): Promise<void> {
  const now = store?.get<ViewedStore>('history.viewed') ?? { order: [], files: {} };
  await store?.update('history.viewed', withViewed(now, key, path, viewed));
}
