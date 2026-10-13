import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';

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
 * オブジェクトを取得するたびに spawn しないためのもの。要求は送った順に応答が返る。
 */
export class CatFileBatch {
  private readonly gitPath: string;
  private readonly cwd: string;
  private child: ChildProcessWithoutNullStreams | undefined;
  private readonly queue: Request[] = [];
  private chunks: Buffer[] = [];
  private length = 0;
  /** 次の応答を処理するのに最低限必要なバイト数 */
  private need = 1;

  constructor(gitPath: string, cwd: string) {
    this.gitPath = gitPath;
    this.cwd = cwd;
  }

  info(rev: string): Promise<ObjectInfo | undefined> {
    return this.send('info', rev) as Promise<ObjectInfo | undefined>;
  }

  contents(rev: string): Promise<GitObject | undefined> {
    return this.send('contents', rev) as Promise<GitObject | undefined>;
  }

  dispose(): void {
    this.child?.kill();
    this.child = undefined;
  }

  private send(kind: Request['kind'], rev: string): Promise<GitObject | ObjectInfo | undefined> {
    if (/[\n\0]/.test(rev)) {
      return Promise.reject(new Error(`invalid revision: ${JSON.stringify(rev)}`));
    }
    return new Promise((resolve, reject) => {
      const child = this.ensureChild();
      this.queue.push({ kind, resolve, reject });
      child.stdin.write(`${kind} ${rev}\n`);
    });
  }

  private ensureChild(): ChildProcessWithoutNullStreams {
    if (this.child) {
      return this.child;
    }
    const child = spawn(this.gitPath, ['--no-optional-locks', 'cat-file', '--batch-command'], {
      cwd: this.cwd,
      env: { ...process.env, GIT_TERMINAL_PROMPT: '0' },
    });
    child.stdout.on('data', (c: Buffer) => this.onData(c));
    child.stdin.on('error', () => {});
    const fail = (err: Error) => {
      if (this.child !== child) {
        return;
      }
      this.child = undefined;
      this.chunks = [];
      this.length = 0;
      this.need = 1;
      for (const req of this.queue.splice(0)) {
        req.reject(err);
      }
    };
    child.on('error', fail);
    child.on('close', (code) => fail(new Error(`git cat-file exited with ${code}`)));
    this.child = child;
    return child;
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
