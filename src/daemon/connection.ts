/**
 * リポジトリごとの、共有の daemon とのつながり。設定 `amonhen.daemon.path`（amonhend の場所）があるときだけ使う。
 * 切れていれば、次に使うときにつなぎ直す
 */
import * as vscode from 'vscode';
import { DaemonClient } from './client.ts';

const clients = new Map<string, Promise<DaemonClient>>();

/** amonhend の場所。設定がなければ undefined（daemon を使わない） */
export function daemonPath(): string | undefined {
  const path = vscode.workspace.getConfiguration('amonhen').get<string>('daemon.path')?.trim();
  return path ? path : undefined;
}

/** `root` のリポジトリを daemon で開いたつながり。daemon を使わない設定なら undefined */
export async function daemonFor(root: string): Promise<DaemonClient | undefined> {
  const daemon = daemonPath();
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

export function disposeDaemonClients(): void {
  for (const c of clients.values()) {
    void c.then((client) => client.dispose()).catch(() => undefined);
  }
  clients.clear();
}
