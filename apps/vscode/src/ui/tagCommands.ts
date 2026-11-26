/**
 * サイドバーのタグとリモートの右クリック（アプリのサイドバーと同じ操作）。タグはリモートへの push・置き換え・
 * 削除と手元の削除（Undo できる）、リモートは追加・名前の変更・URL と push の URL の変更・つながるかの確かめ・
 * prune・削除（Repository Settings の Remotes からも）。どれも共有の daemon の
 * コアで行う（エンジンがなければメニューに出さない）
 */
import * as vscode from 'vscode';
import { OperationError } from '../failure/kind.ts';
import type { Json } from '../daemon/client.ts';
import { daemonFor } from '../daemon/connection.ts';
import { manyTagTargets, tagStateText, type TagRemotes } from '../git/tagRemotes.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import { MainPanel } from './mainPanel.ts';
import type { Node, SidebarProvider } from './sidebar.ts';
import { showError } from './stageCommands.ts';

const CANT_UNDO = 'This can’t be undone.';

/**
 * コアの命令を実行する。うまくいかなければ投げる。daemon は ref を変わったと知るまで読み直さないので、
 * 先に読み直させる（端末の `git tag` で作ったタグも扱えるように）
 */
async function run(repo: Repository, command: Json, title: string): Promise<Json> {
  const client = await daemonFor(repo.root);
  if (!client) throw new Error(`${title} needs the Amon Hen engine (amonhen.daemon.enabled)`);
  const reply = await vscode.window.withProgress({ location: vscode.ProgressLocation.Window, title }, async () => {
    await client.call({ op: 'refs' });
    return (await client.call({ op: 'run', command })).json;
  });
  if (reply['ok'] !== true) throw new OperationError(String(reply['error'] ?? `${title} failed`), command['op']);
  return reply;
}

/**
 * 送る・消す先のリモートと、そこで扱うタグ（1 つしかなければ聞かない）。タグが 1 つならリモートでの様子を、
 * いくつもならそのリモートで扱う数を添える
 */
async function pickTarget(
  targets: { remote: string; tags: string[] }[],
  tags: readonly string[],
  info: TagRemotes,
  title: string,
): Promise<{ remote: string; tags: string[] } | undefined> {
  if (targets.length <= 1) return targets[0];
  const picked = await vscode.window.showQuickPick(
    targets.map((t) => ({
      label: t.remote,
      description: tags.length === 1 ? tagStateText(info, tags[0]!, t.remote) : `${t.tags.length} of ${tags.length} tags`,
      target: t,
    })),
    { title, placeHolder: 'Remote' },
  );
  return picked?.target;
}

/** 知らせに書くタグ（1 つなら名前、いくつもなら数） */
function tagsText(tags: readonly string[]): string {
  return tags.length === 1 ? tags[0]! : `${tags.length} tags`;
}

/** git のリモートの名前に使えるか（空白・制御文字、`/` で始まる・終わる、`..` などは使えない） */
function remoteNameProblem(name: string, taken: readonly string[]): string | undefined {
  if (!name) return 'Enter a name';
  if (/[\s\x00-\x1f~^:?*[\\]/.test(name) || name.startsWith('/') || name.endsWith('/') || name.includes('..'))
    return 'This name can’t be used for a remote';
  if (taken.includes(name)) return `${name} already exists`;
  return undefined;
}

/** URL へつないでみる（コアの `checkRemote`。git ls-remote）。つながらなければ投げる */
async function checkRemote(repo: Repository, url: string): Promise<{ branches: number; tags: number; head?: string | null }> {
  const client = await daemonFor(repo.root);
  if (!client) throw new Error('Check Connection needs the Amon Hen engine (amonhen.daemon.enabled)');
  const reply = (await client.call({ op: 'run', command: { op: 'checkRemote', url } })).json;
  if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `Can’t reach ${url}`));
  const info = reply['info'] as { branches?: number; tags?: number; head?: string | null } | undefined;
  return { branches: info?.branches ?? 0, tags: info?.tags ?? 0, head: info?.head };
}

/**
 * 新しいリモートの名前・fetch の URL・push の URL（空なら fetch と同じ）を聞く。メインビューが開いていればアプリと同じ
 * 1 つのシートで（URL の先が何かの行と、足す前に確かめる Check Connection つき）、でなければ入力欄を順に。やめたら
 * undefined
 */
