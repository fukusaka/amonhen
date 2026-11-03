/** メインビューの Reflog の拡張ホスト側。共有の daemon のコアの `reflog` で、HEAD の reflog を読む */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { ReflogEntry, ReflogHostMessage, ReflogWebviewMessage } from '../reflog/protocol.ts';
import type { Repository, Section } from '../state/repository.ts';
import { showError } from './stageCommands.ts';

/** コアの `reflog` の答えの 1 行 */
interface CoreEntry {
  selector: string;
  new: string;
  time: number;
  message: string;
  subject: string;
  reachable: boolean;
}

export class ReflogController implements vscode.Disposable {
  private readonly post: (message: ReflogHostMessage) => void;
  private repo: Repository | undefined;
  private repoListener: vscode.Disposable | undefined;
  /** 一度出したら、HEAD やブランチが動くたびに読み直す */
  private shown = false;
  private headOid: string | undefined;
  private timer: NodeJS.Timeout | undefined;
  /** 読むたびに増やし、古い答えを捨てる */
  private generation = 0;

  constructor(post: (message: ReflogHostMessage) => void) {
    this.post = post;
  }

  setRepository(repo: Repository | undefined): void {
    this.repoListener?.dispose();
    this.repo = repo;
    this.headOid = repo?.current.status?.branch.oid;
    this.repoListener = repo?.onDidChange((changed) => this.onRepositoryChange(changed));
    if (this.shown) void this.load();
  }

  async handle(message: ReflogWebviewMessage): Promise<void> {
    switch (message.type) {
      case 'reflogLoad':
        this.shown = true;
        await this.load();
        break;
      case 'reflogAction': {
        const target = { oid: message.oid, selector: message.selector };
        await vscode.commands.executeCommand(
          message.action === 'newBranch' ? 'amonhen.newBranch' : 'amonhen.checkoutCommit',
          target,
        );
        break;
      }
    }
  }

  /** HEAD が動いたら（コミット・checkout・reset など）読み直す */
  private onRepositoryChange(changed: ReadonlySet<Section>): void {
    const headOid = this.repo?.current.status?.branch.oid;
    const headMoved = headOid !== undefined && headOid !== this.headOid;
    this.headOid = headOid ?? this.headOid;
    if (this.shown && (changed.has('branches') || headMoved)) {
      clearTimeout(this.timer);
      this.timer = setTimeout(() => void this.load(), 100);
    }
  }

  private async load(): Promise<void> {
    const generation = ++this.generation;
    const repo = this.repo;
    if (!repo) {
      this.post({ type: 'reflog', entries: [] });
      return;
    }
    try {
      const client = await daemonFor(repo.root);
      if (!client) {
        this.post({ type: 'reflog', entries: [], error: 'Reflog needs the Amon Hen engine (amonhen.daemon.enabled)' });
        return;
      }
      const reply = (await client.call({ op: 'run', command: { op: 'reflog' } })).json;
      if (generation !== this.generation) return;
      if (reply['ok'] !== true) {
        this.post({ type: 'reflog', entries: [], error: String(reply['error'] ?? 'Can’t read the reflog') });
        return;
      }
      const info = reply['info'] as { entries?: CoreEntry[] } | undefined;
      const entries: ReflogEntry[] = (info?.entries ?? []).map((e) => ({
        selector: e.selector,
        oid: e.new,
        message: e.message,
        subject: e.subject,
        time: e.time,
        reachable: e.reachable,
      }));
      this.post({ type: 'reflog', entries });
    } catch (err) {
      if (generation === this.generation) showError(err);
    }
  }

  dispose(): void {
    clearTimeout(this.timer);
    this.repoListener?.dispose();
  }
}
