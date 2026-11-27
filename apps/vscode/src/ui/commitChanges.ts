import { basename, join } from 'node:path';
import * as vscode from 'vscode';
import type { Commit, FileChange } from '../git/log.ts';
import type { DaemonClient } from '../daemon/client.ts';
import { daemonFor } from '../daemon/connection.ts';
import { commitFiles } from '../daemon/diff.ts';
import type { Repository } from '../state/repository.ts';
import { showError } from './stageCommands.ts';
import { gitUri } from './uri.ts';

/** コミットの中のファイルの、変更前（最初の親）と変更後の URI */
function fileUris(root: string, oid: string, parent: string | undefined, f: FileChange): [vscode.Uri, vscode.Uri] {
  const original =
    f.status === 'A' || !parent ? gitUri(root, f.path, 'empty') : gitUri(root, f.origPath ?? f.path, 'rev', parent);
  const modified = f.status === 'D' ? gitUri(root, f.path, 'empty') : gitUri(root, f.path, 'rev', oid);
  return [original, modified];
}

/**
 * コミットの変更を、標準の複数ファイル diff エディタで開く。比べる相手は最初の親。`known` は分かっている変わった
 * ファイル（エンジンで読んだもの。なければ git で読む）
 */
export async function openCommitChanges(repo: Repository, commit: Commit, known?: readonly FileChange[]): Promise<void> {
  const root = repo.root;
  const parent = commit.parents[0];
  const files = known ?? (await repo.backend.commitFiles(commit.oid, parent));
  const resources = files.map((f) => [vscode.Uri.file(join(root, f.path)), ...fileUris(root, commit.oid, parent, f)]);
  await vscode.commands.executeCommand('vscode.changes', `${commit.oid.slice(0, 8)} ${commit.subject}`, resources);
}

/**
 * partial clone で手元にない中身 `ids` があるので、エディタで開く（`open`。git が勝手に取りに行く）前に通知で聞く。
 * 通知は待たず、モーダルにしない。「Fetch and Open」を押したときだけ（まとめて 1 回で）取ってから開き、`fetched` を
 * 呼ぶ（History の差分を読み直す）。閉じれば何もしない
 */
export function askBeforeOpen(
  client: DaemonClient,
  ids: readonly string[],
  notice: string,
  open: () => Promise<void>,
  fetched?: () => Promise<void>,
): void {
  void vscode.window.showInformationMessage(notice, 'Fetch and Open').then(async (choice) => {
    if (choice !== 'Fetch and Open') return;
    try {
      await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: 'Fetching the contents…' }, async () => {
        const reply = (await client.call({ op: 'run', command: { op: 'fetchContents', ids: [...ids] } })).json;
        if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? 'Can’t fetch the contents'));
      });
    } catch (err) {
      showError(new Error(`Couldn’t fetch the contents: ${err instanceof Error ? err.message : String(err)}`));
      return;
    }
    await open().catch(showError);
    await fetched?.().catch(showError);
  });
}

/**
 * コミットの変更を、標準の複数ファイル diff エディタで開く（Push・Pull のシートのコミットの一覧）。partial clone なら、
 * 変わったファイルと手元にない中身をエンジンで読み（git の rename の検出も中身を取りに行くので、git では読まない）、
 * なければ開く前に通知で聞く（`askBeforeOpen`）。partial clone でなければ、確かめずにそのまま開く
 */
export async function openCommitChangesAsking(repo: Repository, commit: Commit, fetched?: () => Promise<void>): Promise<void> {
  const client = await daemonFor(repo.root).catch(() => undefined);
  const form = client && (await client.call({ op: 'run', command: { op: 'repositoryForm' } }).catch(() => undefined))?.json;
  const partial = form?.['ok'] === true && !!(form['info'] as { partial?: unknown } | undefined)?.partial;
  if (!client || !partial) {
    await openCommitChanges(repo, commit);
    return;
  }
  const reply = await client.call({ op: 'commitDiff', id: commit.oid });
  const known = commitFiles(reply);
  const ids = Array.isArray(reply.json['missing']) ? (reply.json['missing'] as string[]) : [];
  if (ids.length === 0) {
    await openCommitChanges(repo, commit, known);
    return;
  }
  const files = ((reply.json['files'] as { missing?: boolean }[] | undefined) ?? []).filter((f) => f.missing).length;
  askBeforeOpen(
    client,
    ids,
    `The contents of ${files} file${files === 1 ? '' : 's'} aren’t here yet (partial clone).`,
    () => openCommitChanges(repo, commit, known),
    fetched,
  );
}

/** 2 つのコミットの間の変更（`base` から `tip` まで）を、標準の複数ファイル diff エディタで開く */
export async function openRangeChanges(repo: Repository, base: string, tip: string, title: string): Promise<void> {
  const root = repo.root;
  const files = await repo.backend.commitFiles(tip, base);
  const resources = files.map((f) => [vscode.Uri.file(join(root, f.path)), ...fileUris(root, tip, base, f)]);
  await vscode.commands.executeCommand('vscode.changes', title, resources);
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
