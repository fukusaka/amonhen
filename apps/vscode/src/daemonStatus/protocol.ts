/**
 * メインビューの Daemon（アプリの Activity の Daemon のタブと同じ）と拡張ホストの間のメッセージと、出す文。
 * vscode に依存しない
 */

/** 手元の共有の daemon の様子（`amonhen daemon status --json` と同じ） */
export interface DaemonStatus {
  pid: number;
  version: string;
  /** ビルドの指紋（同じビルドの FE とエンジンだけがこの daemon を使う） */
  build: string;
  socket: string;
  uptimeMs: number;
  connections: number;
  idleExitMs: number;
  repos: DaemonRepo[];
}

/** daemon が開いているリポジトリ */
export interface DaemonRepo {
  path: string;
  clients: number;
  idleMs: number;
  history: boolean;
  watching: boolean;
  indexRows?: number | null;
  indexBytes?: number | null;
  /** SSH 先のリポジトリなら true（そのつながりの送受信量と、切れていれば理由） */
  ssh?: boolean | null;
  sent?: number | null;
  received?: number | null;
  closed?: string | null;
  /** SSH 先で使っているエンジンと、その用意のしかた */
  engine?: string | null;
}

/** この窓の、daemon とのつながり（リポジトリごと） */
export interface WindowConnection {
  path: string;
  /** 切れていれば、そのわけ */
  closed?: string;
}

export type DaemonHostMessage = {
  type: 'daemonStatus';
  /** daemon を使う設定で、合うエンジンがある */
  enabled: boolean;
  /** 一度は調べた（まだなら「調べています」） */
  checked: boolean;
  /** 動いていなければなし */
  status?: DaemonStatus;
  connections: WindowConnection[];
};

export type DaemonWebviewMessage = { type: 'daemonStart' } | { type: 'daemonStop' };

/** アプリと同じ時間の書き方（`5 s`、`3 min`、`3 min 5 s`、`2 h 10 min`） */
export function duration(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s} s`;
  if (s < 3600) return s % 60 === 0 ? `${s / 60} min` : `${Math.floor(s / 60)} min ${s % 60} s`;
  return `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} min`;
}

/** アプリ（macOS のファイルの大きさの書き方）と同じく 1000 ごとに、KB は整数、MB から上は 1 桁まで */
export function bytes(n: number): string {
  if (n === 0) return 'Zero KB';
  if (n < 1000) return `${n} bytes`;
  if (n < 1e6) return `${Math.round(n / 1e3)} KB`;
  const [value, unit] = n < 1e9 ? [n / 1e6, 'MB'] : [n / 1e9, 'GB'];
  return `${Number(value.toFixed(1))} ${unit}`;
}

/** 動いているときの 2 行目（pid・版・動いている時間・つながりの数） */
export function statusLine(s: DaemonStatus): string {
  return `pid ${s.pid} · version ${s.version} (build ${s.build}) · up ${duration(s.uptimeMs)} · connections ${s.connections}`;
}

/** Open Repositories の SSH の欄（送受信量、切れていれば Disconnected） */
export function sshCell(r: DaemonRepo): string {
  if (!r.ssh) return '—';
  return r.closed ? 'Disconnected' : `${bytes(r.sent ?? 0)} · ${bytes(r.received ?? 0)}`;
}

/** Open Repositories の Connections の欄（なければ、つながりがなくなってからの時間） */
export function clientsCell(r: DaemonRepo): string {
  return r.clients > 0 ? String(r.clients) : `None (for ${duration(r.idleMs)})`;
}

/** Open Repositories の Search Index の欄 */
export function indexCell(r: DaemonRepo): string {
  return r.indexRows == null ? '—' : `${r.indexRows.toLocaleString('en-US')} rows · ${bytes(r.indexBytes ?? 0)}`;
}
