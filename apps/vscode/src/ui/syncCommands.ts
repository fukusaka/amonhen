/** 現在のブランチの Pull / Push / Publish */
import * as vscode from 'vscode';
import {
  publishBranch,
  publishChoices,
  pullCurrent,
  pushCurrent,
  shortName,
  type ReconcileChoice,
} from '../state/operations.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import { runAction } from './dragDrop.ts';

export function registerSyncCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.pull', async () => {
      const repo = manager.current;
      if (!repo) return;
      await runAction(repo, {
        label: '$(repo-pull) Pull',
        run: async () => {
          const result = await pullCurrent(repo.backend, repo.undo, chooseReconcile);
          if (result === 'no-upstream') {
            throw new Error('No upstream is set. Publish the branch to set one.');
          }
          return result;
        },
      });
    }),
    vscode.commands.registerCommand('amonhen.push', () => push(manager, false)),
    vscode.commands.registerCommand('amonhen.publish', async () => {
      const repo = manager.current;
      const branch = repo && (await repo.backend.head()).branch;
      if (repo && branch) await publish(repo, branch);
    }),
    vscode.commands.registerCommand('amonhen.forcePush', async () => {
      const choice = await vscode.window.showWarningMessage(
        'Force push the current branch?',
        { modal: true, detail: 'Replaces the remote’s history (--force-with-lease).' },
        'Force Push',
      );
      if (choice === 'Force Push') await push(manager, true);
    }),
  );
}

/**
 * 分かれたブランチを、取り込み方を決めずに pull するとき（git 2.33.1 からは git が断る）。この pull だけの取り込み方を
 * 選ぶ。設定は書かない
 */
async function chooseReconcile(branch: string, upstream: string): Promise<ReconcileChoice | undefined> {
  const choice = await vscode.window.showWarningMessage(
    `${shortName(branch)} and ${shortName(upstream)} have diverged`,
    {
      modal: true,
      detail:
        'Neither pull.rebase nor pull.ff says how to bring them together, so git won’t pull. Choose how for this pull (your settings stay as they are).',
    },
    'Merge',
    'Rebase',
  );
  return choice === 'Merge' ? 'merge' : choice === 'Rebase' ? 'rebase' : undefined;
}

async function push(manager: RepositoryManager, force: boolean): Promise<void> {
  const repo = manager.current;
  if (!repo) return;
  let unpublished: string | undefined;
  await runAction(repo, {
    label: force ? '$(warning) Force Push' : '$(repo-push) Push',
    run: async () => {
      const result = await pushCurrent(repo.backend, { force });
      if (result === 'detached') throw new Error('Can’t push a detached HEAD');
      if (result === 'unpublished') {
        unpublished = (await repo.backend.head()).branch;
        return undefined;
      }
      return 'pushed';
    },
  });
  // upstream が無ければ、送り先を選んで初めて送る
  if (unpublished && !force) await publish(repo, unpublished);
}

/**
 * ブランチ（`refs/heads/...`）を初めて送る。送り先のリモート（いくつもあれば選ぶ）とそこでの名前を決め、送ったらそれを
 * upstream にする。その名前のブランチがもうリモートにあれば（最後の Fetch の時点）、作るのではなく upstream にして送る
 */
export async function publish(repo: Repository, branch: string): Promise<void> {
  const name = shortName(branch);
  const { remotes, preferred } = await publishChoices(repo.backend, branch);
  const first = preferred ?? remotes[0];
  if (!first) {
    void vscode.window.showErrorMessage('There’s no remote. Add one first.');
    return;
  }
  const remote =
    remotes.length > 1
      ? await vscode.window.showQuickPick([first, ...remotes.filter((r) => r !== first)], {
          title: `Publish ${name}`,
          placeHolder: 'Remote to publish to',
        })
      : first;
  if (!remote) return;
  const remoteBranch = (
    await vscode.window.showInputBox({ title: `Publish ${name}`, prompt: `Name on ${remote}`, value: name })
  )?.trim();
  if (!remoteBranch) return;
  const target = `refs/remotes/${remote}/${remoteBranch}`;
  const existing = (await repo.backend.resolveRefs([target])).get(target);
  if (existing) {
    const short = `${remote}/${remoteBranch}`;
    if (!(await repo.backend.isAncestor(existing, branch))) {
      const choice = await vscode.window.showWarningMessage(
        `${short} already exists`,
        { modal: true, detail: `It has commits ${name} doesn’t have (as of the last fetch), so the push would be refused. Make it the upstream and pull first.` },
        'Set as Upstream',
      );
      if (choice) {
        await repo.backend.setUpstream(name, target);
        await repo.refresh();
      }
      return;
    }
    const choice = await vscode.window.showInformationMessage(
      `${short} already exists`,
      { modal: true, detail: 'Pushes there and makes it the upstream.' },
      'Push',
    );
    if (!choice) return;
  }
  await runAction(repo, {
    label: `$(cloud-upload) Publish ${name}`,
    run: async () => {
      await publishBranch(repo.backend, branch, remote, remoteBranch);
      return 'pushed';
    },
  });
}
