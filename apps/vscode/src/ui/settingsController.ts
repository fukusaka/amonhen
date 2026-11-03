/**
 * メインビューの Repository Settings の拡張ホスト側。共有の daemon のコアで、設定の並び（`configList`）・git の版
 * （`gitVersion`）・リポジトリの形（`repositoryForm`）を読み、行の値をこのリポジトリの設定に書く（`configSet`）
 */
import { homedir } from 'node:os';
import { join } from 'node:path';
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import { hooksDirectory } from '../settings/hookFiles.ts';
import { HOOK_NAMES, hookWhen } from '../settings/hooks.ts';
import type { ConfigEntry } from '../settings/model.ts';
import type { FetchingAction, RepositoryForm, SettingsFacts, SettingsHostMessage, SettingsWebviewMessage } from '../settings/protocol.ts';
import type { Repository } from '../state/repository.ts';
import { showError } from './stageCommands.ts';

/** コアの `repositoryForm` の答え（git の行に出す分） */
interface Form {
  shallow?: boolean;
  partial?: { remote: string; filter: string } | null;
  sparse?: { cone: boolean; folders: string[] } | null;
  maintenance?: boolean;
}

/** リポジトリの形の一言（アプリの git の行と同じ） */
function formItems(form: Form | undefined): SettingsFacts['form'] {
  const items: SettingsFacts['form'] = [];
  if (!form) return items;
  if (form.shallow) items.push({ title: 'Shallow clone', help: 'Older history isn’t fetched. Fetch More History… fetches it.' });
  if (form.partial) {
    items.push({
      title: `Partial clone (${form.partial.filter})`,
      help: `Contents are fetched from ${form.partial.remote} when needed. Fetch All Contents… fetches all of them.`,
    });
  }
  if (form.sparse) {
    items.push({
      title: `Sparse checkout (${form.sparse.folders.length} ${form.sparse.cone ? 'folders' : 'patterns'})`,
      help: form.sparse.folders.join('\n'),
    });
  }
  if (form.maintenance) items.push({ title: 'git maintenance on', help: 'git maintenance runs for this repository in the background.' });
  return items;
}

/**
 * HEAD の木を降りて、取り出すフォルダを 1 つ選んでもらう（アプリのフォルダの木の代わりに QuickPick）。`first` なら
 * sparse checkout を始めるので、そう言う。リポジトリの中のパス（`/` で始まらない）を返す
 */
async function pickFolder(client: NonNullable<Awaited<ReturnType<typeof daemonFor>>>, first: boolean): Promise<string | undefined> {
  let path = '/';
  for (;;) {
    const reply = (await client.call({ op: 'run', command: { op: 'treeFolders', path } })).json;
    if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? 'Can’t read the folders'));
    const entries = ((reply['info'] as { entries?: { name: string; kind: string }[] } | undefined)?.entries ?? []).filter((e) => e.kind === 'folder');
    type Item = vscode.QuickPickItem & { to?: string; use?: boolean };
    const items: Item[] = [
      ...(path !== '/' ? [{ label: `$(check) Check out ${path.slice(1)}`, use: true }, { label: '$(arrow-up) ..', to: path.replace(/\/[^/]+$/, '') || '/' }] : []),
      ...entries.map((e) => ({ label: `$(folder) ${e.name}`, to: `${path === '/' ? '' : path}/${e.name}` })),
    ];
    const picked = await vscode.window.showQuickPick(items, {
      title: first ? 'Check Out Only Some Folders' : 'Check Out One More Folder',
      placeHolder: first
        ? 'Only this folder is checked out, with the files at the top. Others are removed from disk until you check them out again.'
        : `Folders in the latest commit, including ones not checked out (${path})`,
    });
    if (!picked) return undefined;
    if (picked.use) return path.slice(1);
    path = picked.to!;
  }
}

export class SettingsController implements vscode.Disposable {
  private readonly post: (message: SettingsHostMessage) => void;
  private repo: Repository | undefined;
  private shown = false;
  /** 読むたびに増やし、古い答えを捨てる */
  private generation = 0;

  constructor(post: (message: SettingsHostMessage) => void) {
    this.post = post;
  }

  setRepository(repo: Repository | undefined): void {
    if (repo === this.repo) return;
    this.repo = repo;
    if (this.shown) void this.load();
  }

