/** メインビューのナビで、ブランチなどを右クリックしたときのコマンド */
import * as vscode from 'vscode';
import { GitError } from '../git/exec.ts';
import { mergeInto, rebaseOnto, shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import { runAction } from './dragDrop.ts';
import { MainPanel } from './mainPanel.ts';
import { showError } from './stageCommands.ts';

/** Webview の右クリックメニューから渡される情報（data-vscode-context） */
interface RefContext {
  ref: string;
  refKind: 'branch' | 'remoteBranch' | 'tag';
  current: boolean;
}

export function registerNavCommands(): vscode.Disposable {
  const command = (id: string, fn: (repo: Repository, ctx: RefContext, head: string | undefined) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (ctx?: RefContext) => {
      const repo = MainPanel.active?.repository;
      if (repo && ctx?.ref) {
        await fn(repo, ctx, (await repo.backend.head()).branch);
      }
    });

  return vscode.Disposable.from(
    command('amonhen.nav.checkout', async (_repo, ctx) => {
      await MainPanel.active?.checkout(ctx.ref);
    }),
    command('amonhen.nav.merge', async (repo, ctx, head) => {
      if (!head) return;
      await runAction(repo, {
        label: `$(git-merge) Merge ${shortName(ctx.ref)} into ${shortName(head)}`,
        run: () => mergeInto(repo.backend, repo.undo, ctx.ref, head),
      });
    }),
    command('amonhen.nav.rebase', async (repo, ctx, head) => {
      if (!head) return;
      await runAction(repo, {
        label: `$(git-pull-request) Rebase ${shortName(head)} onto ${shortName(ctx.ref)}`,
        run: () => rebaseOnto(repo.backend, repo.undo, head, ctx.ref),
      });
    }),
    command('amonhen.nav.delete', async (repo, ctx) => {
      const name = shortName(ctx.ref);
      const choice = await vscode.window.showWarningMessage(
        `ブランチ ${name} を削除しますか？`,
        { modal: true, detail: 'Undo で戻せます。' },
        'Delete',
      );
      if (choice !== 'Delete') return;
      const remove = (force: boolean) =>
        repo.undo.record(`Delete ${name}`, [ctx.ref], 'keep', () => repo.backend.deleteBranch(name, force));
      try {
        await remove(false);
      } catch (err) {
        // merge されていないブランチは、確かめてから強制的に消す
        if (!(err instanceof GitError) || !/not fully merged/.test(err.stderr)) {
          showError(err);
          return;
        }
        const force = await vscode.window.showWarningMessage(
          `${name} は merge されていないコミットを含んでいます。削除しますか？`,
          { modal: true, detail: 'Undo で戻せます。' },
          'Delete Anyway',
        );
        if (force === 'Delete Anyway') await remove(true).catch(showError);
      } finally {
        await repo.refresh();
      }
    }),
  );
}
