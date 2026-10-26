/**
 * 手元の共有の daemon（エンジン `amonhen --serve`）の FE 向けの窓口につなぐ。
 * コアは daemon で動き、ここは命令（JSON）を送って、答え（JSON とレコード）を受け取るだけ。
 * 取り決めはリポジトリのルートの `docs/front.md`。
 */
import { execFile, spawn } from 'node:child_process';
import { connect, type Socket } from 'node:net';

/** 窓口の取り決めの版（`amonhen_front::VERSION`） */
export const FRONT_VERSION = 2;

const REQUEST = 1;
const OK = 2;
const ERR = 3;
const NOTIFY = 4;
const CANCEL = 5;

/** つながるまで待つ時間（daemon を起動したとき） */
const CONNECT_TIMEOUT = 5000;

/** 知らせのビット（`amonhen_watch::Changes`） */
export const Changes = { refs: 1, index: 2, worktree: 4, config: 8 } as const;

export type Json = Record<string, unknown>;

/** 答え: JSON と、あればレコード（`json.data` に区画ごとの [始め, 長さ]） */
export class Reply {
  readonly json: Json;
  readonly data: ArrayBuffer;

  constructor(json: Json, data: ArrayBuffer) {
    this.json = json;
    this.data = data;
  }

  /** 中身（JSON の長さ・JSON・8 バイト境界までの詰め物・レコード）から */
  static decode(payload: Buffer): Reply {
    const len = payload.readUInt32LE(0);
    const json = JSON.parse(payload.toString('utf8', 4, 4 + len)) as Json;
    const start = Math.min((4 + len + 7) & ~7, payload.length);
    // 0 始まりの ArrayBuffer に切り出す（型付き配列は揃った位置からしか読めない）
    const data = payload.buffer.slice(payload.byteOffset + start, payload.byteOffset + payload.length) as ArrayBuffer;
    return new Reply(json, data);
  }

  private range(name: string): [number, number] {
    const at = (this.json['data'] as Record<string, [number, number]> | undefined)?.[name];
    return at ?? [0, 0];
  }

  bytes(name: string): Uint8Array {
    const [off, len] = this.range(name);
    return new Uint8Array(this.data, off, len);
  }

  u16(name: string): Uint16Array {
    const [off, len] = this.range(name);
    return new Uint16Array(this.data, off, len / 2);
  }

  u32(name: string): Uint32Array {
    const [off, len] = this.range(name);
    return new Uint32Array(this.data, off, len / 4);
  }

  i64(name: string): BigInt64Array {
    const [off, len] = this.range(name);
    return new BigInt64Array(this.data, off, len / 8);
  }

  f32(name: string): Float32Array {
    const [off, len] = this.range(name);
    return new Float32Array(this.data, off, len / 4);
  }
}

export class DaemonError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'DaemonError';
  }
}

interface Pending {
  resolve: (reply: Reply) => void;
  reject: (err: Error) => void;
}

/** 共有の daemon で開いた 1 つのリポジトリとのつながり */
export class DaemonClient {
  private readonly socket: Socket;
  private readonly pending = new Map<number, Pending>();
  private readonly listeners = new Set<(changes: number) => void>();
  private readonly closeListeners = new Set<(reason: string) => void>();
  private next = 1;
  private buffered: Buffer = Buffer.alloc(0);
  private closedReason: string | undefined;

  private constructor(socket: Socket) {
    this.socket = socket;
    socket.on('data', (chunk: Buffer) => this.receive(chunk));
    socket.on('close', () => this.close('daemon との接続が切れました'));
    socket.on('error', (err) => this.close(`daemon との接続が切れました: ${err.message}`));
  }

