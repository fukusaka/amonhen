/** 組み込みの `vscode.git` 拡張 API のうち、使う部分だけの型 */
import type { Event, Uri } from 'vscode';

export interface GitExtension {
  readonly enabled: boolean;
  getAPI(version: 1): GitApi;
}

export interface GitApi {
  readonly state: 'uninitialized' | 'initialized';
  readonly onDidChangeState: Event<'uninitialized' | 'initialized'>;
  readonly git: { readonly path: string };
  readonly repositories: readonly GitApiRepository[];
  readonly onDidOpenRepository: Event<GitApiRepository>;
  readonly onDidCloseRepository: Event<GitApiRepository>;
}

export interface GitApiRepository {
  readonly rootUri: Uri;
  readonly state: { readonly onDidChange: Event<void> };
}
