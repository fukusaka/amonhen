/**
 * git と ssh が資格情報を尋ねる先（askpass）のスクリプト。拡張が起動のときに置き（`ui/askpass.ts`）、git 実行層は
 * `GIT_ASKPASS`・`SSH_ASKPASS` に、共有の daemon には `AMONHEN_ASKPASS` で渡す。利用者が `AMONHEN_ASKPASS` を
 * 決めていれば、そちらを使う
 */
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer, type Server, type Socket } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { randomBytes } from 'node:crypto';

let script: string | undefined;

export function setAskpass(path: string | undefined): void {
  script = path;
}

/** 使う askpass。なければ undefined（尋ねられたら失敗する） */
export function askpass(env: NodeJS.ProcessEnv = process.env): string | undefined {
  return env.AMONHEN_ASKPASS || script;
}

/** git を動かすときに足す環境。ssh は端末がなくても askpass を使う（`SSH_ASKPASS_REQUIRE`） */
export function askpassEnv(env: NodeJS.ProcessEnv = process.env): Record<string, string> {
  const path = askpass(env);
  return path ? { GIT_ASKPASS: path, SSH_ASKPASS: path, SSH_ASKPASS_REQUIRE: 'prefer' } : {};
}

/** パスワードとパスフレーズは伏せる。ユーザー名と、初めてつなぐホストの鍵の確認は見せる（アプリと同じ） */
export function hiddenAnswer(prompt: string): boolean {
  return !(prompt.toLowerCase().includes('username') || prompt.includes('(yes/no'));
}

/** スクリプトが動かす JS。引数のプロンプトを窓口に送り、答えを出す（答えがなければ 1 で終わる） */
function clientSource(pipe: string): string {
  return `const net = require('net');
const socket = net.connect(${JSON.stringify(pipe)});
const chunks = [];
socket.on('connect', () => socket.write(JSON.stringify({ prompt: process.argv.slice(2).join(' ') }) + '\\n'));
socket.on('data', (c) => chunks.push(c));
socket.on('error', () => process.exit(1));
socket.on('close', () => {
  let answer;
  try { answer = JSON.parse(Buffer.concat(chunks).toString('utf8')).answer; } catch {}
  if (typeof answer !== 'string') process.exit(1);
  process.stdout.write(answer + '\\n', () => process.exit(0));
});
`;
}

/** sh の単一引用符で囲む */
function quote(text: string): string {
  return `'${text.replace(/'/g, `'\\''`)}'`;
}

export class Askpass {
  /** git・ssh に渡すスクリプト */
  readonly path: string;
  private readonly dir: string;
  private readonly server: Server;

  private constructor(dir: string, path: string, server: Server) {
    this.dir = dir;
    this.path = path;
    this.server = server;
  }

  /** 窓口を開き、スクリプトを置く。`ask` は問いの尋ね方（答えなければ undefined） */
  static async start(ask: (prompt: string) => Promise<string | undefined>): Promise<Askpass> {
    // ソケットのパスは長さに上限がある（macOS で 104 バイト）ので、短い名前にする
    const dir = mkdtempSync(join(tmpdir(), 'amonhen-'));
    chmodSync(dir, 0o700);
    const pipe = process.platform === 'win32' ? `\\\\.\\pipe\\amonhen-askpass-${randomBytes(8).toString('hex')}` : join(dir, 's');
    const server = createServer((socket) => serve(socket, ask));
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      server.listen(pipe, () => resolve());
    });
    writeFileSync(join(dir, 'askpass.js'), clientSource(pipe));
    // git for Windows は sh でスクリプトを動かすので、Windows でも sh で書く（パスは / で区切る）
    const slash = (p: string) => (process.platform === 'win32' ? p.replace(/\\/g, '/') : p);
    const path = join(dir, 'askpass.sh');
    writeFileSync(
      path,
      `#!/bin/sh\nELECTRON_RUN_AS_NODE=1 exec ${quote(slash(process.execPath))} ${quote(slash(join(dir, 'askpass.js')))} "$@"\n`,
      { mode: 0o700 },
    );
    return new Askpass(dir, path, server);
  }

  dispose(): void {
    this.server.close();
    rmSync(this.dir, { recursive: true, force: true });
  }
}

/**
 * 1 つの問い（改行で終わる）を受けて、尋ねて、答えを返して閉じる。Windows の名前付きパイプは片側だけ閉じられない
 * （half-close）ので、問いの終わりは改行で知る
 */
function serve(socket: Socket, ask: (prompt: string) => Promise<string | undefined>): void {
  const chunks: Buffer[] = [];
  socket.on('error', () => {});
  const onData = (c: Buffer) => {
    chunks.push(c);
    if (!c.includes(0x0a)) return;
    socket.off('data', onData);
    let prompt = '';
    try {
      const line = Buffer.concat(chunks).toString('utf8').split('\n')[0]!;
      prompt = String((JSON.parse(line) as { prompt?: unknown }).prompt ?? '');
    } catch {
      socket.end();
      return;
    }
    void ask(prompt)
      .catch(() => undefined)
      .then((answer) => socket.end(answer === undefined ? '{}' : JSON.stringify({ answer })));
  };
  socket.on('data', onData);
}