  /**
   * `daemon`（amonhend の場所）の共有の daemon で `location`（リポジトリの絶対パスか `ssh://host/path`）を開く。
   * daemon が動いていなければ起動する
   */
  static async open(daemon: string, location: string): Promise<DaemonClient> {
    const path = await socketPath(daemon);
    const socket = await connectOrLaunch(daemon, path);
    const name = Buffer.from(`\u0001${location}\u0000${clientEnv()}`, 'utf8');
    const head = Buffer.alloc(4);
    head.writeUInt32LE(name.length, 0);
    socket.write(Buffer.concat([head, name]));
    const client = new DaemonClient(socket);
    const hello = await client.call({ op: 'hello', language });
    if (hello.json['version'] !== FRONT_VERSION) {
      client.dispose();
      throw new DaemonError(`daemon の窓口の版が違います: ${String(hello.json['version'])}`);
    }
    return client;
  }

  get closed(): string | undefined {
    return this.closedReason;
  }

  /** 命令を送る。signal で取り消すと、daemon にも取り消しを送る（検索など） */
  call(request: Json, signal?: AbortSignal): Promise<Reply> {
    if (this.closedReason) {
      return Promise.reject(new DaemonError(this.closedReason));
    }
    const id = this.next++;
    return new Promise<Reply>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.send(REQUEST, id, Buffer.from(JSON.stringify(request), 'utf8'));
      signal?.addEventListener(
        'abort',
        () => {
          if (this.pending.delete(id)) {
            this.send(CANCEL, id, Buffer.alloc(0));
            reject(new DaemonError('取り消しました'));
          }
        },
        { once: true },
      );
    });
  }

  /** リポジトリの変化（`Changes` のビット）を受け取る */
  onDidChange(listener: (changes: number) => void): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  onDidClose(listener: (reason: string) => void): { dispose(): void } {
    this.closeListeners.add(listener);
    return { dispose: () => this.closeListeners.delete(listener) };
  }

  dispose(): void {
    this.socket.destroy();
    this.close('閉じました');
  }

  private send(kind: number, id: number, payload: Buffer): void {
    const head = Buffer.alloc(13);
    head.writeUInt32LE(9 + payload.length, 0);
    head.writeUInt8(kind, 4);
    head.writeBigUInt64LE(BigInt(id), 5);
    this.socket.write(Buffer.concat([head, payload]));
  }

  private receive(chunk: Buffer): void {
    this.buffered = this.buffered.length === 0 ? chunk : Buffer.concat([this.buffered, chunk]);
    while (this.buffered.length >= 4) {
      const len = this.buffered.readUInt32LE(0);
      if (this.buffered.length < 4 + len) {
        return;
      }
      const kind = this.buffered.readUInt8(4) & 0x7f;
      const id = Number(this.buffered.readBigUInt64LE(5));
      const payload = this.buffered.subarray(13, 4 + len);
      this.buffered = this.buffered.subarray(4 + len);
      this.dispatch(kind, id, payload);
    }
  }

  private dispatch(kind: number, id: number, payload: Buffer): void {
    if (kind === NOTIFY) {
      const changes = Number((JSON.parse(payload.toString('utf8')) as Json)['changes'] ?? 0);
      for (const l of this.listeners) {
        l(changes);
      }
      return;
    }
    const p = this.pending.get(id);
    if (!p) {
      return;
    }
    this.pending.delete(id);
    if (kind === OK) {
      try {
        p.resolve(Reply.decode(payload));
      } catch (err) {
        p.reject(err instanceof Error ? err : new DaemonError(String(err)));
      }
    } else if (kind === ERR) {
      p.reject(new DaemonError(payload.toString('utf8')));
    }
  }

  private close(reason: string): void {
    if (this.closedReason) {
      return;
    }
    this.closedReason = reason;
    for (const p of this.pending.values()) {
      p.reject(new DaemonError(reason));
    }
    this.pending.clear();
    for (const l of this.closeListeners) {
      l(reason);
    }
  }
}

/** daemon のソケットの場所（amonhend 自身に聞く。通信の形の版が名前に入っている） */
function socketPath(daemon: string): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(daemon, ['--socket'], { windowsHide: true }, (err, stdout) => {
      if (err) {
        reject(new DaemonError(`${daemon} を起動できません: ${err.message}`));
      } else {
        resolve(stdout.trim());
      }
    });
  });
}