async function askNewRemote(
  repo: Repository,
  taken: readonly string[],
): Promise<{ name: string; fetchUrl: string; pushUrl: string } | undefined> {
  const name = taken.includes('origin') ? '' : 'origin';
  const panel = MainPanel.active;
  if (panel?.canAsk) {
    const answer = await panel.ask(
      {
        title: 'Add Remote',
        lines: [],
        fields: [
          { label: 'Name', value: name, required: true, taken: [...taken] },
          { label: 'Fetch URL', value: '', placeholder: 'git@github.com:owner/repo.git', required: true },
          { label: 'Push URL', value: '', placeholder: 'Same as the fetch URL' },
        ],
        pointsTo: 1,
        check: 1,
        buttons: ['Add'],
      },
      undefined,
      async (url) => {
        const c = await checkRemote(repo, url);
        return { text: `Reachable: ${c.branches} branches, ${c.tags} tags${c.head ? `, HEAD at ${c.head}` : ''}`, ok: true };
      },
    );
    const [n = '', fetchUrl = '', pushUrl = ''] = (answer?.values ?? []).map((v) => v.trim());
    return answer && n && fetchUrl ? { name: n, fetchUrl, pushUrl } : undefined;
  }
  const typed = await vscode.window.showInputBox({
    title: 'Add Remote',
    prompt: 'Name',
    value: name,
    validateInput: (v) => remoteNameProblem(v.trim(), taken),
  });
  if (typed === undefined) return undefined;
  const fetchUrl = await vscode.window.showInputBox({
    title: `Add Remote ${typed.trim()}`,
    prompt: 'Fetch URL',
    placeHolder: 'git@github.com:owner/repo.git',
    validateInput: (v) => (v.trim() ? undefined : 'Enter a URL'),
  });
  if (fetchUrl === undefined) return undefined;
  const pushUrl = await vscode.window.showInputBox({
    title: `Add Remote ${typed.trim()}`,
    prompt: 'Push URL. Empty to push to the fetch URL.',
    placeHolder: 'Same as the fetch URL',
  });
  if (pushUrl === undefined) return undefined;
  return { name: typed.trim(), fetchUrl: fetchUrl.trim(), pushUrl: pushUrl.trim() };
}

