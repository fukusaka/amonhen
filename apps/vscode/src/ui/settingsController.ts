/**
 * メインビューの Repository Settings の拡張ホスト側。共有の daemon のコアで、設定の並び（`configList`）・git の版
 * （`gitVersion`）・リポジトリの形（`repositoryForm`）を読み、行の値をこのリポジトリの設定に書く（`configSet`）
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { ConfigEntry } from '../settings/model.ts';
import type { SettingsFacts, SettingsHostMessage, SettingsWebviewMessage } from '../settings/protocol.ts';
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

  private async load(): Promise<void> {
    const generation = ++this.generation;
    const repo = this.repo;
    const empty: SettingsFacts = { updateRefs: true, rerereCache: false, form: [], branches: [], exclude: '' };
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
      const [list, version, form, groups, ignore] = await Promise.all([
        run('configList'),
        run('gitVersion'),
        run('repositoryForm'),
        run('branchGroups'),
        run('ignoreFiles'),
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
