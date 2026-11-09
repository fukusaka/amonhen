/**
 * Rebase と History の編集の前の確認。共有の daemon のコアから書き換える前の見通し（一緒に動くブランチ・残る
 * ブランチ・push 済みのブランチ・コンフリクトの見込み）を読み、modal の詳しい欄に出す。エンジンを使わないときは
 * 見通しなしで確かめる。積み重ねたブランチの扱いを今回だけ設定と逆にできるなら、そのボタンも出す
 */
import * as vscode from 'vscode';
import { drawingLines, previewLines, stackedAlternative, type PreviewTarget } from '../history/rewritePreview.ts';
import type { Repository } from '../state/repository.ts';

/** 選んだ答え。updateRefs は今回だけの指定（なければ git の `rebase.updateRefs` のとおり） */
export interface RewriteChoice {
  updateRefs?: boolean;
}

interface Item extends vscode.MessageItem {
  updateRefs?: boolean;
}

export async function confirmRewrite(
  repo: Repository,
  title: string,
  ok: string,
  target: PreviewTarget | undefined,
  note?: string,
): Promise<RewriteChoice | undefined> {
  // 調べられなくても書き換えはできる（見通しなしで、調べられなかったわけを添えて確かめる）
  let failure: string | undefined;
  const preview = target
    ? await vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, title: 'Checking what the rewrite will do' },
        () =>
          repo.backend.rewritePreview(target).catch((err: unknown) => {
            failure = err instanceof Error ? err.message : String(err);
            return undefined;
          }),
      )
    : undefined;
  const lines = [
    ...(note ? [note] : []),
    ...(preview ? previewLines(preview) : []),
    // rebase なら、前と後
    ...(preview && drawingLines(preview).length > 0 ? ['', ...drawingLines(preview), ''] : []),
    ...(failure ? [`Couldn’t read what the rewrite will do (${failure}).`] : []),
    'You can undo this.',
  ];
  const items: Item[] = [{ title: ok }];
  const alternative = preview && stackedAlternative(preview);
  if (alternative !== undefined) {
    items.push({
      title: alternative ? `${ok} (Move the Stacked Branches Too)` : `${ok} (Leave the Stacked Branches)`,
      updateRefs: alternative,
    });
  }
  const choice = await vscode.window.showWarningMessage(title, { modal: true, detail: lines.join('\n') }, ...items);
  return choice ? { updateRefs: choice.updateRefs } : undefined;
}
