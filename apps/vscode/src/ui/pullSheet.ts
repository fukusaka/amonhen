/**
 * Pull の前に、取り込むコミットと取り込み方を見せる（アプリの Pull のシートを QuickPick で）。取り込み方の行を選ぶと
 * pull し、それぞれの行に今の状態で何が起きるかを書く。押せない取り込み方は ⚠ を付け、選んでも閉じない。その下に
 * 取り込むコミットと手元だけのコミット（最後の Fetch の時点）を並べる。上流がなければ、先に取り込み元のリモートの
 * ブランチを選び、それを上流にしてから取り込む
 */
import * as vscode from 'vscode';
import type { Commit } from '../git/log.ts';
import { pullWith, shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import {
  describePull,
  gitPullMode,
  methodOf,
  PULL_MODES,
  PULL_TITLES,
  PULL_TOOLTIPS,
  type PullMode,
  type PullSituation,
  pullSources,
} from '../sync/pullPlan.ts';
import { runAction } from './dragDrop.ts';

/** 並べるコミットの数（それより多ければ数だけ） */
const LISTED = 20;

interface Item extends vscode.QuickPickItem {
  mode?: PullMode;
}

export async function pullWithSheet(repo: Repository): Promise<void> {
  const head = repo.current.status?.branch.head;
  if (!head) {
    void vscode.window.showErrorMessage('Can’t pull into a detached HEAD. Switch to a branch first.');
    return;
  }
  const branch = `refs/heads/${head}`;
  const configured = (repo.current.branches ?? []).find((b) => b.name === branch);
  let upstream = configured?.upstream && !configured.gone ? configured.upstream : undefined;
  let source: string | undefined;
  if (!upstream) {
    const sources = pullSources(
      head,
      (repo.current.remoteBranches ?? []).filter((r) => !r.symref).map((r) => shortName(r.name)),
    );
    if (sources.length === 0) {
      void vscode.window.showInformationMessage('No remote branch to pull from. Fetch, or add a remote first.', {
        modal: true,
      });
      return;
    }
    source = await vscode.window.showQuickPick(sources, {
      title: `Pull into ${head}`,
      placeHolder: `${head} has no upstream: the remote branch to pull from (it becomes the upstream)`,
    });
    if (!source) return;
  }
  const from = upstream ?? `refs/remotes/${source}`;
  const keys = [`branch.${head}.rebase`, 'pull.rebase', 'pull.ff'];
  const [values, refuses, incoming, outgoing] = await Promise.all([
    Promise.all(keys.map((key) => repo.backend.config(key).catch(() => undefined))),
    repo.backend.pullLeftUndecided(head).catch(() => false),
    repo.backend.log([`${branch}..${from}`], 0, 200).catch(() => [] as Commit[]),
    repo.backend.log([`${from}..${branch}`], 0, 200).catch(() => [] as Commit[]),
  ]);
  const config = new Map(keys.flatMap((key, i) => (values[i] === undefined ? [] : [[key, values[i]!] as const])));
  const byGit = gitPullMode(head, config);
  const situation: PullSituation = {
    branch: head,
    ...(upstream ? { upstream: shortName(upstream) } : { source }),
    incoming: incoming.length,
    outgoing: outgoing.length,
    changed: (repo.current.status?.entries ?? []).filter((e) => e.kind !== 'untracked').length,
    byGit: byGit.mode,
    gitRefusesDiverged: refuses,
  };
  const count = (n: number) => (n >= 200 ? '200+' : String(n));
  const commitItems = (title: string, commits: readonly Commit[]): Item[] =>
    commits.length === 0
      ? []
      : [
          { label: `${title} (${count(commits.length)})`, kind: vscode.QuickPickItemKind.Separator },
          ...commits.slice(0, LISTED).map((c) => ({
            label: `$(git-commit) ${c.subject}`,
            description: `${c.oid.slice(0, 8)} · ${c.author}`,
          })),
          ...(commits.length > LISTED ? [{ label: `… and ${count(commits.length - LISTED)} more` }] : []),
        ];
  const modeItems: Item[] = PULL_MODES.map((mode) => {
    const { lines, blocked } = describePull(mode, situation);
    const title = mode === 'git' ? `${PULL_TITLES.git} (${byGit.unset ? 'not set' : PULL_TITLES[byGit.mode]})` : PULL_TITLES[mode];
    return { label: `${blocked ? '$(warning)' : '$(repo-pull)'} ${title}`, detail: lines.join(' '), description: PULL_TOOLTIPS[mode], mode };
  });
  const target = upstream ? shortName(upstream) : source!;
  const pick = vscode.window.createQuickPick<Item>();
  pick.title =
    incoming.length > 0
      ? `Pull ${count(incoming.length)} ${incoming.length === 1 ? 'commit' : 'commits'} from ${target} into ${head}?`
      : `Pull from ${target} into ${head}?`;
  pick.placeholder = 'Choose how to integrate (Enter pulls)';
  pick.matchOnDescription = false;
  pick.matchOnDetail = false;
  pick.items = [
    { label: 'Integrate with', kind: vscode.QuickPickItemKind.Separator },
    ...modeItems,
    ...commitItems('Commits to pull', incoming),
    ...commitItems(`Commits only in ${head}`, outgoing),
  ];
  // 最初は git の設定に従う。それで押せなければ Merge
  const first = describePull('git', situation).blocked ? modeItems[1]! : modeItems[0]!;
  pick.activeItems = [first];
  const chosen = await new Promise<PullMode | undefined>((resolve) => {
    pick.onDidAccept(() => {
      const mode = pick.selectedItems[0]?.mode;
      // コミットの行や押せない取り込み方は、選んでも閉じない
      if (!mode || describePull(mode, situation).blocked) return;
      resolve(mode);
      pick.hide();
    });
    pick.onDidHide(() => resolve(undefined));
    pick.show();
  });
  pick.dispose();
  if (!chosen) return;
  const how = methodOf(chosen, situation);
  await runAction(repo, {
    label: `$(repo-pull) Pull ${target} into ${head}`,
    run: async () => {
      if (!upstream) {
        upstream = from;
        await repo.backend.setUpstream(head, from);
      }
      return pullWith(repo.backend, repo.undo, branch, upstream, how, chosen === 'git' && byGit.unset && refuses);
    },
  });
}
