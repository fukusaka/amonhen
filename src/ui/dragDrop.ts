/**
 * ブランチのドラッグ＆ドロップで行う操作。ドロップはメインビューのナビで受ける。
 * - ブランチ（またはタグ）をローカルブランチにドロップ: Merge / Rebase を選ぶ
 * - ローカルブランチをリモート（またはリモートのブランチ）にドロップ: Push / Force Push を選ぶ
 * どちらもドロップ先を書き換える操作になる。
 */
import * as vscode from 'vscode';
import { mergeInto, rebaseOnto, shortName, type OperationResult } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import type { DragSource, DropTarget } from '../view/protocol.ts';
import { showError } from './stageCommands.ts';

export interface Action {
  label: string;
  detail?: string;
  run: () => Promise<OperationResult | 'pushed'>;
}

/** ドロップしたときの操作を選ばせて実行する */
export async function pickAndRun(repo: Repository, source: DragSource, target: DropTarget): Promise<void> {
  const actions = dropActions(repo, source, target);
  if (actions.length === 0) {
    return;
  }
  const picked = await vscode.window.showQuickPick(actions, { placeHolder: 'ドロップしたときの操作を選ぶ' });
  if (picked) {
    await runAction(repo, picked);
  }
}

function dropActions(repo: Repository, source: DragSource, target: DropTarget): Action[] {
  const sourceRef = source.ref;
  const src = shortName(sourceRef);
  const { backend, undo } = repo;

  // ローカルブランチへ: merge / rebase
  if (target.kind === 'branch') {
    const targetRef = target.ref;
    if (targetRef === sourceRef) {
      return [];
    }
    const tgt = shortName(targetRef);
    return [
      { label: `$(git-merge) Merge ${src} into ${tgt}`, run: () => mergeInto(backend, undo, sourceRef, targetRef) },
      {
        label: `$(git-pull-request) Rebase ${tgt} onto ${src}`,
        run: () => rebaseOnto(backend, undo, targetRef, sourceRef),
      },
    ];
  }

  // リモートへ: push。送れるのはローカルブランチだけ
  if (!source.local) {
    return [];
  }
  const remote = target.remote;
  const remoteBranch = target.kind === 'remote' ? src : target.ref.slice(`refs/remotes/${remote}/`.length);
  // upstream が無ければ、push 先を upstream にする
  const setUpstream = !source.upstream;
  const dest = `${remote}/${remoteBranch}`;
  const detail = setUpstream ? `${dest} を upstream に設定する` : undefined;
  const push = (force: boolean) => async () => {
    await backend.push(remote, src, remoteBranch, { force, setUpstream });
    return 'pushed' as const;
  };
  return [
    { label: `$(cloud-upload) Push ${src} to ${dest}`, detail, run: push(false) },
    {
      label: `$(warning) Force Push ${src} to ${dest}`,
      detail: '--force-with-lease。rebase した後など、リモートの履歴を上書きするとき',
      run: push(true),
    },
  ];
}

/** 操作を進捗付きで実行し、結果（最新・コンフリクト・完了）に合わせて知らせる */
export async function runAction(repo: Repository, action: Action): Promise<void> {
  const title = action.label.replace(/^\$\([\w-]+\) /, '');
  let result: OperationResult | 'pushed' | undefined;
  try {
    result = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title }, () =>
      action.run(),
    );
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
  switch (result) {
    case 'up-to-date':
      void vscode.window.showInformationMessage(`${title}: すでに最新です`);
      break;
    case 'conflict':
      await handleConflict(repo, title);
      break;
    case 'done': {
      const choice = await vscode.window.showInformationMessage(`${title}: 完了しました`, 'Undo');
      if (choice === 'Undo') {
        await vscode.commands.executeCommand('amonhen.undo');
      }
      break;
    }
    case 'pushed':
      vscode.window.setStatusBarMessage(`$(check) ${title}`, 5000);
      break;
  }
}

/** コンフリクトしたら、ここで中止するか、解決して Continue するかを選ばせる */
async function handleConflict(repo: Repository, title: string): Promise<void> {
  const sequence = await repo.backend.sequenceInProgress();
  const choice = await vscode.window.showWarningMessage(
    `${title}: コンフリクトが発生しました。解決して stage してから、サイドバーの Continue で続けるか、中止してください`,
    'Abort',
  );
  if (choice === 'Abort' && sequence) {
    try {
      await repo.backend.abort(sequence);
    } catch (err) {
      showError(err);
    } finally {
      await repo.refresh();
    }
  }
}
