/**
 * サイドバーのタグの印のもと。共有の daemon のコアで、タグのリモートでの様子（`tagRemotes`）と署名
 * （`tagSignatures`）を読み、タグ・リモートが変わるたびに読み直す。Tags を開くまでは読まない
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { SignatureCheck } from '../git/signature.ts';
import type { TagRemotes } from '../git/tagRemotes.ts';
import type { Repository, Section } from '../state/repository.ts';

/** コアの `tagSignatures` の 1 件。`signed` がなければ署名のないタグ（軽量タグも） */
export interface TagSignature {
  name: string;
  signed: string | null;
  taggerEmail?: string;
  signature?: SignatureCheck | null;
}

/** 署名は 1 件ごとに git と gpg を起動する（1 件 25 ms ほど）ので、新しいタグから少しずつ確かめて出す */
const SIGNATURE_BATCH = 8;

export class TagMarks implements vscode.Disposable {
  private repo: Repository | undefined;
  private repoListener: vscode.Disposable | undefined;
  private wanted = false;
  private timer: NodeJS.Timeout | undefined;
  /** 読むたびに増やし、古い答えを捨てる */
  private generation = 0;
  private remotesInfo: TagRemotes | undefined;
  private readonly signatures = new Map<string, TagSignature>();

  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChange = this._onDidChange.event;

  /** タグのリモートでの様子（まだ読んでいなければ undefined） */
  get remotes(): TagRemotes | undefined {
    return this.remotesInfo;
  }

  signature(name: string): TagSignature | undefined {
    return this.signatures.get(name);
  }

  setRepository(repo: Repository | undefined): void {
    if (repo === this.repo) return;
    this.repoListener?.dispose();
    this.repo = repo;
    this.clear();
    this.repoListener = repo?.onDidChange((changed) => this.onRepositoryChange(changed));
    if (this.wanted) this.schedule(0);
  }

  /** Tags を開いた（読み始める。もう読んでいれば何もしない） */
  want(): void {
    if (this.wanted) return;
    this.wanted = true;
    this.schedule(0);
  }

  /** 今すぐ読み直す（タグの push・削除の後） */
  async reload(): Promise<void> {
    clearTimeout(this.timer);
    this.timer = undefined;
    await this.load();
  }

  private onRepositoryChange(changed: ReadonlySet<Section>): void {
    // fetch はリモートのブランチを、タグの作成・削除はタグを変える
    if (this.wanted && (changed.has('tags') || changed.has('remotes') || changed.has('branches'))) this.schedule(200);
  }

  private schedule(delay: number): void {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.load();
    }, delay);
  }

  private clear(): void {
    this.generation++;
    this.remotesInfo = undefined;
    this.signatures.clear();
  }

  private async load(): Promise<void> {
    const generation = ++this.generation;
    const repo = this.repo;
    const client = repo && (await daemonFor(repo.root).catch(() => undefined));
    if (!repo || !client || generation !== this.generation) return;
    try {
      // daemon は ref を変わったと知るまで読み直さないので、先に読み直させる（端末の `git tag` で作ったタグも）
      await client.call({ op: 'refs' });
      const reply = (await client.call({ op: 'run', command: { op: 'tagRemotes' } })).json;
      if (generation !== this.generation) return;
      this.remotesInfo = reply['ok'] === true ? (reply['info'] as unknown as TagRemotes) : undefined;
      this._onDidChange.fire();
      // 新しいタグから（サイドバーと同じ順）
      const names = [...(repo.current.tags ?? [])]
        .sort((a, b) => b.date - a.date)
        .map((t) => t.name.replace(/^refs\/tags\//, ''));
      for (let i = 0; i < names.length; i += SIGNATURE_BATCH) {
        const batch = names.slice(i, i + SIGNATURE_BATCH);
        const signed = (await client.call({ op: 'run', command: { op: 'tagSignatures', names: batch } })).json;
        if (generation !== this.generation) return;
        const tags = ((signed['info'] as { tags?: TagSignature[] } | undefined)?.tags ?? []).filter((t) => t.signed);
        for (const name of batch) this.signatures.delete(name);
        for (const t of tags) this.signatures.set(t.name, t);
        if (tags.length > 0) this._onDidChange.fire();
      }
    } catch {
      // 印は出せなくてもよい（読めなければ出さない）
    }
  }

  dispose(): void {
    clearTimeout(this.timer);
    this.repoListener?.dispose();
    this._onDidChange.dispose();
  }
}
