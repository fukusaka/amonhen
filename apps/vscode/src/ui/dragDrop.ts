/**
 * サイドバーのナビでの、ブランチのドラッグ＆ドロップ。
 * - ブランチ（またはタグ）をローカルブランチにドロップ: Merge / Rebase を選ぶ
 * - ローカルブランチをリモート（またはリモートのブランチ）にドロップ: Push / Force Push を選び、送り先を名前で
 *   確かめる（上流なら Push のシート）。上流にするのはリモートの見出しに落とした Push だけ
 * どちらもドロップ先を書き換える操作になる。
 */
import * as vscode from 'vscode';
import { mergeInto, rebaseOnto, shortName, type OperationResult } from '../state/operations.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import type { DragSource, DropTarget } from '../view/protocol.ts';
import { forecastLine } from '../history/rewritePreview.ts';
import type { MergeMode } from '../git/backend.ts';
import { pickMergeMode } from './mergeMode.ts';
import { confirmRewrite } from './rewriteConfirm.ts';
import { sequenceRowName, type Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';
import { pushWithSheet } from './pushSheet.ts';
import { publish } from './syncCommands.ts';
import type { Worktree } from '../git/worktrees.ts';

const MIME = 'application/vnd.code.tree.amonhen.sidebar';

/** サイドバーのナビでのドラッグ＆ドロップ */
export class SidebarDragAndDrop implements vscode.TreeDragAndDropController<Node> {
  readonly dragMimeTypes = [MIME];
  readonly dropMimeTypes = [MIME];
  private readonly manager: RepositoryManager;
  /** ブランチを checkout している、書き込む先以外の worktree（`root` は書き込む先） */
  private readonly elsewhere: (branch: string, root: string) => Worktree | undefined;

  constructor(manager: RepositoryManager, elsewhere: (branch: string, root: string) => Worktree | undefined = () => undefined) {
    this.manager = manager;
    this.elsewhere = elsewhere;
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
    // ほかの worktree が checkout しているブランチは、ここでは checkout できず、fast-forward でも動かせない
    // （その作業ツリーとずれる）。その worktree で行う（アプリと同じ）
    const onto = drop?.kind === 'branch' && drop.ref !== source.ref.name ? shortName(drop.ref) : undefined;
    const other = onto ? this.elsewhere(onto, repo.root) : undefined;
    if (other) {
      const open = `Open the Worktree of ${onto} to Merge or Rebase`;
      const picked = await vscode.window.showQuickPick([{ label: `$(repo-forked) ${open}` }], { placeHolder: 'Choose what dropping does' });
      if (picked) await vscode.commands.executeCommand('amonhen.worktree.edit', { kind: 'worktree', worktree: other });
      return;
    }
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
  /** 失敗の見出しにする操作（コアの命令の名前。`failureTitle`） */
  op?: string;
  detail?: string;
  /** undefined なら何も知らせない（続きを別に行うとき） */
  run: () => Promise<OperationResult | 'pushed' | undefined>;
  /** コンフリクトで止まったときの知らせに書き添えること（実行の後に読む） */
  conflictNote?: () => string | undefined;
  /** 実行の前に確かめる。false なら実行しない */
  confirm?: () => Promise<boolean>;
  /** 確認と実行をこれに任せる（上流への push は Push のシートで） */
  instead?: () => Promise<void>;
}

/**
 * target（`refs/heads/...`）を onto の上に rebase する操作。書き換える前に、見通しを添えて確かめる（今のブランチなら
 * 一緒に動くブランチとコンフリクトの見込み。docs/design.md の「Rewriting operations and stacked branches」）
 */
export function rebaseAction(repo: Repository, target: string, onto: string): Action {
  const tgt = shortName(target);
  const src = shortName(onto);
  let updateRefs: boolean | undefined;
  return {
    label: `$(git-pull-request) Rebase ${tgt} onto ${src}`,
    op: 'rebase',
    confirm: async () => {
      const current = (await repo.backend.head()).branch === target;
      // 今のブランチなら書き換えの見通し。ほかのブランチなら、merge したときのコンフリクトの見込みを目安に（アプリと同じ）
      const note = current
        ? undefined
        : forecastLine(
            await vscode.window.withProgress(
              { location: vscode.ProgressLocation.Notification, title: 'Checking for conflicts…' },
              () => repo.backend.predictMerge([onto], target).catch(() => undefined),
            ),
            true,
          );
      const choice = await confirmRewrite(
        repo,
        `Rebase ${tgt} onto ${src}?`,
        'Rebase',
        current ? { onto } : undefined,
        note,
      );
      updateRefs = choice?.updateRefs;
      return choice !== undefined;
    },
    run: () => rebaseOnto(repo.backend, repo.undo, target, onto, updateRefs),
  };
}

/** ドロップしたときの操作を選ばせて実行する */
export async function pickAndRun(repo: Repository, source: DragSource, target: DropTarget): Promise<void> {
  const actions = dropActions(repo, source, target);
  if (actions.length === 0) {
    return;
  }
  const picked = await vscode.window.showQuickPick(actions, { placeHolder: 'Choose what dropping does' });
  if (picked?.instead) await picked.instead();
  else if (picked) await runAction(repo, picked);
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
    let mode: MergeMode | undefined;
    return [
      {
        label: `$(git-merge) Merge ${src} into ${tgt}`,
        op: 'mergeInto',
        confirm: async () =>
          (mode = await pickMergeMode(`Merge ${src} into ${tgt}?`, true, { repo, theirs: sourceRef, ours: targetRef })) !== undefined,
        run: () => mergeInto(backend, undo, sourceRef, targetRef, mode),
      },
      rebaseAction(repo, targetRef, sourceRef),
    ];
  }

  // リモートへ: push。送れるのはローカルブランチだけで、設定のあるリモートだけ（Fetch from Bundle の名前などには送れない）
  const remote = target.remote;
  if (!source.local || !(repo.current.remotes ?? []).some((r) => r.name === remote)) {
    return [];
  }
  const remoteBranch = target.kind === 'remote' ? src : target.ref.slice(`refs/remotes/${remote}/`.length);
  const dest = `${remote}/${remoteBranch}`;
  // 上流に落としたなら、ふつうの Push（送るコミットを見せるシート）。上流は Remotes の見出しに落としたときだけ
  // 設定し、Force Push では設定しない（アプリと同じ）
  const local = (repo.current.branches ?? []).find((b) => b.name === sourceRef);
  const toUpstream = local?.upstream === `refs/remotes/${dest}` && !local.gone;
  const there = (repo.current.remoteBranches ?? []).find((r) => r.name === `refs/remotes/${dest}`)?.oid;
  const push = (force: boolean): Action => {
    const setUpstream = target.kind === 'remote' && !force;
    return {
      label: force ? `$(warning) Force Push ${src} to ${dest}` : `$(cloud-upload) Push ${src} to ${dest}`,
      op: 'push',
      detail: force
        ? '--force-with-lease: replaces the remote’s history, as after a rebase'
        : setUpstream && !toUpstream
          ? `Set ${dest} as the upstream`
          : undefined,
      ...(toUpstream
        ? { instead: () => pushWithSheet(repo, { branch: sourceRef, force }, publish) }
        : {}),
      // 送り先を名前で見せて確かめる（リモートに送ったものは Undo では戻せない）
      confirm: async () => {
        const detail = force
          ? `${dest} loses the commits ${src} doesn’t have. This can’t be undone.`
          : there === undefined
            ? `Creates ${dest}.`
            : '';
        const ok = force ? 'Force Push' : 'Push';
        const title = force ? `Force push ${src} to ${dest}?` : `Push ${src} to ${dest}?`;
        return (await vscode.window.showWarningMessage(title, { modal: true, ...(detail ? { detail } : {}) }, ok)) === ok;
      },
      run: async () => {
        await backend.push(remote, src, remoteBranch, { force, setUpstream, ...(force && there ? { expect: there } : {}) });
        return 'pushed' as const;
      },
    };
  };
  return [push(false), push(true)];
}

/** 操作を進捗付きで実行し、結果（最新・コンフリクト・完了）に合わせて知らせる */
export async function runAction(repo: Repository, action: Action): Promise<void> {
  const title = action.label.replace(/^\$\([\w-]+\) /, '');
  if (action.confirm && !(await action.confirm())) {
    return;
  }
  let result: OperationResult | 'pushed' | undefined;
  try {
    result = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title }, () =>
      action.run(),
    );
  } catch (err) {
    // つながらない・ロックなら、確かめ直さずに同じ操作をもう一度（アプリの帯の Try Again）
    showError(err, { retry: () => runAction(repo, { ...action, confirm: undefined }), cwd: repo.root, op: action.op });
  } finally {
    await repo.refresh();
  }
  switch (result) {
    case 'up-to-date':
      void vscode.window.showInformationMessage(`${title}: Already up to date`);
      break;
    case 'conflict':
      await handleConflict(repo, title, action.conflictNote?.());
      break;
    case 'done': {
      const choice = await vscode.window.showInformationMessage(`${title}: Done`, 'Undo');
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

/**
 * コンフリクトしたら、ここで中止するか、解決して Continue するかを選ばせる。知らせは VSCode が 12 秒ほどで
 * 通知センターに移すので、いつでも Continue・Abort できるサイドバーの行を指す
 */
async function handleConflict(repo: Repository, title: string, note?: string): Promise<void> {
  const sequence = await repo.backend.sequenceInProgress();
  const row = sequence ? ` on the “${sequenceRowName(sequence)}” row` : '';
  const choice = await vscode.window.showWarningMessage(
    `${title}: There are conflicts. Resolve and stage them, then Continue${row} in the sidebar, or Abort.` +
      (note ? ` ${note}` : ''),
    'Abort',
  );
  if (choice === 'Abort' && sequence) {
    try {
      await repo.backend.abort(sequence);
    } catch (err) {
      showError(err, { op: 'abort' });
    } finally {
      await repo.refresh();
    }
  }
}
