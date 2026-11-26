/** 現在のブランチの Pull / Push / Publish */
import * as vscode from 'vscode';
import { publishBranch, publishChoices, shortName } from '../state/operations.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import { runAction } from './dragDrop.ts';
import { pullWithSheet } from './pullSheet.ts';
import { pushWithSheet } from './pushSheet.ts';
import { refusedMidSequence } from './sequenceCommands.ts';

/** 送るコミットを数える上限（それより多ければ「200+」） */
const LIMIT = 200;

export function registerSyncCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.pull', async () => {
      const repo = manager.current;
      if (repo && !(await refusedMidSequence(repo))) await pullWithSheet(repo);
    }),
    // Commit and Push は { quiet: true } を渡す（上流が先に進んでいなければ確かめずに送る）
    vscode.commands.registerCommand('amonhen.push', async (arg?: unknown) => {
      const repo = manager.current;
      const quiet = (arg as { quiet?: unknown } | undefined)?.quiet === true;
      if (repo) await pushWithSheet(repo, { quiet }, publish);
    }),
    vscode.commands.registerCommand('amonhen.publish', async () => {
      const repo = manager.current;
      const branch = repo && (await repo.backend.head()).branch;
      if (repo && branch) await publish(repo, branch);
    }),
    vscode.commands.registerCommand('amonhen.forcePush', async () => {
      const repo = manager.current;
      if (repo) await pushWithSheet(repo, { force: true }, publish);
    }),
  );
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
    // アプリは Publish を押せなくし、ツールチップでこう言う（ブランチの行とコマンドパレットには出さない）
    void vscode.window.showErrorMessage('No remote to publish to. Add one with Add Remote….');
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
    // 送るものがあれば Push、同じなら upstream にするだけ（アプリと同じ文とボタン）
    const outgoing = (await repo.backend.log([`${target}..${branch}`], 0, LIMIT).catch(() => [])).length;
    const count = outgoing >= LIMIT ? `${LIMIT}+` : String(outgoing);
    const choice = await vscode.window.showInformationMessage(
      `${short} already exists`,
      {
        modal: true,
        detail:
          outgoing > 0
            ? `This pushes ${count} ${outgoing === 1 ? 'commit' : 'commits'} to it and makes it the upstream.`
            : `It is the same as ${name}. This makes it the upstream.`,
      },
      outgoing > 0 ? 'Push' : 'Set Upstream',
    );
    if (!choice) return;
  }
  await runAction(repo, {
    label: `$(cloud-upload) Publish ${name}`,
    op: 'push',
    run: async () => {
      await publishBranch(repo.backend, branch, remote, remoteBranch);
      return 'pushed';
    },
  });
}
