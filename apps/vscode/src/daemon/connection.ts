/**
 * リポジトリごとの、共有の daemon とのつながり。エンジンは拡張に同梱したものを置き場所に置いて使う（`engine.ts`）。
 * 設定 `amonhen.daemon.path` で別のエンジンを指せ、`amonhen.daemon.enabled` を切ると daemon を使わない（git 実行層
 * だけで動く。合うエンジンがない OS と CPU でも同じ）。切れていれば、次に使うときにつなぎ直す
 */
import * as vscode from 'vscode';
import { DaemonClient } from './client.ts';
import { prepareEngine } from './engine.ts';
import { bundledRender, renderNextTo } from './render.ts';

const clients = new Map<string, Promise<DaemonClient>>();
let binDir: string | undefined;
let prepared: Promise<string | undefined> | undefined;

/** 拡張の bin の場所を覚え、同梱のエンジンを置き場所に置く（起動時に 1 回） */
export function initDaemon(extensionPath: string, log?: vscode.LogOutputChannel): void {
  binDir = `${extensionPath}/bin`;
  prepared = prepareEngine(binDir).catch((err: unknown) => {
    log?.warn('Can’t install the engine:', err);
    return undefined;
  });
  void prepared.then((path) => log?.info(path ? `Daemon engine: ${path}` : 'No engine for this host (running on the git execution layer)'));
}

/** daemon に使うエンジンの場所。使わない設定か、エンジンがなければ undefined */
export async function daemonPath(): Promise<string | undefined> {
  const config = vscode.workspace.getConfiguration('amonhen');
  if (!config.get<boolean>('daemon.enabled', true)) {
    return undefined;
  }
  const path = config.get<string>('daemon.path')?.trim();
  if (path) {
    return path;
  }
  return prepared;
}

/**
 * 絵の手伝い（`amonhen-render`。Markdown の図・数式などを描く）の場所。エンジンと同じく拡張に同梱したもので、
 * Remote-SSH では SSH 先で動く。設定 `amonhen.daemon.path` でエンジンを指していれば、その隣のもの（開発中の
 * ビルド）。daemon を使わない設定か、手伝いがなければ undefined（図・数式はコードのまま）
 */
export async function renderPath(): Promise<string | undefined> {
  const config = vscode.workspace.getConfiguration('amonhen');
  if (!config.get<boolean>('daemon.enabled', true)) {
    return undefined;
  }
  const path = config.get<string>('daemon.path')?.trim();
  if (path) {
    return renderNextTo(path);
  }
  return binDir ? bundledRender(binDir) : undefined;
}

/** `root` のリポジトリを daemon で開いたつながり。daemon を使わないなら undefined */
export async function daemonFor(root: string): Promise<DaemonClient | undefined> {
  const daemon = await daemonPath();
  if (!daemon) {
    return undefined;
  }
  const key = `${daemon}\n${root}`;
  let client = clients.get(key);
  if (client) {
    const c = await client.catch(() => undefined);
    if (c && !c.closed) {
      return c;
    }
  }
  client = DaemonClient.open(daemon, root);
  clients.set(key, client);
  client.catch(() => clients.delete(key));
  return client;
}

/** この窓の、daemon とのつながり（開き終えたものだけ。送受信量と、切れていれば、そのわけ） */
export async function daemonConnections(): Promise<{ path: string; sent: number; received: number; closed?: string }[]> {
  const list = await Promise.all(
    [...clients].map(async ([key, client]) => {
      const c = await client.catch(() => undefined);
      return c && { path: key.slice(key.indexOf('\n') + 1), ...c.traffic, ...(c.closed ? { closed: c.closed } : {}) };
    }),
  );
  return list.filter((c) => c !== undefined);
}

/** 開いているつながりに、この拡張の環境を送り直す（`git.path` を変えたとき）。送れないつながりは、次に開くときに送る */
export async function resendDaemonEnvironment(): Promise<void> {
  await Promise.all(
    [...clients.values()].map((c) => c.then((client) => (client.closed ? undefined : client.sendEnvironment())).catch(() => undefined)),
  );
}

/**
 * `root` のリポジトリへのつながりを閉じる（worktree を消す前に。Windows では、使っているフォルダーを消せない）。
 * 次に頼まれたら、また開く
 */
export function closeDaemonClient(root: string): void {
  const fold = (p: string) => (process.platform === 'win32' ? p.toLowerCase() : p);
  for (const [key, client] of clients) {
    if (fold(key.slice(key.indexOf('\n') + 1)) !== fold(root)) continue;
    clients.delete(key);
    void client.then((c) => c.dispose()).catch(() => undefined);
  }
}

export function disposeDaemonClients(): void {
  for (const c of clients.values()) {
    void c.then((client) => client.dispose()).catch(() => undefined);
  }
  clients.clear();
}
