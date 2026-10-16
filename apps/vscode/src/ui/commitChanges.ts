import { basename, join } from 'node:path';
import * as vscode from 'vscode';
import type { Commit, FileChange } from '../git/log.ts';
import type { Repository } from '../state/repository.ts';
import { gitUri } from './uri.ts';

/** コミットの中のファイルの、変更前（最初の親）と変更後の URI */
function fileUris(root: string, oid: string, parent: string | undefined, f: FileChange): [vscode.Uri, vscode.Uri] {
  const original =
    f.status === 'A' || !parent ? gitUri(root, f.path, 'empty') : gitUri(root, f.origPath ?? f.path, 'rev', parent);
  const modified = f.status === 'D' ? gitUri(root, f.path, 'empty') : gitUri(root, f.path, 'rev', oid);
  return [original, modified];
}

/** コミットの変更を、標準の複数ファイル diff エディタで開く。比べる相手は最初の親 */
export async function openCommitChanges(repo: Repository, commit: Commit): Promise<void> {
  const root = repo.root;
  const parent = commit.parents[0];
  const files = await repo.backend.commitFiles(commit.oid, parent);
  const resources = files.map((f) => [vscode.Uri.file(join(root, f.path)), ...fileUris(root, commit.oid, parent, f)]);
  await vscode.commands.executeCommand('vscode.changes', `${commit.oid.slice(0, 8)} ${commit.subject}`, resources);
}

/** コミットの中の 1 ファイルの差分を、History の横に開く */
export async function openCommitFile(
  repo: Repository,
  oid: string,
  parent: string | undefined,
  file: FileChange,
): Promise<void> {
  const [original, modified] = fileUris(repo.root, oid, parent, file);
  const title = `${basename(file.path)} (${oid.slice(0, 8)})`;
  await vscode.commands.executeCommand('vscode.diff', original, modified, title, {
    preview: true,
    viewColumn: vscode.ViewColumn.Beside,
  });
}
