/** サイドバーのナビで、ブランチなどを右クリックしたときのコマンド */
import * as vscode from 'vscode';
import { GitError } from '../git/exec.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { mergeInto, rebaseOnto, shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import { runAction } from './dragDrop.ts';
import type { Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';

/** ブランチ・リモートブランチ・タグのノードから ref を取り出す */
function refOf(node: Node | undefined): string | undefined {
  return node?.kind === 'branch' || node?.kind === 'tag' ? node.ref.name : undefined;
}

export function registerNavCommands(manager: RepositoryManager): vscode.Disposable {
  const command = (id: string, fn: (repo: Repository, ref: string, head: string | undefined) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (node?: Node) => {
      const repo = manager.current;
      const ref = refOf(node);
      if (repo && ref) {
        await fn(repo, ref, (await repo.backend.head()).branch);
      }
    });

  return vscode.Disposable.from(
    command('amonhen.nav.checkout', (repo, ref) => checkoutRef(repo, ref)),
    command('amonhen.nav.merge', async (repo, ref, head) => {
      if (!head) return;
      await runAction(repo, {
        label: `$(git-merge) Merge ${shortName(ref)} into ${shortName(head)}`,
        run: () => mergeInto(repo.backend, repo.undo, ref, head),
      });
    }),
    command('amonhen.nav.rebase', async (repo, ref, head) => {
      if (!head) return;
      await runAction(repo, {
        label: `$(git-pull-request) Rebase ${shortName(head)} onto ${shortName(ref)}`,
        run: () => rebaseOnto(repo.backend, repo.undo, head, ref),
      });
    }),
    command('amonhen.nav.delete', (repo, ref) => deleteBranch(repo, ref)),
  );
}

/** ローカルブランチなら切り替え、リモートブランチなら追跡するブランチを作って切り替える */
export async function checkoutRef(repo: Repository, ref: string): Promise<void> {
  try {
    if (ref.startsWith('refs/heads/')) {
      await repo.backend.switchBranch(shortName(ref));
    } else if (ref.startsWith('refs/remotes/')) {
      const local = (repo.current.branches ?? []).find((b) => b.upstream === ref);
      await (local ? repo.backend.switchBranch(shortName(local.name)) : repo.backend.trackBranch(shortName(ref)));
    }
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}

/** ブランチを削除する。Undo で戻せる。merge されていなければ、もう一度確かめてから消す */
async function deleteBranch(repo: Repository, ref: string): Promise<void> {
  const name = shortName(ref);
  const choice = await vscode.window.showWarningMessage(
    `ブランチ ${name} を削除しますか？`,
    { modal: true, detail: 'Undo で戻せます。' },
    'Delete',
  );
  if (choice !== 'Delete') return;
  const remove = (force: boolean) =>
    repo.undo.record(`Delete ${name}`, [ref], 'keep', () => repo.backend.deleteBranch(name, force));
  try {
    await remove(false);
  } catch (err) {
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
}
