import { basename } from 'node:path';
import * as vscode from 'vscode';
import { CliBackend } from '../git/backend.ts';
import { GitExecutor } from '../git/exec.ts';
import type { GitApi, GitApiRepository, GitExtension } from './gitApi.ts';
import { Repository } from './repository.ts';

/** `vscode.git` の変更通知はまとめて来るので、少し待ってから 1 回だけ取り直す */
const DEBOUNCE_MS = 50;

interface Entry {
  repository: Repository;
  /** `vscode.git` 側のリポジトリ。ルートのパス表記が一致するとは限らないので、対応はこれで取る */
  apiRepo: GitApiRepository | undefined;
  disposables: vscode.Disposable[];
}

/**
 * 開いているリポジトリの一覧と、サイドバーに表示する現在のリポジトリ。
 * 状態変化の検知は `vscode.git` の通知に任せ、自前の file watcher は持たない。
 */
export class RepositoryManager implements vscode.Disposable {
  private readonly log: vscode.LogOutputChannel;
  private git: GitExecutor | undefined;
  private readonly entries = new Map<string, Entry>();
  private _current: Repository | undefined;
  private readonly disposables: vscode.Disposable[] = [];

  private readonly _onDidChangeCurrent = new vscode.EventEmitter<Repository | undefined>();
  readonly onDidChangeCurrent = this._onDidChangeCurrent.event;

  constructor(log: vscode.LogOutputChannel) {
    this.log = log;
  }

  get current(): Repository | undefined {
    return this._current;
  }

  get repositories(): Repository[] {
    return [...this.entries.values()].map((e) => e.repository);
  }

  async initialize(): Promise<void> {
    const api = await getGitApi();
    this.git = new GitExecutor(api?.git.path ?? 'git', (l) =>
      this.log.trace(`${Math.round(l.ms)}ms git ${l.args.join(' ')}${l.exitCode ? ` (exit ${l.exitCode})` : ''}`),
    );
    if (api) {
      this.disposables.push(
        api.onDidOpenRepository((r) => void this.open(r.rootUri.fsPath, r)),
        api.onDidCloseRepository((r) => this.close(r)),
      );
      await Promise.all(api.repositories.map((r) => this.open(r.rootUri.fsPath, r)));
    } else {
      // vscode.git が無効のときはワークスペースのフォルダから探す。自動更新はしない
      this.log.warn('vscode.git is unavailable; automatic refresh is disabled');
      await Promise.all((vscode.workspace.workspaceFolders ?? []).map((f) => this.open(f.uri.fsPath)));
    }
  }

  private async open(path: string, apiRepo?: GitApiRepository): Promise<void> {
    if (!this.git) {
      return;
    }
    let backend: CliBackend;
    try {
      backend = await CliBackend.open(this.git, path);
    } catch (err) {
      this.log.debug(`not a git repository: ${path}`, err);
      return;
    }
    if (this.entries.has(backend.root)) {
      backend.dispose();
      return;
    }
    const repository = new Repository(backend, this.log);
    const entry: Entry = { repository, apiRepo, disposables: [repository] };
    if (apiRepo) {
      let timer: NodeJS.Timeout | undefined;
      entry.disposables.push(
        apiRepo.state.onDidChange(() => {
          clearTimeout(timer);
          timer = setTimeout(() => void repository.refresh(), DEBOUNCE_MS);
        }),
        { dispose: () => clearTimeout(timer) },
      );
    }
    this.entries.set(backend.root, entry);
    this.log.info(`opened ${backend.root}`);
    await repository.refresh();
    this.updateContext();
    if (!this._current || isPreferred(backend.root, this._current.root)) {
      this.select(repository);
    }
  }

  private close(apiRepo: GitApiRepository): void {
    const entry = [...this.entries.values()].find((e) => e.apiRepo === apiRepo);
    if (!entry) {
      return;
    }
    this.entries.delete(entry.repository.root);
    vscode.Disposable.from(...entry.disposables).dispose();
    this.updateContext();
    if (this._current === entry.repository) {
      this.select(this.repositories[0]);
    }
  }

  select(repository: Repository | undefined): void {
    if (this._current === repository) {
      return;
    }
    this._current = repository;
    this._onDidChangeCurrent.fire(repository);
  }

  async pick(): Promise<void> {
    const items = this.repositories.map((r) => ({
      label: basename(r.root),
      description: r.root,
      repository: r,
    }));
    const picked = await vscode.window.showQuickPick(items, { placeHolder: 'リポジトリを選択' });
    if (picked) {
      this.select(picked.repository);
    }
  }

  private updateContext(): void {
    void vscode.commands.executeCommand('setContext', 'amonhen.hasRepository', this.entries.size > 0);
    void vscode.commands.executeCommand('setContext', 'amonhen.multipleRepositories', this.entries.size > 1);
  }

  dispose(): void {
    for (const entry of this.entries.values()) {
      vscode.Disposable.from(...entry.disposables).dispose();
    }
    this.entries.clear();
    vscode.Disposable.from(...this.disposables).dispose();
    this._onDidChangeCurrent.dispose();
  }
}

/** ワークスペースのフォルダそのものであるリポジトリを、サブモジュールなどより優先する */
function isPreferred(candidate: string, current: string): boolean {
  const folders = (vscode.workspace.workspaceFolders ?? []).map((f) => f.uri.fsPath);
  return folders.includes(candidate) && !folders.includes(current);
}

async function getGitApi(): Promise<GitApi | undefined> {
  const ext = vscode.extensions.getExtension<GitExtension>('vscode.git');
  if (!ext) {
    return undefined;
  }
  const exports = ext.isActive ? ext.exports : await ext.activate();
  if (!exports.enabled) {
    return undefined;
  }
  const api = exports.getAPI(1);
  if (api.state !== 'initialized') {
    await new Promise<void>((resolve) => {
      const d = api.onDidChangeState((s) => {
        if (s === 'initialized') {
          d.dispose();
          resolve();
        }
      });
    });
  }
  return api;
}
