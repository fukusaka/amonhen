import { randomBytes } from 'node:crypto';
import { basename } from 'node:path';
import * as vscode from 'vscode';
import type { Commit } from '../git/log.ts';
import { GraphBuilder } from '../history/graph.ts';
import type { HostMessage, RefLabel, RowData, Scope, WebviewMessage } from '../history/protocol.ts';
import { editableOids, refLabels } from '../history/rows.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository, Section } from '../state/repository.ts';
import { openCommitChanges } from './commitChanges.ts';
import { showError } from './stageCommands.ts';

/** 1 回に読み込むコミットの数 */
const PAGE = 200;
/** 編集できるかを調べる範囲 */
const EDITABLE_LIMIT = 500;

/**
 * コミット履歴をエディタ領域の Webview に表示する。
 * log は分割して読み込み、グラフのレーンはこちらで計算して、描画に必要な最小限の配列だけ送る。
 */
export class HistoryPanel implements vscode.Disposable {
  private static current: HistoryPanel | undefined;

  private readonly panel: vscode.WebviewPanel;
  private readonly manager: RepositoryManager;
  private readonly disposables: vscode.Disposable[] = [];
  private repoListener: vscode.Disposable | undefined;
  private repo: Repository | undefined;
  private scope: Scope = 'head';

  private commits: Commit[] = [];
  private graph = new GraphBuilder();
  private labels = new Map<string, RefLabel[]>();
  private editable = new Set<string>();
  private done = false;
  private loading = false;
  /** reset のたびに増やし、古い読み込みの結果を捨てる */
  private generation = 0;
  private headOid: string | undefined;
  private resetTimer: NodeJS.Timeout | undefined;

  private readonly _onDidReceiveEdit = new vscode.EventEmitter<Extract<WebviewMessage, { type: 'saveMessage' | 'move' }>>();
  /** Webview でメッセージの編集や並べ替えが行われた */
  readonly onDidReceiveEdit = this._onDidReceiveEdit.event;

  static show(extensionUri: vscode.Uri, manager: RepositoryManager): HistoryPanel {
    if (HistoryPanel.current) {
      HistoryPanel.current.panel.reveal();
      return HistoryPanel.current;
    }
    const panel = vscode.window.createWebviewPanel('amonhen.history', 'History', vscode.ViewColumn.Active, {
      enableScripts: true,
      localResourceRoots: [vscode.Uri.joinPath(extensionUri, 'dist', 'webview'), vscode.Uri.joinPath(extensionUri, 'media')],
    });
    HistoryPanel.current = new HistoryPanel(panel, extensionUri, manager);
    return HistoryPanel.current;
  }

  /** 開いていれば返す（Webview の右クリックメニューのコマンドから使う） */
  static get active(): HistoryPanel | undefined {
    return HistoryPanel.current;
  }

  private constructor(panel: vscode.WebviewPanel, extensionUri: vscode.Uri, manager: RepositoryManager) {
    this.panel = panel;
    this.manager = manager;
    panel.iconPath = new vscode.ThemeIcon('history');
    panel.webview.html = html(panel.webview, extensionUri);
    this.disposables.push(
      panel.onDidDispose(() => this.dispose()),
      panel.webview.onDidReceiveMessage((m: WebviewMessage) => void this.onMessage(m)),
      manager.onDidChangeCurrent((r) => this.watch(r)),
    );
    this.watch(manager.current);
  }

  get repository(): Repository | undefined {
    return this.repo;
  }

  commit(oid: string): Commit | undefined {
    return this.commits.find((c) => c.oid === oid);
  }

  post(message: HostMessage): void {
    void this.panel.webview.postMessage(message);
  }

  private watch(repo: Repository | undefined): void {
    this.repoListener?.dispose();
    this.repo = repo;
    this.repoListener = repo?.onDidChange((changed) => this.onRepositoryChange(changed));
    this.panel.title = repo ? `History: ${basename(repo.root)}` : 'History';
    void this.reset();
  }

