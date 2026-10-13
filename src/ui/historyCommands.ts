/** History の右クリックメニューと、Webview でのメッセージ編集・並べ替えから、コミットを編集する */
import * as vscode from 'vscode';
import type { RebaseResult } from '../git/backend.ts';
import { parseCommitObject } from '../git/commitObject.ts';
import type { HistoryEdit } from '../history/rebasePlan.ts';
import { rewriteHistory } from '../state/rewrite.ts';
import type { Repository } from '../state/repository.ts';
import { HistoryController } from './historyController.ts';
import { MainPanel } from './mainPanel.ts';
import { showError } from './stageCommands.ts';

/** Webview の右クリックメニューから渡される情報（data-vscode-context） */
interface CommitContext {
  oid: string;
}

export function registerHistoryCommands(): vscode.Disposable {
  const command = (id: string, fn: (panel: HistoryController, repo: Repository, oid: string) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (ctx?: CommitContext) => {
      const panel = MainPanel.active?.history;
      const repo = panel?.repository;
      if (panel && repo && ctx?.oid) {
        await fn(panel, repo, ctx.oid);
      }
    });

  return vscode.Disposable.from(
    command('amonhen.history.reword', async (panel, repo, oid) => {
      const message = await fullMessage(repo, oid);
      panel.post({ type: 'editMessage', oid, kind: 'reword', title: 'Reword', message });
    }),
    command('amonhen.history.squash', async (panel, repo, oid) => {
      const parent = panel.commit(oid)?.parents[0];
      if (!parent) return;
      const message = `${(await fullMessage(repo, parent)).trimEnd()}\n\n${await fullMessage(repo, oid)}`;
      panel.post({ type: 'editMessage', oid, kind: 'squash', title: 'Squash into Parent', message });
    }),
    command('amonhen.history.fixup', (panel, repo, oid) =>
      run(panel, repo, `Fixup ${short(panel, oid)}`, { kind: 'fixup', oid }),
    ),
    command('amonhen.history.drop', async (panel, repo, oid) => {
      const choice = await vscode.window.showWarningMessage(
        `${short(panel, oid)} を履歴から取り除きますか？（Undo で戻せます）`,
        { modal: true },
        'Drop',
      );
      if (choice === 'Drop') {
        await run(panel, repo, `Drop ${short(panel, oid)}`, { kind: 'drop', oid });
      }
    }),
    command('amonhen.history.edit', (panel, repo, oid) =>
      run(panel, repo, `Edit ${short(panel, oid)}`, { kind: 'edit', oid }),
    ),
    command('amonhen.history.split', (panel, repo, oid) =>
      run(panel, repo, `Split ${short(panel, oid)}`, { kind: 'split', oid }),
    ),
    command('amonhen.history.moveUp', (panel, repo, oid) => move(panel, repo, oid, -1)),
    command('amonhen.history.moveDown', (panel, repo, oid) => move(panel, repo, oid, +1)),
    command('amonhen.history.copyId', async (_panel, _repo, oid) => {
      await vscode.env.clipboard.writeText(oid);
      vscode.window.setStatusBarMessage(`$(copy) ${oid.slice(0, 8)}`, 3000);
    }),
    HistoryController.onDidReceiveEdit(async (message) => {
      const panel = MainPanel.active?.history;
      const repo = panel?.repository;
      if (!panel || !repo) return;
      if (message.type === 'saveMessage') {
        const label = `${message.kind === 'reword' ? 'Reword' : 'Squash'} ${short(panel, message.oid)}`;
        await run(panel, repo, label, { kind: message.kind, oid: message.oid, message: message.message });
      } else {
        await run(panel, repo, `Move ${short(panel, message.oid)}`, { kind: 'move', oid: message.oid, to: message.to });
      }
    }),
  );
}

function short(panel: HistoryController, oid: string): string {
  const subject = panel.commit(oid)?.subject;
  return subject ? `"${subject}"` : oid.slice(0, 8);
}

async function move(panel: HistoryController, repo: Repository, oid: string, delta: number): Promise<void> {
  const index = panel.chainIndex(oid);
  if (index === undefined) return;
  await run(panel, repo, `Move ${short(panel, oid)}`, { kind: 'move', oid, to: index + delta });
}

/** コミットのメッセージ全体（subject と本文） */
async function fullMessage(repo: Repository, oid: string): Promise<string> {
  const obj = await repo.backend.object(oid);
  return obj ? parseCommitObject(obj.data.toString('utf8')).message : '';
}

async function run(panel: HistoryController, repo: Repository, label: string, edit: HistoryEdit): Promise<void> {
  let result: RebaseResult | undefined;
  panel.post({ type: 'busy', busy: true });
  try {
    result = await vscode.window.withProgress({ location: vscode.ProgressLocation.Window, title: label }, () =>
      rewriteHistory(repo.backend, repo.undo, label, edit),
    );
  } catch (err) {
    showError(err);
  } finally {
    panel.post({ type: 'busy', busy: false });
    await repo.refresh();
  }
  switch (result) {
    case 'done': {
      const choice = await vscode.window.showInformationMessage(`${label}: 完了しました`, 'Undo');
      if (choice === 'Undo') {
        await vscode.commands.executeCommand('amonhen.undo');
      }
      break;
    }
    case 'stopped':
      void vscode.window.showInformationMessage(
        edit.kind === 'split'
          ? `${label}: 変更を Working Copy に戻しました。分けてコミットしてから、サイドバーの Continue で続けてください`
          : `${label}: このコミットで止めました。変更してコミット（amend）してから、サイドバーの Continue で続けてください`,
      );
      break;
    case 'conflict':
      void vscode.window.showWarningMessage(
        `${label}: コンフリクトが発生しました。解決して stage してから Continue するか、Abort してください`,
      );
      break;
  }
}
