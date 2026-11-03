/**
 * メインビューの Blame の拡張ホスト側。共有の daemon のコアの `blame` で、ファイルの各行を最後に変えたコミットを
 * 読む。コミットを決めなければ作業ツリーの内容で、作業中の変更や HEAD が動くたびに読み直す
 */
import * as vscode from 'vscode';
import type { BlameCommit, BlameHostMessage, BlameWebviewMessage } from '../blame/protocol.ts';
import { daemonFor } from '../daemon/connection.ts';
import type { Repository, Section } from '../state/repository.ts';

export class BlameController implements vscode.Disposable {
  private readonly post: (message: BlameHostMessage) => void;
  private repo: Repository | undefined;
  private repoListener: vscode.Disposable | undefined;
  /** リポジトリの中のパス（/ 区切り）と、そのコミット（なければ作業ツリー） */
  private target: { path: string; rev?: string } | undefined;
  private timer: NodeJS.Timeout | undefined;
  /** 作業ツリーの blame のファイル（書き換えても status が変わらなければ、リポジトリの知らせは来ない） */
  private fileWatcher: vscode.FileSystemWatcher | undefined;
  /** 読むたびに増やし、古い答えを捨てる */
  private generation = 0;

  constructor(post: (message: BlameHostMessage) => void) {
    this.post = post;
  }

  setRepository(repo: Repository | undefined): void {
    if (repo === this.repo) return;
    this.repoListener?.dispose();
    const switched = this.repo !== undefined;
    this.repoListener = repo?.onDidChange((changed) => this.onRepositoryChange(changed));
    this.repo = repo;
    this.generation++;
    // 別のリポジトリに切り替えたら、そのパスは意味がない。開いた直後（Webview の準備の前に頼まれた）なら読む
    if (switched) this.target = undefined;
    else if (this.target) void this.load();
    this.watchFile();
  }

  private watchFile(): void {
    this.fileWatcher?.dispose();
    this.fileWatcher = undefined;
    if (!this.repo || !this.target || this.target.rev) return;
    this.fileWatcher = vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(this.repo.root, this.target.path));
    const changed = () => this.schedule();
    this.fileWatcher.onDidChange(changed);
    this.fileWatcher.onDidCreate(changed);
    this.fileWatcher.onDidDelete(changed);
  }

  private schedule(): void {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.load(), 300);
  }

  /** `path` の blame を出す（`rev` があればそのコミットの内容） */
  async show(path: string, rev?: string): Promise<void> {
    this.target = { path, ...(rev ? { rev } : {}) };
    this.watchFile();
    // 前のファイルの行を残さない（読み直しのときは、読めるまで前の行を出したまま）
    this.post({ type: 'blame', ...this.target, commits: [], lines: [], loading: true });
    await this.load();
  }

  async handle(message: BlameWebviewMessage): Promise<void> {
    switch (message.type) {
      case 'blameLoad':
        await this.load();
        break;
      case 'blameReveal':
        await vscode.commands.executeCommand('amonhen.reflog.showInHistory', { oid: message.oid });
        break;
    }
  }

  /** 作業ツリーの blame は、作業中の変更でも変わる */
  private onRepositoryChange(changed: ReadonlySet<Section>): void {
    if (!this.target || this.target.rev) return;
    if (changed.has('workingCopy') || changed.has('branches')) this.schedule();
  }

  private async load(): Promise<void> {
    const generation = ++this.generation;
    const { repo, target } = this;
    if (!repo || !target) return;
    const send = (commits: BlameCommit[], lines: { c: number; t: string }[], loading: boolean, error?: string) =>
      this.post({ type: 'blame', ...target, commits, lines, loading, ...(error ? { error } : {}) });
    try {
      const client = await daemonFor(repo.root);
      if (!client) {
        send([], [], false, 'Blame needs the Amon Hen engine (amonhen.daemon.enabled)');
        return;
      }
      const reply = (await client.call({ op: 'run', command: { op: 'blame', path: target.path, rev: target.rev ?? null } })).json;
      if (generation !== this.generation) return;
      if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? 'Can’t blame the file'));
      const info = reply['info'] as { commits?: BlameCommit[]; lines?: { c: number; t: string }[] } | undefined;
      send(info?.commits ?? [], info?.lines ?? [], false);
    } catch (err) {
      if (generation === this.generation) send([], [], false, err instanceof Error ? err.message : String(err));
    }
  }

  dispose(): void {
    clearTimeout(this.timer);
    this.repoListener?.dispose();
    this.fileWatcher?.dispose();
  }
}
