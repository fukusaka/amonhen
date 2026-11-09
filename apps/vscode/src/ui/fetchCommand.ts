import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import { pruneLine } from '../sync/fetchPlan.ts';
import { showError } from './stageCommands.ts';

/**
 * Fetch の前に、どこから取るかを見せる（アプリの Fetch のシート）。リモートが 1 つなら確かめ、いくつもあればすべてか
 * 1 つを選ぶ。どれが片づける（prune）かは git の設定のとおりで、それを添える。`fetched` は fetch の後に呼ぶ（リモートの
 * タグの様子は、ref が変わらなくても変わる）
 */
async function chooseRemote(repo: Repository): Promise<{ remote?: string } | undefined> {
  const remotes = (repo.current.remotes ?? []).map((r) => r.name);
  if (remotes.length === 0) {
    void vscode.window.showErrorMessage('There’s no remote to fetch from. Add one first.');
    return undefined;
  }
  const keys = ['fetch.prune', ...remotes.map((r) => `remote.${r}.prune`)];
  const values = await Promise.all(keys.map((key) => repo.backend.config(key).catch(() => undefined)));
  const config = new Map(keys.flatMap((key, i) => (values[i] === undefined ? [] : [[key, values[i]!] as const])));
  if (remotes.length === 1) {
    const line = pruneLine(remotes, config);
    const choice = await vscode.window.showInformationMessage(
      `Fetch from ${remotes[0]}?`,
      { modal: true, ...(line ? { detail: line } : {}) },
      'Fetch',
    );
    return choice === 'Fetch' ? {} : undefined;
  }
  const items: (vscode.QuickPickItem & { remote?: string })[] = [
    { label: `All Remotes (${remotes.join(', ')})`, detail: pruneLine(remotes, config) || undefined },
    { label: '', kind: vscode.QuickPickItemKind.Separator },
    ...remotes.map((remote) => ({ label: remote, detail: pruneLine([remote], config) || undefined, remote })),
  ];
  const picked = await vscode.window.showQuickPick(items, { title: 'Fetch', placeHolder: 'Fetch from' });
  return picked ? { ...(picked.remote ? { remote: picked.remote } : {}) } : undefined;
}

export function registerFetchCommand(manager: RepositoryManager, fetched?: () => Promise<void>): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.fetch', async () => {
    const repo = manager.current;
    if (!repo) {
      return;
    }
    const chosen = await chooseRemote(repo);
    if (!chosen) return;
    try {
      await vscode.window.withProgress(
        { location: vscode.ProgressLocation.Window, title: chosen.remote ? `Fetch from ${chosen.remote}` : 'Fetch' },
        () => repo.backend.fetch(chosen.remote),
      );
    } catch (err) {
      showError(err);
    } finally {
      await repo.refresh();
      await fetched?.();
    }
  });
}
