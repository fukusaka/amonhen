/** 拡張の見た目の好み。設定（settings.json）に出すほどではないものを、VSCode の globalState に覚える */
import type * as vscode from 'vscode';
import type { HistoryItem, HistoryOrder } from '../history/protocol.ts';
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
