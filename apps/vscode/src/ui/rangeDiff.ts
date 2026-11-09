/**
 * Range Diff: 同じ一連のコミットの 2 つの版（rebase の前と後など）を比べる（`git range-diff`）。メインビューの
 * ページ（アプリと同じく、上に比べる範囲、左に組の一覧、右に差分の差分かコミットの中身）に出す。開いている間だけ
 * サイドバーに Range Diff の行を出す
 */
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import type { ViewName } from '../view/protocol.ts';
import { MainPanel } from './mainPanel.ts';
import type { Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';

/** Range Diff の行の右クリックに渡される情報（data-vscode-context） */
interface PairContext {
  oldOid?: string;
  newOid?: string;
}

/** Reflog の行の右クリックに渡される情報 */
interface ReflogContext {
  oid?: string;
  /** 一番上の行（今の HEAD）のコミット。一番上の行にはない */
  reflogHead?: string;
}

export function registerRangeDiffCommands(
  manager: RepositoryManager,
  show: (view: ViewName) => MainPanel,
): vscode.Disposable {
  const page = () => show('rangeDiff').rangeDiff;
  const showInHistory = async (oid: string | undefined) => {
    const panel = MainPanel.active;
    if (!panel || !oid) return;
    panel.showView('history');
    if (!(await panel.history.reveal(oid))) {
      void vscode.window.showInformationMessage(`${oid.slice(0, 8)} isn’t in History (no branch or tag reaches it)`);
    }
  };
  const copy = async (oid: string | undefined) => {
    if (!oid) return;
    await vscode.env.clipboard.writeText(oid);
    vscode.window.setStatusBarMessage(`$(copy) ${oid}`, 3000);
  };
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.showRangeDiff', () => show('rangeDiff')),
    vscode.commands.registerCommand('amonhen.compareWithPrevious', async (target?: Node) => {
      if (!manager.current) return;
      const branch = target && 'kind' in target && target.kind === 'branch' && !target.remote ? target.ref.name : undefined;
      await page().openPrevious(branch).catch(showError);
    }),
    // 2 つ選んだコミットを、それぞれを先端とする 2 つの版として比べる（rebase の前と後など）
    vscode.commands.registerCommand('amonhen.history.selection.rangeDiff', async () => {
      const history = MainPanel.active?.history;
      if (!history?.repository) return;
      const [newer, older] = await history.selectedCommits();
      if (newer && older) await page().open([`${older.oid}...${newer.oid}`]).catch(showError);
    }),
    // Reflog のこの時点の HEAD と今の HEAD を、同じ一連のコミットの 2 つの版として比べる。今の HEAD は一番上の行の
    // コミット（`HEAD` と書くと、比べている間に動いたときに別のものになる）
    vscode.commands.registerCommand('amonhen.reflog.rangeDiff', async (ctx?: ReflogContext) => {
      if (manager.current && ctx?.oid && ctx.reflogHead) await page().open([`${ctx.oid}...${ctx.reflogHead}`]).catch(showError);
    }),
    // パレットの Range Diff…: 今のブランチの前の版と比べて開く（範囲は上の欄で直せる）
    vscode.commands.registerCommand('amonhen.rangeDiff', async () => {
      if (manager.current) await page().openPrevious().catch(showError);
    }),
    vscode.commands.registerCommand('amonhen.rangeDiff.showOldInHistory', (ctx?: PairContext) => showInHistory(ctx?.oldOid)),
    vscode.commands.registerCommand('amonhen.rangeDiff.showNewInHistory', (ctx?: PairContext) => showInHistory(ctx?.newOid)),
    vscode.commands.registerCommand('amonhen.rangeDiff.copyOldId', (ctx?: PairContext) => copy(ctx?.oldOid)),
    vscode.commands.registerCommand('amonhen.rangeDiff.copyNewId', (ctx?: PairContext) => copy(ctx?.newOid)),
  );
}
