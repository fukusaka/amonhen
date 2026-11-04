/**
 * リポジトリの形を変える取得（アプリの shallow clone の帯と、Fetching and Folders の取得）。shallow clone の古い
 * 履歴を取り足す・すべて取る、partial clone で取らずにおいた中身をすべて取る。共有の daemon のコアで行う
 * （エンジンがなければメニューに出さない）。どれも Undo には記録しない（Fetch と同じ）
 */
import * as vscode from 'vscode';
import type { Json } from '../daemon/client.ts';
import { daemonFor } from '../daemon/connection.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import { MainPanel } from './mainPanel.ts';
import { showError } from './stageCommands.ts';

async function run(repo: Repository, command: Json, title: string): Promise<void> {
  const client = await daemonFor(repo.root);
  if (!client) throw new Error(`${title} needs the Amon Hen engine (amonhen.daemon.enabled)`);
  const reply = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title }, async () =>
    (await client.call({ op: 'run', command })).json,
  );
  if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? `${title} failed`));
  // ref は変わらないので、daemon は覚えている History（境目のある）を使い続ける。起点を決めずに開くと、shallow の
  // 境目が変わったことに気づいて読み直す（今のブランチの History もそこから作る）
  const history = (await client.call({ op: 'history' })).json['history'];
  await client.call({ op: 'historyRelease', history }).catch(() => undefined);
}

/** 取り足す数（アプリの Fetch More の選択肢） */
const COUNTS = [100, 1_000, 10_000];

export function registerFormCommands(manager: RepositoryManager): vscode.Disposable {
  const command = (id: string, fn: (repo: Repository) => Promise<void>) =>
    vscode.commands.registerCommand(id, async () => {
      const repo = manager.current;
      if (!repo) return;
      try {
        await fn(repo);
      } catch (err) {
        showError(err);
      } finally {
        await repo.refresh();
        // 取った履歴を History に出す（ref が変わらないと、History は読み直さない）
        await MainPanel.active?.history.reset();
      }
    });

  return vscode.Disposable.from(
    command('amonhen.fetchMoreHistory', async (repo) => {
      const picked = await vscode.window.showQuickPick(
        [
          ...COUNTS.map((n) => ({ label: `${n.toLocaleString('en')} Commits`, commits: n as number | undefined })),
          { label: 'Other…', commits: undefined, other: true },
          { label: 'Up to a Date…', commits: undefined, since: true },
        ],
        { title: 'Fetch More History', placeHolder: 'How many older commits to fetch (git fetch --deepen)' },
      );
      if (!picked) return;
      if ('since' in picked) {
        const date = await vscode.window.showInputBox({
          title: 'Fetch History Up to a Date',
          prompt: 'Fetches the commits made after this date (git fetch --shallow-since)',
          placeHolder: '2024-01-01',
          validateInput: (v) => (v.trim() ? undefined : 'Enter a date'),
        });
        if (date === undefined) return;
        await run(repo, { op: 'deepen', commits: null, since: date.trim() }, 'Fetch history');
        return;
      }
      let commits = picked.commits;
      if (commits === undefined) {
        const text = await vscode.window.showInputBox({
          title: 'Fetch More History',
          prompt: 'How many older commits to fetch (git fetch --deepen)',
          validateInput: (v) => (/^[1-9]\d*$/.test(v.trim()) ? undefined : 'Enter a number of commits'),
        });
        if (text === undefined) return;
        commits = Number(text.trim());
      }
      await run(repo, { op: 'deepen', commits, since: null }, `Fetch ${commits.toLocaleString('en')} more commits`);
    }),
    command('amonhen.fetchAllHistory', (repo) => run(repo, { op: 'unshallow' }, 'Fetch all history')),
    // partial clone の中身をすべて取る（ダウンロードが大きくなりうるので確かめる。git 2.36 から）
    command('amonhen.fetchAllContents', async (repo) => {
      const answer = await vscode.window.showWarningMessage(
        'Fetch all contents of this partial clone?',
        {
          modal: true,
          detail:
            'Downloads the contents of every file in the history from the remote, and stops fetching them only when needed (git fetch --refetch). It can be large.',
        },
        'Fetch All',
      );
      if (answer !== 'Fetch All') return;
      await run(repo, { op: 'fetchAllContents' }, 'Fetch all contents');
    }),
  );
}
