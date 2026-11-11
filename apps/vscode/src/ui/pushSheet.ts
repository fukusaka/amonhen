/**
 * Push の前に、送るコミットを見せる（アプリの Push のシート。メインビューが開いていればその上に重ねたシートで、送った
 * 後の形の絵つき。開いていなければ QuickPick で）。どこへ送るか、見ておくとよいこと、送る
 * コミットを並べ、Push の行で送る。上流が先に進んでいれば、同じ一覧で消えるコミットを見せ、Pull… か Force Push を
 * 選ばせる。Force Push は、ここで見せた上流のコミットのときだけ置き換える（`--force-with-lease=<ref>:<commit>`）。
 * 送る変更すべての行やコミットの行を選ぶと、その差分を標準の複数ファイル diff エディタで開く（シートは閉じない）
 */
import * as vscode from 'vscode';
import type { Commit } from '../git/log.ts';
import { moveDrawing, type MovePreview } from '../history/drawing.ts';
import { pushTarget, shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import { pushWarnings } from '../sync/pushPlan.ts';
import { openCommitChanges, openRangeChanges } from './commitChanges.ts';
import type { SheetList } from '../view/protocol.ts';
import { runAction } from './dragDrop.ts';
import { MainPanel } from './mainPanel.ts';
import { pullWithSheet } from './pullSheet.ts';
import { showError } from './stageCommands.ts';

/** 並べるコミットの数（それより多ければ数だけ） */
const LISTED = 20;
const LIMIT = 500;

type Choice = 'push' | 'force' | 'pull';

interface Item extends vscode.QuickPickItem {
  choice?: Choice;
  /** 選ぶと差分を開く */
  open?: () => Promise<void>;
}

export interface PushRequest {
  /** 送るローカルブランチ（`refs/heads/...`）。なければ今のブランチ */
  branch?: string;
  force?: boolean;
  /** 上流が先に進んでいなければ、確かめずに送る（Commit and Push） */
  quiet?: boolean;
}

/** 上流のないブランチを初めて送る（syncCommands の publish） */
type Publish = (repo: Repository, branch: string) => Promise<void>;

export async function pushWithSheet(repo: Repository, request: PushRequest, publish: Publish): Promise<void> {
  const head = repo.current.status?.branch.head;
  const branch = request.branch ?? (head ? `refs/heads/${head}` : undefined);
  if (!branch) {
    void vscode.window.showErrorMessage('Can’t push a detached HEAD. Switch to a branch first.');
    return;
  }
  const name = shortName(branch);
  const target = await pushTarget(repo.backend, branch);
  if (!target) {
    if (!request.force) await publish(repo, branch);
    else void vscode.window.showErrorMessage(`${name} has no upstream. Publish it first.`);
    return;
  }
  const upstreamRef = `refs/remotes/${target.remote}/${target.branch}`;
  const upstream = `${target.remote}/${target.branch}`;
  const [values, outgoing, incoming] = await Promise.all([
    repo.backend.resolveRefs([upstreamRef]),
    repo.backend.log([`${upstreamRef}..${branch}`], 0, LIMIT),
    repo.backend.log([`${branch}..${upstreamRef}`], 0, LIMIT),
  ]);
  const expect = values.get(upstreamRef);
  const diverged = !request.force && incoming.length > 0;
  if (!request.force && outgoing.length === 0) {
    void vscode.window.showInformationMessage(
      incoming.length > 0
        ? `Nothing to push: ${upstream} has ${incoming.length} ${incoming.length === 1 ? 'commit' : 'commits'} ${name} doesn’t have. Pull them.`
        : `Nothing to push: ${name} is up to date with ${upstream} (as of the last fetch).`,
    );
    return;
  }
  const send = () =>
    runAction(repo, {
      label: `$(repo-push) ${request.force || diverged ? 'Force Push' : 'Push'} ${name} to ${upstream}`,
      run: async () => {
        await repo.backend.push(target.remote, name, target.branch, {
          force: request.force || diverged,
          ...(expect ? { expect } : {}),
        });
        return 'pushed';
      },
    });
  if (request.quiet && !diverged && !request.force) {
    await send();
    return;
  }
  // 上流が先に進んでいれば、送る変更の起点は分かれたところ（いちばん古い送るコミットの親）
  const oldest = outgoing.at(-1);
  const base = incoming.length > 0 && oldest && outgoing.length < LIMIT ? `${oldest.oid}^` : upstreamRef;
  const added = outgoing.length > 0 ? await repo.backend.addedFiles(base, branch).catch(() => []) : [];
  const warnings = pushWarnings(
    upstream,
    outgoing.map((c) => c.subject),
    added,
  );
  const count = (n: number) => (n >= LIMIT ? `${LIMIT}+` : String(n));
  const plural = (n: number) => `${count(n)} ${n === 1 ? 'commit' : 'commits'}`;
  const commitItems = (title: string, commits: readonly Commit[], first: Item[] = []): Item[] =>
    commits.length === 0
      ? []
      : [
          { label: `${title} (${count(commits.length)})`, kind: vscode.QuickPickItemKind.Separator },
          ...first,
          ...commits.slice(0, LISTED).map((c) => ({
            label: `$(git-commit) ${c.subject}`,
            description: `${c.oid.slice(0, 8)} · ${c.author}`,
            open: () => openCommitChanges(repo, c),
          })),
          ...(commits.length > LISTED ? [{ label: `… and ${count(commits.length - LISTED)} more` }] : []),
        ];
  // 送る変更すべて: 上流（分かれていれば分かれたところ）から送る先端まで
  const tip = outgoing[0];
  const from = incoming.length > 0 && oldest && outgoing.length < LIMIT ? oldest.parents[0] : expect;
  const allTitle = `All changes to send (${count(outgoing.length)})`;
  const all: Item[] =
    tip && from
      ? [
          {
            label: `$(diff-multiple) ${allTitle}`,
            description: `${upstream} → ${name}, all files`,
            open: () => openRangeChanges(repo, from, tip.oid, `${allTitle}: ${upstream} → ${name}`),
          },
        ]
      : [];
  const lease = expect ? ` Replaces ${upstream} only if it is still at ${expect.slice(0, 8)}.` : '';
  const actions: Item[] = request.force
    ? [
        { label: '$(close) Cancel', choice: undefined },
        {
          label: `$(warning) Force Push ${name} to ${upstream}`,
          detail:
            (incoming.length > 0 ? `${upstream} loses ${plural(incoming.length)}.` : `It fails if ${upstream} has moved since you last fetched.`) +
            lease +
            ' This can’t be undone.',
          choice: 'force',
        },
      ]
    : diverged
      ? [
          { label: '$(repo-pull) Pull…', detail: `Bring in the ${plural(incoming.length)} first, then push.`, choice: 'pull' },
          {
            label: `$(warning) Force Push ${name} to ${upstream}`,
            detail: `${upstream} loses ${plural(incoming.length)}.${lease} This can’t be undone.`,
            choice: 'force',
          },
        ]
      : [
          {
            label: `$(repo-push) Push ${plural(outgoing.length)} to ${upstream}`,
            detail: 'Enter pushes. Choose a commit below to see its changes.',
            choice: 'push',
          },
        ];
  const title = diverged
    ? `${upstream} has ${plural(incoming.length)} you don’t have`
    : request.force
      ? `Force push ${name}?`
      : `Push ${plural(outgoing.length)} to ${upstream}?`;
  // メインビューが開いていれば、アプリと同じく 1 枚のシートに（送った後の形の絵つき）
  const panel = MainPanel.active;
  if (panel?.canAsk) {
    const replacing = request.force || diverged;
    const preview = await repo.backend.graphPreview<MovePreview>({ op: 'movePreview', from: upstreamRef, to: branch });
    // 送った後の形（上流の札が手元の先端へ。Force Push なら上流から消えるコミットを赤で）
    const drawing = moveDrawing(preview, {
      moving: upstream,
      kind: 'remote',
      afterTitle: replacing ? 'After the force push' : 'After the push',
      lostNote: `removed from ${upstream}`,
      tone: 'red',
    });
    const opens: (() => Promise<void>)[] = [];
    const list = (listTitle: string, commits: readonly Commit[], tone?: 'red'): SheetList | undefined => {
      if (commits.length === 0) return undefined;
      const items = commits.slice(0, LISTED).map((c) => {
        opens.push(() => openCommitChanges(repo, c));
        return { label: c.subject, description: `${c.oid.slice(0, 8)} · ${c.author}`, open: true };
      });
      // 送る変更すべて（先頭の行）
      if (!tone && all[0]?.open) {
        opens.unshift(all[0].open);
        items.unshift({ label: allTitle, description: `${upstream} → ${name}, all files`, open: true });
      }
      const rest = commits.length - LISTED;
      return { title: `${listTitle} (${count(commits.length)})`, items, more: rest > 0 ? `… and ${count(rest)} more` : undefined, tone };
    };
    const lists = [list('Commits to send', outgoing), replacing ? list(`Commits ${upstream} will lose`, incoming, 'red') : undefined];
    const answer = await panel.ask(
      {
        title,
        lines: diverged
          ? ['Pull them first, or force push to replace them. Force Push can’t be undone.']
          : request.force
            ? [
                (incoming.length > 0 ? `${upstream} loses ${plural(incoming.length)}.` : `It fails if ${upstream} has moved since you last fetched.`) +
                  lease +
                  ' This can’t be undone.',
              ]
            : [],
        warnings,
        drawing,
        lists: lists.filter((l) => l !== undefined),
        buttons: diverged ? ['Pull…', 'Force Push'] : [replacing ? 'Force Push' : 'Push'],
        destructive: request.force,
      },
      (item) => void opens[item]?.().catch(showError),
    );
    if (!answer) return;
    if (diverged && answer.index === 0) await pullWithSheet(repo);
    else await send();
    return;
  }
  const pick = vscode.window.createQuickPick<Item>();
  pick.title = title;
  pick.placeholder = diverged ? 'Pull them first, or force push to replace them' : `${name} → ${upstream}`;
  pick.matchOnDescription = false;
  // 差分を開いてもシートは閉じない
  pick.ignoreFocusOut = true;
  pick.items = [
    ...actions,
    ...(warnings.length > 0
      ? [{ label: 'Worth a look', kind: vscode.QuickPickItemKind.Separator }, ...warnings.map((w) => ({ label: `$(warning) ${w}` }))]
      : []),
    ...commitItems('Commits to send', outgoing, all),
    ...commitItems(`Commits ${upstream} will lose`, request.force || diverged ? incoming : []),
  ];
  pick.activeItems = [actions[0]!];
  const chosen = await new Promise<Choice | undefined>((resolve) => {
    pick.onDidAccept(() => {
      const item = pick.selectedItems[0];
      // 送る変更すべてとコミットの行は差分を開く。どの行も、選んでも閉じない（Cancel は閉じる）
      if (item?.open) {
        void item.open().catch(showError);
        return;
      }
      if (item && !item.choice && item !== actions[0]) return;
      resolve(item?.choice);
      pick.hide();
    });
    pick.onDidHide(() => resolve(undefined));
    pick.show();
  });
  pick.dispose();
  if (chosen === 'pull') await pullWithSheet(repo);
  else if (chosen) await send();
}
