import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { parseGitUri, SCHEME } from './uri.ts';

/** HEAD・index のファイル内容を返す。取得は常駐 cat-file なので速い */
export class GitContentProvider implements vscode.TextDocumentContentProvider, vscode.Disposable {
  private readonly manager: RepositoryManager;
  private readonly _onDidChange = new vscode.EventEmitter<vscode.Uri>();
  readonly onDidChange = this._onDidChange.event;
  private readonly listener: vscode.Disposable;

  constructor(manager: RepositoryManager) {
    this.manager = manager;
    // index や HEAD が変わったら、開いている文書を取り直させる
    this.listener = manager.onDidChangeContents((repo) => {
      for (const doc of vscode.workspace.textDocuments) {
        const parsed = parseGitUri(doc.uri);
        // コミットの内容は変わらないので取り直さない
        if (parsed && (parsed.side === 'HEAD' || parsed.side === 'index') && parsed.root === repo.root) {
          this._onDidChange.fire(doc.uri);
        }
      }
    });
  }

  async provideTextDocumentContent(uri: vscode.Uri): Promise<string> {
    const parsed = parseGitUri(uri);
    if (!parsed || parsed.side === 'empty') {
      return '';
    }
    const repo = this.manager.get(parsed.root);
    const rev =
      parsed.side === 'HEAD' ? `HEAD:${parsed.path}` : parsed.side === 'rev' ? `${parsed.rev}:${parsed.path}` : `:${parsed.path}`;
    const obj = await repo?.backend.object(rev);
    return obj?.data.toString('utf8') ?? '';
  }

  dispose(): void {
    this.listener.dispose();
    this._onDidChange.dispose();
  }
}

export function registerContentProvider(manager: RepositoryManager): vscode.Disposable {
  const provider = new GitContentProvider(manager);
  return vscode.Disposable.from(provider, vscode.workspace.registerTextDocumentContentProvider(SCHEME, provider));
}
