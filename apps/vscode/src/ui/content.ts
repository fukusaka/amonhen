import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { rangeDiffDocument } from './rangeDiff.ts';
import { parseGitUri, SCHEME } from './uri.ts';

/**
 * コミットや index の stage にあるファイルの内容を返す（History の差分やマージエディタで使う）。
 * 取得は常駐 cat-file なので速い。指す内容は変わらないので、取り直しの通知はしない
 */
export class GitContentProvider implements vscode.TextDocumentContentProvider {
  private readonly manager: RepositoryManager;

  constructor(manager: RepositoryManager) {
    this.manager = manager;
  }

  async provideTextDocumentContent(uri: vscode.Uri): Promise<string> {
    const parsed = parseGitUri(uri);
    if (!parsed || parsed.side === 'empty') {
      return '';
    }
    const repo = this.manager.get(parsed.root);
    if (parsed.side === 'rangeDiff') {
      if (!repo) return '';
      return rangeDiffDocument(repo, parsed.ranges ?? [], parsed.note).catch((err: unknown) => `# ${String(err)}\n`);
    }
    const obj = await repo?.backend.object(`${parsed.rev}:${parsed.path}`);
    return obj?.data.toString('utf8') ?? '';
  }
}

export function registerContentProvider(manager: RepositoryManager): vscode.Disposable {
  return vscode.workspace.registerTextDocumentContentProvider(SCHEME, new GitContentProvider(manager));
}