export function registerTagCommands(manager: RepositoryManager, sidebar: SidebarProvider): vscode.Disposable {
  /**
   * タグのコマンド。今のリモートでの様子を読み直してから決める（印は最後の読み込みの時点のもの）。いくつも選んで
   * いれば、選んだタグすべて（アプリの「… N Tags」）
   */
  const tagCommand = (ids: string[], fn: (repo: Repository, tags: string[], info: TagRemotes) => Promise<void>) =>
    ids.map((id) =>
      vscode.commands.registerCommand(id, async (node?: Node, selected?: Node[]) => {
        const repo = manager.current;
        const nodes = selected && selected.length > 1 ? selected : node ? [node] : [];
        const tags = nodes.flatMap((n) => (n.kind === 'tag' ? [shortName(n.ref.name)] : []));
        if (!repo || tags.length === 0) return;
        try {
          const info = (await run(repo, { op: 'tagRemotes' }, 'Read tags on remotes'))['info'] as unknown as TagRemotes;
          await fn(repo, tags, info);
        } catch (err) {
          showError(err);
        } finally {
          await repo.refresh();
          await sidebar.tagMarks.reload();
        }
      }),
    );
  const remoteCommand = (id: string, fn: (repo: Repository, remote: string) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (node?: Node) => {
      const repo = manager.current;
      if (!repo || node?.kind !== 'remote') return;
      try {
        await fn(repo, node.remote.name);
      } catch (err) {
        showError(err);
      } finally {
        await repo.refresh();
      }
    });
  const changeRemote = (repo: Repository, change: Json, title: string) => run(repo, { op: 'changeRemote', change }, title);
  const remoteNames = (repo: Repository) => (repo.current.remotes ?? []).map((r) => r.name);

  return vscode.Disposable.from(
    // リモートと話す操作は先に聞く（送ったものは Undo では戻せない）
    ...tagCommand(['amonhen.tag.push', 'amonhen.tags.push'], async (repo, tags, info) => {
      const targets = manyTagTargets(info, tags).push;
      if (targets.length === 0) {
        void vscode.window.showInformationMessage(tags.length === 1 ? `Every remote has ${tags[0]}` : 'Every remote has these tags');
        return;
      }
      const target = await pickTarget(targets, tags, info, tags.length === 1 ? `Push ${tags[0]} to` : `Push ${tags.length} tags to`);
      if (!target) return;
      const { remote, tags: names } = target;
      const answer = await vscode.window.showWarningMessage(
        names.length === 1 ? `Push ${names[0]} to ${remote}?` : `Push ${names.length} tags to ${remote}?`,
        { modal: true, detail: `${names.join('\n')}\n\nEveryone who fetches from ${remote} gets them. ${CANT_UNDO}` },
        'Push',
      );
      if (answer !== 'Push') return;
      await run(repo, { op: 'pushTags', remote, names }, `Push ${tagsText(names)} to ${remote}`);
      vscode.window.setStatusBarMessage(`$(check) Pushed ${tagsText(names)} to ${remote}`, 5000);
    }),
    // リモートにある同じ名前の別のタグを、手元のもので置き換える（fetch した人みんなに効くので確かめる）
    ...tagCommand(['amonhen.tag.replace', 'amonhen.tags.replace'], async (repo, tags, info) => {
      const targets = manyTagTargets(info, tags).replace;
      if (targets.length === 0) {
        void vscode.window.showInformationMessage('No remote has a different tag with these names');
        return;
      }
      const target = await pickTarget(targets, tags, info, tags.length === 1 ? `Replace ${tags[0]} on` : `Replace ${tags.length} tags on`);
      if (!target) return;
      const { remote, tags: names } = target;
      const answer = await vscode.window.showWarningMessage(
        names.length === 1 ? `Replace ${names[0]} on ${remote}?` : `Replace ${names.length} tags on ${remote}?`,
        {
          modal: true,
          detail:
            (names.length === 1 ? '' : `${names.join('\n')}\n`) +
            `${remote} has a different tag with this name; yours replaces it for everyone who fetches. ${CANT_UNDO}`,
        },
        'Replace',
      );
      if (answer !== 'Replace') return;
      await run(repo, { op: 'pushTags', remote, names, force: true }, `Replace ${tagsText(names)} on ${remote}`);
      vscode.window.setStatusBarMessage(`$(check) Replaced ${tagsText(names)} on ${remote}`, 5000);
    }),
    // 手元のタグを消す（注釈付きのタグもオブジェクトごと Undo で戻る）。リモートのものは残る
    ...tagCommand(['amonhen.tag.delete', 'amonhen.tags.delete'], async (repo, tags) => {
      const one = tags.length === 1;
      const answer = await vscode.window.showWarningMessage(
        one ? `Delete tag ${tags[0]}?` : `Delete ${tags.length} tags?`,
        {
          modal: true,
          detail: one
            ? 'The remotes keep theirs. You can Undo this.'
            : `${tags.join('\n')}\n\nThe remotes keep theirs. You can undo this, one tag at a time.`,
        },
        'Delete',
      );
      if (answer !== 'Delete') return;
      for (const tag of tags) await run(repo, { op: 'deleteTag', name: tag }, `Delete tag ${tag}`);
      void vscode.window.showInformationMessage(one ? `Deleted tag ${tags[0]}` : `Deleted ${tags.length} tags`, 'Undo').then((choice) => {
        if (choice === 'Undo') void vscode.commands.executeCommand('amonhen.undo');
      });
    }),
    ...tagCommand(['amonhen.tag.deleteFromRemote', 'amonhen.tags.deleteFromRemote'], async (repo, tags, info) => {
      const targets = manyTagTargets(info, tags).onRemote;
      if (targets.length === 0) {
        void vscode.window.showInformationMessage(tags.length === 1 ? `No remote has ${tags[0]}` : 'No remote has these tags');
        return;
      }
      const target = await pickTarget(targets, tags, info, tags.length === 1 ? `Delete ${tags[0]} from` : `Delete ${tags.length} tags from`);
      if (!target) return;
      const { remote, tags: names } = target;
      const one = names.length === 1;
      const unchecked = !info.remotes.some((r) => r.name === remote && r.checked !== null);
      const answer = await vscode.window.showWarningMessage(
        one ? `Delete ${names[0]} from ${remote}?` : `Delete ${names.length} tags from ${remote}?`,
        {
          modal: true,
          detail: one
            ? `Everyone who fetches from ${remote} loses it; yours stays. ${CANT_UNDO}` +
              (unchecked ? ` Whether ${remote} has it hasn’t been checked yet (Fetch reads it).` : '')
            : `${names.join('\n')}\n\nEveryone who fetches from ${remote} loses them; yours stay. ${CANT_UNDO}` +
              (unchecked ? ` Whether ${remote} has them hasn’t been checked yet (Fetch reads it).` : ''),
        },
        'Delete',
      );
      if (answer !== 'Delete') return;
      await run(repo, { op: 'pushTags', remote, names, delete: true }, `Delete ${tagsText(names)} from ${remote}`);
      vscode.window.setStatusBarMessage(`$(check) Deleted ${tagsText(names)} from ${remote}`, 5000);
    }),

    // Remotes の見出しから（リモートの行からも）
    vscode.commands.registerCommand('amonhen.remote.add', async () => {
      const repo = manager.current;
      if (!repo) return;
      try {
        const remote = await askNewRemote(repo, remoteNames(repo));
        if (!remote) return;
        // シートでは名前の形を確かめない（もうある名前だけ）ので、ここで断る
        const problem = remoteNameProblem(remote.name, remoteNames(repo));
        if (problem) throw new Error(problem);
        await changeRemote(repo, { kind: 'add', name: remote.name, url: remote.fetchUrl }, `Add remote ${remote.name}`);
        if (remote.pushUrl && remote.pushUrl !== remote.fetchUrl) {
          await changeRemote(repo, { kind: 'setPushUrl', name: remote.name, url: remote.pushUrl }, `Edit push URL of ${remote.name}`);
        }
      } catch (err) {
        showError(err);
      } finally {
        await repo.refresh();
      }
    }),
    remoteCommand('amonhen.remote.rename', async (repo, remote) => {
      const taken = remoteNames(repo).filter((r) => r !== remote);
      const to = await vscode.window.showInputBox({
        title: `Rename ${remote}`,
        prompt: 'New name (its remote-tracking branches and the upstreams that use it follow)',
        value: remote,
        validateInput: (v) => remoteNameProblem(v.trim(), taken),
      });
      if (to === undefined || to.trim() === remote) return;
      await changeRemote(repo, { kind: 'rename', from: remote, to: to.trim() }, `Rename remote ${remote}`);
    }),
    remoteCommand('amonhen.remote.editUrl', async (repo, remote) => {
      const current = repo.current.remotes?.find((r) => r.name === remote)?.fetchUrl ?? '';
      const url = await vscode.window.showInputBox({
        title: `Edit URL of ${remote}`,
        prompt: 'URL (fetch and push)',
        value: current,
        validateInput: (v) => (v.trim() ? undefined : 'Enter a URL'),
      });
      if (url === undefined || url.trim() === current) return;
      await changeRemote(repo, { kind: 'setUrl', name: remote, url: url.trim() }, `Edit URL of ${remote}`);
    }),
    // push の URL（アプリの Edit… のシートの Push URL）。空なら fetch の URL へ送る
    remoteCommand('amonhen.remote.editPushUrl', async (repo, remote) => {
      const info = repo.current.remotes?.find((r) => r.name === remote);
      const fetchUrl = info?.fetchUrl ?? '';
      const pushUrl = info?.pushUrl && info.pushUrl !== fetchUrl ? info.pushUrl : '';
      const url = await vscode.window.showInputBox({
        title: `Push URL of ${remote}`,
        prompt: 'Where Push sends to. Empty to push to the fetch URL.',
        value: pushUrl,
        placeHolder: fetchUrl,
      });
      if (url === undefined) return;
      await changeRemote(repo, { kind: 'setPushUrl', name: remote, url: url.trim() || null }, `Edit push URL of ${remote}`);
    }),
    // つながるかを確かめる（アプリの Edit… のシートの Check Connection。git ls-remote）
    remoteCommand('amonhen.remote.checkConnection', async (repo, remote) => {
      const url = repo.current.remotes?.find((r) => r.name === remote)?.fetchUrl;
      if (!url) return;
      const info = await vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, title: `Connecting to ${remote}…` },
        () => checkRemote(repo, url),
      );
      void vscode.window.showInformationMessage(
        `${remote}: reachable, ${info.branches} branches, ${info.tags} tags${info.head ? `, HEAD at ${info.head}` : ''}`,
      );
    }),
    // リモートで消えたブランチの追跡を消す。リモートと話す（消えたブランチを読む）ので、先に聞く
    remoteCommand('amonhen.remote.prune', async (repo, remote) => {
      const answer = await vscode.window.showWarningMessage(
        `Prune ${remote}?`,
        {
          modal: true,
          detail: `Removes the remote-tracking branches whose branches are gone from ${remote} (git remote prune). ${CANT_UNDO}`,
        },
        'Prune',
      );
      if (answer !== 'Prune') return;
      await changeRemote(repo, { kind: 'prune', name: remote }, `Prune ${remote}`);
      vscode.window.setStatusBarMessage(`$(check) Pruned ${remote}`, 5000);
    }),
    remoteCommand('amonhen.remote.remove', async (repo, remote) => {
      const answer = await vscode.window.showWarningMessage(
        `Remove ${remote}?`,
        {
          modal: true,
          detail: `This removes the remote’s settings and its remote-tracking branches (${remote}/…). The remote repository isn’t touched. ${CANT_UNDO}`,
        },
        'Remove',
      );
      if (answer !== 'Remove') return;
      await changeRemote(repo, { kind: 'remove', name: remote }, `Remove remote ${remote}`);
    }),
  );
}
