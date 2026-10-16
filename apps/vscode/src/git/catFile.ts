import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { stat } from 'node:fs/promises';

export interface ObjectInfo {
  oid: string;
  type: string;
  size: number;
}

export interface GitObject extends ObjectInfo {
  data: Buffer;
}

interface Request {
  kind: 'info' | 'contents';
  resolve: (value: GitObject | ObjectInfo | undefined) => void;
  reject: (err: Error) => void;
}

/**
 * 常駐させた `git cat-file --batch-command`。
 * オブジェクトを取得するたびに spawn しないためのもの。
 *
 * cat-file は index を最初の 1 回しか読まない（ref は毎回読み直す）。
 * そのため `:path` の問い合わせの前に index の更新時刻を確かめ、変わっていれば起動し直す。
 */
export class CatFileBatch {
  private readonly gitPath: string;
  private readonly cwd: string;
  private readonly indexPath: string;
  private process: CatFileProcess | undefined;

  constructor(gitPath: string, cwd: string, indexPath: string) {
    this.gitPath = gitPath;
    this.cwd = cwd;
    this.indexPath = indexPath;
  }

  async info(rev: string): Promise<ObjectInfo | undefined> {
    return (await this.send('info', rev)) as ObjectInfo | undefined;
  }

  async contents(rev: string): Promise<GitObject | undefined> {
    return (await this.send('contents', rev)) as GitObject | undefined;
  }

  dispose(): void {
    this.process?.kill();
    this.process = undefined;
  }

  private async send(kind: Request['kind'], rev: string): Promise<GitObject | ObjectInfo | undefined> {
    if (/[\n\0]/.test(rev)) {
      throw new Error(`invalid revision: ${JSON.stringify(rev)}`);
    }
    const readsIndex = rev.startsWith(':');
    let proc = this.process;
    if (!proc?.alive || readsIndex) {
      const stamp = await indexStamp(this.indexPath);
      // 待っている間に、同時に来た別の要求が起動し直しているかもしれない
      proc = this.process;
      if (proc?.alive && readsIndex && proc.indexStamp !== stamp) {
        // 受け付け済みの問い合わせに答え終えてから終了させる
        proc.end();
      }
      if (!proc?.alive) {
        proc = this.start(stamp);
      }
    }
    return proc!.send(kind, rev);
  }

  private start(stamp: string): CatFileProcess {
    const proc = new CatFileProcess(this.gitPath, this.cwd, stamp);
    this.process = proc;
    return proc;
  }
}

async function indexStamp(path: string): Promise<string> {
  try {
    const s = await stat(path, { bigint: true });
    return `${s.mtimeNs}:${s.size}:${s.ino}`;
  } catch {
    return '-';
  }
}

/** 1 つの cat-file プロセス。要求は送った順に応答が返る */
class CatFileProcess {
  /** 起動前に見た index の更新時刻 */
  readonly indexStamp: string;
  alive = true;
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly queue: Request[] = [];
  private chunks: Buffer[] = [];
  private length = 0;
  /** 次の応答を処理するのに最低限必要なバイト数 */
  private need = 1;

  constructor(gitPath: string, cwd: string, indexStamp: string) {
    this.indexStamp = indexStamp;
    this.child = spawn(gitPath, ['--no-optional-locks', 'cat-file', '--batch-command'], {
      cwd,
      env: { ...process.env, GIT_TERMINAL_PROMPT: '0' },
    });
    this.child.stdout.on('data', (c: Buffer) => this.onData(c));
    this.child.stdin.on('error', () => {});
    this.child.on('error', (err) => this.fail(err));
    this.child.on('close', (code) => this.fail(new Error(`git cat-file exited with ${code}`)));
  }

  send(kind: Request['kind'], rev: string): Promise<GitObject | ObjectInfo | undefined> {
    if (!this.alive) {
      return Promise.reject(new Error('git cat-file is not running'));
    }
    return new Promise((resolve, reject) => {
      this.queue.push({ kind, resolve, reject });
      this.child.stdin.write(`${kind} ${rev}\n`);
    });
  }

  /** 新しい要求は受け付けず、残りに答えたら終了する */
  end(): void {
    this.alive = false;
    this.child.stdin.end();
  }

  kill(): void {
    this.alive = false;
    this.child.kill();
  }

  private fail(err: Error): void {
    this.alive = false;
    for (const req of this.queue.splice(0)) {
      req.reject(err);
    }
  }

  private onData(chunk: Buffer): void {
    this.chunks.push(chunk);
    this.length += chunk.length;
    if (this.length < this.need) {
      return;
    }
    let buf = this.chunks.length === 1 ? chunk : Buffer.concat(this.chunks, this.length);
    let pos = 0;
    this.need = 1;
    while (this.queue.length > 0) {
      const nl = buf.indexOf(0x0a, pos);
      if (nl < 0) {
        break;
      }
      const header = buf.toString('utf8', pos, nl);
      const req = this.queue[0]!;
      if (header.endsWith(' missing') || header.endsWith(' ambiguous')) {
        this.queue.shift();
        pos = nl + 1;
        req.resolve(undefined);
        continue;
      }
      const [oid = '', type = '', sizeText = ''] = header.split(' ');
      const size = Number(sizeText);
      if (req.kind === 'info') {
        this.queue.shift();
        pos = nl + 1;
        req.resolve({ oid, type, size });
        continue;
      }
      // contents: ヘッダー、本体、末尾の LF
      const end = nl + 1 + size + 1;
      if (buf.length < end) {
        this.need = end - pos;
        break;
      }
      this.queue.shift();
      const data = Buffer.from(buf.subarray(nl + 1, nl + 1 + size));
      pos = end;
      req.resolve({ oid, type, size, data });
    }
    buf = buf.subarray(pos);
    this.chunks = buf.length > 0 ? [buf] : [];
    this.length = buf.length;
  }
}
