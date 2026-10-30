/**
 * サイドバーのナビでの、ブランチのドラッグ＆ドロップ。
 * - ブランチ（またはタグ）をローカルブランチにドロップ: Merge / Rebase を選ぶ
 * - ローカルブランチをリモート（またはリモートのブランチ）にドロップ: Push / Force Push を選ぶ
 * どちらもドロップ先を書き換える操作になる。
 */
import * as vscode from 'vscode';
import { mergeInto, rebaseOnto, shortName, type OperationResult } from '../state/operations.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import type { DragSource, DropTarget } from '../view/protocol.ts';
import type { Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';

const MIME = 'application/vnd.code.tree.amonhen.sidebar';

/** サイドバーのナビでのドラッグ＆ドロップ */
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
    const drop = toDropTarget(target);
    if (drop) {
      const local = source.kind === 'branch' && !source.remote;
      await pickAndRun(repo, { ref: source.ref.name, local, upstream: source.ref.upstream }, drop);
    }
  }
}

function toDropTarget(node: Node): DropTarget | undefined {
  if (node.kind === 'branch') {
    return node.remote
      ? { kind: 'remoteBranch', remote: node.remote, ref: node.ref.name }
      : { kind: 'branch', ref: node.ref.name };
  }
  if (node.kind === 'remote') {
    return { kind: 'remote', remote: node.remote.name };
  }
  return undefined;
}

export interface Action {
  label: string;
  detail?: string;
  /** undefined なら何も知らせない（続きを別に行うとき） */
  run: () => Promise<OperationResult | 'pushed' | undefined>;
  /** コンフリクトで止まったときの知らせに書き添えること（実行の後に読む） */
  conflictNote?: () => string | undefined;
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
      await handleConflict(repo, title, action.conflictNote?.());
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
async function handleConflict(repo: Repository, title: string, note?: string): Promise<void> {
  const sequence = await repo.backend.sequenceInProgress();
  const choice = await vscode.window.showWarningMessage(
    `${title}: コンフリクトが発生しました。解決して stage してから、サイドバーの Continue で続けるか、中止してください` +
      (note ? `。${note}` : ''),
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