  /** ブランチやタグ、HEAD が動いたときだけ読み直す。stage などでは読み直さない */
  private onRepositoryChange(changed: ReadonlySet<Section>): void {
    const headOid = this.repo?.current.status?.branch.oid;
    const refsChanged = changed.has('branches') || changed.has('remotes') || changed.has('tags');
    if (refsChanged || headOid !== this.headOid) {
      clearTimeout(this.resetTimer);
      this.resetTimer = setTimeout(() => void this.reset(), 100);
    }
  }

  private async onMessage(message: WebviewMessage): Promise<void> {
    switch (message.type) {
      case 'ready':
        await this.reset();
        break;
      case 'more':
        await this.loadMore();
        break;
      case 'scope':
        this.scope = message.scope;
        await this.reset();
        break;
      case 'open': {
        const commit = this.commit(message.oid);
        if (this.repo && commit) {
          await openCommitChanges(this.repo, commit).catch(showError);
        }
        break;
      }
      case 'saveMessage':
      case 'move':
        // コミットの編集は historyCommands が受け取る
        this._onDidReceiveEdit.fire(message);
        break;
    }
  }

  /** 最初から読み直す */
  async reset(): Promise<void> {
    const generation = ++this.generation;
    this.commits = [];
    this.graph = new GraphBuilder();
    this.done = false;
    this.loading = false;
    const repo = this.repo;
    this.post({ type: 'reset', title: repo ? basename(repo.root) : '', scope: this.scope });
    if (!repo) {
      this.post({ type: 'rows', start: 0, rows: [], done: true });
      return;
    }
    try {
      await repo.want('tags');
      const head = await repo.backend.head();
      const state = repo.current;
      this.headOid = head.oid;
      this.labels = refLabels(head, state.branches ?? [], state.remoteBranches ?? [], state.tags ?? []);
      this.editable = head.oid
        ? editableOids(await repo.backend.log(['--first-parent', 'HEAD'], 0, EDITABLE_LIMIT))
        : new Set();
    } catch (err) {
      showError(err);
    }
    if (generation === this.generation) {
      await this.loadMore();
    }
  }

  private async loadMore(): Promise<void> {
    const repo = this.repo;
    if (!repo || this.loading || this.done) {
      return;
    }
    this.loading = true;
    const generation = this.generation;
    const revs = this.scope === 'all' ? ['HEAD', '--branches', '--remotes', '--tags'] : ['HEAD'];
    let page: Commit[] = [];
    try {
      page = this.headOid ? await repo.backend.log(revs, this.commits.length, PAGE) : [];
    } catch (err) {
      showError(err);
    }
    if (generation !== this.generation) {
      return;
    }
    const start = this.commits.length;
    this.commits.push(...page);
    const rows: RowData[] = page.map((c) => ({
      oid: c.oid,
      subject: c.subject,
      author: c.author,
      date: c.date,
      refs: this.labels.get(c.oid) ?? [],
      graph: this.graph.add(c.oid, c.parents),
      editable: this.editable.has(c.oid),
    }));
    this.done = page.length < PAGE;
    this.loading = false;
    this.post({ type: 'rows', start, rows, done: this.done });
  }

  dispose(): void {
    HistoryPanel.current = undefined;
    clearTimeout(this.resetTimer);
    this.repoListener?.dispose();
    this._onDidReceiveEdit.dispose();
    vscode.Disposable.from(...this.disposables).dispose();
    this.panel.dispose();
  }
}

function html(webview: vscode.Webview, extensionUri: vscode.Uri): string {
  const nonce = randomBytes(16).toString('base64');
  const script = webview.asWebviewUri(vscode.Uri.joinPath(extensionUri, 'dist', 'webview', 'history.js'));
  const style = webview.asWebviewUri(vscode.Uri.joinPath(extensionUri, 'media', 'history.css'));
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<link rel="stylesheet" href="${style}">
</head>
<body>
<header>
  <select id="scope" aria-label="Scope">
    <option value="head">Current Branch</option>
    <option value="all">All Branches</option>
  </select>
  <span id="status"></span>
</header>
<div id="list" tabindex="0"><div id="spacer"></div></div>
<script nonce="${nonce}" src="${script}"></script>
</body>
</html>`;
}
