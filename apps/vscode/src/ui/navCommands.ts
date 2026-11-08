/** サイドバーのナビで、ブランチなどを右クリックしたときのコマンド */
import * as vscode from 'vscode';
import { GitError } from '../git/exec.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { fastForwardBranch, mergeInto, pushTarget, shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import { rebaseAction, runAction } from './dragDrop.ts';
import type { Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';
import { mergeBranches } from './mergeBranches.ts';
import { publish } from './syncCommands.ts';

/** ブランチ・リモートブランチ・タグのノードから ref を取り出す */
function refOf(node: Node | undefined): string | undefined {
  return node?.kind === 'branch' || node?.kind === 'tag' ? node.ref.name : undefined;
}

/** `mergedBy` はブランチをマージしたプルリクエストの番号（squash・rebase。消すときの確認に添え、`-D` で消す） */
export function registerNavCommands(manager: RepositoryManager, mergedBy: MergedBy): vscode.Disposable {
  const command = (id: string, fn: (repo: Repository, ref: string, head: string | undefined) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (node?: Node) => {
      const repo = manager.current;
      const ref = refOf(node);
      if (repo && ref) {
        await fn(repo, ref, (await repo.backend.head()).branch);
      }
    });
  /** いくつも選んでいれば、選んだものすべて（右クリックした行を含む） */
  const several = (
    id: string,
    one: (repo: Repository, ref: string, head: string | undefined) => Promise<void>,
    many: (repo: Repository, refs: string[]) => Promise<void>,
  ) =>
    vscode.commands.registerCommand(id, async (node?: Node, selected?: Node[]) => {
      const repo = manager.current;
      const nodes = selected && selected.length > 1 ? selected : node ? [node] : [];
      const refs = nodes.map(refOf).filter((r) => r !== undefined);
      if (!repo || refs.length === 0) return;
      if (refs.length > 1) await many(repo, refs);
      else await one(repo, refs[0]!, (await repo.backend.head()).branch);
    });

  return vscode.Disposable.from(
    command('amonhen.nav.checkout', (repo, ref) => checkoutRef(repo, ref)),
    several(
      'amonhen.nav.merge',
      async (repo, ref, head) => {
        if (!head) return;
        await runAction(repo, {
          label: `$(git-merge) Merge ${shortName(ref)} into ${shortName(head)}`,
          run: () => mergeInto(repo.backend, repo.undo, ref, head),
        });
      },
      // いくつも選んだら、Merge Branches… と同じく 1 つのコミットで（コンフリクトの見込みを見せてから）
      (repo, refs) => mergeBranches(repo, refs),
    ),
    command('amonhen.nav.rebase', async (repo, ref, head) => {
      if (!head) return;
      await runAction(repo, rebaseAction(repo, head, ref));
    }),
    several('amonhen.nav.delete', (repo, ref) => deleteBranch(repo, ref, mergedBy), (repo, refs) => deleteBranches(repo, refs, mergedBy)),
    // チェックアウトしていないブランチを upstream と合わせる（最後の Fetch の時点）
    command('amonhen.nav.fastForward', async (repo, ref) => {
      await runAction(repo, {
        label: `$(arrow-down) Fast-Forward ${shortName(ref)}`,
        run: async () => {
          const result = await fastForwardBranch(repo.backend, repo.undo, ref);
          if (result === 'no-upstream') throw new Error(`${shortName(ref)} has no upstream`);
          if (result === 'checked-out') throw new Error(`${shortName(ref)} is checked out. Pull it instead.`);
          if (result === 'diverged')
            throw new Error(`${shortName(ref)} has diverged from its upstream. Check it out, then merge or rebase.`);
          return result;
        },
      });
    }),
    command('amonhen.nav.push', async (repo, ref) => {
      await runAction(repo, {
        label: `$(repo-push) Push ${shortName(ref)}`,
        run: async () => {
          const target = await pushTarget(repo.backend, ref);
          if (!target) throw new Error(`${shortName(ref)} has no upstream. Publish it.`);
          await repo.backend.push(target.remote, shortName(ref), target.branch);
          return 'pushed';
        },
      });
    }),
    command('amonhen.nav.publish', (repo, ref) => publish(repo, ref)),
  );
}

/** ローカルブランチなら切り替え、リモートブランチなら追跡するブランチを作って切り替える */
export async function checkoutRef(repo: Repository, ref: string): Promise<void> {
  try {
    if (ref.startsWith('refs/heads/')) {
      await repo.backend.switchBranch(shortName(ref));
    } else if (ref.startsWith('refs/remotes/')) {
      const local = (repo.current.branches ?? []).find((b) => b.upstream === ref);
      await (local ? repo.backend.switchBranch(shortName(local.name)) : repo.backend.trackBranch(shortName(ref)));
    }
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}

type MergedBy = (repo: Repository, name: string) => number | undefined;

/** ブランチを削除する。Undo で戻せる。merge されていなければ、もう一度確かめてから消す */
/** 取り込んでいないので git が消すのを断った（git 実行層なら GitError、daemon ならコアのエラーの文に git の文が入る） */
function notMerged(err: unknown): boolean {
  const text = err instanceof GitError ? err.stderr : err instanceof Error ? err.message : '';
  return /not fully merged/.test(text);
}

/** いくつものブランチを、まとめて確かめてから消す。取り込んでいないものは、もう一度確かめて消す */
async function deleteBranches(repo: Repository, refs: string[], mergedBy: MergedBy): Promise<void> {
  const names = refs.map((ref) => {
    const pr = mergedBy(repo, shortName(ref));
    return pr === undefined ? shortName(ref) : `${shortName(ref)} (merged by pull request #${pr})`;
  });
  const choice = await vscode.window.showWarningMessage(
    `Delete ${names.length} branches?`,
    { modal: true, detail: `${names.join('\n')}\n\nYou can undo this, one branch at a time.` },
    'Delete',
  );
  if (choice !== 'Delete') return;
  const remove = (ref: string, force: boolean) =>
    repo.undo.record(`Delete ${shortName(ref)}`, [ref], 'keep', () => repo.backend.deleteBranch(shortName(ref), force), {
      branches: [shortName(ref)],
    });
  const unmerged: string[] = [];
  try {
    for (const ref of refs) {
      try {
        await remove(ref, mergedBy(repo, shortName(ref)) !== undefined);
      } catch (err) {
        if (!notMerged(err)) throw err;
        unmerged.push(ref);
      }
    }
    if (unmerged.length === 0) return;
    const force = await vscode.window.showWarningMessage(
      `${unmerged.map(shortName).join(', ')} ${unmerged.length === 1 ? 'has' : 'have'} commits that aren’t merged. Delete anyway?`,
      { modal: true, detail: 'You can undo this.' },
      'Delete Anyway',
    );
    if (force === 'Delete Anyway') for (const ref of unmerged) await remove(ref, true);
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}

async function deleteBranch(repo: Repository, ref: string, mergedBy: MergedBy): Promise<void> {
  const name = shortName(ref);
  const pr = mergedBy(repo, name);
  const choice = await vscode.window.showWarningMessage(
    `Delete the branch ${name}?`,
    { modal: true, detail: `${pr === undefined ? '' : `Merged by pull request #${pr}. `}You can undo this.` },
    'Delete',
  );
  if (choice !== 'Delete') return;
  const remove = (force: boolean) =>
    repo.undo.record(`Delete ${name}`, [ref], 'keep', () => repo.backend.deleteBranch(name, force), { branches: [name] });
  try {
    await remove(pr !== undefined);
  } catch (err) {
    if (!notMerged(err)) {
      showError(err);
      return;
    }
    const force = await vscode.window.showWarningMessage(
      `${name} has commits that aren’t merged. Delete anyway?`,
      { modal: true, detail: 'You can undo this.' },
      'Delete Anyway',
    );
    if (force === 'Delete Anyway') await remove(true).catch(showError);
  } finally {
    await repo.refresh();
  }
}
