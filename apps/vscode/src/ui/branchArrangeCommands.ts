/**
 * ブランチのお気に入りとアーカイブ（アプリのサイドバーと同じ）。どちらも git の設定（`branch.<name>.amonhenPinned`・
 * `branch.<name>.amonhenArchived`）に書くので、アプリと共有し、ブランチを消せば一緒に消え、名前を変えればついていく。
 * 共有の daemon のコアの `configSet` で書く（エンジンがなければメニューに出さない）
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import { pinnedOrder } from '../git/branchGroups.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import type { Node, SidebarProvider } from './sidebar.ts';
import { showError } from './stageCommands.ts';

/** 右クリックしたブランチ（いくつも選んでいれば、選んだローカルブランチすべて） */
function branchesOf(node: Node | undefined, selected: readonly Node[] | undefined): string[] {
  const nodes = selected && selected.length > 0 ? selected : node ? [node] : [];
  return nodes.filter((n) => n.kind === 'branch' && !n.remote).map((n) => shortName((n as { ref: { name: string } }).ref.name));
}

export function registerBranchArrangeCommands(manager: RepositoryManager, sidebar: SidebarProvider): vscode.Disposable {
  /** git の設定をまとめて書く（`null` は消す） */
  const write = async (repo: Repository, values: [key: string, value: string | null][]): Promise<void> => {
    const client = await daemonFor(repo.root);
    if (!client) throw new Error('This needs the Amon Hen engine (amonhen.daemon.enabled)');
    for (const [key, value] of values) {
      const reply = (await client.call({ op: 'run', command: { op: 'configSet', key, value } })).json;
      if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `Can’t set ${key}`));
    }
  };
  /** 今のお気に入りの順（上から） */
  const favorites = (): string[] =>
    [...(sidebar.branchFacts.info?.branches ?? [])]
      .filter((b) => b.pinned !== null)
      .sort((a, b) => a.pinned! - b.pinned! || a.name.localeCompare(b.name))
      .map((b) => b.name);
  /** お気に入りの順を書く（1 から順に）。外したものは消す */
  const savePinned = (repo: Repository, order: string[], removed: string[] = []) =>
    write(repo, [
      ...order.map((name, i): [string, string] => [`branch.${name}.amonhenPinned`, String(i + 1)]),
      ...removed.map((name): [string, null] => [`branch.${name}.amonhenPinned`, null]),
    ]);
  const command = (id: string, fn: (repo: Repository, names: string[]) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (node?: Node, selected?: Node[]) => {
      const repo = manager.current;
      const names = branchesOf(node, selected);
      if (!repo || names.length === 0) return;
      try {
        await fn(repo, names);
      } catch (err) {
        showError(err);
      } finally {
        await sidebar.branchFacts.reload();
      }
    });
  const move = (delta: number) => async (repo: Repository, names: string[]) => {
    const order = favorites();
    const i = order.indexOf(names[0]!);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= order.length) return;
    [order[i], order[j]] = [order[j]!, order[i]!];
    await savePinned(repo, order);
  };

  return vscode.Disposable.from(
    command('amonhen.branch.addToFavorites', (repo, names) => savePinned(repo, pinnedOrder(favorites(), names, true))),
    command('amonhen.branch.removeFromFavorites', (repo, names) =>
      savePinned(repo, pinnedOrder(favorites(), names, false), names),
    ),
    command('amonhen.branch.favoriteUp', move(-1)),
    command('amonhen.branch.favoriteDown', move(+1)),
    command('amonhen.branch.archive', (repo, names) =>
      write(repo, names.map((name): [string, string] => [`branch.${name}.amonhenArchived`, 'true'])),
    ),
    command('amonhen.branch.unarchive', (repo, names) =>
      write(repo, names.map((name): [string, null] => [`branch.${name}.amonhenArchived`, null])),
    ),
  );
}
