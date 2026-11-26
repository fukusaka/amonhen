/** サイドバーのナビで、ブランチなどを右クリックしたときのコマンド */
import * as vscode from 'vscode';
import { GitError } from '../git/exec.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { fastForwardBranch, mergeInto, pushTarget, shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import { rebaseAction, runAction } from './dragDrop.ts';
import type { Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';
import type { MergeMode } from '../git/backend.ts';
import { mergeBranches } from './mergeBranches.ts';
import { pickMergeMode } from './mergeMode.ts';
import { publish } from './syncCommands.ts';
import { confirmInSheet, confirmOperation, uncommittedLine } from './confirm.ts';
import { moveDrawing, type Drawing, type MovePreview } from '../history/drawing.ts';
import { pushWithSheet } from './pushSheet.ts';
import { MainPanel } from './mainPanel.ts';

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

  /** 名前をコピー（ターミナルや PR に貼るため。リモートブランチは origin/ を付けたまま。いくつも選べば 1 行に 1 つ） */
  const copyNames = async (node?: Node, selected?: Node[]) => {
    const nodes = selected && selected.length > 1 ? selected : node ? [node] : [];
    const names = nodes.map(refOf).filter((r) => r !== undefined).map(shortName);
    if (names.length === 0) return;
    await vscode.env.clipboard.writeText(names.join('\n'));
    vscode.window.setStatusBarMessage(`$(copy) ${names.length === 1 ? names[0] : `${names.length} names`}`, 3000);
  };

  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.branch.copyName', copyNames),
    vscode.commands.registerCommand('amonhen.tag.copyName', copyNames),
    vscode.commands.registerCommand('amonhen.refs.copyNames', copyNames),
    command('amonhen.nav.checkout', (repo, ref) => checkoutRef(repo, ref)),
    command('amonhen.nav.checkoutAsLocal', (repo, ref) => checkoutRef(repo, ref)),
    several(
      'amonhen.nav.merge',
      async (repo, ref, head) => {
        if (!head) return;
        let mode: MergeMode | undefined;
        await runAction(repo, {
          label: `$(git-merge) Merge ${shortName(ref)} into ${shortName(head)}`,
          op: 'merge',
          // 取り込み方を選ぶことが確認になる
          confirm: async () =>
            (mode = await pickMergeMode(`Merge ${shortName(ref)} into ${shortName(head)}?`, true, { repo, theirs: ref, ours: head })) !==
            undefined,
          run: () => mergeInto(repo.backend, repo.undo, ref, head, mode),
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
      if (!(await confirmFastForward(repo, ref))) return;
      await runAction(repo, {
        label: `$(arrow-down) Fast-Forward ${shortName(ref)}`,
        op: 'fastForward',
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
    command('amonhen.nav.push', (repo, ref) => pushWithSheet(repo, { branch: ref }, publish)),
    command('amonhen.nav.publish', (repo, ref) => publish(repo, ref)),
  );
}

/** ローカルブランチなら切り替え、リモートブランチなら追跡するブランチを作って切り替える */
export async function checkoutRef(repo: Repository, ref: string): Promise<void> {
  // 何が起きるか（追跡するブランチを作る、作業中の変更は持って行く）を見せてから
  const local = ref.startsWith('refs/remotes/') ? (repo.current.branches ?? []).find((b) => b.upstream === ref) : undefined;
  const name = shortName(local?.name ?? ref);
  const creates = ref.startsWith('refs/remotes/') && !local ? `Creates ${name.slice(name.indexOf('/') + 1)} tracking ${name}.` : '';
  if (!(await confirmOperation(`Checkout ${name}?`, [creates, uncommittedLine(repo)], 'Checkout'))) return;
  try {
    if (ref.startsWith('refs/heads/')) {
      await repo.backend.switchBranch(shortName(ref));
    } else if (ref.startsWith('refs/remotes/')) {
      const local = (repo.current.branches ?? []).find((b) => b.upstream === ref);
      await (local ? repo.backend.switchBranch(shortName(local.name)) : repo.backend.trackBranch(shortName(ref)));
    }
  } catch (err) {
    showError(err, { op: 'checkout', branch: name });
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

/** 数えるコミットの上限（それより多ければ「100+」） */
const UNIQUE_LIMIT = 100;

/**
 * ブランチにしかないコミットの数（アプリと同じく、今のブランチと上流のうち、取り込まれていないものが少ない方と
 * 比べて）。git は、どちらかに取り込まれていれば `-d` で消せる。数えられなければ 0（消すときに git が断れば聞き直す）
 */
async function uniqueCount(repo: Repository, ref: string): Promise<number> {
  const branch = (repo.current.branches ?? []).find((b) => b.name === ref);
  const upstream = branch && !branch.gone ? branch.upstream : undefined;
  const only = (base: string) =>
    repo.backend
      .log([`${base}..${ref}`], 0, UNIQUE_LIMIT)
      .then((c) => c.length)
      .catch(() => 0);
  const fromHead = await only('HEAD');
  if (fromHead === 0 || !upstream) return fromHead;
  return Math.min(fromHead, await only(upstream));
}

/** 「3 commits」（上限に届いていれば「100+ commits」） */
function commitCount(n: number): string {
  return `${n >= UNIQUE_LIMIT ? `${UNIQUE_LIMIT}+` : n} ${n === 1 ? 'commit' : 'commits'}`;
}

/**
 * いくつものブランチを、アプリと同じく 1 度だけ確かめてから消す。どこにも取り込まれていないコミットのあるものは、
 * その数を並べて Delete Anyway で消す（1 本だけ断られて途中で止まることがない）
 */
async function deleteBranches(repo: Repository, refs: string[], mergedBy: MergedBy): Promise<void> {
  const counts = await Promise.all(refs.map((ref) => (mergedBy(repo, shortName(ref)) === undefined ? uniqueCount(repo, ref) : 0)));
  const names = refs.map((ref, i) => {
    const pr = mergedBy(repo, shortName(ref));
    const count = counts[i] ?? 0;
    if (pr !== undefined) return `${shortName(ref)} (merged by pull request #${pr})`;
    return count > 0 ? `${shortName(ref)} (${commitCount(count)} only on it)` : shortName(ref);
  });
  const anyway = counts.some((n) => n > 0);
  const note = anyway ? 'Some commits are on no other branch; they go with the branch.\n' : '';
  const ok = anyway ? 'Delete Anyway' : 'Delete';
  const choice = await vscode.window.showWarningMessage(
    `Delete ${names.length} branches?`,
    { modal: true, detail: `${names.join('\n')}\n\n${note}You can undo this, one branch at a time.` },
    ok,
  );
  if (choice !== ok) return;
  const remove = (ref: string, force: boolean) =>
    repo.undo.record(`Delete ${shortName(ref)}`, [ref], 'keep', () => repo.backend.deleteBranch(shortName(ref), force), {
      branches: [shortName(ref)],
    });
  const unmerged: string[] = [];
  try {
    for (const [i, ref] of refs.entries()) {
      try {
        await remove(ref, mergedBy(repo, shortName(ref)) !== undefined || (counts[i] ?? 0) > 0);
      } catch (err) {
        if (!notMerged(err)) throw err;
        unmerged.push(ref);
      }
    }
    // 数えたときとは違い git が断ったものだけ、もう一度確かめる
    if (unmerged.length === 0) return;
    const force = await vscode.window.showWarningMessage(
      `${unmerged.map(shortName).join(', ')} ${unmerged.length === 1 ? 'has' : 'have'} commits that aren’t merged. Delete anyway?`,
      { modal: true, detail: 'You can undo this.' },
      'Delete Anyway',
    );
    if (force === 'Delete Anyway') for (const ref of unmerged) await remove(ref, true);
  } catch (err) {
    showError(err, { op: 'deleteBranch' });
  } finally {
    await repo.refresh();
  }
}

/**
 * アプリと同じく、先に確かめる。メインビューが開いていれば前と後の絵を見せたシートで、でなければ modal で
 */
async function confirmFastForward(repo: Repository, ref: string): Promise<boolean> {
  const upstream = (repo.current.branches ?? []).find((b) => b.name === ref)?.upstream;
  if (!upstream) return true;
  const name = shortName(ref);
  return confirmOperation(`Fast-forward ${name} to ${shortName(upstream)}?`, ['You can undo this.'], 'Fast-Forward', {
    drawing: async () =>
      moveDrawing(await repo.backend.graphPreview<MovePreview>({ op: 'movePreview', from: ref, to: upstream, branch: name }), {
        moving: name,
        kind: 'local',
        afterTitle: 'After the fast-forward',
        lostNote: '',
      }),
  });
}

/**
 * 消すとたどれなくなるコミットの前と後の絵（今のブランチと上流のうち、取り込まれていないものが少ない方と比べて）。
 * そういうコミットがなければ undefined
 */
async function deleteDrawing(repo: Repository, ref: string): Promise<Drawing | undefined> {
  const name = shortName(ref);
  const upstream = (repo.current.branches ?? []).find((b) => b.name === ref)?.upstream;
  const previews = await Promise.all(
    ['HEAD', ...(upstream ? [upstream] : [])].map((against) =>
      repo.backend.graphPreview<MovePreview>({ op: 'deletePreview', branch: name, against }),
    ),
  );
  const fewest = previews
    .filter((p) => p !== undefined)
    .sort((a, b) => a.lostCount - b.lostCount)[0];
  if (!fewest || fewest.lostCount === 0) return undefined;
  return moveDrawing(fewest, {
    moving: name,
    kind: 'local',
    afterTitle: 'After deleting',
    lostNote: 'goes with the branch',
    tone: 'red',
    removes: true,
  });
}

async function deleteBranch(repo: Repository, ref: string, mergedBy: MergedBy): Promise<void> {
  const name = shortName(ref);
  const pr = mergedBy(repo, name);
  const title = `Delete the branch ${name}?`;
  const detail = `${pr === undefined ? '' : `Merged by pull request #${pr}. `}You can undo this.`;
  // シートに消えるコミットを描けば、アプリと同じくそこで Delete Anyway を聞き、git に断られてからもう一度は聞かない。
  // シートを出せなければ、ほかにないコミットを数えて modal で同じように聞く
  const drawing = MainPanel.active?.canAsk ? await deleteDrawing(repo, ref) : undefined;
  let anyway = drawing !== undefined && pr === undefined;
  const lines = anyway ? [`Some commits are on no other branch; they go with the branch. ${detail}`] : [detail];
  const asked = drawing && (await confirmInSheet(title, lines, anyway ? 'Delete Anyway' : 'Delete', { drawing: async () => drawing }));
  let ok = asked ?? false;
  if (asked === undefined) {
    const count = pr === undefined ? await uniqueCount(repo, ref) : 0;
    anyway = count > 0;
    const button = anyway ? 'Delete Anyway' : 'Delete';
    const note = anyway
      ? `${commitCount(count)} ${count === 1 ? 'is on no other branch; it goes' : 'are on no other branch; they go'} with the branch. `
      : '';
    ok = (await vscode.window.showWarningMessage(title, { modal: true, detail: note + detail }, button)) === button;
  }
  if (!ok) return;
  const remove = (force: boolean) =>
    repo.undo.record(`Delete ${name}`, [ref], 'keep', () => repo.backend.deleteBranch(name, force), { branches: [name] });
  try {
    await remove(pr !== undefined || anyway);
  } catch (err) {
    if (!notMerged(err)) {
      showError(err, { op: 'deleteBranch', branch: name });
      return;
    }
    const force = await vscode.window.showWarningMessage(
      `${name} has commits that aren’t merged. Delete anyway?`,
      { modal: true, detail: 'You can undo this.' },
      'Delete Anyway',
    );
    if (force === 'Delete Anyway') await remove(true).catch((err) => showError(err, { op: 'deleteBranch', branch: name }));
  } finally {
    await repo.refresh();
  }
}
