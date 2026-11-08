/**
 * アーカイブと bundle（docs/ux.md の 7d・7e。アプリのシートを QuickPick と入力欄で）。書き出す・bundle を作る・bundle から
 * 取り込むは共有の daemon のコアで行い（partial clone の中身・shallow clone の断りもコアが見る）、bundle から clone は
 * リポジトリなしで git を動かす
 */
import { open } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import * as vscode from 'vscode';
import {
  ARCHIVE_FORMATS,
  type ArchiveFormat,
  archiveTopFolder,
  type BundleHeader,
  cloneBranch,
  defaultArchiveFormat,
  folderForBundle,
  readBundleHeader,
} from '../bundle/archiveBundle.ts';
import type { Json } from '../daemon/client.ts';
import { daemonFor } from '../daemon/connection.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import { archiveFormat, setArchiveFormat } from './preferences.ts';
import type { Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';

/** コアの命令を実行する。うまくいかなければ投げる */
async function run(repo: Repository, command: Json, title: string): Promise<Json> {
  const client = await daemonFor(repo.root);
  if (!client) throw new Error(`${title} needs the Amon Hen engine (amonhen.daemon.enabled)`);
  const reply = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title }, async () =>
    (await client.call({ op: 'run', command })).json,
  );
  if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `${title} failed`));
  return reply;
}

function info(reply: Json): Json {
  return (reply['info'] as Json | undefined) ?? {};
}

