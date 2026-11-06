/**
 * ignore されたファイルを消す（アプリの Delete Ignored Files…）。git clean -X が消すもの（フォルダは 1 つ）を先に
 * 見せ、選んだものだけを、戻せないと断ってから消す。共有の daemon のコアで行う
 */
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { runEngine } from './engine.ts';
import { showError } from './stageCommands.ts';

export function registerIgnoredCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.deleteIgnored', async () => {
    const repo = manager.current;
    if (!repo) return;
    try {
      const reply = await runEngine(repo, { op: 'ignoredFiles' }, 'List ignored files');
      const paths = ((reply['info'] as { paths?: string[] } | undefined)?.paths ?? []).filter(Boolean);
      if (paths.length === 0) {
        void vscode.window.showInformationMessage('No ignored files to delete');
        return;
      }
      const picked = await vscode.window.showQuickPick(
        paths.map((p) => ({ label: p, picked: true, iconPath: new vscode.ThemeIcon(p.endsWith('/') ? 'folder' : 'file') })),
        { title: 'Delete Ignored Files', canPickMany: true, placeHolder: 'What git clean -X would delete (folders as one entry)' },
      );
      if (!picked || picked.length === 0) return;
      const count = picked.length;
      const answer = await vscode.window.showWarningMessage(
        `Delete ${count} ignored ${count === 1 ? 'entry' : 'entries'}?`,
        { modal: true, detail: `${picked.slice(0, 8).map((p) => p.label).join('\n')}${count > 8 ? '\n…' : ''}\n\nUndo can’t bring ${count === 1 ? 'it' : 'them'} back.` },
        'Delete',
      );
      if (answer !== 'Delete') return;
      await runEngine(repo, { op: 'deleteIgnored', paths: picked.map((p) => p.label) }, 'Delete ignored files');
      vscode.window.setStatusBarMessage(`$(trash) Deleted ${count} ignored ${count === 1 ? 'entry' : 'entries'}`, 5000);
    } catch (err) {
      showError(err);
    } finally {
      await repo.refresh();
    }
  });
}
