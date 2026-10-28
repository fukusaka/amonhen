/**
 * リポジトリごとの、共有の daemon とのつながり。エンジンは拡張に同梱したものを置き場所に置いて使う（`engine.ts`）。
 * 設定 `amonhen.daemon.path` で別のエンジンを指せ、`amonhen.daemon.enabled` を切ると daemon を使わない（git 実行層
 * だけで動く。合うエンジンがない OS と CPU でも同じ）。切れていれば、次に使うときにつなぎ直す
 */
import * as vscode from 'vscode';
import { DaemonClient } from './client.ts';
import { prepareEngine } from './engine.ts';

const clients = new Map<string, Promise<DaemonClient>>();
let binDir: string | undefined;
let prepared: Promise<string | undefined> | undefined;

/** 拡張の bin の場所を覚え、同梱のエンジンを置き場所に置く（起動時に 1 回） */
export function initDaemon(extensionPath: string, log?: vscode.LogOutputChannel): void {
  binDir = `${extensionPath}/bin`;
  prepared = prepareEngine(binDir).catch((err: unknown) => {
    log?.warn('エンジンを置けません:', err);
    return undefined;
  });
  void prepared.then((path) => log?.info(path ? `daemon のエンジン: ${path}` : 'このホスト向けのエンジンがありません（git 実行層で動きます）'));
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

/** 開いているつながりに、この拡張の環境を送り直す（`git.path` を変えたとき）。送れないつながりは、次に開くときに送る */
export async function resendDaemonEnvironment(): Promise<void> {
  await Promise.all(
    [...clients.values()].map((c) => c.then((client) => (client.closed ? undefined : client.sendEnvironment())).catch(() => undefined)),
  );
}

export function disposeDaemonClients(): void {
  for (const c of clients.values()) {
    void c.then((client) => client.dispose()).catch(() => undefined);
  }
  clients.clear();
}