function tryConnect(path: string): Promise<Socket | undefined> {
  return new Promise((resolve) => {
    const socket = connect(path);
    socket.once('connect', () => resolve(socket));
    socket.once('error', () => resolve(undefined));
  });
}

/**
 * daemon が作る文（エラーなど）の言語（VSCode の表示言語）。拡張を起動したときに決め、つなぐときの `hello` と、
 * daemon を起動するときの `AMONHEN_LANG` で伝える
 */
let language = 'en';

export function setDaemonLanguage(value: string): void {
  language = value;
}

/** つなぐ。daemon がいなければ、拡張と切り離して起動し、つながるまで待つ */
async function connectOrLaunch(daemon: string, path: string): Promise<Socket> {
  const first = await tryConnect(path);
  if (first) {
    return first;
  }
  // Windows では窓を出さない
  spawn(daemon, ['--serve', path], {
    detached: true,
    stdio: 'ignore',
    windowsHide: true,
    env: { ...process.env, AMONHEN_LANG: language },
  }).unref();
  const started = Date.now();
  while (Date.now() - started < CONNECT_TIMEOUT) {
    await new Promise((r) => setTimeout(r, 20));
    const socket = await tryConnect(path);
    if (socket) {
      return socket;
    }
  }
  throw new DaemonError(`daemon（${path}）につなげません`);
}

/**
 * daemon に送る、この拡張の環境（`NAME=value` を NUL で区切ったもの）。共有の daemon は、このつながりで動かす
 * git・ssh をこの環境で動かす（先に起動したアプリの PATH・SSH の agent・`GIT_*` で動かさない）。受け取る名前は
 * daemon が決める（`amonhen_model::process::ClientEnv`）。ここでは git・ssh がいつも使うものと、`AMONHEN_PASS_ENV`
 * で足したものだけに絞って送る
 */
export function clientEnv(env: NodeJS.ProcessEnv = process.env, windows = process.platform === 'win32'): string {
  // Windows の環境変数の名前は大文字と小文字を区別しない（PATH は `Path` で入っている）
  const key = (k: string): string => (windows ? k.toUpperCase() : k);
  const passEnv = Object.entries(env).find(([k]) => key(k) === 'AMONHEN_PASS_ENV')?.[1] ?? '';
  const names = new Set([
    'PATH',
    'SSH_AUTH_SOCK',
    'SSH_AGENT_PID',
    'XDG_CONFIG_HOME',
    'XDG_DATA_HOME',
    'AMONHEN_ASKPASS',
    'AMONHEN_GIT',
    'AMONHEN_PASS_ENV',
    ...passEnv.split(/[\s,]+/).filter((n) => n !== '').map(key),
  ]);
  return Object.entries(env)
    .filter(([k, v]) => v !== undefined && !v.includes('\0') && (names.has(key(k)) || key(k).startsWith('GIT_')))
    .map(([k, v]) => `${k}=${v ?? ''}`)
    .join('\0');
}

/** daemon への指示（`status` なら様子の JSON、`stop` で止める）。動いていなければ undefined */
export async function controlDaemon(daemon: string, command: 'status' | 'stop'): Promise<string | undefined> {
  const socket = await tryConnect(await socketPath(daemon));
  if (!socket) {
    return undefined;
  }
  const name = Buffer.from(`\u0000${command}`, 'utf8');
  const head = Buffer.alloc(4);
  head.writeUInt32LE(name.length, 0);
  socket.write(Buffer.concat([head, name]));
  return new Promise((resolve, reject) => {
    let got = Buffer.alloc(0);
    socket.on('data', (chunk: Buffer) => {
      got = Buffer.concat([got, chunk]);
      if (got.length >= 4 && got.length >= 4 + got.readUInt32LE(0)) {
        socket.destroy();
        const text = got.toString('utf8', 13, 4 + got.readUInt32LE(0));
        if ((got.readUInt8(4) & 0x7f) === OK) {
          resolve(text);
        } else {
          reject(new DaemonError(text));
        }
      }
    });
    socket.on('error', (err) => reject(new DaemonError(err.message)));
  });
}
