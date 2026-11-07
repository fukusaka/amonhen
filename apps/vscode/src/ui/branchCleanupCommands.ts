/**
 * ブランチを片付ける（アプリの Clean Up Branches…）。既定のブランチに取り込んだもの、GitHub のプルリクエストで
 * （squash・rebase で）取り込んだもの、上流が消えたものを理由ごとに並べ、選んだものをまとめて消す（Undo 1 回で戻る）。
 * 共有の daemon のコアで行う
 */
import * as vscode from 'vscode';
import { GhMissingError, type MergedPullRequest, mergedPullRequests, repositorySlug } from '../git/github.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { runEngine } from './engine.ts';
import { showError } from './stageCommands.ts';

interface Candidate {
  name: string;
  reason: 'merged' | 'pullRequest' | 'gone';
  force: boolean;
  checked: boolean;
  pullRequest: number | null;
  upstream: string | null;
}

type Item = vscode.QuickPickItem & { branch?: Candidate };

export function registerBranchCleanupCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.branches.cleanUp', async () => {
    const repo = manager.current;
    if (!repo) return;
    try {
      // squash・rebase で取り込んだものは、GitHub のマージされたプルリクエストで見分ける（gh がなければ出さない）
      let pullRequests: MergedPullRequest[] = [];
      let note: string | undefined;
      const slug = repositorySlug(await repo.backend.remotes());
      if (slug) {
        try {
          pullRequests = await mergedPullRequests(slug);
        } catch (err) {
          if (!(err instanceof GhMissingError)) note = `Couldn’t read the merged pull requests: ${(err as Error).message}`;
        }
      }
      const reply = await runEngine(repo, { op: 'branchCleanup', pullRequests }, 'Find branches to clean up');
      const info = (reply['info'] ?? {}) as { base?: string | null; branches?: Candidate[] };
      const base = info.base ?? 'main';
      const branches = info.branches ?? [];
      if (branches.length === 0) {
        void vscode.window.showInformationMessage(
          `No branch is merged into ${base}, merged by a pull request, or left without its upstream.${note ? ` ${note}` : ''}`,
        );
        return;
      }
      const groups: [Candidate['reason'], string][] = [
        ['merged', `Merged into ${base}`],
        ['pullRequest', 'Merged by a pull request (squash or rebase)'],
        ['gone', 'Upstream gone, not merged (their own commits go with them)'],
      ];
      const items: Item[] = [];
      for (const [reason, title] of groups) {
        const group = branches.filter((b) => b.reason === reason);
        if (group.length === 0) continue;
        items.push({ label: title, kind: vscode.QuickPickItemKind.Separator });
        for (const b of group) {
          items.push({
            label: b.name,
            description: b.pullRequest !== null ? `#${b.pullRequest}` : reason === 'gone' && b.upstream ? b.upstream : undefined,
            iconPath: new vscode.ThemeIcon('git-branch'),
            picked: b.checked,
            branch: b,
          });
        }
      }
      const picked = await vscode.window.showQuickPick(items, {
        title: 'Clean Up Branches',
        canPickMany: true,
        // 今のブランチ・お気に入り・worktree で使っているものは並ばない（README に書く。ここに書くと切れる）
        placeHolder: note ?? 'Delete the checked branches (Undo brings them back)',
      });
      const chosen = (picked ?? []).flatMap((i) => (i.branch ? [i.branch] : []));
      if (chosen.length === 0) return;
      await runEngine(
        repo,
        { op: 'deleteBranches', branches: chosen.map((b) => ({ name: b.name, force: b.force })) },
        chosen.length === 1 ? 'Delete 1 branch' : `Delete ${chosen.length} branches`,
      );
      vscode.window.setStatusBarMessage(`$(trash) Deleted ${chosen.length === 1 ? '1 branch' : `${chosen.length} branches`}`, 5000);
    } catch (err) {
      showError(err);
    } finally {
      await repo.refresh();
    }
  });
}
