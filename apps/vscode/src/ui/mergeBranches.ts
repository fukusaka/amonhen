/**
 * いくつものブランチ・タグを今のブランチに merge する（Merge Branches…）。2 つ以上なら 1 つのコミットにまとめる
 * （octopus。`git merge a b c`）。git は最後より前のものでコンフリクトすると断るので、前もって調べ、そうなりそうなら
 * それを最後に回すか、1 つずつ merge することを勧める。git が断ったときも 1 つずつを勧める
 */
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { mergeInto, mergeSeveral, namesLabel, shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import type { MergeMode } from '../git/backend.ts';
import { runAction } from './dragDrop.ts';
import { pickMergeMode } from './mergeMode.ts';
import { MainPanel } from './mainPanel.ts';
import { refusedMidSequence } from './sequenceCommands.ts';

interface RefItem extends vscode.QuickPickItem {
  ref: string;
}

interface Choice extends vscode.QuickPickItem {
  revs: string[];
  oneByOne: boolean;
  /** 選ぶと順を決め直す（アプリのシートでドラッグして並べ替えるのと同じ） */
  reorder?: boolean;
}

export function registerMergeBranchesCommand(manager: RepositoryManager): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.mergeBranches', async () => {
    const repo = manager.current;
    if (repo && !(await refusedMidSequence(repo))) {
      await mergeBranches(repo);
    }
  });
}

/** `chosen` があれば（サイドバーでいくつも選んだ）、選ぶところを飛ばしてそれを merge する */
export async function mergeBranches(repo: Repository, chosen?: readonly string[]): Promise<void> {
  const head = (await repo.backend.head()).branch;
  const target = head ? shortName(head) : 'HEAD';
  if (chosen && chosen.length > 0) {
    await mergeRevs(repo, [...chosen], head, target);
    return;
  }
  const tags = repo.current.tags ?? (await repo.backend.refs(['refs/tags']));
  const refs = [
    ...(repo.current.branches ?? []).filter((r) => r.name !== head),
    ...(repo.current.remoteBranches ?? []).filter((r) => !r.symref),
    ...tags,
  ];
  const icon = (ref: string) =>
    ref.startsWith('refs/tags/') ? '$(tag)' : ref.startsWith('refs/remotes/') ? '$(cloud)' : '$(git-branch)';
  // History で 1 つだけ選んでいるコミットも選べる（アプリの Merge… と同じ）
  const selected = MainPanel.active?.history.selectedCommit;
  const items: RefItem[] = [
    ...(selected ? [{ label: `$(git-commit) ${selected.slice(0, 8)}`, description: 'selected in History', ref: selected }] : []),
    ...refs.map((r) => ({ label: `${icon(r.name)} ${shortName(r.name)}`, detail: r.subject, ref: r.name })),
  ];
  const picked = await vscode.window.showQuickPick(items, {
    canPickMany: true,
    matchOnDetail: true,
    placeHolder: `Choose what to merge into ${target} (several make one merge commit)`,
  });
  if (!picked || picked.length === 0) {
    return;
  }
  await mergeRevs(repo, picked.map((p) => p.ref), head, target);
}

async function mergeRevs(repo: Repository, revs: string[], head: string | undefined, target: string): Promise<void> {
  if (revs.length === 1 && head) {
    const [ref = ''] = revs;
    let mode: MergeMode | undefined;
    await runAction(repo, {
      label: `$(git-merge) Merge ${shortName(ref)} into ${target}`,
      op: 'merge',
      confirm: async () =>
        (mode = await pickMergeMode(`Merge ${shortName(ref)} into ${target}?`, true, { repo, theirs: ref, ours: head })) !== undefined,
      run: () => mergeInto(repo.backend, repo.undo, ref, head, mode),
    });
    return;
  }

  // 順を変えるたびに調べ直す
  let choice: Choice | undefined;
  for (;;) {
    // 調べられなくても merge はできる（見込みなしで選ばせる）
    const order = revs;
    const steps = await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: 'Checking for conflicts…' },
      () => repo.backend.predictMerge(order.map(shortName)).catch(() => undefined),
    );
    choice = await vscode.window.showQuickPick(choices(revs, target, steps), {
      placeHolder: advice(revs, target, steps),
    });
    if (!choice?.reorder) break;
    const reordered = await pickOrder(revs);
    if (reordered) revs = reordered;
  }
  if (!choice) return;
  // いくつもなので Fast-Forward Only はない
  // 1 つのコミットにまとめるなら、その前と後の絵を添える
  const mode = await pickMergeMode(
    `${choice.oneByOne ? 'Merge one by one' : 'Merge'} into ${target}?`,
    false,
    head && !choice.oneByOne ? { repo, theirs: choice.revs, ours: head } : undefined,
  );
  if (mode) {
    await run(repo, choice.revs, choice.oneByOne, target, mode);
  }
}

