import { spawn, type ChildProcess } from 'node:child_process';
import { chooseGit, type GitChoice } from './choice.ts';

export interface ExecOptions {
  cwd: string;
  /** 読み取り系のコマンド。`--no-optional-locks` を付けてユーザーの git 操作とロックを奪い合わない */
  readOnly?: boolean;
  input?: string | Buffer;
  env?: Record<string, string>;
  /** 0 以外で成功扱いにする終了コード */
  okCodes?: readonly number[];
  /** 成功したときの標準エラー出力を受け取る（hooks の出力など） */
  onStderr?: (text: string) => void;
  /**
   * ネットワークを使う git（fetch・push）。`--progress` を付け、何も出さないまま silenceMs（既定 120 秒）経てば止める
   * （たいていは誰にも見えないところでの認証の入力待ち。コアと同じ）
   */
  network?: boolean;
  silenceMs?: number;
}

/** ネットワークを使う git が、これだけ何も出さなければ止める（`--progress` なので、動いていれば数秒ごとに出す） */
export const NETWORK_SILENCE_MS = 120_000;

/** git と、git が起動した ssh なども止める */
function killTree(child: ChildProcess): void {
  if (child.pid === undefined) return;
  if (process.platform === 'win32') {
    spawn('taskkill', ['/pid', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' });
  } else {
    try {
      process.kill(-child.pid, 'SIGKILL');
    } catch {
      child.kill('SIGKILL');
    }
  }
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
  private readonly choose: () => GitChoice;
  private readonly onLog: ((log: ExecLog) => void) | undefined;

  /** `path` の git を使う。関数なら、起動のたびにそれで決める（`GitExecutor.chosen`） */
  constructor(path: string | (() => GitChoice), onLog?: (log: ExecLog) => void) {
    this.choose = typeof path === 'string' ? () => ({ path, source: 'vscode' }) : path;
    this.onLog = onLog;
  }

  /**
   * Amon Hen の設定に従う git（`chooseGit`。アプリで選び直せば、次の起動から替わる）。`vscodeGit` は、設定で
   * 決めていないときの VSCode の git（関数なら、起動のたびに読む。`git.path` を変えればすぐ替わる）
   */
  static chosen(vscodeGit: string | (() => string), onLog?: (log: ExecLog) => void): GitExecutor {
    const current = typeof vscodeGit === 'string' ? () => vscodeGit : vscodeGit;
    return new GitExecutor(() => chooseGit(current()), onLog);
  }

  /** 今使う git */
  get path(): string {
    return this.choose().path;
  }

  exec(args: readonly string[], opts: ExecOptions): Promise<Buffer> {
    // --progress はサブコマンドのすぐ後に（後ろだと `--` の後のリモートの名前になりうる）
    const withProgress = opts.network && args.length > 0 ? [args[0]!, '--progress', ...args.slice(1)] : [...args];
    const fullArgs = opts.readOnly ? ['--no-optional-locks', ...withProgress] : withProgress;
    const start = performance.now();
    const git = this.choose();
    if (git.problem) {
      // 選んだ git がなければ、ほかの git に黙って戻さずに止める
      return Promise.reject(new GitError(fullArgs, null, git.problem));
    }
    return new Promise((resolve, reject) => {
      const child = spawn(git.path, fullArgs, {
        cwd: opts.cwd,
        env: { ...process.env, ...BASE_ENV, ...opts.env },
        // Windows でコンソールの窓を出さない
        windowsHide: true,
        // 止めるときに git が起動した ssh なども止められるよう、別のプロセスグループにする
        detached: !!opts.network && process.platform !== 'win32',
      });
      const out: Buffer[] = [];
      const err: Buffer[] = [];
      let heard = performance.now();
      let stopped: number | undefined;
      const limit = opts.silenceMs ?? NETWORK_SILENCE_MS;
      const watch = opts.network
        ? setInterval(() => {
            if (performance.now() - heard <= limit) return;
            stopped = limit;
            clearInterval(watch);
            killTree(child);
          }, Math.min(1000, limit / 4))
        : undefined;
      child.stdout.on('data', (c: Buffer) => {
        heard = performance.now();
        out.push(c);
      });
      child.stderr.on('data', (c: Buffer) => {
        heard = performance.now();
        err.push(c);
      });
      child.on('error', (e) => {
        clearInterval(watch);
        reject(e);
      });
      child.on('close', (code) => {
        clearInterval(watch);
        this.onLog?.({ args: fullArgs, ms: performance.now() - start, exitCode: code });
        if (stopped !== undefined) {
          reject(
            new GitError(
              fullArgs,
              code,
              `Stopped after ${Math.round(stopped / 1000)} seconds without a response. It may have been waiting for a sign-in (a password or passphrase, or the SSH agent) or for the network.`,
            ),
          );
          return;
        }
        if (code === 0 || (code !== null && opts.okCodes?.includes(code))) {
          opts.onStderr?.(Buffer.concat(err).toString('utf8'));
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

/** sh に渡す文字列を単一引用符で囲む */
export function shellQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}
