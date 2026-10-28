import { basename } from 'node:path';
import * as vscode from 'vscode';
import { accessSync, constants } from 'node:fs';
import { setFallbackGit } from '../daemon/client.ts';
import { daemonFor, resendDaemonEnvironment } from '../daemon/connection.ts';
import { withDaemonWrites } from '../daemon/writes.ts';
import { CliBackend } from '../git/backend.ts';
import { GitExecutor } from '../git/exec.ts';
import type { GitApi, GitApiRepository, GitExtension } from './gitApi.ts';
import { Repository } from './repository.ts';

/**
 * 設定 `git.path` の git（文字列か、その並び。並びなら最初に実行できるもの）。決めていなければ undefined
 */
function configuredGitPath(): string | undefined {
  const value = vscode.workspace.getConfiguration('git').get<string | string[]>('path');
  const paths = (Array.isArray(value) ? value : [value]).filter((p): p is string => typeof p === 'string' && p !== '');
  return (
    paths.find((p) => {
      try {
        accessSync(p, process.platform === 'win32' ? constants.F_OK : constants.X_OK);
        return true;
      } catch {
        return false;
      }
    }) ?? paths[0]
  );
}

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
  /** VSCode の git（`git.path`、なければ vscode.git が見つけたもの）。Amon Hen の設定で決めていないときに使う */
  private vscodeGit = 'git';
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

  get(root: string): Repository | undefined {
    return this.entries.get(root)?.repository;
  }

  async initialize(): Promise<void> {
    const api = await getGitApi();
    // daemon を使うときも、Amon Hen の設定で git を決めていなければ git.path の git を使ってもらう。git.path を
    // 変えたら、開いているつながりにも送り直す（開き直さずに効く）
    const configured = configuredGitPath();
    this.vscodeGit = configured ?? api?.git.path ?? 'git';
    setFallbackGit(configured ? api?.git.path ?? configured : undefined);
    this.disposables.push(
      vscode.workspace.onDidChangeConfiguration((e) => {
        if (!e.affectsConfiguration('git.path')) {
          return;
        }
        const changed = configuredGitPath();
        this.vscodeGit = changed ?? api?.git.path ?? 'git';
        setFallbackGit(changed);
        void resendDaemonEnvironment();
      }),
    );
    this.git = GitExecutor.chosen(() => this.vscodeGit, (l) =>
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
    // 設定 `amonhen.daemon.path` があれば、書き込みは共有の daemon のコアで行う
    const repository = new Repository(
      withDaemonWrites(backend, () => daemonFor(backend.root)),
      this.log,
    );
    const entry: Entry = {
      repository,
      apiRepo,
      disposables: [repository],
    };
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
