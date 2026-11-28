import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';
import * as vscode from 'vscode';
import { parseCommitObject } from '../git/commitObject.ts';
import { groupChanges, type Change, type ChangeGroup } from '../state/changes.ts';
import { conflictStates } from '../state/conflicts.ts';
import type { Repository } from '../state/repository.ts';
import { daemonFor } from '../daemon/connection.ts';
import { applySyntax, changedRows, readWorkingDiff, selectedRows, type DaemonDiff } from '../daemon/diff.ts';
import { applyDiffSelection, computeDiff, type ComputedDiff } from '../diff/fileDiff.ts';
import { discardFiles, discardLines } from '../state/discard.ts';
import { failureCause } from '../failure/cause.ts';
import { templateFilePath } from '../settings/templateFile.ts';
import { COMMIT_TYPES, fixupSubjects, templateMessage, type MessageParts } from '../workingCopy/commitMessage.ts';
import type { DiffWhitespace } from '../history/protocol.ts';
import type {
  DiffMenuAction,
  FileStat,
  LineStaging,
  WorkingCopyHostMessage,
  WorkingCopyWebviewMessage,
} from '../workingCopy/protocol.ts';
import { gapKey, type GapOpen } from '../workingCopy/gaps.ts';
import { modeOf, sideNoteFor, type DiffSide, type SideNote } from '../workingCopy/sides.ts';
import { commitStaged } from './commit.ts';
import { openConflict } from './diff.ts';
import { commitDraft, fileLayout, setCommitDraft, setFileLayout, setWorkingCopyLook, workingCopyLook } from './preferences.ts';
import { acceptConflicts, indexMode, showError, stageChanges } from './stageCommands.ts';

/**
 * メインビューの Working Copy の拡張ホスト側。
 * 変更ファイルの一覧と、選んだファイルの差分を送り、ファイル・hunk・行単位の stage とコミットを受け持つ。
 */
const RECENT_COMMITS = 30;

/** 行の Stage の形（設定 `amonhen.diff.lineStaging`） */
export function lineStaging(): LineStaging {
  return vscode.workspace.getConfiguration('amonhen').get<string>('diff.lineStaging') === 'bar' ? 'bar' : 'select';
}

/**
 * たたんだ差分の、隙間の開き方（リポジトリ・見る側・ファイルごと）。拡張が動いている間だけ覚える（メインビューを
 * 閉じて開き直しても同じ。アプリと同じく保存はしない）
 */
const openGaps = new Map<string, GapOpen[]>();

export class WorkingCopyController implements vscode.Disposable {
  private readonly post: (message: WorkingCopyHostMessage) => void;
  private repo: Repository | undefined;
  private listeners: vscode.Disposable[] = [];
  private files: Change[] = [];
  private selected: { group: ChangeGroup; path: string } | undefined;
  private timer: NodeJS.Timeout | undefined;
  /** amend のときにメッセージ欄へ入れる、HEAD のメッセージ */
  private head: { oid: string; message: string } | undefined;

  /** 最近のコミット（新しい順） */
  private factsReading = 0;
  private recent: { message: string; author: string; email: string }[] = [];

  constructor(post: (message: WorkingCopyHostMessage) => void) {
    this.post = post;
  }

  setRepository(repo: Repository | undefined): void {
    vscode.Disposable.from(...this.listeners).dispose();
    this.repo = repo;
    this.selected = undefined;
    // 見る側は Working Copy で 1 つ。開き直したら Unstaged から（アプリと同じく覚えない）
    this.side = 'unstaged';
    this.head = undefined;
    this.recent = [];
    this.listeners = repo
      ? [
          repo.onDidChange((changed) => changed.has('workingCopy') && this.schedule()),
          repo.onDidChangeContents(() => this.schedule()),
          // コンフリクトのファイルは、status が UU のままでもマーカーが消えることがある（ほかのエディタ・rerere）。
          // 取り直すたびに読み直す（アプリと同じ）。コンフリクトがなければ何もしない
          repo.onDidRefresh(() => this.files.some((f) => f.group === 'conflicts') && this.schedule()),
        ]
      : [];
    this.schedule();
    void this.sendMessage(repo).catch(showError);
  }

