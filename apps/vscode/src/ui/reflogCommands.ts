/**
 * History と Reflog の行から、そのコミットを checkout（detached）する・今のブランチをそこへ reset する、
 * Reflog の行を History で見せる・名前をコピーする
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import { MainPanel } from './mainPanel.ts';
import { showError } from './stageCommands.ts';

/** Webview の右クリックメニューやボタンから渡される情報（data-vscode-context） */
interface CommitContext {
  oid: string;
  /** Reflog の行なら `HEAD@{n}` */
  selector?: string;
  subject?: string;
}

type ResetMode = 'soft' | 'mixed' | 'hard';

/** 確かめるときに出す、コミットの名前（短い ID と件名） */
function commitName(ctx: CommitContext): string {
  const subject = ctx.subject ?? MainPanel.active?.history.commit(ctx.oid)?.subject;
  return subject ? `${ctx.oid.slice(0, 8)} “${subject}”` : ctx.oid.slice(0, 8);
}

/** コミットを detached HEAD で checkout する */
async function checkoutCommit(repo: Repository, ctx: CommitContext): Promise<void> {
  try {
    await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: `Checkout ${ctx.oid.slice(0, 8)}` },
      () => repo.backend.switchTo({ oid: ctx.oid }),
    );
    vscode.window.setStatusBarMessage(`$(check) HEAD is now at ${commitName(ctx)} (detached)`, 5000);
  } finally {
    await repo.refresh();
  }
}

/**
 * 今のブランチ（detached なら HEAD）を、git reset のようにコミットへ動かす。Soft・Mixed・Hard を選んでもらう。
 * コアが Undo を記録し、Hard は作業中の変更を先に stash に残す
 */
async function resetToCommit(repo: Repository, ctx: CommitContext): Promise<void> {
  const client = await daemonFor(repo.root);
  if (!client) {
    void vscode.window.showWarningMessage('Reset needs the Amon Hen engine (amonhen.daemon.enabled)');
    return;
  }
  const branch = repo.current.status?.branch.head ?? 'HEAD';
  const modes: Record<string, ResetMode> = { Mixed: 'mixed', Soft: 'soft', Hard: 'hard' };
  const answer = await vscode.window.showWarningMessage(
    `Reset ${branch} to ${commitName(ctx)}?`,
    {
      modal: true,
      detail:
        'Mixed keeps the changes of the commits taken off in the working tree, unstaged. ' +
        'Soft keeps them staged. ' +
        'Hard makes the index and the working tree match the commit; uncommitted changes are saved as a stash first ' +
        '(untracked files are left alone).\n\n' +
        'Undo puts the branch back, and the commits taken off stay in Reflog.',
    },
    ...Object.keys(modes),
  );
  const mode = answer ? modes[answer] : undefined;
  if (!mode) return;
  const label = `Reset (${mode}) to ${ctx.oid.slice(0, 8)}`;
  let stashed = false;
  try {
    const reply = await vscode.window.withProgress({ location: vscode.ProgressLocation.Window, title: label }, async () =>
      (await client.call({ op: 'run', command: { op: 'reset', oid: ctx.oid, mode } })).json,
    );
    if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `${label} failed`));
    stashed = (reply['info'] as { stashed?: boolean } | undefined)?.stashed === true;
  } finally {
    await repo.refresh();
  }
  const choice = await vscode.window.showInformationMessage(
    stashed ? `${label}: done. Uncommitted changes were saved as a stash` : `${label}: done`,
    'Undo',
  );
  if (choice === 'Undo') {
    await vscode.commands.executeCommand('amonhen.undo');
  }
}

/** Reflog の行のコミットを History で選ぶ */
async function showInHistory(ctx: CommitContext): Promise<void> {
  const panel = MainPanel.active;
  if (!panel) return;
  panel.showView('history');
  if (!(await panel.history.reveal(ctx.oid))) {
    void vscode.window.showInformationMessage(`${ctx.oid.slice(0, 8)} isn’t in History (no branch or tag reaches it)`);
  }
}

export function registerReflogCommands(manager: RepositoryManager): vscode.Disposable {
  const command = (id: string, fn: (repo: Repository, ctx: CommitContext) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (ctx?: CommitContext) => {
      const repo = manager.current;
      if (repo && ctx?.oid) await fn(repo, ctx).catch(showError);
    });
  return vscode.Disposable.from(
    command('amonhen.checkoutCommit', checkoutCommit),
    command('amonhen.resetToCommit', resetToCommit),
    command('amonhen.reflog.showInHistory', (_repo, ctx) => showInHistory(ctx)),
    command('amonhen.reflog.copyName', async (_repo, ctx) => {
      if (!ctx.selector) return;
      await vscode.env.clipboard.writeText(ctx.selector);
      vscode.window.setStatusBarMessage(`$(copy) ${ctx.selector}`, 3000);
    }),
  );
}
