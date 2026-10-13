/**
 * サイドバーのドラッグ＆ドロップ。
 * - ブランチ（またはタグ）をローカルブランチにドロップ: Merge / Rebase を選ぶ
 * - ローカルブランチをリモート（またはリモートのブランチ）にドロップ: Push / Force Push を選ぶ
 * どちらもドロップ先を書き換える操作になる。
 */
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { mergeInto, rebaseOnto, shortName, type OperationResult } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import type { Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';

const MIME = 'application/vnd.code.tree.amonhen.sidebar';

interface Action {
  label: string;
  detail?: string;
  run: () => Promise<OperationResult | 'pushed'>;
}

export class SidebarDragAndDrop implements vscode.TreeDragAndDropController<Node> {
  readonly dragMimeTypes = [MIME];
  readonly dropMimeTypes = [MIME];
  private readonly manager: RepositoryManager;

  constructor(manager: RepositoryManager) {
    this.manager = manager;
  }

  handleDrag(source: readonly Node[], data: vscode.DataTransfer): void {
    const node = source.length === 1 ? source[0] : undefined;
    if (node && (node.kind === 'branch' || node.kind === 'tag')) {
      data.set(MIME, new vscode.DataTransferItem(node));
    }
  }

  async handleDrop(target: Node | undefined, data: vscode.DataTransfer): Promise<void> {
    const source = data.get(MIME)?.value as Node | undefined;
    const repo = this.manager.current;
    if (!repo || !source || !target || (source.kind !== 'branch' && source.kind !== 'tag')) {
      return;
    }
    const actions = dropActions(repo, source, target);
    if (actions.length === 0) {
      return;
    }
    const picked = await vscode.window.showQuickPick(actions, { placeHolder: 'ドロップしたときの操作を選ぶ' });
    if (picked) {
      await runAction(repo, picked);
    }
  }
}

function dropActions(repo: Repository, source: Extract<Node, { kind: 'branch' | 'tag' }>, target: Node): Action[] {
  const sourceRef = source.ref.name;
  const src = shortName(sourceRef);
  const { backend, undo } = repo;

  // ローカルブランチへ: merge / rebase
  if (target.kind === 'branch' && !target.remote) {
    const targetRef = target.ref.name;
    if (targetRef === sourceRef) {
      return [];
    }
    const tgt = shortName(targetRef);
    return [
      { label: `$(git-merge) Merge ${src} into ${tgt}`, run: () => mergeInto(backend, undo, sourceRef, targetRef) },
      { label: `$(git-pull-request) Rebase ${tgt} onto ${src}`, run: () => rebaseOnto(backend, undo, targetRef, sourceRef) },
    ];
  }

  // リモートへ: push。送れるのはローカルブランチだけ
  if (source.kind !== 'branch' || source.remote) {
    return [];
  }
  let remote: string;
  let remoteBranch: string;
  if (target.kind === 'remote') {
    remote = target.remote.name;
    remoteBranch = src;
  } else if (target.kind === 'branch' && target.remote) {
    remote = target.remote;
    remoteBranch = target.ref.name.slice(`refs/remotes/${remote}/`.length);
  } else {
    return [];
  }
  // upstream が無ければ、push 先を upstream にする
  const setUpstream = !source.ref.upstream;
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

async function runAction(repo: Repository, action: Action): Promise<void> {
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

/** コンフリクトの解決 UI はまだ無いので、abort するか、自分で解決するかを選ばせる */
async function handleConflict(repo: Repository, title: string): Promise<void> {
  const sequence = await repo.backend.sequenceInProgress();
  const choice = await vscode.window.showWarningMessage(
    `${title}: コンフリクトが発生しました。解決してから git ${sequence ?? ''} --continue を実行するか、中止してください`,
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
