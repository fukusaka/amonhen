/**
 * サイドバーのタグとリモートの右クリック（アプリのサイドバーと同じ操作）。タグはリモートへの push・置き換え・
 * 削除と手元の削除（Undo できる）、リモートは追加・名前の変更・URL の変更・prune・削除。どれも共有の daemon の
 * コアで行う（エンジンがなければメニューに出さない）
 */
import * as vscode from 'vscode';
import type { Json } from '../daemon/client.ts';
import { daemonFor } from '../daemon/connection.ts';
import { tagStateText, tagTargets, type TagRemotes } from '../git/tagRemotes.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
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
  if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `${title} failed`));
  return reply;
}

/** 選ぶリモート（1 つしかなければ聞かない） */
async function pickRemote(remotes: string[], title: string, describe: (remote: string) => string): Promise<string | undefined> {
  if (remotes.length <= 1) return remotes[0];
  const picked = await vscode.window.showQuickPick(
    remotes.map((r) => ({ label: r, description: describe(r) })),
    { title, placeHolder: 'Remote' },
  );
  return picked?.label;
}

/** git のリモートの名前に使えるか（空白・制御文字、`/` で始まる・終わる、`..` などは使えない） */
function remoteNameProblem(name: string, taken: readonly string[]): string | undefined {
  if (!name) return 'Enter a name';
  if (/[\s\x00-\x1f~^:?*[\\]/.test(name) || name.startsWith('/') || name.endsWith('/') || name.includes('..'))
    return 'This name can’t be used for a remote';
  if (taken.includes(name)) return `${name} already exists`;
  return undefined;
}

export function registerTagCommands(manager: RepositoryManager, sidebar: SidebarProvider): vscode.Disposable {
  /** タグのコマンド。今のリモートでの様子を読み直してから決める（印は最後の読み込みの時点のもの） */
  const tagCommand = (id: string, fn: (repo: Repository, tag: string, info: TagRemotes) => Promise<void>) =>
    vscode.commands.registerCommand(id, async (node?: Node) => {
      const repo = manager.current;
      if (!repo || node?.kind !== 'tag') return;
      const tag = shortName(node.ref.name);
      try {
        const info = (await run(repo, { op: 'tagRemotes' }, 'Read tags on remotes'))['info'] as unknown as TagRemotes;
        await fn(repo, tag, info);
      } catch (err) {
        showError(err);
      } finally {
        await repo.refresh();
        await sidebar.tagMarks.reload();
      }
    });
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
    tagCommand('amonhen.tag.push', async (repo, tag, info) => {
      const remotes = tagTargets(info, tag).push;
      if (remotes.length === 0) {
        void vscode.window.showInformationMessage(`Every remote has ${tag}`);
        return;
      }
      const remote = await pickRemote(remotes, `Push ${tag} to`, (r) => tagStateText(info, tag, r));
      if (!remote) return;
      await run(repo, { op: 'pushTags', remote, names: [tag] }, `Push ${tag} to ${remote}`);
      vscode.window.setStatusBarMessage(`$(check) Pushed ${tag} to ${remote}`, 5000);
    }),
    // リモートにある同じ名前の別のタグを、手元のもので置き換える（fetch した人みんなに効くので確かめる）
    tagCommand('amonhen.tag.replace', async (repo, tag, info) => {
      const remote = await pickRemote(tagTargets(info, tag).replace, `Replace ${tag} on`, (r) => tagStateText(info, tag, r));
      if (!remote) return;
      const answer = await vscode.window.showWarningMessage(
        `Replace ${tag} on ${remote}?`,
        {
          modal: true,
          detail: `${remote} has a different tag with this name; yours replaces it for everyone who fetches. ${CANT_UNDO}`,
        },
        'Replace',
      );
      if (answer !== 'Replace') return;
      await run(repo, { op: 'pushTags', remote, names: [tag], force: true }, `Replace ${tag} on ${remote}`);
      vscode.window.setStatusBarMessage(`$(check) Replaced ${tag} on ${remote}`, 5000);
    }),
    // 手元のタグを消す（注釈付きのタグもオブジェクトごと Undo で戻る）。リモートのものは残る
    tagCommand('amonhen.tag.delete', async (repo, tag) => {
      const answer = await vscode.window.showWarningMessage(
        `Delete tag ${tag}?`,
        { modal: true, detail: 'The remotes keep theirs. You can Undo this.' },
        'Delete',
      );
      if (answer !== 'Delete') return;
      await run(repo, { op: 'deleteTag', name: tag }, `Delete tag ${tag}`);
      void vscode.window.showInformationMessage(`Deleted tag ${tag}`, 'Undo').then((choice) => {
        if (choice === 'Undo') void vscode.commands.executeCommand('amonhen.undo');
      });
    }),
    tagCommand('amonhen.tag.deleteFromRemote', async (repo, tag, info) => {
      const remote = await pickRemote(tagTargets(info, tag).onRemote, `Delete ${tag} from`, (r) => tagStateText(info, tag, r));
      if (!remote) return;
      const unchecked = !info.remotes.some((r) => r.name === remote && r.checked !== null);
      const answer = await vscode.window.showWarningMessage(
        `Delete ${tag} from ${remote}?`,
        {
          modal: true,
          detail:
            `Everyone who fetches from ${remote} loses it; yours stays. ${CANT_UNDO}` +
            (unchecked ? ` Whether ${remote} has it hasn’t been checked yet (Fetch reads it).` : ''),
        },
        'Delete',
      );
      if (answer !== 'Delete') return;
      await run(repo, { op: 'pushTags', remote, names: [tag], delete: true }, `Delete ${tag} from ${remote}`);
      vscode.window.setStatusBarMessage(`$(check) Deleted ${tag} from ${remote}`, 5000);
    }),

    // Remotes の見出しから（リモートの行からも）
    vscode.commands.registerCommand('amonhen.remote.add', async () => {
      const repo = manager.current;
      if (!repo) return;
      try {
        const taken = remoteNames(repo);
        const name = await vscode.window.showInputBox({
          title: 'Add Remote',
          prompt: 'Name',
          value: taken.includes('origin') ? '' : 'origin',
          validateInput: (v) => remoteNameProblem(v.trim(), taken),
        });
        if (name === undefined) return;
        const url = await vscode.window.showInputBox({
          title: `Add Remote ${name.trim()}`,
          prompt: 'URL (fetch and push)',
          placeHolder: 'git@github.com:owner/repo.git',
          validateInput: (v) => (v.trim() ? undefined : 'Enter a URL'),
        });
        if (url === undefined) return;
        await changeRemote(repo, { kind: 'add', name: name.trim(), url: url.trim() }, `Add remote ${name.trim()}`);
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
    // リモートで消えたブランチの追跡を消す
    remoteCommand('amonhen.remote.prune', async (repo, remote) => {
      await changeRemote(repo, { kind: 'prune', name: remote }, `Prune ${remote}`);
      vscode.window.setStatusBarMessage(`$(check) Pruned ${remote}`, 5000);
    }),
    remoteCommand('amonhen.remote.remove', async (repo, remote) => {
      const answer = await vscode.window.showWarningMessage(
        `Remove ${remote}?`,
        {
          modal: true,
          detail: `This removes the remote’s settings and its remote-tracking branches (${remote}/…). The remote repository isn’t touched.`,
        },
        'Remove',
      );
      if (answer !== 'Remove') return;
      await changeRemote(repo, { kind: 'remove', name: remote }, `Remove remote ${remote}`);
    }),
  );
}
