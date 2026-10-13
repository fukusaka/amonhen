import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import * as vscode from 'vscode';
import { parseCommitObject } from '../git/commitObject.ts';
import { groupChanges, type Change, type ChangeGroup } from '../state/changes.ts';
import type { Repository } from '../state/repository.ts';
import { applyDiffSelection, computeDiff, type ComputedDiff } from '../diff/fileDiff.ts';
import type { WorkingCopyHostMessage, WorkingCopyWebviewMessage } from '../workingCopy/protocol.ts';
import { commitStaged } from './commit.ts';
import { openChangeDiff } from './diff.ts';
import { acceptConflicts, indexMode, showError, stageChanges } from './stageCommands.ts';

/**
 * メインビューの Working Copy の拡張ホスト側。
 * 変更ファイルの一覧と、選んだファイルの差分を送り、ファイル・hunk・行単位の stage とコミットを受け持つ。
 */
export class WorkingCopyController implements vscode.Disposable {
  private readonly post: (message: WorkingCopyHostMessage) => void;
  private repo: Repository | undefined;
  private listeners: vscode.Disposable[] = [];
  private files: Change[] = [];
  private selected: { group: ChangeGroup; path: string } | undefined;
  private timer: NodeJS.Timeout | undefined;
  /** amend のときにメッセージ欄へ入れる、HEAD のメッセージ */
  private head: { oid: string; message: string } | undefined;

  constructor(post: (message: WorkingCopyHostMessage) => void) {
    this.post = post;
  }

  setRepository(repo: Repository | undefined): void {
    vscode.Disposable.from(...this.listeners).dispose();
    this.repo = repo;
    this.selected = undefined;
    this.listeners = repo
      ? [
          repo.onDidChange((changed) => changed.has('workingCopy') && this.schedule()),
          repo.onDidChangeContents(() => this.schedule()),
        ]
      : [];
    this.schedule();
  }

  /** 変更が続けて来てもまとめて 1 回送る */
  private schedule(): void {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.refresh().catch(showError), 30);
  }

  private async refresh(): Promise<void> {
    await this.sendFiles();
    await this.sendDiff();
  }

  private async sendFiles(): Promise<void> {
    const repo = this.repo;
    const status = repo?.current.status;
    this.files = groupChanges(status?.entries ?? []).flatMap(([, changes]) => changes);
    let lastMessage: string | undefined;
    const oid = status?.branch.oid;
    if (repo && oid) {
      if (this.head?.oid !== oid) {
        const obj = await repo.backend.object(oid);
        this.head = { oid, message: obj ? parseCommitObject(obj.data.toString('utf8')).message : '' };
      }
      lastMessage = this.head.message;
    }
    this.post({ type: 'wcFiles', files: this.files, branch: status?.branch.head, lastMessage });
  }

  /** 選んでいるファイルの差分を送る。stage してグループが変わっても、同じファイルを見続ける */
  private async sendDiff(): Promise<void> {
    const selected = this.selected;
    if (!selected) {
      return;
    }
    const change =
      this.files.find((f) => f.group === selected.group && f.path === selected.path) ??
      this.files.find((f) => f.path === selected.path);
    if (!change) {
      this.selected = undefined;
      this.post({ type: 'wcDiff', diff: undefined });
      return;
    }
    this.selected = { group: change.group, path: change.path };
    const computed = await this.loadDiff(change);
    this.post({ type: 'wcDiff', group: change.group, diff: computed.diff });
  }

  private async loadDiff(change: Change): Promise<ComputedDiff> {
    const repo = this.repo!;
    const object = async (rev: string) => (await repo.backend.object(rev))?.data;
    switch (change.group) {
      case 'conflicts':
        // コンフリクトはマージエディタで解決する。差分は出さない
        return computeDiff(change.path, undefined, undefined);
      case 'unstaged': {
        const old = change.letter === 'U' ? undefined : await object(`:${change.path}`);
        const now =
          change.letter === 'D' ? undefined : await readFile(join(repo.root, change.path)).catch(() => undefined);
        return computeDiff(change.path, old, now);
      }
      case 'staged': {
        const old = change.letter === 'A' ? undefined : await object(`HEAD:${change.origPath ?? change.path}`);
        const now = change.letter === 'D' ? undefined : await object(`:${change.path}`);
        return computeDiff(change.path, old, now);
      }
    }
  }

  async handle(message: WorkingCopyWebviewMessage): Promise<void> {
    const repo = this.repo;
    if (!repo) {
      return;
    }
    const find = (group: ChangeGroup, path: string) => this.files.find((f) => f.group === group && f.path === path);
    switch (message.type) {
      case 'wcSelect':
        this.selected = { group: message.group, path: message.path };
        await this.sendDiff();
        break;
      case 'wcStageFiles': {
        const changes = message.files.flatMap((f) => find(f.group, f.path) ?? []);
        await stageChanges(repo, changes, message.op);
        break;
      }
      case 'wcStageLines': {
        const change = find(message.group, message.path);
        if (change) {
          await this.stageLines(repo, change, message.version, message.lines).catch(showError);
        }
        break;
      }
      case 'wcCommit':
        if (await commitStaged(repo, message.message, message.amend)) {
          this.post({ type: 'wcCommitted' });
        }
        break;
      case 'wcOpenFile':
        await vscode.commands.executeCommand('vscode.open', vscode.Uri.file(join(repo.root, message.path)), {
          viewColumn: vscode.ViewColumn.Beside,
        });
        break;
      case 'wcResolve': {
        const change = find('conflicts', message.path);
        if (!change) break;
        if (message.action === 'merge') {
          await openChangeDiff(repo.root, change, repo.current.sequence);
        } else {
          await acceptConflicts(repo, [change], message.action);
        }
        break;
      }
    }
  }

  private async stageLines(
    repo: Repository,
    change: Change,
    version: string,
    lines: Extract<WorkingCopyWebviewMessage, { type: 'wcStageLines' }>['lines'],
  ): Promise<void> {
    // 表示している差分と今の内容が違えば、選んだ行がずれるので当てない
    const fresh = await this.loadDiff(change);
    if (fresh.diff.version !== version) {
      this.post({ type: 'wcDiff', group: change.group, diff: fresh.diff });
      vscode.window.setStatusBarMessage('ファイルが変わったので差分を更新しました。もう一度選んでください', 4000);
      return;
    }
    const op = change.group === 'staged' ? 'unstage' : 'stage';
    const next = applyDiffSelection(fresh, op, lines);
    const current = (op === 'stage' ? fresh.oldLines : fresh.newLines).join('');
    if (next === current) {
      return;
    }
    try {
      // 中身が空になったら、index のエントリごと戻す（削除の stage、新規ファイルの unstage）
      const otherSideMissing = op === 'stage' ? change.letter === 'D' : change.letter === 'A';
      if (next === '' && otherSideMissing) {
        await (op === 'stage' ? repo.backend.stage([change.path]) : repo.backend.unstage([change.path]));
      } else {
        const mode = await indexMode(repo, change.path, join(repo.root, change.path));
        await repo.backend.writeIndex(change.path, mode, Buffer.from(next, 'latin1'));
      }
    } finally {
      await repo.refresh();
    }
  }

  dispose(): void {
    clearTimeout(this.timer);
    vscode.Disposable.from(...this.listeners).dispose();
  }
}
