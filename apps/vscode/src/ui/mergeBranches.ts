/**
 * いくつものブランチ・タグを今のブランチに merge する（Merge Branches…）。2 つ以上なら 1 つのコミットにまとめる
 * （octopus。`git merge a b c`）。git は最後より前のものでコンフリクトすると断るので、前もって調べ、そうなりそうなら
 * それを最後に回すか、1 つずつ merge することを勧める。git が断ったときも 1 つずつを勧める
 */
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { mergeInto, mergeSeveral, namesLabel, shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import { runAction } from './dragDrop.ts';

interface RefItem extends vscode.QuickPickItem {
  ref: string;
}

interface Choice extends vscode.QuickPickItem {
  revs: string[];
  oneByOne: boolean;
}

export function registerMergeBranchesCommand(manager: RepositoryManager): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.mergeBranches', async () => {
    const repo = manager.current;
    if (repo) {
      await mergeBranches(repo);
    }
  });
}

async function mergeBranches(repo: Repository): Promise<void> {
  const head = (await repo.backend.head()).branch;
  const target = head ? shortName(head) : 'HEAD';
  const tags = repo.current.tags ?? (await repo.backend.refs(['refs/tags']));
  const refs = [
    ...(repo.current.branches ?? []).filter((r) => r.name !== head),
    ...(repo.current.remoteBranches ?? []).filter((r) => !r.symref),
    ...tags,
  ];
  const icon = (ref: string) =>
    ref.startsWith('refs/tags/') ? '$(tag)' : ref.startsWith('refs/remotes/') ? '$(cloud)' : '$(git-branch)';
  const items: RefItem[] = refs.map((r) => ({ label: `${icon(r.name)} ${shortName(r.name)}`, detail: r.subject, ref: r.name }));
  const picked = await vscode.window.showQuickPick(items, {
    canPickMany: true,
    matchOnDetail: true,
    placeHolder: `${target} に merge するものを選ぶ（いくつも選ぶと 1 つのコミットにまとめる）`,
  });
  if (!picked || picked.length === 0) {
    return;
  }
  const revs = picked.map((p) => p.ref);
  if (revs.length === 1 && head) {
    const [ref = ''] = revs;
    await runAction(repo, {
      label: `$(git-merge) Merge ${shortName(ref)} into ${target}`,
      run: () => mergeInto(repo.backend, repo.undo, ref, head),
    });
    return;
  }

  // 調べられなくても merge はできる（見込みなしで選ばせる）
  const steps = await vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, title: 'コンフリクトするかを調べています' },
    () => repo.backend.predictMerge(revs.map(shortName)).catch(() => undefined),
  );
  const choice = await vscode.window.showQuickPick(choices(revs, target, steps), {
    placeHolder: advice(revs, target, steps),
  });
  if (choice) {
    await run(repo, choice.revs, choice.oneByOne, target);
  }
}

/** 予測から、選べる操作。git が止まりそうなら、勧めるものを先に */
function choices(revs: string[], target: string, steps: string[][] | undefined): Choice[] {
  const names = revs.map(shortName);
  const conflicting = conflictingIndexes(steps);
  const early = conflicting.filter((i) => i < revs.length - 1);
  const summary = steps
    ? names.map((n, i) => (steps[i]?.length ? `${n} ⚠ ${steps[i]?.join(', ')}` : `${n} ✓`)).join(' · ')
    : 'コンフリクトの見込みは調べられません（git 2.38 以降が要ります）';
  const merge: Choice = {
    label: `$(git-merge) Merge ${namesLabel(names)} into ${target}`,
    detail: summary,
    revs,
    oneByOne: false,
  };
  const oneByOne: Choice = {
    label: '$(list-ordered) Merge One by One',
    detail: 'それぞれを merge のコミットにする。コンフリクトはその都度解く。Undo 1 回で全部戻る',
    revs,
    oneByOne: true,
  };
  if (early.length === 0) {
    return [merge, oneByOne];
  }
  const [only] = early;
  if (conflicting.length === 1 && only !== undefined) {
    const order = [...revs.slice(0, only), ...revs.slice(only + 1), revs[only] ?? ''];
    const putLast: Choice = {
      label: `$(arrow-down) Put ${names[only]} Last and Merge`,
      detail: `${order.map(shortName).join(', ')} の順に。この merge でコンフリクトを解ける`,
      revs: order,
      oneByOne: false,
    };
    return [putLast, oneByOne, merge];
  }
  return [oneByOne, merge];
}

/** QuickPick の上に出す一言 */
function advice(revs: string[], target: string, steps: string[][] | undefined): string {
  const names = revs.map(shortName);
  const conflicting = conflictingIndexes(steps);
  const early = conflicting.filter((i) => i < revs.length - 1);
  if (early.length === 0) {
    return `${revs.length} 個を ${target} に 1 つのコミットで merge する`;
  }
  const which = early.map((i) => names[i]).join('、');
  return conflicting.length === 1
    ? `${which} がコンフリクトします。最後より前のものがコンフリクトすると git は止まるので、最後に回すか、1 つずつ merge してください`
    : `${which} がコンフリクトします。最後より前のものがコンフリクトすると git は止まるので、1 つずつ merge してください`;
}

function conflictingIndexes(steps: string[][] | undefined): number[] {
  return (steps ?? []).flatMap((files, i) => (files.length > 0 ? [i] : []));
}

/** merge する。1 つのコミットにまとめるのを git が断ったら（何も変わっていない）、1 つずつ merge するかを尋ねる */
async function run(repo: Repository, revs: string[], oneByOne: boolean, target: string): Promise<void> {
  const names = revs.map(shortName);
  let gaveUp = false;
  let remaining: string[] = [];
  await runAction(repo, {
    label: oneByOne
      ? `$(list-ordered) Merge ${namesLabel(names)} into ${target} one by one`
      : `$(git-merge) Merge ${namesLabel(names)} into ${target}`,
    run: async () => {
      try {
        const result = await mergeSeveral(repo.backend, repo.undo, revs, oneByOne);
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
  });
  if (remaining.length > 0) {
    void vscode.window.showInformationMessage(
      `まだ merge していないもの: ${remaining.join('、')}。コンフリクトを解いて Continue してから、改めて merge してください`,
    );
  }
  if (gaveUp) {
    const answer = await vscode.window.showWarningMessage(
      '1 つのコミットにまとめられません',
      {
        modal: true,
        detail:
          '最後より前のものがコンフリクトしたので git は止まりました。何も変わっていません。代わりに 1 つずつ merge しますか？ それぞれが merge のコミットになり、コンフリクトはその都度解きます。Undo ですべて戻せます。',
      },
      'Merge One by One',
    );
    if (answer === 'Merge One by One') {
      await run(repo, revs, true, target);
    }
  }
}

/** octopus が、最後より前のもののコンフリクトで断った（作業中の変更で断ったときは除く） */
function octopusGaveUp(err: unknown): boolean {
  const message = err instanceof Error ? err.message : String(err);
  return /Merge with strategy octopus failed/.test(message) && !/not uptodate|would be overwritten/.test(message);
}
