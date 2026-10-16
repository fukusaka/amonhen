/** ブランチ・タグの作成と、ブランチ名の変更 */
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import type { Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';

/** サイドバーのノード、または History の右クリックで渡される情報（data-vscode-context） */
type Target = Node | { oid: string } | undefined;

/** どこから作るか。ブランチ・タグならその ref、History のコミットならその oid、無ければ HEAD */
function startOf(target: Target): { rev: string; label: string } {
  if (target && 'kind' in target && (target.kind === 'branch' || target.kind === 'tag')) {
    return { rev: target.ref.name, label: shortName(target.ref.name) };
  }
  if (target && 'oid' in target) {
    return { rev: target.oid, label: target.oid.slice(0, 8) };
  }
  return { rev: 'HEAD', label: 'HEAD' };
}

export function registerBranchCommands(manager: RepositoryManager): vscode.Disposable {
  const command = (id: string, fn: (repo: Repository, target: Target) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (target?: Target) => {
      const repo = manager.current;
      if (repo) await fn(repo, target).catch(showError);
    });

  return vscode.Disposable.from(
    command('amonhen.newBranch', async (repo, target) => {
      const start = startOf(target);
      const name = await askName(repo, `New Branch from ${start.label}`, 'heads');
      if (!name) return;
      // Undo すると、元のブランチに戻ってからこのブランチを消す
      await repo.undo.record(`Create branch ${name}`, [`refs/heads/${name}`], 'keep', () =>
        repo.backend.createBranch(name, start.rev, true),
      );
      await repo.refresh();
    }),
    command('amonhen.newTag', async (repo, target) => {
      const start = startOf(target);
      const name = await askName(repo, `New Tag on ${start.label}`, 'tags');
      if (!name) return;
      const message = await vscode.window.showInputBox({
        title: `Tag ${name}`,
        prompt: 'メッセージ（空なら lightweight tag）',
      });
      if (message === undefined) return;
      await repo.undo.record(`Create tag ${name}`, [`refs/tags/${name}`], 'keep', () =>
        repo.backend.createTag(name, start.rev, message.trim() ? `${message.trim()}\n` : undefined),
      );
      await repo.refresh();
    }),
    command('amonhen.renameBranch', async (repo, target) => {
      if (!target || !('kind' in target) || target.kind !== 'branch' || target.remote) return;
      const from = shortName(target.ref.name);
      const to = await askName(repo, `Rename ${from}`, 'heads', from);
      if (!to || to === from) return;
      try {
        await repo.backend.renameBranch(from, to);
      } finally {
        await repo.refresh();
      }
    }),
  );
}

/** 名前を入れてもらう。使えない名前や、すでにある名前ならその場で知らせる */
async function askName(
  repo: Repository,
  title: string,
  kind: 'heads' | 'tags',
  value?: string,
): Promise<string | undefined> {
  const existing = new Set(
    (kind === 'heads' ? (repo.current.branches ?? []) : (repo.current.tags ?? [])).map((r) => shortName(r.name)),
  );
  const name = await vscode.window.showInputBox({
    title,
    value,
    placeHolder: kind === 'heads' ? 'feature/name' : 'v1.0.0',
    validateInput: async (v) => {
      const trimmed = v.trim();
      if (!trimmed) return undefined;
      if (trimmed !== value && existing.has(trimmed)) return 'すでにあります';
      return (await repo.backend.checkBranchName(trimmed)) ? '名前に使えない文字や形が含まれています' : undefined;
    },
  });
  return name?.trim() || undefined;
}
