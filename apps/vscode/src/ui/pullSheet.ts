/**
 * Pull の前に、取り込むコミットと取り込み方を見せる（アプリの Pull のシート。メインビューが開いていればその上に重ねた
 * シートで、取り込み方ごとの前と後の絵つき。開いていなければ QuickPick で）。取り込み方の行を選ぶと
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
import { mergeDrawing, moveDrawing, rebaseDrawing, type MergePreview, type MovePreview } from '../history/drawing.ts';
import type { SheetList } from '../view/protocol.ts';
import { openCommitChanges } from './commitChanges.ts';
import { runAction } from './dragDrop.ts';
import { MainPanel } from './mainPanel.ts';
import { showError } from './stageCommands.ts';

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
  const title =
    incoming.length > 0
      ? `Pull ${count(incoming.length)} ${incoming.length === 1 ? 'commit' : 'commits'} from ${target} into ${head}?`
      : `Pull from ${target} into ${head}?`;
  // 最初は git の設定に従う。それで押せなければ Merge
  const firstMode = describePull('git', situation).blocked ? 1 : 0;
  const panel = MainPanel.active;
  const sheet = panel?.canAsk
    ? await askInSheet(panel, repo, { title, head, from, situation, incoming, outgoing, firstMode, labels: modeItems.map((i) => i.label) })
    : null;
  const chosen =
    sheet !== null
      ? sheet
      : await askInQuickPick(
          title,
          modeItems,
          firstMode,
          commitItems('Commits to pull', incoming),
          commitItems(`Commits only in ${head}`, outgoing),
          situation,
        );
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

interface SheetInput {
  title: string;
  head: string;
  /** 取り込むもの（`refs/remotes/...`） */
  from: string;
  situation: PullSituation;
  incoming: readonly Commit[];
  outgoing: readonly Commit[];
  firstMode: number;
  /** 取り込み方の見出し（QuickPick と同じ） */
  labels: string[];
}

/**
 * メインビューに重ねたシートで選ばせる（アプリの Pull のシート）。取り込み方ごとに前と後の絵: 手元に新しいコミットが
 * なければ先へ進めるだけ、あれば merge は merge コミット、rebase は載せ直し。押せない取り込み方では、左に今だけを
 * 描き、右に押せないわけを書く。どの取り込み方も描けなければ（取り込むものがない・エンジンがない）null
 */
async function askInSheet(panel: MainPanel, repo: Repository, i: SheetInput): Promise<PullMode | undefined | null> {
  const { situation: s } = i;
  const after = 'After the pull';
  const [move, merge, rebase] =
    i.incoming.length === 0
      ? []
      : i.outgoing.length === 0
        ? [await repo.backend.graphPreview<MovePreview>({ op: 'movePreview', from: 'HEAD', to: i.from, branch: i.head })]
        : await Promise.all([
            undefined,
            // merge コミットの件名を git pull と同じにするため、短い名前で（`Merge branch 'main' of <URL>`）
            repo.backend.graphPreview<MergePreview>({ op: 'mergePreview', theirs: shortName(i.from), ffSetting: 'pull.ff' }),
            repo.backend.rewritePreview({ onto: i.from }).catch(() => undefined),
          ]);
  const forward = moveDrawing(move, { moving: i.head, kind: 'local', afterTitle: after, lostNote: '' });
  const choices = PULL_MODES.map((mode, n) => {
    const { lines, blocked } = describePull(mode, s);
    const reasons = lines.filter((l) => l.startsWith('⚠'));
    const drawing = forward
      ? forward
      : blocked
        ? mergeDrawing(merge, i.head, after, reasons.map((l) => l.replace(/^⚠\s*/, '')).join(' ') || 'Can’t pull this way.')
        : methodOf(mode, s) === 'rebase'
          ? rebase && rebaseDrawing(rebase)
          : mergeDrawing(merge, i.head, after);
    return {
      label: i.labels[n]!.replace(/^\$\([^)]*\) /, ''),
      lines: drawing && blocked ? lines.filter((l) => !l.startsWith('⚠')) : lines,
      blocked,
      ...(drawing ? { drawing } : {}),
    };
  });
  if (!choices.some((c) => c.drawing)) return null;
  const opens: Commit[] = [];
  const list = (title: string, commits: readonly Commit[]): SheetList | undefined => {
    if (commits.length === 0) return undefined;
    const rest = commits.length - LISTED;
    return {
      title: `${title} (${commits.length >= 200 ? '200+' : commits.length})`,
      items: commits.slice(0, LISTED).map((c) => {
        opens.push(c);
        return { label: c.subject, description: `${c.oid.slice(0, 8)} · ${c.author}`, open: true };
      }),
      more: rest > 0 ? `… and ${rest >= 180 ? '180+' : rest} more` : undefined,
    };
  };
  const lists = [list('Commits to pull', i.incoming), list(`Commits only in ${i.head}`, i.outgoing)];
  const answer = await panel.ask(
    { title: i.title, lines: [], choices, choice: i.firstMode, lists: lists.filter((l) => l !== undefined), buttons: ['Pull'] },
    (item) => void (opens[item] && openCommitChanges(repo, opens[item]).catch(showError)),
  );
  return answer && PULL_MODES[answer.choice];
}

/** メインビューがなければ QuickPick で選ばせる */
async function askInQuickPick(
  title: string,
  modeItems: Item[],
  firstMode: number,
  incoming: Item[],
  outgoing: Item[],
  situation: PullSituation,
): Promise<PullMode | undefined> {
  const pick = vscode.window.createQuickPick<Item>();
  pick.title = title;
  pick.placeholder = 'Choose how to integrate (Enter pulls)';
  pick.matchOnDescription = false;
  pick.matchOnDetail = false;
  pick.items = [{ label: 'Integrate with', kind: vscode.QuickPickItemKind.Separator }, ...modeItems, ...incoming, ...outgoing];
  pick.activeItems = [modeItems[firstMode]!];
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
  return chosen;
}
