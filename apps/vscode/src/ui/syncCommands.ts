/** 現在のブランチの Pull / Push / Publish */
import * as vscode from 'vscode';
import { publishBranch, publishChoices, pullCurrent, pushCurrent, shortName } from '../state/operations.ts';
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
          const result = await pullCurrent(repo.backend, repo.undo);
          if (result === 'no-upstream') {
            throw new Error('upstream が設定されていません。Publish でリモートに送ると設定されます');
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
  let unpublished: string | undefined;
  await runAction(repo, {
    label: force ? '$(warning) Force Push' : '$(repo-push) Push',
    run: async () => {
      const result = await pushCurrent(repo.backend, { force });
      if (result === 'detached') throw new Error('detached HEAD では push できません');
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
    void vscode.window.showErrorMessage('リモートがありません。先にリモートを足してください');
    return;
  }
  const remote =
    remotes.length > 1
      ? await vscode.window.showQuickPick([first, ...remotes.filter((r) => r !== first)], {
          title: `Publish ${name}`,
          placeHolder: 'ブランチを作るリモート',
        })
      : first;
  if (!remote) return;
  const remoteBranch = (
    await vscode.window.showInputBox({ title: `Publish ${name}`, prompt: `${remote} での名前`, value: name })
  )?.trim();
  if (!remoteBranch) return;
  const target = `refs/remotes/${remote}/${remoteBranch}`;
  const existing = (await repo.backend.resolveRefs([target])).get(target);
  if (existing) {
    const short = `${remote}/${remoteBranch}`;
    if (!(await repo.backend.isAncestor(existing, branch))) {
      const choice = await vscode.window.showWarningMessage(
        `${short} はもうあります`,
        { modal: true, detail: `${name} にないコミットがあるので（最後の Fetch の時点）、push は断られます。upstream にして、先に Pull してください。` },
        'upstream にする',
      );
      if (choice) {
        await repo.backend.setUpstream(name, target);
        await repo.refresh();
      }
      return;
    }
    const choice = await vscode.window.showInformationMessage(
      `${short} はもうあります`,
      { modal: true, detail: 'そこへ push して、upstream にします。' },
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
