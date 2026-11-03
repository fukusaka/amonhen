/**
 * メインビューの File History の拡張ホスト側。共有の daemon のコアの `fileHistory` で、ファイルを変えたコミットを
 * 読む（名前の変更も追う）。新しい方の 100 件を先に送ってから、残りを読んで送り直す（アプリと同じ）
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { FileHistoryEntry, FileHistoryHostMessage, FileHistoryWebviewMessage } from '../fileHistory/protocol.ts';
import type { Repository, Section } from '../state/repository.ts';

/** 先に読む数（`git log -n` は Bloom フィルタがなくても早く返る） */
const FIRST_PAGE = 100;

export class FileHistoryController implements vscode.Disposable {
  private readonly post: (message: FileHistoryHostMessage) => void;
  private repo: Repository | undefined;
  private repoListener: vscode.Disposable | undefined;
  /** リポジトリの中のパス（/ 区切り） */
  private path: string | undefined;
  /** マージも出すか（ページの Merge Commits のチェック。既定は出さない、アプリと同じ） */
  private merges = false;
  private headOid: string | undefined;
  private timer: NodeJS.Timeout | undefined;
  /** 読むたびに増やし、古い答えを捨てる */
  private generation = 0;

  constructor(post: (message: FileHistoryHostMessage) => void) {
    this.post = post;
  }

  setRepository(repo: Repository | undefined): void {
    if (repo === this.repo) return;
    this.repoListener?.dispose();
    this.headOid = repo?.current.status?.branch.oid;
    const switched = this.repo !== undefined;
    this.repoListener = repo?.onDidChange((changed) => this.onRepositoryChange(changed));
    this.repo = repo;
    this.generation++;
    // 別のリポジトリに切り替えたら、そのパスは意味がない。開いた直後（Webview の準備の前に頼まれた）なら読む
    if (switched) this.path = undefined;
    else if (this.path) void this.load();
  }

  /** `path` の履歴を出す */
  async show(path: string): Promise<void> {
    this.path = path;
    await this.load();
  }

  async handle(message: FileHistoryWebviewMessage): Promise<void> {
    switch (message.type) {
      case 'fileHistoryLoad':
        this.merges = message.merges;
        await this.load();
        break;
      case 'fileHistoryReveal':
        await vscode.commands.executeCommand('amonhen.reflog.showInHistory', { oid: message.oid });
        break;
    }
  }

  /** HEAD が動いたら（コミット・checkout・reset など）読み直す */
  private onRepositoryChange(changed: ReadonlySet<Section>): void {
    const headOid = this.repo?.current.status?.branch.oid;
    const headMoved = headOid !== undefined && headOid !== this.headOid;
    this.headOid = headOid ?? this.headOid;
    if (this.path && (changed.has('branches') || headMoved)) {
      clearTimeout(this.timer);
      this.timer = setTimeout(() => void this.load(), 100);
    }
  }

  private async load(): Promise<void> {
    const generation = ++this.generation;
    const { repo, path, merges } = this;
    if (!repo || !path) return;
    const send = (entries: FileHistoryEntry[], complete: boolean, error?: string) =>
      this.post({ type: 'fileHistory', path, entries, complete, merges, ...(error ? { error } : {}) });
    try {
      const client = await daemonFor(repo.root);
      if (!client) {
        send([], true, 'File History needs the Amon Hen engine (amonhen.daemon.enabled)');
        return;
      }
      const read = async (limit?: number) => {
        const reply = (await client.call({ op: 'run', command: { op: 'fileHistory', path, merges, limit: limit ?? null } })).json;
        if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? 'Can’t read the file’s history'));
        const info = reply['info'] as { entries?: FileHistoryEntry[]; more?: boolean } | undefined;
        const entries = (info?.entries ?? []).map((e) => ({
          oid: e.oid,
          subject: e.subject,
          author: e.author,
          time: e.time,
          path: e.path,
        }));
        return { entries, more: info?.more === true };
      };
      const first = await read(FIRST_PAGE);
      if (generation !== this.generation) return;
      send(first.entries, !first.more);
      if (!first.more) return;
      const all = await read();
      if (generation !== this.generation) return;
      send(all.entries, true);
    } catch (err) {
      if (generation === this.generation) send([], true, err instanceof Error ? err.message : String(err));
    }
  }

  dispose(): void {
    clearTimeout(this.timer);
    this.repoListener?.dispose();
  }
}
