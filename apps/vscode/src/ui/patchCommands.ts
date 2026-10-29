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
      void vscode.window.showWarningMessage('このコミットは HEAD から first-parent で辿れません');
      return;
    }
    await savePatches(repo, oids);
  };
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.history.savePatch', save(false)),
    vscode.commands.registerCommand('amonhen.history.savePatches', save(true)),
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
          void vscode.window.showWarningMessage('コンフリクトを解決して stage してから、サイドバーの Continue で続けてください');
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
      { label: '1 コミット 1 ファイル', description: '0001-….patch（git format-patch の既定）', single: false, cover: false },
      { label: '1 コミット 1 ファイル（送り状つき）', description: '0000-cover-letter.patch も作る', single: false, cover: true },
      { label: '1 つの mbox ファイル', description: 'git format-patch --stdout', single: true, cover: false },
    ],
    { title: oids.length === 1 ? 'Save as Patch' : `Save ${oids.length} Commits as Patches` },
  );
  if (!choice) return;
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
        title: 'パッチを書き出すフォルダ（同じ名前のファイルは置き換えます）',
      })) ?? [];
    if (!uri) return;
    out.dir = uri.fsPath;
  }
  const options: FormatPatchOptions = { coverLetter: choice.cover };
  try {
    const files = await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: 'Save as Patch' },
      () => repo.backend.formatPatch(oids, options, out),
    );
    const open = await vscode.window.showInformationMessage(`${files.length} 個のパッチを保存しました`, 'Open');
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
    void vscode.window.showWarningMessage('パッチに変更がありません');
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
  const status = problem ? `そのままでは当たりません: ${firstError(problem)}` : 'そのまま当たります';
  const title = mailbox
    ? summary.patches.length === 1
      ? `「${summary.patches[0]!.subject}」を当てますか？`
      : `${summary.patches.length} 個のパッチを当てますか？`
    : `${paths.length === 1 ? basename(paths[0]!) : `${paths.length} 個のパッチ`}を当てますか？`;
  const how = mailbox
    ? `パッチごとに ${branch} のコミットになります（git am）。Undo で戻せます。`
    : '変更を作業ツリーに当てます（git apply）。Undo で戻せます。';
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
    const undo = await vscode.window.showInformationMessage(`${label}: 完了しました`, 'Undo');
    if (undo === 'Undo') await vscode.commands.executeCommand('amonhen.undo');
  } else if (result && mailbox) {
    void vscode.window.showWarningMessage(`${label}: パッチで止まりました。理由と次にできることはサイドバーに出ています`);
  } else if (result) {
    void vscode.window.showWarningMessage(`${label}: コンフリクトつきで当てました。解決して stage してください`);
  }
}

function firstError(text: string): string {
  const lines = text.split('\n').map((l) => l.trim());
  const error = lines.find((l) => l.startsWith('error:'));
  return error ? error.slice('error:'.length).trim() : (lines[0] ?? '');
}
