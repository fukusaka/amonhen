/**
 * Rebase と History の編集の前の確認。共有の daemon のコアから書き換える前の見通し（一緒に動くブランチ・残る
 * ブランチ・push 済みのブランチ・コンフリクトの見込み）を読み、modal の詳しい欄に出す。エンジンを使わないときは
 * 見通しなしで確かめる。積み重ねたブランチの扱いを今回だけ設定と逆にできるなら、そのボタンも出す
 */
import * as vscode from 'vscode';
import { previewLines, stackedAlternative, type PreviewTarget } from '../history/rewritePreview.ts';
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
  // 調べられなくても書き換えはできる（見通しなしで確かめる）
  const preview = target
    ? await vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, title: '書き換えの見通しを調べています' },
        () => repo.backend.rewritePreview(target).catch(() => undefined),
      )
    : undefined;
  const lines = [...(note ? [note] : []), ...(preview ? previewLines(preview) : []), 'Undo で戻せます。'];
  const items: Item[] = [{ title: ok }];
  const alternative = preview && stackedAlternative(preview);
  if (alternative !== undefined) {
    items.push({
      title: alternative ? `${ok}（積み重ねたブランチも動かす）` : `${ok}（積み重ねたブランチは動かさない）`,
      updateRefs: alternative,
    });
  }
  const choice = await vscode.window.showWarningMessage(title, { modal: true, detail: lines.join('\n') }, ...items);
  return choice ? { updateRefs: choice.updateRefs } : undefined;
}