  async handle(message: SettingsWebviewMessage): Promise<void> {
    switch (message.type) {
      case 'settingsLoad':
        this.shown = true;
        await this.load();
        break;
      case 'settingsSet':
        await this.write(message.writes).catch(showError);
        await this.load();
        break;
      case 'settingsSaveExclude':
        await this.saveExclude(message.exclude).catch(showError);
        await this.load();
        break;
      case 'settingsRun':
        await this.runAction(message).catch(showError);
        await this.load();
        break;
    }
  }

  private async write(writes: [string, string | null][]): Promise<void> {
    const repo = this.repo;
    const client = repo && (await daemonFor(repo.root));
    if (!client) throw new Error('Repository Settings needs the Amon Hen engine (amonhen.daemon.enabled)');
    for (const [key, value] of writes) {
      const reply = (await client.call({ op: 'run', command: { op: 'configSet', key, value } })).json;
      if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `Can’t set ${key}`));
    }
    // 設定は Pull・rebase・merge などの操作が読む。サイドバーなどは読み直しで追いつく
    await repo.refresh();
  }

  /** `.git/info/exclude` を書く。コアは `.gitignore` も一緒に書くので、今の中身を読んでそのまま渡す */
  private async saveExclude(exclude: string): Promise<void> {
    const repo = this.repo;
    const client = repo && (await daemonFor(repo.root));
    if (!client) throw new Error('Repository Settings needs the Amon Hen engine (amonhen.daemon.enabled)');
    const files = (await client.call({ op: 'run', command: { op: 'ignoreFiles' } })).json['info'] as { gitignore?: string } | undefined;
    const reply = (await client.call({ op: 'run', command: { op: 'saveIgnoreFiles', gitignore: files?.gitignore ?? '', exclude } })).json;
    if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? 'Can’t save .git/info/exclude'));
    await repo.refresh();
  }

  /** Fetching and Folders のボタン。取得は History の帯と同じコマンド、sparse checkout と maintenance はコアで */
  private async runAction(a: FetchingAction): Promise<void> {
    const repo = this.repo;
    const client = repo && (await daemonFor(repo.root));
    if (!client) throw new Error('Repository Settings needs the Amon Hen engine (amonhen.daemon.enabled)');
    const run = async (command: Record<string, unknown>, title: string) => {
      const reply = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title }, async () =>
        (await client.call({ op: 'run', command })).json,
      );
      if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `${title} failed`));
    };
    const form = (await client.call({ op: 'run', command: { op: 'repositoryForm' } })).json['info'] as RepositoryForm | undefined;
    const folders = form?.sparse?.cone ? form.sparse.folders : [];
    switch (a.action) {
      case 'hookEnabled':
        await run({ op: 'setHookEnabled', name: a.name, enabled: a.enabled }, a.enabled ? `Enable ${a.name}` : `Disable ${a.name}`);
        return;
      case 'hookOpen':
        await this.openHook(client, a.name);
        return;
      case 'hookCreate': {
        const info = (await client.call({ op: 'run', command: { op: 'hooksAndLfs' } })).json['info'] as { hooks?: { name: string }[] } | undefined;
        const present = new Set((info?.hooks ?? []).map((h) => h.name));
        const picked = await vscode.window.showQuickPick(
          HOOK_NAMES.filter((n) => !present.has(n)).map((n) => ({ label: n, description: hookWhen(n) })),
          { title: 'Create Hook', placeHolder: 'Puts a #!/bin/sh skeleton in place and makes it executable.' },
        );
        if (!picked) return;
        await run({ op: 'createHook', name: picked.label }, `Create ${picked.label}`);
        await this.openHook(client, picked.label);
        return;
      }
      case 'lfsTrack': {
        let pattern = a.pattern;
        if (!pattern) {
          pattern = (
            await vscode.window.showInputBox({
              title: 'Track a Pattern with LFS',
              prompt: 'Files matching the pattern are stored in LFS from now on (git lfs track).',
              placeHolder: '*.psd',
              validateInput: (v) => (v.trim() ? undefined : 'Enter a pattern'),
            })
          )?.trim();
          if (!pattern) return;
        }
        await run({ op: 'lfsTrack', pattern, track: a.track }, a.track ? `Track ${pattern} with LFS` : `Stop tracking ${pattern} with LFS`);
        break;
      }
      case 'configEdit':
        await this.editConfig(client, a.key);
        return;
      case 'remote':
        await this.remoteAction(client, a.op, a.name);
        return;
      case 'fetchMore':
        await vscode.commands.executeCommand('amonhen.fetchMoreHistory');
        return;
      case 'fetchAll':
        await vscode.commands.executeCommand('amonhen.fetchAllHistory');
        return;
      case 'fetchContents':
        await vscode.commands.executeCommand('amonhen.fetchAllContents');
        return;
      case 'maintenance':
        await run({ op: 'maintenance', on: a.on }, a.on ? 'Start git maintenance' : 'Stop git maintenance');
        return;
      case 'removeFolder':
        await run({ op: 'sparseCheckout', folders: folders.filter((f) => f !== a.folder) }, `Stop checking out ${a.folder}`);
        break;
      case 'addFolder': {
        const folder = await pickFolder(client, folders.length === 0 && !form?.sparse);
        if (!folder || folders.includes(folder)) return;
        await run({ op: 'sparseCheckout', folders: [...folders, folder] }, `Check out ${folder}`);
        break;
      }
      case 'checkOutAll': {
        const answer = await vscode.window.showWarningMessage(
          'Check Out All Folders?',
          {
            modal: true,
            detail: 'Every folder is checked out again (git sparse-checkout disable). In a large repository this can take long and use a lot of disk.',
          },
          'Check Out All',
        );
        if (answer !== 'Check Out All') return;
        await run({ op: 'disableSparseCheckout' }, 'Check out all folders');
        break;
      }
    }
    await repo.refresh();
  }

  /** フックをエディタで開く（置き場所は core.hooksPath、なければ git dir の hooks） */
  private async openHook(client: NonNullable<Awaited<ReturnType<typeof daemonFor>>>, name: string): Promise<void> {
    const repo = this.repo;
    if (!repo) return;
    const info = (await client.call({ op: 'run', command: { op: 'hooksAndLfs' } })).json['info'] as { hooksPath?: string | null } | undefined;
    const dir = hooksDirectory(info?.hooksPath, repo.root, repo.backend.dirs.commonDir, homedir());
    await vscode.window.showTextDocument(vscode.Uri.file(join(dir, name)));
  }

  /** Keys and Values: このリポジトリの値を聞いて書く。キーがなければ、キーから聞く（git の知っている名前を出す） */
  private async editConfig(client: NonNullable<Awaited<ReturnType<typeof daemonFor>>>, key: string | undefined): Promise<void> {
    let name = key;
    if (!name) {
      name = (
        await vscode.window.showInputBox({
          title: 'Add a Key to This Repository',
          prompt: 'The key (section.name, or section.subsection.name)',
          placeHolder: 'core.editor',
          validateInput: (v) => (/^[A-Za-z0-9-]+(\..+)?\.[A-Za-z][A-Za-z0-9-]*$/.test(v.trim()) ? undefined : 'Write the key as section.name'),
        })
      )?.trim();
      if (!name) return;
    }
    const list = (await client.call({ op: 'run', command: { op: 'configList' } })).json['info'] as { entries?: ConfigEntry[] } | undefined;
    const local = [...(list?.entries ?? [])].reverse().find((e) => e.key.toLowerCase() === name!.toLowerCase() && e.scope === 'local');
    const value = await vscode.window.showInputBox({
      title: `${name} in This Repository`,
      prompt: 'The value to set in this repository (.git/config)',
      value: local?.value ?? '',
    });
    if (value === undefined) return;
    const reply = (await client.call({ op: 'run', command: { op: 'configSet', key: name, value } })).json;
    if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `Can’t set ${name}`));
    await this.repo?.refresh();
  }

  /**
   * Remotes の行のボタン。追加・名前・URL・prune・削除はサイドバーのリモートのメニューと同じコマンド、push の URL と
   * つながるかの確かめは、ここで（アプリの Remotes のシートの Push URL と Check Connection）
   */
  private async remoteAction(
    client: NonNullable<Awaited<ReturnType<typeof daemonFor>>>,
    op: 'add' | 'rename' | 'editUrl' | 'editPushUrl' | 'check' | 'prune' | 'remove',
    name: string | undefined,
  ): Promise<void> {
    if (op === 'add') {
      await vscode.commands.executeCommand('amonhen.remote.add');
      return;
    }
    if (!name) return;
    const remote = this.repo?.current.remotes?.find((r) => r.name === name);
    const node = { kind: 'remote', remote: remote ?? { name } };
    switch (op) {
      case 'rename':
      case 'editUrl':
      case 'prune':
      case 'remove':
        await vscode.commands.executeCommand(`amonhen.remote.${op}`, node);
        return;
      case 'editPushUrl': {
        const fetchUrl = remote?.fetchUrl ?? '';
        const pushUrl = remote?.pushUrl && remote.pushUrl !== fetchUrl ? remote.pushUrl : '';
        const url = await vscode.window.showInputBox({
          title: `Push URL of ${name}`,
          prompt: 'Where Push sends to. Empty to push to the fetch URL.',
          value: pushUrl,
          placeHolder: fetchUrl,
        });
        if (url === undefined) return;
        const reply = (
          await client.call({ op: 'run', command: { op: 'changeRemote', change: { kind: 'setPushUrl', name, url: url.trim() || null } } })
        ).json;
        if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? 'Can’t set the push URL'));
        await this.repo?.refresh();
        return;
      }
      case 'check': {
        const url = remote?.fetchUrl;
        if (!url) return;
        const reply = await vscode.window.withProgress(
          { location: vscode.ProgressLocation.Notification, title: `Connecting to ${name}…` },
          async () => (await client.call({ op: 'run', command: { op: 'checkRemote', url } })).json,
        );
        if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `Can’t reach ${name}`));
        const info = reply['info'] as { branches?: number; tags?: number; head?: string | null } | undefined;
        void vscode.window.showInformationMessage(
          `${name}: reachable, ${info?.branches ?? 0} branches, ${info?.tags ?? 0} tags${info?.head ? `, HEAD at ${info.head}` : ''}`,
        );
        return;
      }
    }
  }

  private async load(): Promise<void> {
    const generation = ++this.generation;
    const repo = this.repo;
    const empty: SettingsFacts = { updateRefs: true, rerereCache: false, form: [], branches: [], exclude: '', remotes: [], knownKeys: [] };
    if (!repo) {
      this.post({ type: 'settings', entries: [], facts: empty });
      return;
    }
    try {
      const client = await daemonFor(repo.root);
      if (!client) {
        this.post({ type: 'settings', entries: [], facts: empty, error: 'Repository Settings needs the Amon Hen engine (amonhen.daemon.enabled)' });
        return;
      }
      const run = async (op: string) => (await client.call({ op: 'run', command: { op } })).json['info'] as Record<string, unknown> | undefined;
      const [list, version, form, groups, ignore, remotes, known, hooks] = await Promise.all([
        run('configList'),
        run('gitVersion'),
        run('repositoryForm'),
        run('branchGroups'),
        run('ignoreFiles'),
        run('remotes'),
        run('configKeys'),
        run('hooksAndLfs'),
      ]);
      if (generation !== this.generation) return;
      this.post({
        type: 'settings',
        entries: (list?.['entries'] as ConfigEntry[] | undefined) ?? [],
        facts: {
          ...(typeof version?.['version'] === 'string' ? { gitVersion: version['version'] } : {}),
          updateRefs: version?.['updateRefs'] !== false,
          rerereCache: version?.['rerereCache'] === true,
          form: formItems(form as Form | undefined),
          branches: ((groups?.['branches'] as { name: string }[] | undefined) ?? []).map((b) => b.name).sort(),
          ...(typeof groups?.['base'] === 'string' ? { base: groups['base'] } : {}),
          exclude: String(ignore?.['exclude'] ?? ''),
          ...(form ? { repositoryForm: form as unknown as RepositoryForm } : {}),
          remotes: (remotes?.['remotes'] as SettingsFacts['remotes'] | undefined) ?? [],
          knownKeys: (known?.['keys'] as string[] | undefined) ?? [],
          ...(hooks ? { hooksAndLfs: hooks as unknown as NonNullable<SettingsFacts['hooksAndLfs']> } : {}),
        },
      });
    } catch (err) {
      if (generation === this.generation) {
        this.post({ type: 'settings', entries: [], facts: empty, error: err instanceof Error ? err.message : String(err) });
      }
    }
  }

  dispose(): void {
    this.generation++;
  }
}
