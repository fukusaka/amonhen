/**
 * 絵の手伝い（`amonhen-render`）。Markdown の Mermaid・DOT の図と数式、Android の VectorDrawable の絵、SVG のモバイル
 * 向けの確認を作る、エンジンとは別の実行ファイル（絵の機能はエンジンに入れない）。見せるファイルごとに起動し、
 * 標準入力と標準出力で JSON の行をやりとりする。手伝いはリポジトリを読まないので、同じコミットのほかのファイル
 * （DOT の図の画像・VectorDrawable の色の資源）を求める行が来たら、`RenderFiles` で読んで答える。取り決めは
 * リポジトリのルートの `docs/front.md`（The drawing helper）。vscode に依存しない
 */
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { access, chmod } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { createInterface } from 'node:readline';
import { platformName } from './engine.ts';

export type Json = Record<string, unknown>;

/** 手伝いが求める、同じコミットのファイル（パスはリポジトリの根から） */
export interface RenderFiles {
  /** ファイルの中身（なければ undefined） */
  read(path: string): Promise<Uint8Array | undefined>;
  /** フォルダ（根は空）の中身 */
  list(folder: string): Promise<{ name: string; folder: boolean }[]>;
}

const exe = process.platform === 'win32' ? '.exe' : '';

/** 拡張の bin に同梱した、このホスト向けの手伝い（なければ undefined） */
export async function bundledRender(binDir: string): Promise<string | undefined> {
  const name = platformName();
  if (!name) return undefined;
  const path = join(binDir, `amonhen-render-${name}${exe}`);
  try {
    await access(path);
    // .vsix を展開すると実行の権限が落ちることがある
    await chmod(path, 0o755);
    return path;
  } catch {
    return undefined;
  }
}

/** 設定で指したエンジン（開発中のビルド）の隣の手伝い（`cargo build -p amonhen-render` したもの） */
export async function renderNextTo(engine: string): Promise<string | undefined> {
  const path = join(dirname(engine), `amonhen-render${exe}`);
  try {
    await access(path);
    return path;
  } catch {
    return undefined;
  }
}

/** 手伝いの 1 回の起動（1 つのファイルの絵を作る間）。求めはまとめて送ってよい */
export class RenderSession {
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly pending = new Map<number, { resolve: (reply: Json) => void; reject: (err: Error) => void }>();
  private next = 0;
  private failed: Error | undefined;
  private readonly timer: NodeJS.Timeout;
  private readonly files: RenderFiles;

  /**
   * `language` は手伝いが書く文（描けないわけ・絵の下の説明）の言語。`timeout` ミリ秒たっても終わらなければ止める
   * （描けなかった絵はコードのまま）
   */
  constructor(program: string, files: RenderFiles, options: { language?: string; timeout?: number } = {}) {
    this.files = files;
    // Windows では窓を出さない
    this.child = spawn(program, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
      ...(options.language ? { env: { ...process.env, AMONHEN_LANG: options.language } } : {}),
    });
    this.child.stdin.on('error', () => undefined);
    this.child.stderr.resume();
    this.child.on('error', (err) => this.fail(err));
    this.child.on('exit', () => this.fail(new Error('The drawing helper ended')));
    createInterface({ input: this.child.stdout }).on('line', (line) => this.receive(line));
    this.timer = setTimeout(() => {
      this.fail(new Error('The drawing helper took too long'));
      this.child.kill();
    }, options.timeout ?? 60_000);
    this.timer.unref();
  }

  /** 求め `op` を送って答えを待つ（`ok` でなければ、そのわけで失敗する） */
  request(op: string, fields: Json): Promise<Json> {
    if (this.failed) return Promise.reject(this.failed);
    const id = ++this.next;
    return new Promise<Json>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.write({ ...fields, id, op });
    });
  }

  /** 入力を閉じる（手伝いは残りの求めに答えてから終わる） */
  close(): void {
    clearTimeout(this.timer);
    this.child.stdin.end();
  }

  private write(message: Json): void {
    if (!this.child.stdin.writable) return;
    this.child.stdin.write(`${JSON.stringify(message)}\n`);
  }

  private receive(line: string): void {
    let message: Json;
    try {
      message = JSON.parse(line) as Json;
    } catch {
      return;
    }
    if (message['ask'] !== undefined) {
      void this.answer(message);
      return;
    }
    const id = message['id'];
    const waiting = typeof id === 'number' ? this.pending.get(id) : undefined;
    if (!waiting) return;
    this.pending.delete(id as number);
    if (message['ok'] === true) waiting.resolve(message);
    else waiting.reject(new Error(String(message['message'] ?? 'The drawing helper failed')));
  }

  /** 同じコミットのファイル・フォルダの求めに答える（読めなければ、ないものとして） */
  private async answer(message: Json): Promise<void> {
    const answer = message['ask'];
    if (typeof message['read'] === 'string') {
      const data = await this.files.read(message['read']).catch(() => undefined);
      this.write({ answer, data: data ? Buffer.from(data).toString('base64') : null });
    } else {
      const folder = typeof message['list'] === 'string' ? message['list'] : '';
      const entries = await this.files.list(folder).catch(() => []);
      this.write({ answer, entries });
    }
  }

  private fail(err: Error): void {
    clearTimeout(this.timer);
    this.failed ??= err;
    for (const waiting of this.pending.values()) waiting.reject(this.failed);
    this.pending.clear();
  }
}
