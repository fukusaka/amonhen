/** History の右クリックメニューと、Webview でのメッセージ編集・並べ替えから、コミットを編集する */
import * as vscode from 'vscode';
import type { RebaseResult } from '../git/backend.ts';
import type { Commit } from '../git/log.ts';
import { parseCommitObject } from '../git/commitObject.ts';
import type { HistoryEdit } from '../history/rebasePlan.ts';
import { rewriteHistory } from '../state/rewrite.ts';
import type { Repository } from '../state/repository.ts';
import { HistoryController } from './historyController.ts';
import { MainPanel } from './mainPanel.ts';
import { mergeBranches } from './mergeBranches.ts';
import { runAction } from './dragDrop.ts';
import { confirmRewrite } from './rewriteConfirm.ts';
import { sequenceRowName } from './sidebar.ts';
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

  const selected = (id: string, fn: (panel: HistoryController, repo: Repository, commits: Commit[]) => Promise<void>) =>
    vscode.commands.registerCommand(id, async () => {
      const panel = MainPanel.active?.history;
      const repo = panel?.repository;
      if (!panel || !repo) return;
      try {
        const commits = await panel.selectedCommits();
        if (commits.length > 1) await fn(panel, repo, commits);
      } catch (err) {
        showError(err);
      }
    });

  return vscode.Disposable.from(
    command('amonhen.history.reword', async (panel, repo, oid) => {
      const message = await fullMessage(repo, oid);
      panel.post({ type: 'editMessage', oid, kind: 'reword', title: EDIT_TITLES.reword, message });
    }),
    command('amonhen.history.squash', async (panel, repo, oid) => {
      const parent = panel.commit(oid)?.parents[0];
      if (!parent) return;
      const message = `${(await fullMessage(repo, parent)).trimEnd()}\n\n${await fullMessage(repo, oid)}`;
      panel.post({ type: 'editMessage', oid, kind: 'squash', title: EDIT_TITLES.squash, message });
    }),
    command('amonhen.history.fixup', (panel, repo, oid) =>
      run(panel, repo, `Fixup ${short(panel, oid)}`, { kind: 'fixup', oid }),
    ),
    command('amonhen.history.drop', (panel, repo, oid) =>
      run(panel, repo, `Drop ${short(panel, oid)}`, { kind: 'drop', oid }),
    ),
    command('amonhen.history.edit', (panel, repo, oid) =>
      run(panel, repo, `Edit ${short(panel, oid)}`, { kind: 'edit', oid }),
    ),
    command('amonhen.history.split', (panel, repo, oid) =>
      run(panel, repo, `Split ${short(panel, oid)}`, { kind: 'split', oid }),
    ),
    command('amonhen.history.moveUp', (panel, repo, oid) => move(panel, repo, oid, -1)),
    command('amonhen.history.moveDown', (panel, repo, oid) => move(panel, repo, oid, +1)),
    command('amonhen.history.cherryPick', (panel, repo, oid) => applyCommit(panel, repo, oid, 'cherry-pick')),
    command('amonhen.history.revert', (panel, repo, oid) => applyCommit(panel, repo, oid, 'revert')),
    // 親（最初の親）・子のコミットへ（子がいくつもあれば選ぶ）。エンジンだけ
    command('amonhen.history.goToParent', (panel, _repo, oid) => panel.goTo('parent', oid)),
    command('amonhen.history.goToChild', (panel, _repo, oid) => panel.goTo('child', oid)),
    // いくつも選んだコミットをまとめて（エンジンだけ）
    selected('amonhen.history.selection.cherryPick', (_panel, repo, commits) => pickSelected(repo, commits, 'cherry-pick')),
    selected('amonhen.history.selection.revert', (_panel, repo, commits) => pickSelected(repo, commits, 'revert')),
    selected('amonhen.history.selection.combine', async (panel, _repo, commits) => {
      const { oldest, newest, message } = await panel.combineMessage(commits.map((c) => c.oid));
      panel.post({ type: 'editMessage', oid: oldest, last: newest, kind: 'combine', title: `Combine ${commits.length} Commits`, message });
    }),
    selected('amonhen.history.selection.merge', async (panel, repo, commits) => {
      const revs = commits.filter((c) => c.oid !== panel.headCommit).map((c) => panel.mergeName(c.oid));
      await mergeBranches(repo, revs);
    }),
    selected('amonhen.history.selection.copyIds', async (_panel, _repo, commits) => {
      await vscode.env.clipboard.writeText(commits.map((c) => c.oid).join('\n'));
      vscode.window.setStatusBarMessage(`$(copy) ${commits.length} commit IDs`, 3000);
    }),
    command('amonhen.history.copyId', async (_panel, _repo, oid) => {
      await vscode.env.clipboard.writeText(oid);
      vscode.window.setStatusBarMessage(`$(copy) ${oid.slice(0, 8)}`, 3000);
    }),
    HistoryController.onDidReceiveEdit(async (message) => {
      const panel = MainPanel.active?.history;
      const repo = panel?.repository;
      if (!panel || !repo) return;
      if (message.type === 'saveMessage') {
        if (message.kind === 'combine') {
          const edit = { kind: 'squashRange', oid: message.oid, last: message.last!, message: message.message } as const;
          await run(panel, repo, 'Combine commits', edit);
          return;
        }
        const label = `${message.kind === 'reword' ? 'Reword' : 'Squash'} ${short(panel, message.oid)}`;
        await run(panel, repo, label, { kind: message.kind, oid: message.oid, message: message.message });
      } else {
        await run(panel, repo, `Move ${short(panel, message.oid)}`, { kind: 'move', oid: message.oid, to: message.to });
      }
    }),
  );
}

