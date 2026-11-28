/** 拡張の見た目の好み。設定（settings.json）に出すほどではないものを、VSCode の globalState に覚える */
import type * as vscode from 'vscode';
import type { ArchiveFormat } from '../bundle/archiveBundle.ts';
import type { DetailsStyle, DetailsLayout, HistoryItem, HistoryOrder } from '../history/protocol.ts';
import type { MessageParts } from '../workingCopy/commitMessage.ts';
import type { FileLayout } from '../workingCopy/fileList.ts';
import type { WorkingCopyLook } from '../workingCopy/protocol.ts';

let store: vscode.Memento | undefined;

export function initPreferences(memento: vscode.Memento): void {
  store = memento;
}

export function fileLayout(): FileLayout {
  return store?.get<FileLayout>('workingCopy.fileLayout') ?? 'list';
}

export async function setFileLayout(layout: FileLayout): Promise<void> {
  await store?.update('workingCopy.fileLayout', layout);
}

/** Working Copy の見た目（一覧の節分け・コミットの欄のたたみと高さ）。既定はアプリと同じく Mixed で、たたまない */
export function workingCopyLook(): WorkingCopyLook {
  const look = store?.get<Partial<WorkingCopyLook>>('workingCopy.look') ?? {};
  return {
    sections: look.sections ?? false,
    commitCollapsed: look.commitCollapsed ?? false,
    // 0 は「中身の高さに戻す」
    ...(typeof look.commitHeight === 'number' && look.commitHeight > 0 ? { commitHeight: look.commitHeight } : {}),
  };
}

export async function setWorkingCopyLook(patch: Partial<WorkingCopyLook>): Promise<void> {
  await store?.update('workingCopy.look', { ...workingCopyLook(), ...patch });
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

/** History の行を 1 行にするか（既定は、アプリと同じく件名と作者・日時の 2 行） */
export function historyOneLine(): boolean {
  return store?.get<boolean>('history.oneLine') ?? false;
}

export async function setHistoryOneLine(oneLine: boolean): Promise<void> {
  await store?.update('history.oneLine', oneLine);
}

/** History で、選んだコミットの祖先だけをはっきり出すか */
export function historyHighlight(): boolean {
  return store?.get<boolean>('history.highlight') ?? true;
}

export async function setHistoryHighlight(highlight: boolean): Promise<void> {
  await store?.update('history.highlight', highlight);
}

/** History の詳細の、変わったファイルの見せ方（既定はパスの並び） */
export function detailsLayout(): DetailsLayout {
  return { tree: store?.get<DetailsLayout>('history.detailsLayout')?.tree ?? false };
}

export async function setDetailsLayout(layout: DetailsLayout): Promise<void> {
  await store?.update('history.detailsLayout', { tree: layout.tree });
}

/** コミット情報の見せ方（アプリの表示項目。既定は標準） */
export function detailsStyle(): DetailsStyle {
  const style = store?.get<DetailsStyle>('history.detailsStyle');
  return style === 'brief' || style === 'full' ? style : 'standard';
}

export async function setDetailsStyle(style: DetailsStyle): Promise<void> {
  await store?.update('history.detailsStyle', style);
}

/** 前に選んだパッチの形（1 つの mbox か）と送り状 */
export function patchFormat(): { single: boolean; cover: boolean } {
  const v = store?.get<{ single?: boolean; cover?: boolean }>('patches.format');
  return { single: v?.single === true, cover: v?.cover === true };
}

export async function setPatchFormat(format: { single: boolean; cover: boolean }): Promise<void> {
  await store?.update('patches.format', format);
}

/** コミットのメッセージを等幅で出すか（既定はアプリと同じく出さない） */
/** パッチを前に書き出したフォルダ（次に書き出すときに選び始める場所） */
export function patchFolder(): string | undefined {
  return store?.get<string>('patches.folder');
}

export async function setPatchFolder(folder: string): Promise<void> {
  await store?.update('patches.folder', folder);
}

export function monospacedMessage(): boolean {
  return store?.get<boolean>('details.monospace') ?? false;
}

export async function setMonospacedMessage(on: boolean): Promise<void> {
  await store?.update('details.monospace', on);
}

/** 前に選んだアーカイブの形 */
export function archiveFormat(): ArchiveFormat | undefined {
  return store?.get<ArchiveFormat>('archive.format');
}

export async function setArchiveFormat(format: ArchiveFormat): Promise<void> {
  await store?.update('archive.format', format);
}

/** リポジトリごとの、最近切り替えたブランチ（新しい順。ブランチの切り替えで上に出す） */
export function recentBranches(root: string): string[] {
  return store?.get<string[]>(`branch.recent:${root}`) ?? [];
}

export async function setRecentBranches(root: string, names: string[]): Promise<void> {
  await store?.update(`branch.recent:${root}`, names);
}
