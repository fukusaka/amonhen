/**
 * メインビューの Daemon の拡張ホスト側。アプリの Activity の Daemon のタブと同じく、手元の共有の daemon の様子と、
 * この窓の daemon とのつながりを、出している間 2 秒ごとに読み直す。Start・Stop もここから（Stop で切れたこの窓の
 * リポジトリは、アプリの Reconnect と同じく Start でつなぎ直す）
 */
import * as vscode from 'vscode';
import { controlDaemon, startDaemon } from '../daemon/client.ts';
import { daemonConnections, daemonPath } from '../daemon/connection.ts';
import type { DaemonHostMessage, DaemonStatus, DaemonWebviewMessage } from '../daemonStatus/protocol.ts';
import { showError } from './stageCommands.ts';

const INTERVAL = 2000;

export class DaemonController implements vscode.Disposable {
  private readonly post: (message: DaemonHostMessage) => void;
  private readonly reconnect: () => void;
  private timer: NodeJS.Timeout | undefined;
  private checked = false;
  private loading = false;
  /** 最後に読んだ様子（e2e が確かめる） */
  last: DaemonHostMessage | undefined;

  /** reconnect はメインビューの画面を開き直す（切れたつながりを捨て、新しくつなぐ） */
  constructor(post: (message: DaemonHostMessage) => void, reconnect: () => void) {
    this.post = post;
    this.reconnect = reconnect;
  }

  /** メインビューで Daemon を出したか、ほかの画面に替えたか */
  setVisible(visible: boolean): void {
    clearInterval(this.timer);
    this.timer = undefined;
    if (visible) {
      void this.load();
      this.timer = setInterval(() => void this.load(), INTERVAL);
    }
  }

  async handle(message: DaemonWebviewMessage): Promise<void> {
    const daemon = await daemonPath().catch(() => undefined);
    if (!daemon) return;
    try {
      if (message.type === 'daemonStart') {
        await startDaemon(daemon);
        if ((await daemonConnections()).some((c) => c.closed)) this.reconnect();
      } else {
        await controlDaemon(daemon, 'stop');
      }
    } catch (err) {
      showError(err);
    }
    await this.load();
  }

  /** 読み直して送る（読んでいる途中なら何もしない） */
  async load(): Promise<void> {
    if (this.loading) return;
    this.loading = true;
    try {
      const daemon = await daemonPath().catch(() => undefined);
      const text = daemon ? await controlDaemon(daemon, 'status').catch(() => undefined) : undefined;
      this.checked = true;
      this.last = {
        type: 'daemonStatus',
        enabled: daemon !== undefined,
        checked: this.checked,
        status: text ? (JSON.parse(text) as DaemonStatus) : undefined,
        connections: await daemonConnections(),
      };
      this.post(this.last);
    } finally {
      this.loading = false;
    }
  }

  dispose(): void {
    clearInterval(this.timer);
  }
}