/** コミットを現在のブランチに当てる（cherry-pick）か、打ち消す（revert）。Undo で戻せる */
async function applyCommit(
  panel: HistoryController,
  repo: Repository,
  oid: string,
  kind: 'cherry-pick' | 'revert',
): Promise<void> {
  const isMerge = (panel.commit(oid)?.parents.length ?? 0) > 1;
  const branch = (await repo.backend.head()).branch;
  const label = `${kind === 'cherry-pick' ? 'Cherry-pick' : 'Revert'} ${short(panel, oid)}`;
  await runAction(repo, {
    label: `$(${kind === 'cherry-pick' ? 'git-commit' : 'discard'}) ${label}`,
    run: async () => {
      const result = await repo.undo.record(
        label,
        branch ? [branch] : [],
        'keep',
        () => (kind === 'cherry-pick' ? repo.backend.cherryPick(oid, isMerge) : repo.backend.revert(oid, isMerge)),
        { skip: (r) => r !== 'done' },
      );
      return result === 'stopped' ? 'conflict' : result;
    },
  });
}

/** いくつものコミットを 1 回の命令で当てる。commits は新しい順 */
async function pickSelected(
  repo: Repository,
  commits: readonly Commit[],
  kind: 'cherry-pick' | 'revert',
): Promise<void> {
  const name = kind === 'cherry-pick' ? 'Cherry-pick' : 'Revert';
  const branch = (await repo.backend.head()).branch;
  const onto = branch ? branch.replace(/^refs\/heads\//, '') : 'HEAD';
  const what = `${commits.length} commits`;
  const answer = await vscode.window.showWarningMessage(
    kind === 'cherry-pick' ? `Cherry-pick ${what} onto ${onto}?` : `Revert ${what} on ${onto}?`,
    { modal: true, detail: `${kind === 'cherry-pick' ? 'Adds copies of them.' : 'Adds commits that undo them.'} You can undo this.` },
    name,
  );
  if (answer !== name) return;
  // Cherry-pick は古い順、Revert は新しい順（アプリと同じ）
  const oids = (kind === 'cherry-pick' ? [...commits].reverse() : commits).map((c) => c.oid);
  const isMerge = commits.some((c) => c.parents.length > 1);
  const label = `${name} ${what}`;
  await runAction(repo, {
    label: `$(${kind === 'cherry-pick' ? 'git-commit' : 'discard'}) ${label}`,
    run: async () => {
      const result = await repo.undo.record(
        label,
        branch ? [branch] : [],
        'keep',
        () => (kind === 'cherry-pick' ? repo.backend.cherryPick(oids, isMerge) : repo.backend.revert(oids, isMerge)),
        { skip: (r) => r !== 'done' },
      );
      return result === 'stopped' ? 'conflict' : result;
    },
  });
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

/** メッセージの編集欄の題 */
const EDIT_TITLES = { reword: 'Reword', squash: 'Squash into Parent' } as const;

/** 編集ごとの確認の題と、書き添えること */
const KINDS: Record<HistoryEdit['kind'], { name: string; note?: string }> = {
  reword: { name: 'Reword' },
  squash: { name: 'Squash', note: '親のコミットにまとめます。' },
  squashRange: { name: 'Combine', note: 'Combines the selected commits into one.' },
  fixup: { name: 'Fixup', note: '親のコミットにまとめ、親のメッセージを残します。' },
  drop: { name: 'Drop', note: '履歴から取り除きます。' },
  edit: { name: 'Edit', note: 'このコミットで止まります。変更して amend してから Continue で続けます。' },
  split: { name: 'Split', note: 'このコミットを取り消して、変更を Working Copy に戻したところで止まります。' },
  move: { name: 'Move' },
};

async function run(panel: HistoryController, repo: Repository, label: string, edit: HistoryEdit): Promise<void> {
  // コミットを書き換えるので、見通し（一緒に動くブランチ・コンフリクトの見込み）を添えて確かめる
  const kind = KINDS[edit.kind];
  const title = edit.kind === 'squashRange' ? 'Combine the selected commits?' : `${short(panel, edit.oid)} を ${kind.name} しますか？`;
  const choice = await confirmRewrite(repo, title, kind.name, { edit }, kind.note);
  if (!choice) {
    // 書いたメッセージを失わないように、書いたままの編集欄を開き直す（Webview は Save で欄を閉じている）
    if (edit.kind === 'reword' || edit.kind === 'squash') {
      panel.post({ type: 'editMessage', oid: edit.oid, kind: edit.kind, title: EDIT_TITLES[edit.kind], message: edit.message });
    } else if (edit.kind === 'squashRange') {
      panel.post({ type: 'editMessage', oid: edit.oid, last: edit.last, kind: 'combine', title: 'Combine Commits', message: edit.message });
    }
    return;
  }
  let result: RebaseResult | undefined;
  panel.post({ type: 'busy', busy: true });
  try {
    result = await vscode.window.withProgress({ location: vscode.ProgressLocation.Window, title: label }, () =>
      rewriteHistory(repo.backend, repo.undo, label, edit, choice.updateRefs),
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
        `${label}: コンフリクトが発生しました。解決して stage してから、サイドバーの「${sequenceRowName('rebase')}」の行の Continue で続けるか、Abort で中止してください`,
      );
      break;
  }
}