/** 予測から、選べる操作。git が止まりそうなら、勧めるものを先に */
function choices(revs: string[], target: string, steps: string[][] | undefined): Choice[] {
  const names = revs.map(shortName);
  const conflicting = conflictingIndexes(steps);
  const early = conflicting.filter((i) => i < revs.length - 1);
  const summary = steps
    ? names.map((n, i) => (steps[i]?.length ? `${n} ⚠ ${steps[i]?.join(', ')}` : `${n} ✓`)).join(' · ')
    : 'Can’t check for conflicts ahead (needs git 2.38 or newer).';
  const merge: Choice = {
    label: `$(git-merge) Merge ${namesLabel(names)} into ${target}`,
    detail: summary,
    revs,
    oneByOne: false,
  };
  const oneByOne: Choice = {
    label: '$(list-ordered) Merge One by One',
    detail: 'Each makes its own merge commit, and you resolve conflicts as they come. One Undo puts them all back.',
    revs,
    oneByOne: true,
  };
  const reorder: Choice = {
    label: '$(arrow-swap) Change the Order…',
    detail: `Now ${names.join(', ')}. git merges them in this order, and the merge commit lists them as parents in it.`,
    revs,
    oneByOne: false,
    reorder: true,
  };
  if (early.length === 0) {
    return [merge, oneByOne, reorder];
  }
  const [only] = early;
  if (conflicting.length === 1 && only !== undefined) {
    const order = [...revs.slice(0, only), ...revs.slice(only + 1), revs[only] ?? ''];
    const putLast: Choice = {
      label: `$(arrow-down) Put ${names[only]} Last and Merge`,
      detail: `In the order ${order.map(shortName).join(', ')} (conflicts in the last one can be resolved in this merge)`,
      revs: order,
      oneByOne: false,
    };
    return [putLast, oneByOne, merge, reorder];
  }
  return [oneByOne, merge, reorder];
}

/** 先に merge するものから 1 つずつ選んで、順を決める（最後の 1 つは残ったもの）。やめれば undefined */
async function pickOrder(revs: readonly string[]): Promise<string[] | undefined> {
  const left = [...revs];
  const order: string[] = [];
  while (left.length > 1) {
    const picked = await vscode.window.showQuickPick(
      left.map((rev) => ({ label: shortName(rev), rev })),
      {
        title: `Change the Order (${order.length + 1} of ${revs.length})`,
        placeHolder: order.length === 0 ? 'Choose what to merge first' : `After ${order.map(shortName).join(', ')}, choose what comes next`,
      },
    );
    if (!picked) return undefined;
    order.push(picked.rev);
    left.splice(left.indexOf(picked.rev), 1);
  }
  return [...order, ...left];
}

/** QuickPick の上に出す一言 */
function advice(revs: string[], target: string, steps: string[][] | undefined): string {
  const names = revs.map(shortName);
  const conflicting = conflictingIndexes(steps);
  const early = conflicting.filter((i) => i < revs.length - 1);
  if (early.length === 0) {
    return `Merge ${revs.length} into ${target} in one commit`;
  }
  // 長いと切れるので短く。理由と手は、勧める項目の説明に書く
  const which = early.map((i) => names[i]).join(', ');
  return `${which} conflict${early.length === 1 ? 's' : ''}, so git stops in this order`;
}

function conflictingIndexes(steps: string[][] | undefined): number[] {
  return (steps ?? []).flatMap((files, i) => (files.length > 0 ? [i] : []));
}

/** merge する。1 つのコミットにまとめるのを git が断ったら（何も変わっていない）、1 つずつ merge するかを尋ねる */
async function run(repo: Repository, revs: string[], oneByOne: boolean, target: string, mode: MergeMode): Promise<void> {
  const names = revs.map(shortName);
  let gaveUp = false;
  let remaining: string[] = [];
  await runAction(repo, {
    label: oneByOne
      ? `$(list-ordered) Merge ${namesLabel(names)} into ${target} one by one`
      : `$(git-merge) Merge ${namesLabel(names)} into ${target}`,
    op: 'merge',
    run: async () => {
      try {
        const result = await mergeSeveral(repo.backend, repo.undo, revs, oneByOne, mode);
        remaining = result.remaining;
        return result.result;
      } catch (err) {
        if (!oneByOne && octopusGaveUp(err)) {
          gaveUp = true;
          return undefined;
        }
        throw err;
      }
    },
    conflictNote: () =>
      remaining.length > 0
        ? `Not merged yet: ${remaining.join(', ')}. Merge them again after you Continue.`
        : undefined,
  });
  if (gaveUp) {
    const answer = await vscode.window.showWarningMessage(
      'Can’t make them one commit',
      {
        modal: true,
        detail:
          'git stopped because one of them conflicts before the last, and nothing was changed. Merge them one by one instead? Each makes its own merge commit, and you resolve conflicts as they come. Undo puts them all back.',
      },
      'Merge One by One',
    );
    if (answer === 'Merge One by One') {
      await run(repo, revs, true, target, mode);
    }
  }
}

/** octopus が、最後より前のもののコンフリクトで断った（作業中の変更で断ったときは除く） */
function octopusGaveUp(err: unknown): boolean {
  const message = err instanceof Error ? err.message : String(err);
  return /Merge with strategy octopus failed/.test(message) && !/not uptodate|would be overwritten/.test(message);
}