function sizeText(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 書いたファイルを知らせ、ファイルマネージャーで見せられるようにする */
async function saved(title: string, file: string, size: number): Promise<void> {
  const reveal = process.platform === 'darwin' ? 'Reveal in Finder' : 'Reveal in File Explorer';
  const choice = await vscode.window.showInformationMessage(`${title}: ${basename(file)} (${sizeText(size)})`, reveal);
  if (choice === reveal) await vscode.commands.executeCommand('revealFileInOS', vscode.Uri.file(file));
}

/** 書き出す時点: サイドバーのブランチ・リモートブランチ・タグ、History の行のコミット、なければ HEAD */
async function targetOf(repo: Repository, arg: unknown): Promise<{ rev: string; label: string }> {
  const node = arg as Node | undefined;
  if (node?.kind === 'branch' || node?.kind === 'tag') return { rev: node.ref.name, label: shortName(node.ref.name) };
  const oid = (arg as { oid?: unknown } | undefined)?.oid;
  if (typeof oid === 'string') return { rev: oid, label: oid.slice(0, 8) };
  const head = await repo.backend.head();
  return { rev: 'HEAD', label: head.branch ? shortName(head.branch) : (head.oid ?? 'HEAD').slice(0, 8) };
}

/** `git archive` で書き出す（形・最上位のフォルダ・1 つのフォルダを選んでから、保存先を選ぶ） */
async function exportArchive(repo: Repository, arg: unknown): Promise<void> {
  const target = await targetOf(repo, arg);
  const title = `Export Archive of ${target.label}`;
  const first = defaultArchiveFormat(process.platform, archiveFormat());
  const formats = [first, ...ARCHIVE_FORMATS.filter((f) => f !== first)];
  const format = (
    await vscode.window.showQuickPick(
      formats.map((f) => ({ label: f, format: f })),
      { title, placeHolder: 'Format' },
    )
  )?.format;
  if (!format) return;
  const top = archiveTopFolder(basename(repo.root), target.label);
  const topPick = await vscode.window.showQuickPick(
    [
      { label: top, description: 'Top folder inside the archive', prefix: `${top}/` },
      { label: 'None', description: 'Files at the top of the archive', prefix: '' },
    ],
    { title, placeHolder: 'Top folder' },
  );
  if (!topPick) return;
  const scope = await vscode.window.showQuickPick(
    [
      { label: 'All Files', description: 'As git archive writes them; .gitattributes decides what is left out', one: false },
      { label: 'One Folder…', description: 'Choose a folder in this commit', one: true },
    ],
    { title, placeHolder: 'What to export' },
  );
  if (!scope) return;
  let paths: string[] = [];
  if (scope.one) {
    const folders = await repo.backend.treeFolders(target.rev);
    if (folders.length === 0) {
      void vscode.window.showInformationMessage('This commit has no folders.');
      return;
    }
    const folder = await vscode.window.showQuickPick(folders, { title, placeHolder: 'Folders in this commit' });
    if (!folder) return;
    paths = [folder];
  }
  // partial clone なら、手元にない中身を先にまとめて取ると確かめる
  const need = info(await run(repo, { op: 'archiveContents', rev: target.rev, paths }, 'Checking the file contents'));
  const count = Number(need['count'] ?? 0);
  if (count > 0) {
    const ok = await vscode.window.showWarningMessage(
      'Fetch File Contents First?',
      {
        modal: true,
        detail: `This is a partial clone: the contents of ${count}${need['more'] ? ' or more' : ''} files in the archive aren’t here yet. They are fetched from the remote in one go before the archive is written.`,
      },
      'Fetch and Export',
    );
    if (ok !== 'Fetch and Export') return;
  }
  const name = `${paths[0] ? `${top}-${basename(paths[0])}` : top}.${format}`;
  const uri = await vscode.window.showSaveDialog({
    title,
    defaultUri: vscode.Uri.file(join(dirname(repo.root), name)),
    filters: { [format]: [format === 'tar.gz' ? 'gz' : format] },
  });
  if (!uri) return;
  await setArchiveFormat(format as ArchiveFormat);
  const reply = await run(
    repo,
    { op: 'archive', rev: target.rev, format, prefix: topPick.prefix, paths, file: uri.fsPath },
    `Exporting ${basename(uri.fsPath)}`,
  );
  await saved('Exported', uri.fsPath, Number(info(reply)['size'] ?? 0));
}

/** サイドバーで選んだブランチ・タグ（いくつも選べる）。なければ HEAD とすべてのブランチ・タグ */
function refsOf(arg: unknown, selected: unknown): string[] {
  const nodes = (Array.isArray(selected) && selected.length > 0 ? selected : [arg]) as (Node | undefined)[];
  return nodes.flatMap((n) => (n?.kind === 'branch' || n?.kind === 'tag' ? [n.ref.name] : []));
}

/** `git bundle create`（すべての履歴か、受け取る側が持っているコミットより後だけ） */
async function saveBundle(repo: Repository, refs: string[]): Promise<void> {
  const what = refs.length === 0 ? 'HEAD and all branches and tags' : refs.map(shortName).join(', ');
  const title = `Save as Bundle: ${what}`;
  const range = await vscode.window.showQuickPick(
    [
      { label: 'All history', after: false },
      { label: 'Only commits after…', description: 'Whoever receives the bundle needs that commit already', after: true },
    ],
    { title },
  );
  if (!range) return;
  let bases: string[] = [];
  if (range.after) {
    const state = repo.current;
    const known = [...(state.branches ?? []), ...(state.remoteBranches ?? []), ...(state.tags ?? [])].map((r) => shortName(r.name));
    const picked = await vscode.window.showQuickPick(
      [{ label: 'A Commit…', description: 'Type a commit ID', commit: true }, ...known.map((label) => ({ label, commit: false }))],
      { title, placeHolder: 'Tag, branch or commit' },
    );
    if (!picked) return;
    const base = picked.commit
      ? (await vscode.window.showInputBox({ title, prompt: 'Commit ID', validateInput: (v) => (v.trim() ? undefined : 'Type a commit ID') }))?.trim()
      : picked.label;
    if (!base) return;
    bases = [base];
  }
  const preview = info(await run(repo, { op: 'bundlePreview', refs, bases }, 'Checking the bundle'));
  if (preview['beyondShallow'] === true) {
    throw new Error('This shallow clone doesn’t have all the history the bundle needs. Fetch more history, or choose a newer base.');
  }
  if (Number(preview['commits'] ?? 0) === 0) {
    void vscode.window.showInformationMessage('Nothing to Save: there are no commits to put in the bundle.');
    return;
  }
  const contents = Number(preview['contents'] ?? 0);
  if (contents > 0) {
    const ok = await vscode.window.showWarningMessage(
      'Fetch File Contents First?',
      {
        modal: true,
        detail: `This is a partial clone: the contents of ${contents}${preview['more'] ? ' or more' : ''} files in the bundle aren’t here yet. They are fetched from the remote in one go before the bundle is written.`,
      },
      'Fetch and Save',
    );
    if (ok !== 'Fetch and Save') return;
  }
  const name = refs.length === 1 ? `${basename(repo.root)}-${shortName(refs[0]!).replace(/\//g, '-')}` : basename(repo.root);
  const uri = await vscode.window.showSaveDialog({
    title,
    defaultUri: vscode.Uri.file(join(dirname(repo.root), `${name}.bundle`)),
    filters: { Bundle: ['bundle'] },
  });
  if (!uri) return;
  const reply = await run(repo, { op: 'bundle', refs, bases, file: uri.fsPath }, `Saving ${basename(uri.fsPath)}`);
  await saved('Saved', uri.fsPath, Number(info(reply)['size'] ?? 0));
}

/** bundle のファイルを選ぶ */
async function pickBundle(title: string): Promise<string | undefined> {
  const [uri] =
    (await vscode.window.showOpenDialog({ title, canSelectMany: false, filters: { Bundle: ['bundle'], 'All Files': ['*'] } })) ?? [];
  return uri?.fsPath;
}

/** bundle のブランチを `refs/remotes/<name>/*` に取り込む（タグは取らない） */
async function fetchBundle(repo: Repository): Promise<void> {
  const title = 'Fetch from Bundle';
  const file = await pickBundle('Choose a bundle file (git bundle) to fetch from');
  if (!file) return;
  const check = info(await run(repo, { op: 'bundleCheck', file }, 'Reading the bundle'));
  const refs = ((check['refs'] as string[] | undefined) ?? []).filter((r) => r.startsWith('refs/heads/'));
  if (refs.length === 0) throw new Error('The bundle has no branches.');
  const missing = Number(check['missing'] ?? 0);
  if (missing > 0) {
    throw new Error(`This repository doesn’t have ${missing} ${missing === 1 ? 'commit' : 'commits'} the bundle builds on. Fetch them first.`);
  }
  const remotes = new Set((repo.current.remotes ?? []).map((r) => r.name));
  const name = await vscode.window.showInputBox({
    title,
    prompt: `Fetch ${refs.map(shortName).join(', ')} under refs/remotes/<name>/`,
    value: String(check['name'] ?? 'bundle'),
    validateInput: (v) => {
      const n = v.trim();
      if (!n) return 'Type a name';
      if (/[\s~^:?*[\\]|\.\.|^[./]|[./]$|@\{/.test(n)) return 'Not a valid name';
      if (remotes.has(n)) return `${n} is a remote; choose another name`;
      return undefined;
    },
  });
  if (!name) return;
  const reply = await run(repo, { op: 'fetchBundle', file, name: name.trim() }, `Fetching from ${basename(file)}`);
  const fetched = (info(reply)['refs'] as string[] | undefined) ?? [];
  await repo.refresh();
  void vscode.window.showInformationMessage(
    `Fetched ${fetched.length} ${fetched.length === 1 ? 'branch' : 'branches'} under refs/remotes/${name.trim()}/`,
  );
}

/** parent の中でまだない名前（name、name-2、name-3…） */
function freeName(parent: string, base: string): string {
  let name = base;
  for (let i = 2; existsSync(join(parent, name)); i++) name = `${base}-${i}`;
  return name;
}

/** bundle のファイルから clone して開く（HEAD がなければブランチを選ぶ） */
async function cloneBundle(manager: RepositoryManager): Promise<void> {
  const title = 'Clone from Bundle';
  const git = manager.executor;
  if (!git) throw new Error('git isn’t available');
  const file = await pickBundle('Choose a bundle file (git bundle) to clone');
  if (!file) return;
  const handle = await open(file, 'r');
  let header: BundleHeader | undefined;
  try {
    const buffer = Buffer.alloc(1024 * 1024);
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
    header = readBundleHeader(buffer.subarray(0, bytesRead));
  } finally {
    await handle.close();
  }
  if (!header) throw new Error(`${basename(file)} isn’t a bundle`);
  if (header.prerequisites > 0) {
    throw new Error('This bundle has only the commits after others, so it can’t be cloned. Fetch from it in a repository that has them.');
  }
  let branch: string | undefined;
  if (!header.head) {
    const first = cloneBranch(header);
    if (!first) throw new Error('The bundle has no branches.');
    const branches = header.refs.filter((r) => r.startsWith('refs/heads/')).map(shortName);
    branch = (
      await vscode.window.showQuickPick([first, ...branches.filter((b) => b !== first)], {
        title,
        placeHolder: 'The bundle has no HEAD: branch to check out',
      })
    )?.trim();
    if (!branch) return;
  }
  const [parent] =
    (await vscode.window.showOpenDialog({ title: 'Folder to clone into', canSelectFiles: false, canSelectFolders: true, openLabel: 'Select' })) ?? [];
  if (!parent) return;
  const folder = await vscode.window.showInputBox({
    title,
    prompt: `Name of the new folder in ${parent.fsPath}`,
    value: freeName(parent.fsPath, folderForBundle(file)),
    validateInput: (v) => {
      const n = v.trim();
      if (!n || /[\\/]/.test(n)) return 'Type a folder name';
      if (existsSync(join(parent.fsPath, n))) return `${n} already exists`;
      return undefined;
    },
  });
  if (!folder) return;
  const dest = join(parent.fsPath, folder.trim());
  await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: `Cloning ${basename(file)}` }, () =>
    git.exec(['clone', ...(branch ? ['-b', branch] : []), '--', file, dest], { cwd: parent.fsPath }),
  );
  // git の clone（vscode.git）と同じく、開き方を尋ねる
  const choice = await vscode.window.showInformationMessage(
    'Would you like to open the cloned repository, or add it to the current workspace?',
    { modal: true, detail: dest },
    'Open',
    'Open in New Window',
    'Add to Workspace',
  );
  if (choice === 'Open') await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(dest));
  else if (choice === 'Open in New Window') await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(dest), { forceNewWindow: true });
  else if (choice === 'Add to Workspace') {
    vscode.workspace.updateWorkspaceFolders(vscode.workspace.workspaceFolders?.length ?? 0, 0, { uri: vscode.Uri.file(dest) });
  }
}

export function registerBundleArchiveCommands(manager: RepositoryManager): vscode.Disposable {
  const withRepo = (id: string, fn: (repo: Repository, arg: unknown, selected: unknown) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (arg?: unknown, selected?: unknown) => {
      const repo = manager.current;
      if (!repo) return;
      try {
        await fn(repo, arg, selected);
      } catch (err) {
        showError(err);
      }
    });
  return vscode.Disposable.from(
    withRepo('amonhen.exportArchive', (repo, arg) => exportArchive(repo, arg)),
    withRepo('amonhen.saveBundle', (repo, arg, selected) => saveBundle(repo, refsOf(arg, selected))),
    withRepo('amonhen.saveRepositoryBundle', (repo) => saveBundle(repo, [])),
    withRepo('amonhen.fetchBundle', (repo) => fetchBundle(repo)),
    vscode.commands.registerCommand('amonhen.cloneBundle', async () => {
      try {
        await cloneBundle(manager);
      } catch (err) {
        showError(err);
      }
    }),
  );
}
