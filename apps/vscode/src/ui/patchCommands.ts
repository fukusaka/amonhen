/**
 * パッチ: コミットをパッチに書き出す（git format-patch）、パッチを当てる（メールの形は git am、ただの diff は git apply）。
 * Remote-SSH では拡張が SSH 先で動くので、パッチのファイルも SSH 先のもの
 */
import { basename } from 'node:path';
import * as vscode from 'vscode';
import type { FormatPatchOptions, RebaseResult } from '../git/backend.ts';
import { parsePatch, patchPaths, type PatchSummary } from '../git/patch.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import { MainPanel } from './mainPanel.ts';
import { showError } from './stageCommands.ts';

/** Webview の右クリックメニューから渡される情報（data-vscode-context） */
interface CommitContext {
  oid: string;
}

const PATCH_EXTENSIONS = ['patch', 'diff', 'mbox', 'eml'];

export function registerPatchCommands(manager: RepositoryManager): vscode.Disposable {
  const save = (all: boolean) => async (ctx?: CommitContext) => {
    const repo = MainPanel.active?.history?.repository;
    if (!repo || !ctx?.oid) return;
    const oids = all ? await repo.backend.firstParentsToHead(ctx.oid) : [ctx.oid];
    if (!oids) {
      void vscode.window.showWarningMessage('This commit isn’t on HEAD’s first-parent line');
      return;
    }
    await savePatches(repo, oids);
  };
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.history.savePatch', save(false)),
    vscode.commands.registerCommand('amonhen.history.savePatches', save(true)),
    vscode.commands.registerCommand('amonhen.history.selection.savePatches', async () => {
      const panel = MainPanel.active?.history;
      const repo = panel?.repository;
      if (!panel || !repo) return;
      const commits = await panel.selectedCommits();
      // マージはパッチにできない（git format-patch は飛ばす）
      if (commits.some((c) => c.parents.length > 1)) {
        void vscode.window.showWarningMessage('A merge can’t be saved as a patch. Leave the merges out of the selection.');
        return;
      }
      if (commits.length > 1) await savePatches(repo, commits.map((c) => c.oid).reverse());
    }),
    vscode.commands.registerCommand('amonhen.applyPatch', async (uri?: vscode.Uri, uris?: vscode.Uri[]) => {
      const repo = manager.current;
      if (!repo) return;
      const chosen = uris?.length ? uris : uri instanceof vscode.Uri ? [uri] : await pickPatches();
      if (chosen?.length) await applyPatches(repo, chosen);
    }),
    vscode.commands.registerCommand('amonhen.amRetryThreeWay', async () => {
      const repo = manager.current;
      if (!repo) return;
      try {
        const result = await vscode.window.withProgress(
          { location: vscode.ProgressLocation.Window, title: 'Retry with 3-Way Merge' },
          () => repo.backend.amThreeWay(),
        );
        if (result !== 'done') {
          void vscode.window.showWarningMessage('Resolve and stage the conflicts, then Continue in the sidebar');
        }
      } catch (err) {
        showError(err);
      } finally {
        await repo.refresh();
      }
    }),
  );
}

/** oids（古い順）をパッチにする。形（1 コミット 1 ファイルか 1 つの mbox か）と送り状を選んでもらう */
async function savePatches(repo: Repository, oids: string[]): Promise<void> {
  const choice = await vscode.window.showQuickPick(
    [
      { label: 'One File per Commit', description: '0001-….patch (git format-patch’s default)', single: false, cover: false },
      { label: 'One File per Commit, with a Cover Letter', description: 'Also writes 0000-cover-letter.patch', single: false, cover: true },
      { label: 'One mbox File', description: 'git format-patch --stdout', single: true, cover: false },
    ],
    { title: oids.length === 1 ? 'Save as Patch' : `Save ${oids.length} Commits as Patches` },
  );
  if (!choice) return;
  // 版（-v）。空か 1 なら付けない。2 からは v2-0001-….patch のように名前と件名に入る
  const version = await vscode.window.showInputBox({
    title: 'Version (-v)',
    prompt: 'Leave empty for the first version. 2 or more names the files v2-0001-….patch and the subjects [PATCH v2].',
    placeHolder: '1',
    validateInput: (v) => (v.trim() === '' || /^[1-9]\d*$/.test(v.trim()) ? undefined : 'Type a whole number'),
  });
  if (version === undefined) return;
  const reroll = Number(version.trim() || '1');
  const root = vscode.Uri.file(repo.root);
  const out: { dir?: string; file?: string } = {};
  if (choice.single) {
    const uri = await vscode.window.showSaveDialog({
      defaultUri: vscode.Uri.joinPath(root, '..', 'patches.mbox'),
      filters: { mbox: ['mbox'] },
    });
    if (!uri) return;
    out.file = uri.fsPath;
  } else {
    const [uri] =
      (await vscode.window.showOpenDialog({
        canSelectFiles: false,
        canSelectFolders: true,
        canSelectMany: false,
        defaultUri: root,
        openLabel: 'Save Here',
        title: 'Folder for the patches (files with the same names are replaced)',
      })) ?? [];
    if (!uri) return;
    out.dir = uri.fsPath;
  }
  const options: FormatPatchOptions = { coverLetter: choice.cover, ...(reroll > 1 ? { reroll } : {}) };
  try {
    const files = await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: 'Save as Patch' },
      () => repo.backend.formatPatch(oids, options, out),
    );
    const open = await vscode.window.showInformationMessage(`Saved ${files.length} ${files.length === 1 ? 'patch' : 'patches'}`, 'Open');
    if (open && files[0]) {
      await vscode.commands.executeCommand('vscode.open', vscode.Uri.file(files[0]));
    }
  } catch (err) {
    showError(err);
  }
}

