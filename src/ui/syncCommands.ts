/** 現在のブランチの Pull / Push */
import * as vscode from 'vscode';
import { pullCurrent, pushCurrent } from '../state/operations.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { runAction } from './dragDrop.ts';

export function registerSyncCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.pull', async () => {
      const repo = manager.current;
      if (!repo) return;
      await runAction(repo, {
        label: '$(repo-pull) Pull',
        run: async () => {
          const result = await pullCurrent(repo.backend, repo.undo);
          if (result === 'no-upstream') {
            throw new Error('upstream が設定されていません。ブランチをリモートにドラッグして push すると設定されます');
          }
          return result;
        },
      });
    }),
    vscode.commands.registerCommand('amonhen.push', () => push(manager, false)),
    vscode.commands.registerCommand('amonhen.forcePush', async () => {
      const choice = await vscode.window.showWarningMessage(
        '現在のブランチを Force Push しますか？',
        { modal: true, detail: '--force-with-lease で、リモートの履歴を上書きします。' },
        'Force Push',
      );
      if (choice === 'Force Push') await push(manager, true);
    }),
  );
}

async function push(manager: RepositoryManager, force: boolean): Promise<void> {
  const repo = manager.current;
  if (!repo) return;
  await runAction(repo, {
    label: force ? '$(warning) Force Push' : '$(repo-push) Push',
    run: async () => {
      const result = await pushCurrent(repo.backend, { force });
      if (result === 'detached') throw new Error('detached HEAD では push できません');
      if (result === 'no-remote')
        throw new Error('push 先のリモートを決められません。ブランチをリモートにドラッグしてください');
      return 'pushed';
    },
  });
}
