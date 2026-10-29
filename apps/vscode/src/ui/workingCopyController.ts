import { join } from 'node:path';
import * as vscode from 'vscode';
import { parseCommitObject } from '../git/commitObject.ts';
import { groupChanges, type Change, type ChangeGroup } from '../state/changes.ts';
import { conflictStates } from '../state/conflicts.ts';
import type { Repository } from '../state/repository.ts';
import { daemonFor } from '../daemon/connection.ts';
import { changedRows, readWorkingDiff, selectedRows, type DaemonDiff } from '../daemon/diff.ts';
import { applyDiffSelection, computeDiff, type ComputedDiff } from '../diff/fileDiff.ts';
import { discardFiles, discardLines } from '../state/discard.ts';
import type { WorkingCopyHostMessage, WorkingCopyWebviewMessage } from '../workingCopy/protocol.ts';
import { commitStaged } from './commit.ts';
import { openConflict } from './diff.ts';
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
    // マーカーの残っていないコンフリクト（rerere が解決を当てた、ほかのエディタで直した）を見分ける
    if (repo && status && this.files.some((f) => f.group === 'conflicts')) {
      const states = await conflictStates(repo.root, repo.backend.dirs.gitDir, status.entries);
      for (const f of this.files) {
        const state = f.group === 'conflicts' ? states.get(f.path) : undefined;
        if (state) Object.assign(f, state);
      }
    }
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
    const daemon = await this.daemonDiff(change);
    const diff = daemon ? daemon.diff : (await this.loadDiff(change)).diff;
    this.post({ type: 'wcDiff', group: change.group, diff });
  }

  /**
   * 共有の daemon で読んだ差分（設定 `amonhen.daemon.path` があるとき）。コンフリクトは出さないので undefined。
   * 行単位の stage・破棄も、この差分の行の番号で daemon に頼む
   */
  private async daemonDiff(change: Change): Promise<DaemonDiff | undefined> {
    if (change.group === 'conflicts' || !this.repo) {
      return undefined;
    }
    const client = await daemonFor(this.repo.root);
    if (!client) {
      return undefined;
    }
    const mode = change.group === 'staged' ? 'staged' : 'unstaged';
    return readWorkingDiff(await client.call({ op: 'workingDiff', path: change.path, mode }));
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
        // git に入れる形（改行の変換をかけたもの）で index と比べる
        const now = change.letter === 'D' ? undefined : await repo.backend.worktreeFile(change.path, old);
        const computed = computeDiff(change.path, old, now);
        if (change.letter !== 'D') {
          computed.diff.lineEndings = await repo.backend.lineEndings(change.path, old ?? Buffer.alloc(0));
        }
        return computed;
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
      case 'wcDiscardFiles': {
        const changes = message.paths.flatMap((p) => find('unstaged', p) ?? []);
        await this.discard(repo, changes.length === 1 ? changes[0]!.path : `${changes.length} 個のファイル`, () =>
          discardFiles(repo.backend, repo.undo, changes),
        );
        break;
      }
      case 'wcDiscardLines': {
        const change = find('unstaged', message.path);
        if (!change) break;
        const n = message.lines.length;
        const daemon = await this.daemonDiff(change);
        if (daemon) {
          if (daemon.diff.version !== message.version) {
            this.post({ type: 'wcDiff', group: change.group, diff: daemon.diff });
            vscode.window.setStatusBarMessage('ファイルが変わったので差分を更新しました。もう一度選んでください', 4000);
            break;
          }
          const client = (await daemonFor(repo.root))!;
          await this.discard(repo, `${change.path} の ${n} 行`, async () => {
            await client.call({ op: 'discardRows', diff: daemon.handle, rows: selectedRows(daemon.rows, message.lines) });
          });
          break;
        }
        const fresh = await this.loadDiff(change);
        if (fresh.diff.version !== message.version) {
          this.post({ type: 'wcDiff', group: change.group, diff: fresh.diff });
          vscode.window.setStatusBarMessage('ファイルが変わったので差分を更新しました。もう一度選んでください', 4000);
          break;
        }
        await this.discard(repo, `${change.path} の ${n} 行`, () =>
          discardLines(repo.backend, repo.undo, change.path, fresh, message.lines),
        );
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
          await openConflict(repo.root, change, repo.current.sequence);
        } else {
          await acceptConflicts(repo, [change], message.action);
        }
        break;
      }
    }
  }

  /** 確かめてから破棄する。破棄した内容は Undo で戻せる */
  private async discard(repo: Repository, target: string, run: () => Promise<void>): Promise<void> {
    const choice = await vscode.window.showWarningMessage(
      `${target}の変更を破棄しますか？`,
      { modal: true, detail: 'stage していない変更が失われます。Undo で戻せます。' },
      'Discard',
    );
    if (choice !== 'Discard') {
      return;
    }
    try {
      await run();
    } catch (err) {
      showError(err);
    } finally {
      await repo.refresh();
    }
  }

  private async stageLines(
    repo: Repository,
    change: Change,
    version: string,
    lines: Extract<WorkingCopyWebviewMessage, { type: 'wcStageLines' }>['lines'],
  ): Promise<void> {
    const daemon = await this.daemonDiff(change);
    if (daemon) {
      return this.stageLinesThroughDaemon(repo, change, version, lines, daemon);
    }
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

  /** 共有の daemon で、選んだ行を stage（staged の差分なら unstage）する */
  private async stageLinesThroughDaemon(
    repo: Repository,
    change: Change,
    version: string,
    lines: Extract<WorkingCopyWebviewMessage, { type: 'wcStageLines' }>['lines'],
    fresh: DaemonDiff,
  ): Promise<void> {
    if (fresh.diff.version !== version) {
      this.post({ type: 'wcDiff', group: change.group, diff: fresh.diff });
      vscode.window.setStatusBarMessage('ファイルが変わったので差分を更新しました。もう一度選んでください', 4000);
      return;
    }
    const rows = selectedRows(fresh.rows, lines);
    if (rows.length === 0) {
      return;
    }
    const client = (await daemonFor(repo.root))!;
    const op = change.group === 'staged' ? 'unstage' : 'stage';
    try {
      // すべての行を選んで中身が空になるなら、index のエントリごと戻す（削除の stage、新規ファイルの unstage）
      const otherSideMissing = op === 'stage' ? change.letter === 'D' : change.letter === 'A';
      if (otherSideMissing && rows.length === changedRows(fresh.rows).length) {
        await client.call({ op, paths: [change.path] });
      } else {
        await client.call({ op: 'stageRows', diff: fresh.handle, rows });
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
