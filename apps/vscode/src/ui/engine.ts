/** 共有の daemon のコアの命令を、進み具合を出して実行する（エンジンがなければ投げる） */
import * as vscode from 'vscode';
import type { Json } from '../daemon/client.ts';
import { daemonFor } from '../daemon/connection.ts';
import type { Repository } from '../state/repository.ts';

/** コアの命令を実行し、答え（`info` など）を返す。うまくいかなければ投げる */
export async function runEngine(repo: Repository, command: Json, title: string): Promise<Json> {
  const client = await daemonFor(repo.root);
  if (!client) throw new Error(`${title} needs the Amon Hen engine (amonhen.daemon.enabled)`);
  const reply = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title }, async () =>
    (await client.call({ op: 'run', command })).json,
  );
  if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `${title} failed`));
  return reply;
}
