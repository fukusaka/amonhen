/**
 * History と Reflog の行から、そのコミットを checkout（detached）する・今のブランチをそこへ reset する、
 * Reflog の行を History で見せる・名前をコピーする
 */
import * as vscode from 'vscode';
import { confirmOperation, uncommittedLine } from './confirm.ts';
import { daemonFor } from '../daemon/connection.ts';
import { moveDrawing, type MovePreview } from '../history/drawing.ts';
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
  const ok = await confirmOperation(
    `Checkout ${ctx.oid.slice(0, 8)}?`,
    ['HEAD is detached (on no branch).', uncommittedLine(repo)],
    'Checkout',
  );
  if (!ok) return;
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
  const mode = await askResetMode(repo, ctx, branch);
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

/**
 * Reset の動かし方を選ばせる。メインビューが開いていれば、アプリと同じくシートで: 前と後の絵（外れるコミット）、
 * 選んだ動かし方で今との違いがどこへ行くか、外れるコミットを上流が持っていれば、このあとの Push に Force Push が
 * 要ること。開いていない・描けなければモーダルのボタンで
 */
async function askResetMode(repo: Repository, ctx: CommitContext, branch: string): Promise<ResetMode | undefined> {
  const title = `Reset ${branch} to ${commitName(ctx)}?`;
  const panel = MainPanel.active;
  const current = repo.current.status?.branch.head;
  const preview = panel?.canAsk
    ? await repo.backend.graphPreview<MovePreview>({ op: 'movePreview', from: 'HEAD', to: ctx.oid, ...(current ? { branch: current } : {}) })
    : undefined;
  const drawing = moveDrawing(preview, { moving: branch, kind: 'local', afterTitle: 'After the reset', lostNote: 'taken off' });
  if (panel && preview && drawing) {
    const files = preview?.files ?? 0;
    const modes: [ResetMode, string, string][] = [
      ['soft', 'Soft: keep the changes staged', `The difference from now (${files} files) stays staged.`],
      ['mixed', 'Mixed: keep the changes in the working tree', `The difference from now (${files} files) stays in the working tree.`],
      [
        'hard',
        'Hard: match the commit (uncommitted changes are stashed first)',
        `The working tree matches the commit; the difference from now (${files} files) goes away (Undo brings it back).`,
      ],
    ];
    const answer = await panel.ask({
      title,
      lines: ['Commits taken off stay in Reflog. You can undo this.'],
      warnings:
        preview && preview.upstreamHas > 0 && preview.upstream
          ? [`${preview.upstream} has commits this takes off, so pushing afterwards needs Force Push.`]
          : [],
      drawing,
      choices: modes.map(([, label, note]) => ({ label, lines: files > 0 ? [note] : [] })),
      choice: 1,
      buttons: ['Reset'],
    });
    return answer && modes[answer.choice]?.[0];
  }
  const modes: Record<string, ResetMode> = { Mixed: 'mixed', Soft: 'soft', Hard: 'hard' };
  const answer = await vscode.window.showWarningMessage(
    title,
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
  return answer ? modes[answer] : undefined;
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