async function pickPatches(): Promise<vscode.Uri[] | undefined> {
  return vscode.window.showOpenDialog({
    canSelectMany: true,
    filters: { Patches: PATCH_EXTENSIONS, All: ['*'] },
    title: 'Apply Patch',
  });
}

/** パッチのファイルを読んで、何を変えるかと当たるかを見せてから当てる。名前の順につなぐ（0001-… から） */
async function applyPatches(repo: Repository, uris: readonly vscode.Uri[]): Promise<void> {
  const sorted = [...uris].sort((a, b) => a.fsPath.localeCompare(b.fsPath, undefined, { numeric: true }));
  const paths = sorted.map((u) => u.fsPath);
  let summary: PatchSummary;
  try {
    const texts = await Promise.all(sorted.map(async (u) => Buffer.from(await vscode.workspace.fs.readFile(u)).toString('utf8')));
    summary = parsePatch(texts.join('\n'));
  } catch (err) {
    showError(err);
    return;
  }
  if (summary.patches.every((p) => p.files.length === 0)) {
    void vscode.window.showWarningMessage('The patch has no changes');
    return;
  }
  const mailbox = summary.kind === 'mailbox';
  // am は index にも当てるので、index と作業ツリーの両方に当たるかを調べる
  const problem = await repo.backend.checkPatch(paths, mailbox);
  const branch = (await repo.backend.head()).branch?.replace(/^refs\/heads\//, '') ?? 'HEAD';
  const list = summary.patches
    .map((p) => {
      const files = p.files.map((f) => `  ${f.oldPath ? `${f.oldPath} → ` : ''}${f.path}  ${f.binary ? 'bin' : `+${f.added} −${f.removed}`}`);
      return [p.subject ? `${p.subject}${p.author ? `  (${p.author})` : ''}` : undefined, ...files].filter(Boolean).join('\n');
    })
    .join('\n');
  const status = problem ? `Doesn’t apply as it is: ${firstError(problem)}` : 'Applies cleanly';
  const title = mailbox
    ? summary.patches.length === 1
      ? `Apply “${summary.patches[0]!.subject}”?`
      : `Apply ${summary.patches.length} patches?`
    : `Apply ${paths.length === 1 ? basename(paths[0]!) : `${paths.length} patches`}?`;
  const how = mailbox
    ? `Each patch becomes a commit on ${branch} (git am). You can undo this.`
    : 'Applies the changes to the working tree (git apply). You can undo this.';
  // 既定のボタンは git の既定（am は am.threeWay に任せ、apply は作業ツリーだけ）
  const buttons = mailbox ? ['Apply', 'Apply with 3-Way Merge'] : ['Apply', 'Apply and Stage', 'Apply with 3-Way Merge'];
  const choice = await vscode.window.showInformationMessage(title, { modal: true, detail: `${how}\n\n${list}\n\n${status}` }, ...buttons);
  if (!choice) return;
  const threeWay = choice === 'Apply with 3-Way Merge';
  const label = mailbox
    ? `Apply ${summary.patches.length} ${summary.patches.length === 1 ? 'patch' : 'patches'}`
    : paths.length === 1
      ? `Apply ${basename(paths[0]!)}`
      : `Apply ${paths.length} patches`;
  let result: RebaseResult | undefined;
  try {
    result = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: label }, async () => {
      if (mailbox) {
        const head = (await repo.backend.head()).branch;
        // 止まったら、終わったとき（Continue・Skip）にまとめて 1 つ記録する。Abort なら残らない
        return repo.undo.record(label, head ? [head] : [], 'keep', () => repo.backend.am(paths, threeWay ? true : undefined), {
          pending: (r) => r !== 'done',
        });
      }
      return repo.undo.recordFiles(label, patchPaths(summary), () =>
        repo.backend.applyPatch(paths, { index: choice === 'Apply and Stage', threeWay }),
      );
    });
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
  if (result === 'done') {
    const undo = await vscode.window.showInformationMessage(`${label}: Done`, 'Undo');
    if (undo === 'Undo') await vscode.commands.executeCommand('amonhen.undo');
  } else if (result && mailbox) {
    void vscode.window.showWarningMessage(`${label}: Stopped at a patch. The sidebar says why and what to do next.`);
  } else if (result) {
    void vscode.window.showWarningMessage(`${label}: Applied with conflicts. Resolve and stage them.`);
  }
}

function firstError(text: string): string {
  const lines = text.split('\n').map((l) => l.trim());
  const error = lines.find((l) => l.startsWith('error:'));
  return error ? error.slice('error:'.length).trim() : (lines[0] ?? '');
}
