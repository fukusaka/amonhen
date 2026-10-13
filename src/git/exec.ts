import { spawn } from 'node:child_process';

export interface ExecOptions {
  cwd: string;
  /** 読み取り系のコマンド。`--no-optional-locks` を付けてユーザーの git 操作とロックを奪い合わない */
  readOnly?: boolean;
  input?: string | Buffer;
  env?: Record<string, string>;
  /** 0 以外で成功扱いにする終了コード */
  okCodes?: readonly number[];
}

export interface ExecLog {
  args: readonly string[];
  ms: number;
  exitCode: number | null;
}

export class GitError extends Error {
  readonly args: readonly string[];
  readonly exitCode: number | null;
  readonly stderr: string;

  constructor(args: readonly string[], exitCode: number | null, stderr: string) {
    super(stderr.trim() || `git ${args.join(' ')} exited with ${exitCode}`);
    this.name = 'GitError';
    this.args = args;
    this.exitCode = exitCode;
    this.stderr = stderr;
  }
}

const BASE_ENV = {
  GIT_TERMINAL_PROMPT: '0',
  // エディタを待って止まらないようにする。メッセージが必要な操作は引数で渡す
  GIT_EDITOR: 'true',
};

export class GitExecutor {
  readonly path: string;
  private readonly onLog: ((log: ExecLog) => void) | undefined;

  constructor(path: string, onLog?: (log: ExecLog) => void) {
    this.path = path;
    this.onLog = onLog;
  }

  exec(args: readonly string[], opts: ExecOptions): Promise<Buffer> {
    const fullArgs = opts.readOnly ? ['--no-optional-locks', ...args] : [...args];
    const start = performance.now();
    return new Promise((resolve, reject) => {
      const child = spawn(this.path, fullArgs, {
        cwd: opts.cwd,
        env: { ...process.env, ...BASE_ENV, ...opts.env },
      });
      const out: Buffer[] = [];
      const err: Buffer[] = [];
      child.stdout.on('data', (c: Buffer) => out.push(c));
      child.stderr.on('data', (c: Buffer) => err.push(c));
      child.on('error', reject);
      child.on('close', (code) => {
        this.onLog?.({ args: fullArgs, ms: performance.now() - start, exitCode: code });
        if (code === 0 || (code !== null && opts.okCodes?.includes(code))) {
          resolve(Buffer.concat(out));
        } else {
          reject(new GitError(fullArgs, code, Buffer.concat(err).toString('utf8')));
        }
      });
      // git が stdin を読まずに終了した場合の EPIPE を無視する
      child.stdin.on('error', () => {});
      child.stdin.end(opts.input);
    });
  }

  async text(args: readonly string[], opts: ExecOptions): Promise<string> {
    return (await this.exec(args, opts)).toString('utf8');
  }
}
