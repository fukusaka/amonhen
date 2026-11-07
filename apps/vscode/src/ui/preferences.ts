/** 拡張の見た目の好み。設定（settings.json）に出すほどではないものを、VSCode の globalState に覚える */
import type * as vscode from 'vscode';
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
