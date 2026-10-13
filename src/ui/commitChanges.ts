import { join } from 'node:path';
import * as vscode from 'vscode';
import type { Commit } from '../git/log.ts';
import type { Repository } from '../state/repository.ts';
import { gitUri } from './uri.ts';

/** コミットの変更を、標準の複数ファイル diff エディタで開く。比べる相手は最初の親 */
export async function openCommitChanges(repo: Repository, commit: Commit): Promise<void> {
  const root = repo.root;
  const parent = commit.parents[0];
  const files = await repo.backend.commitFiles(commit.oid, parent);
  const resources = files.map((f) => {
    const original =
      f.status === 'A' || !parent ? gitUri(root, f.path, 'empty') : gitUri(root, f.origPath ?? f.path, 'rev', parent);
    const modified = f.status === 'D' ? gitUri(root, f.path, 'empty') : gitUri(root, f.path, 'rev', commit.oid);
    return [vscode.Uri.file(join(root, f.path)), original, modified];
  });
  await vscode.commands.executeCommand('vscode.changes', `${commit.oid.slice(0, 8)} ${commit.subject}`, resources);
}