  /** コミット欄の中身を送る。下書きがなければ `commit.template` のファイル（`#` の行は除く） */
  private async sendMessage(repo: Repository | undefined): Promise<void> {
    const draft = repo && commitDraft(repo.root);
    const parts = draft ?? (repo ? await commitTemplate(repo) : undefined) ?? { summary: '', body: '' };
    if (repo === this.repo) this.post({ type: 'wcMessage', ...parts });
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
        void this.sendRecent(repo).catch(showError);
      }
      lastMessage = this.head.message;
    }
    const engine = repo ? (await daemonFor(repo.root).catch(() => undefined)) !== undefined : false;
    if (repo !== this.repo) return;
    this.post({
      type: 'wcFiles',
      files: this.files,
      branch: status?.branch.head,
      lastMessage,
      layout: fileLayout(),
      engine,
      lineStaging: lineStaging(),
      ...workingCopyLook(),
    });
    if (repo) void this.sendEngineFacts(repo).catch(() => undefined);
  }

  /** 一覧に添える、エンジンから読む様子 */
  private async sendEngineFacts(repo: Repository): Promise<void> {
    const client = await daemonFor(repo.root);
    if (!client) return;
    // 読む間に次の読み直しが始まっていれば、古い答えで上書きしない
    const reading = ++this.factsReading;
    const reply = await client.call({ op: 'run', command: { op: 'workingStats' } });
    if (reading !== this.factsReading) return;
    const info = reply.json['info'] as { stats?: FileStat[] } | undefined;
    if (repo === this.repo) this.post({ type: 'wcStats', stats: info?.stats ?? [] });
    if (workingCopyLook().sections) await this.sendSplitStats(repo, reading);
    const form = await client.call({ op: 'run', command: { op: 'repositoryForm' } });
    const sparse = (form.json['info'] as { sparse?: { cone: boolean; folders: string[] } | null } | undefined)?.sparse;
    if (repo === this.repo && reading === this.factsReading) this.post({ type: 'wcSparse', sparse: sparse ?? null });
  }

  /** 節ごとの行数（Staged の節は HEAD → index、Unstaged の節は index → 作業ツリー）。節に分けているときだけ読む */
  private async sendSplitStats(repo: Repository, reading = this.factsReading): Promise<void> {
    const client = await daemonFor(repo.root);
    if (!client) return;
    const reply = await client.call({ op: 'run', command: { op: 'workingStatsSplit' } });
    if (reading !== this.factsReading || repo !== this.repo) return;
    const info = reply.json['info'] as { staged?: FileStat[]; unstaged?: FileStat[] } | undefined;
    this.post({ type: 'wcSplitStats', staged: info?.staged ?? [], unstaged: info?.unstaged ?? [] });
  }

  /** 最近のコミット 30 個を読む（一覧と ⌥↑・⌥↓ のため） */
  private async sendRecent(repo: Repository): Promise<void> {
    const commits = await repo.backend.log(['HEAD'], 0, RECENT_COMMITS);
    const recent = await Promise.all(
      commits.map(async (c) => {
        const obj = await repo.backend.object(c.oid);
        const message = obj ? parseCommitObject(obj.data.toString('utf8')).message.trim() : c.subject;
        return { message, author: c.author, email: c.email };
      }),
    );
    if (repo !== this.repo) return;
    this.recent = recent;
    this.post({ type: 'wcRecent', messages: recent.map((c) => c.message) });
  }

  private async commit(repo: Repository, message: string, amend: boolean, push: boolean): Promise<void> {
    this.post({ type: 'wcCommitting', committing: true });
    let result: Awaited<ReturnType<typeof commitStaged>>;
    try {
      result = await commitStaged(repo, message, amend);
    } finally {
      this.post({ type: 'wcCommitting', committing: false });
    }
    if (result.error) this.post({ type: 'wcCommitOutput', title: 'Couldn’t Commit', text: result.error });
    if (!result.committed) return;
    await setCommitDraft(repo.root, undefined);
    this.post({ type: 'wcCommitted' });
    const output = result.output?.trim();
    if (output) this.post({ type: 'wcCommitOutput', title: 'Committed (hooks output)', text: output });
    // Commit and Push は、上流が先に進んでいなければ確かめずに送る（アプリと同じ）
    if (push) await vscode.commands.executeCommand('amonhen.push', { quiet: true });
  }

  /** Commit の続き。Commit and Push は、Commit を押せるときだけ */
  private async commitMenu(repo: Repository, canCommit: boolean, message: string, amend: boolean): Promise<void> {
    const publishes = !repo.current.status?.branch.upstream;
    const push = { label: publishes ? 'Commit and Publish…' : 'Commit and Push', description: '⌘⇧↩' };
    const startAmend = { label: 'Amend Last Commit' };
    const subjects = fixupSubjects(this.recent.map((c) => c.message));
    const fixup = {
      label: 'Fixup Commit For…',
      detail: 'Commit as a fixup! of the chosen commit (combine later with rebase -i --autosquash)',
    };
    const squash = {
      label: 'Squash Commit For…',
      detail: 'Commit as a squash! of the chosen commit (its message is added when combined)',
    };
    const picked = await vscode.window.showQuickPick(
      [...(canCommit ? [push] : []), startAmend, ...(subjects.length > 0 ? [fixup, squash] : [])],
      { placeHolder: 'Commit' },
    );
    if (picked === fixup || picked === squash) {
      const subject = await vscode.window.showQuickPick(subjects, { placeHolder: picked.label });
      if (subject) this.post({ type: 'wcStartFixup', kind: picked === fixup ? 'fixup' : 'squash', subject });
      return;
    }
    if (picked === push) {
      await this.commit(repo, message, amend, true);
    } else if (picked === startAmend) {
      this.post({ type: 'wcStartAmend' });
    }
  }

  /** Conventional Commits の型と、トレーラー（Co-authored-by・Signed-off-by）を選んで入れる */
  private async pickInsert(repo: Repository): Promise<void> {
    type Item = vscode.QuickPickItem & { commitType?: string; line?: string; signOff?: boolean };
    const separator = (label: string): Item => ({ label, kind: vscode.QuickPickItemKind.Separator });
    const seen = new Set<string>();
    const authors = this.recent.flatMap((c): Item[] => {
      const key = c.email.toLowerCase();
      if (!key || seen.has(key)) return [];
      seen.add(key);
      return [{ label: `${c.author} <${c.email}>`, line: `Co-authored-by: ${c.author} <${c.email}>` }];
    });
    const picked = await vscode.window.showQuickPick<Item>(
      [
        separator('Conventional Commits Type'),
        ...COMMIT_TYPES.map((t) => ({ label: `${t}:`, commitType: t })),
        separator('Signed-off-by'),
        { label: 'Signed-off-by (Me)', signOff: true },
        separator('Co-authored-by'),
        ...authors,
      ],
      { placeHolder: 'Insert a Conventional Commits type, Co-authored-by or Signed-off-by' },
    );
    if (picked?.commitType) {
      this.post({ type: 'wcInsertType', commitType: picked.commitType });
    } else if (picked?.line) {
      this.post({ type: 'wcInsertTrailer', line: picked.line });
    } else if (picked?.signOff) {
      const [name, email] = await Promise.all([repo.backend.config('user.name'), repo.backend.config('user.email')]);
      if (name && email) {
        this.post({ type: 'wcInsertTrailer', line: `Signed-off-by: ${name} <${email}>` });
      } else {
        void vscode.window.showWarningMessage('Set user.name and user.email to sign off.');
      }
    }
  }

  /** ファイルを選ぶ（操作がコンフリクトで止まったとき、最初のコンフリクトのファイルを）。差分を送ると一覧でも選ばれる */
  select(change: Change): void {
    this.selected = { group: change.group, path: change.path };
    this.schedule();
  }

  /** 差分で行を比べるときに無視する空白（エンジンがあるときだけ。覚えない） */
  private whitespace: DiffWhitespace = 'exact';
  /** 差分で見る側（エンジンがあるときだけ。ファイルを替えても同じ、覚えない） */
  private side: DiffSide = 'unstaged';
  /** 差分を頼んだ回数（読む間に選び直したら、古い差分を送らない） */
  private diffRequest = 0;

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
      this.post({ type: 'wcDiff', diff: undefined, gone: selected.path });
      return;
    }
    this.selected = { group: change.group, path: change.path };
    try {
      await this.sendDiffOf(change);
    } catch (err) {
      // 空の差分や知らせにせず、差分の場所に読めなかった理由を出す（アプリと同じ）
      if (this.selected?.path !== change.path) return;
      const message = err instanceof Error ? err.message : String(err);
      this.post({ type: 'wcDiffFailed', group: change.group, path: change.path, error: failureCause(message) });
    }
  }

  private async sendDiffOf(change: Change): Promise<void> {
    // 読んでいる間に空白の扱い・見る側・ファイルを選び直したら、この差分は送らない（選び直しで読み直した方を送る。
    // 古い差分に新しい扱いの札を付けない）
    const whitespace = this.whitespace;
    const side = this.side;
    const request = ++this.diffRequest;
    const stale = () => request !== this.diffRequest || whitespace !== this.whitespace || side !== this.side;
    let shown: SideDiff | undefined;
    for (let attempt = 0; ; attempt++) {
      try {
        shown = await this.sideDiff(change, side, whitespace);
        break;
      } catch (err) {
        // daemon は拡張より新しい status を読むので、その間に変更がなくなったファイルは断られる。読み直して、もう変更が
        // なければ選びを外す（エラーにしない）
        await this.repo?.refresh();
        if (this.repo?.current.status?.entries.some((e) => e.path === change.path)) {
          // 読み直している間に、また変更された（daemon が見たのは、その間の変更のない一瞬）。もう一度だけ頼む
          if (attempt === 0) continue;
          throw err;
        }
        if (this.selected?.path === change.path) {
          this.selected = undefined;
          this.post({ type: 'wcDiff', diff: undefined, gone: change.path });
        }
        return;
      }
    }
    if (stale()) return;
    if (shown && 'note' in shown) {
      this.post({ type: 'wcDiffNote', group: change.group, path: change.path, side, whitespace, note: shown.note });
      return;
    }
    if (shown) {
      const open = this.gapsOf(side === 'all' ? 'combined' : side, change.path);
      const message = { type: 'wcDiff', group: shown.group, diff: shown.daemon.diff, whitespace, side, open } as const;
      this.post(message);
      void this.sendSyntax(shown.daemon, message, stale);
      return;
    }
    const { diff } = await this.loadDiff(change);
    if (stale()) return;
    this.post({ type: 'wcDiff', group: change.group, diff });
  }

  /**
   * エンジンで読む、見る側の差分（エンジンがない・コンフリクトなら undefined）。その側に変更がない・All で出せない
   * ときは、そのわけ
   */
  private async sideDiff(change: Change, side: DiffSide, whitespace: DiffWhitespace): Promise<SideDiff | undefined> {
    if (change.group === 'conflicts' || !this.repo || !(await daemonFor(this.repo.root))) return undefined;
    const has = {
      staged: this.files.some((f) => f.group === 'staged' && f.path === change.path),
      unstaged: this.files.some((f) => f.group === 'unstaged' && f.path === change.path),
    };
    const note = sideNoteFor(side, has, whitespace !== 'exact');
    if (note) return { note };
    if (side !== 'all') {
      return { group: side, daemon: (await this.daemonDiff(change, side, whitespace))! };
    }
    try {
      return { group: change.group, daemon: (await this.daemonDiff(change, 'combined'))! };
    } catch (err) {
      // index を「HEAD に変更の一部を当てたもの」で表せないファイルは、コアが断る。片方の側は読めれば、そう言って
      // Unstaged・Staged へ案内する（読めなければ、読めないわけを出す）
      await this.daemonDiff(change, has.unstaged ? 'unstaged' : 'staged').catch(() => {
        throw err;
      });
      return { note: 'allUnavailable' };
    }
  }

  /** 構文の色を後から頼み、付いたら同じ差分を送り直す（選んだ行はそのまま）。古いエンジンが断れば色なしのまま */
  private async sendSyntax(daemon: DaemonDiff, message: WorkingCopyHostMessage, stale: () => boolean): Promise<void> {
    try {
      const client = this.repo && (await daemonFor(this.repo.root));
      if (!client) return;
      const touched = applySyntax(await client.call({ op: 'diffSyntax', diff: daemon.handle }), daemon.lines);
      if (touched.size > 0 && this.selected?.path === daemon.diff.path && !stale()) {
        this.post(message);
      }
    } catch {
      // 色なしのまま
    }
  }

  /**
   * 共有の daemon で読んだ差分（エンジンがあるとき）。コンフリクトは出さないので undefined。
   * 行単位の stage・破棄も、この差分の行の番号で daemon に頼むので、見せた差分と同じ mode で読む
   */
  private async daemonDiff(
    change: Change,
    mode: 'unstaged' | 'staged' | 'combined',
    whitespace = this.whitespace,
  ): Promise<DaemonDiff | undefined> {
    if (change.group === 'conflicts' || !this.repo) {
      return undefined;
    }
    const client = await daemonFor(this.repo.root);
    if (!client) {
      return undefined;
    }
    // HEAD → 作業ツリーの差分では空白を無視できない（コアが断る）
    const ws = mode !== 'combined' && whitespace !== 'exact' ? { whitespace } : {};
    // 変更のない所は隙間の行にたたみ、開いた分だけ見せる（stage・破棄の前の読み直しも、見せた差分と同じ開き方で）
    const open = this.gapsOf(mode, change.path);
    return readWorkingDiff(await client.call({ op: 'workingDiff', path: change.path, mode, ...ws, open }), true);
  }

  /** mode の差分の、path の隙間の開き方 */
  private gapsOf(mode: 'unstaged' | 'staged' | 'combined', path: string): GapOpen[] {
    if (!this.repo) return [];
    return openGaps.get(gapKey(this.repo.root, mode === 'combined' ? 'all' : mode, path)) ?? [];
  }

  /** 差分の右クリックの項目（package.json の webview/context）。選んだ行や hunk は Webview が知っているので、頼み返す */
  diffAction(action: DiffMenuAction, at: { gap?: number; hunk?: number; line?: string }): void {
    this.post({ type: 'wcDiffAction', action, ...at });
  }

  /** 行の Stage の形を設定で変えた */
  lineStagingChanged(): void {
    this.post({ type: 'wcLineStaging', lineStaging: lineStaging() });
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
    if (message.type === 'wcLayout') {
      await setFileLayout(message.layout);
      return;
    }
    if (message.type === 'wcLook') {
      const { type: _, ...patch } = message;
      const before = workingCopyLook().sections;
      await setWorkingCopyLook(patch);
      // 節に分けたら、節ごとの行数を読む
      if (patch.sections && !before && this.repo) await this.sendSplitStats(this.repo).catch(() => undefined);
      return;
    }
    const repo = this.repo;
    if (!repo) {
      // コミットを頼まれていれば、Webview がコミット中のままにならないよう知らせる
      if (message.type === 'wcCommit') this.post({ type: 'wcCommitting', committing: false });
      return;
    }
    if (message.type === 'wcDraft') {
      await setCommitDraft(repo.root, { summary: message.summary, body: message.body });
      return;
    }
    if (message.type === 'wcShowFolders') {
      await vscode.commands.executeCommand('amonhen.showRepositorySettings');
      return;
    }
    if (message.type === 'wcPickRecent') {
      const seen = new Set<string>();
      const items = this.recent.flatMap((c) => {
        if (seen.has(c.message)) return [];
        seen.add(c.message);
        const [subject = '', ...rest] = c.message.split('\n');
        return [{ label: subject.slice(0, 80), detail: rest.join(' ').trim() || undefined, message: c.message }];
      });
      const picked = await vscode.window.showQuickPick(items, { placeHolder: 'Insert the message of a recent commit' });
      if (picked) this.post({ type: 'wcUseMessage', message: picked.message });
      return;
    }
    if (message.type === 'wcPickInsert') {
      await this.pickInsert(repo);
      return;
    }
    const find = (group: ChangeGroup, path: string) => this.files.find((f) => f.group === group && f.path === path);
    switch (message.type) {
      case 'wcSelect':
        this.selected = { group: message.group, path: message.path };
        if (message.side) this.side = message.side;
        await this.sendDiff();
        break;
      case 'wcSide':
        if (message.side !== this.side) {
          this.side = message.side;
          await this.sendDiff();
        }
        break;
      case 'wcGaps': {
        const key = gapKey(repo.root, message.side, message.path);
        if (message.open.length > 0) openGaps.set(key, message.open);
        else openGaps.delete(key);
        if (this.selected?.path === message.path && message.side === this.side) await this.sendDiff();
        break;
      }
      case 'wcStageFiles': {
        const changes = message.files.flatMap((f) => find(f.group, f.path) ?? []);
        await stageChanges(repo, changes, message.op);
        break;
      }
      case 'wcStageLines': {
        if (this.whitespace !== 'exact') break;
        const change = find(message.group, message.path);
        if (change) {
          await this.stageLines(repo, change, message.version, message.lines, message.stage ?? true, message.combined === true).catch(showError);
        }
        break;
      }
      case 'wcDiscardFiles': {
        const changes = message.paths.flatMap((p) => find('unstaged', p) ?? []);
        if (changes.length === 0) break;
        const target = changes.length === 1 ? changes[0]!.path : `${changes.length} files`;
        // untracked のファイルは消す（アプリと同じく Delete と言う。消す前の中身を記録するので Undo で戻せる）
        const untracked = changes.every((c) => c.letter === 'U');
        const choice = await vscode.window.showWarningMessage(
          untracked ? `Delete ${target}?` : `Discard the changes to ${target}?`,
          {
            modal: true,
            detail: untracked
              ? `${changes.length === 1 ? 'It’s' : 'They’re'} untracked. You can undo this.`
              : 'The unstaged changes are discarded. You can undo this.',
          },
          untracked ? 'Delete' : 'Discard',
        );
        if (choice !== (untracked ? 'Delete' : 'Discard')) break;
        await this.discard(repo, () => discardFiles(repo.backend, repo.undo, changes));
        break;
      }
      case 'wcDiscardLines': {
        if (this.whitespace !== 'exact') break;
        const change = find('unstaged', message.path);
        if (!change) break;
        const daemon = await this.daemonDiff(change, message.combined ? 'combined' : 'unstaged');
        if (daemon) {
          if (daemon.diff.version !== message.version) {
            vscode.window.setStatusBarMessage('The file changed, so the diff was updated. Select the lines again.', 4000);
            await this.sendDiff();
            break;
          }
          const client = (await daemonFor(repo.root))!;
          await this.discard(repo, async () => {
            await client.call({ op: 'discardRows', diff: daemon.handle, rows: selectedRows(daemon.rows, message.lines) });
          });
          break;
        }
        const fresh = await this.loadDiff(change);
        if (fresh.diff.version !== message.version) {
          this.post({ type: 'wcDiff', group: change.group, diff: fresh.diff });
          vscode.window.setStatusBarMessage('The file changed, so the diff was updated. Select the lines again.', 4000);
          break;
        }
        await this.discard(repo, () => discardLines(repo.backend, repo.undo, change.path, fresh, message.lines));
        break;
      }
      case 'wcCommit':
        await this.commit(repo, message.message, message.amend, message.push === true);
        break;
      case 'wcCommitMenu':
        await this.commitMenu(repo, message.canCommit, message.message, message.amend);
        break;
      case 'wcCommitRefused':
        void vscode.window.showWarningMessage(`Can’t commit: ${message.reason}`);
        break;
      case 'wcWhitespace':
        if (message.whitespace !== this.whitespace) {
          this.whitespace = message.whitespace;
          await this.sendDiff();
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

  /**
   * 破棄する。破棄した内容は Undo で戻せる。ファイルごとのときは呼ぶ前に確かめ、選んだ行・hunk はアプリと同じく
   * 確かめない（選んで押すので。Undo で戻せる）
   */
  private async discard(repo: Repository, run: () => Promise<void>): Promise<void> {
    try {
      await run();
    } catch (err) {
      showError(err, { op: 'discard' });
    } finally {
      await repo.refresh();
    }
  }

  private async stageLines(
    repo: Repository,
    change: Change,
    version: string,
    lines: Extract<WorkingCopyWebviewMessage, { type: 'wcStageLines' }>['lines'],
    stage = true,
    combined = false,
  ): Promise<void> {
    const daemon = await this.daemonDiff(change, combined ? 'combined' : change.group === 'staged' ? 'staged' : 'unstaged');
    if (daemon) {
      return this.stageLinesThroughDaemon(repo, change, version, lines, daemon, stage);
    }
    // 表示している差分と今の内容が違えば、選んだ行がずれるので当てない
    const fresh = await this.loadDiff(change);
    if (fresh.diff.version !== version) {
      this.post({ type: 'wcDiff', group: change.group, diff: fresh.diff });
      vscode.window.setStatusBarMessage('The file changed, so the diff was updated. Select the lines again.', 4000);
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
    stage: boolean,
  ): Promise<void> {
    if (fresh.diff.version !== version) {
      vscode.window.setStatusBarMessage('The file changed, so the diff was updated. Select the lines again.', 4000);
      await this.sendDiff();
      return;
    }
    const rows = selectedRows(fresh.rows, lines);
    if (rows.length === 0) {
      return;
    }
    const client = (await daemonFor(repo.root))!;
    if (fresh.diff.combined) {
      // HEAD → 作業ツリーの差分: 選んだ行を index に入れる・外す（すべて入れれば git add、すべて外せば git reset と同じ）
      try {
        await client.call({ op: 'stageRows', diff: fresh.handle, rows, stage });
      } finally {
        await repo.refresh();
      }
      return;
    }
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

/** エンジンで読んだ、見る側の差分（group はその側のグループ）か、出さないわけ */
type SideDiff = { group: ChangeGroup; daemon: DaemonDiff } | { note: SideNote };

/** `commit.template` のファイルの中身。相対パスはリポジトリから（git と同じ） */
async function commitTemplate(repo: Repository): Promise<MessageParts | undefined> {
  const path = await repo.backend.config('commit.template');
  if (!path) return undefined;
  const file = templateFilePath(path, repo.root, homedir());
  try {
    return templateMessage(await readFile(file, 'utf8'));
  } catch {
    return undefined;
  }
}
