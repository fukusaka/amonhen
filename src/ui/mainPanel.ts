import { randomBytes } from 'node:crypto';
import { basename } from 'node:path';
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import type { HostMessage, ViewName, WebviewMessage } from '../view/protocol.ts';
import { HistoryController } from './historyController.ts';
import { WorkingCopyController } from './workingCopyController.ts';

/**
 * メインビュー。エディタ領域の 1 つの Webview に、Working Copy か History を出す。
 * どちらを出すかはサイドバーのナビで選ぶ。こちらは画面ごとのコントローラーにメッセージを振り分ける。
 */
export class MainPanel implements vscode.Disposable {
  private static current: MainPanel | undefined;

  readonly history: HistoryController;
  readonly workingCopy: WorkingCopyController;
  private readonly panel: vscode.WebviewPanel;
  private readonly manager: RepositoryManager;
  private readonly disposables: vscode.Disposable[] = [];
  private repo: Repository | undefined;
  private ready = false;
  /** 準備ができる前に求められた画面 */
  private pendingView: ViewName | undefined;

  /** 開いて、view を出す。ref を渡すと History にその ref の履歴を出す */
  static show(extensionUri: vscode.Uri, manager: RepositoryManager, view?: ViewName, ref?: string): MainPanel {
    let current = MainPanel.current;
    if (current) {
      current.panel.reveal();
    } else {
      const panel = vscode.window.createWebviewPanel('amonhen.main', 'Amon Hen', vscode.ViewColumn.Active, {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [
          vscode.Uri.joinPath(extensionUri, 'dist', 'webview'),
          vscode.Uri.joinPath(extensionUri, 'media'),
        ],
      });
      current = new MainPanel(panel, extensionUri, manager);
      MainPanel.current = current;
    }
    if (view === 'history') {
      current.history.showScope(ref ? `ref:${ref}` : undefined);
    }
    if (view) {
      current.showView(view);
    }
    return current;
  }

  /** 開いていれば返す（Webview の右クリックメニューのコマンドから使う） */
  static get active(): MainPanel | undefined {
    return MainPanel.current;
  }

  private constructor(panel: vscode.WebviewPanel, extensionUri: vscode.Uri, manager: RepositoryManager) {
    this.panel = panel;
    this.manager = manager;
    this.history = new HistoryController((m) => this.post(m));
    this.workingCopy = new WorkingCopyController((m) => this.post(m));
    panel.iconPath = vscode.Uri.joinPath(extensionUri, 'media', 'amonhen.svg');
    panel.webview.html = html(panel.webview, extensionUri);
    this.disposables.push(
      this.history,
      this.workingCopy,
      panel.onDidDispose(() => this.dispose()),
      panel.webview.onDidReceiveMessage((m: WebviewMessage) => void this.onMessage(m)),
      manager.onDidChangeCurrent((r) => this.watch(r)),
    );
    this.watch(manager.current);
  }

  get repository(): Repository | undefined {
    return this.repo;
  }

  post(message: HostMessage): void {
    void this.panel.webview.postMessage(message);
  }

  showView(view: ViewName): void {
    if (this.ready) {
      this.post({ type: 'show', view });
    } else {
      this.pendingView = view;
    }
  }

  private watch(repo: Repository | undefined): void {
    this.repo = repo;
    this.panel.title = repo ? `Amon Hen: ${basename(repo.root)}` : 'Amon Hen';
    if (this.ready) {
      this.history.setRepository(repo);
      this.workingCopy.setRepository(repo);
    }
  }

  private async onMessage(message: WebviewMessage): Promise<void> {
    switch (message.type) {
      case 'ready':
        this.ready = true;
        if (this.pendingView) {
          this.post({ type: 'show', view: this.pendingView });
          this.pendingView = undefined;
        }
        this.history.setRepository(this.repo);
        this.workingCopy.setRepository(this.repo);
        break;
      case 'view':
        break;
      case 'wcSelect':
      case 'wcStageFiles':
      case 'wcStageLines':
      case 'wcCommit':
      case 'wcOpenFile':
      case 'wcResolve':
        await this.workingCopy.handle(message);
        break;
      default:
        await this.history.handle(message);
    }
  }

  dispose(): void {
    MainPanel.current = undefined;
    vscode.Disposable.from(...this.disposables).dispose();
    this.panel.dispose();
  }
}

function html(webview: vscode.Webview, extensionUri: vscode.Uri): string {
  const nonce = randomBytes(16).toString('base64');
  const script = webview.asWebviewUri(vscode.Uri.joinPath(extensionUri, 'dist', 'webview', 'main.js'));
  const style = webview.asWebviewUri(vscode.Uri.joinPath(extensionUri, 'media', 'main.css'));
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<link rel="stylesheet" href="${style}">
</head>
<body>
<script nonce="${nonce}" src="${script}"></script>
</body>
</html>`;
}
