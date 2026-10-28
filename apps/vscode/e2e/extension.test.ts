/// <reference types="mocha" />
/**
 * VSCode を起動して、拡張を実際に動かす結合テスト（`npm run test:e2e`。e2e/run.mjs が一時ディレクトリにリポジトリと
 * エンジンの置き場所を用意し、そのリポジトリを開いた VSCode の拡張ホストで走る）。
 * メインビューの Webview が送るメッセージを拡張ホストに直接入れ、拡張ホストが Webview に送るメッセージと git の様子を
 * 確かめる。テストは上から順に、前のテストの結果を引き継いで進む
 */
import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import * as vscode from 'vscode';
import { DaemonClient } from '../src/daemon/client.ts';
import type { SelectedLine } from '../src/diff/fileDiff.ts';
import { settingsFile } from '../src/git/choice.ts';
import type { activate } from '../src/extension.ts';
import type { HostMessage } from '../src/view/protocol.ts';

type Api = Awaited<ReturnType<typeof activate>>;
type Message<T extends HostMessage['type']> = Extract<HostMessage, { type: T }>;

/** 拡張が起動した子プロセス */
interface Spawned {
  args: readonly string[];
  options: childProcess.SpawnOptions;
  child: childProcess.ChildProcess;
}

const FILE = 'crlf.txt';
const ext = vscode.extensions.getExtension<Api>('fukusaka.amonhen')!;
const root = vscode.workspace.workspaceFolders![0]!.uri.fsPath;
const spawned: Spawned[] = [];
const posted: HostMessage[] = [];
let api: Api;

function git(...args: string[]): string {
  return childProcess.execFileSync('git', args, { cwd: root, encoding: 'utf8' });
}

/** fn が値を返すまで待つ */
async function until<T>(what: string, fn: () => T | undefined | false | Promise<T | undefined | false>, ms = 20_000): Promise<T> {
  const start = Date.now();
  for (;;) {
    const value = await fn();
    if (value) {
      return value;
    }
    if (Date.now() - start > ms) {
      throw new Error(`待っても来ません: ${what}`);
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

/** since 番目より後に Webview に送られた type のメッセージで、match に合う最初のもの */
function waitPosted<T extends HostMessage['type']>(
  type: T,
  since: number,
  match: (m: Message<T>) => boolean = () => true,
): Promise<Message<T>> {
  return until(`${type} のメッセージ`, () =>
    posted.slice(since).find((m): m is Message<T> => m.type === type && match(m as Message<T>)),
  );
}

/**
 * 拡張が spawn・execFile で起動した子プロセスを記録する（呼び出し元が拡張の dist/extension.js のものだけ）。
 * 拡張は child_process の関数を呼ぶたびに引くので、モジュールの関数を差し替えれば拡張の呼び出しも通る
 */
function recordSpawns(): void {
  // Windows ではドライブ文字の大文字小文字が extensionPath とスタックで違うことがあるので、そろえて比べる
  const normalize = (path: string) => path.replaceAll('\\', '/').toLowerCase();
  const own = normalize(join(ext.extensionPath, 'dist', 'extension.js'));
  const cp = childProcess as unknown as Record<string, (...a: unknown[]) => childProcess.ChildProcess>;
  for (const name of ['spawn', 'execFile']) {
    const original = cp[name]!;
    cp[name] = function (this: unknown, ...a: unknown[]) {
      const child = original.apply(this, a);
      if (normalize(new Error().stack ?? '').includes(own)) {
        const args = Array.isArray(a[1]) ? (a[1] as string[]) : [];
        const options = (a.slice(1).find((x) => x && typeof x === 'object' && !Array.isArray(x)) ?? {}) as childProcess.SpawnOptions;
        spawned.push({ args, options, child });
      }
      return child;
    };
  }
}

async function showView(command: 'amonhen.showWorkingCopy' | 'amonhen.showHistory') {
  const start = Date.now();
  await vscode.commands.executeCommand(command);
  const panel = await until('メインビュー', () => api.panel());
  // Webview に送るメッセージを記録する（コントローラーは panel.post を呼ぶたびに引く）
  if (!Object.hasOwn(panel, 'post')) {
    const post = panel.post.bind(panel);
    panel.post = (m) => {
      if (posted.length === 0) {
        console.log(`      Webview の準備: ${((Date.now() - start) / 1000).toFixed(1)}s`);
      }
      posted.push(m);
      post(m);
    };
  }
  return panel;
}

/** daemon の様子（`amonhen daemon status --json`）。動いていなければ undefined */
function daemonStatus(): { pid: number; connections: number; repos: { path: string }[] } | undefined {
  const engine = process.env['AMONHEN_E2E_ENGINE']!;
  try {
    return JSON.parse(childProcess.execFileSync(engine, ['daemon', 'status', '--json'], { encoding: 'utf8' }));
  } catch {
    return undefined;
  }
}

/**
 * CRLF のファイルの 1 行を変え、Working Copy で行単位の stage・unstage、行の破棄と Undo、コミットと Undo をする。
 * どれも、変えた 1 行だけが変わったものとして扱われることを確かめる
 */
async function workingCopyFlow(label: string): Promise<void> {
  const panel = await showView('amonhen.showWorkingCopy');
  const wc = panel.workingCopy;
  const path = join(root, FILE);
  const before = readFileSync(path, 'latin1');
  // 3 行目（前の流れでコミットした内容）を書き換える
  const old = before.split('\r\n')[2]!;
  assert.ok(old.startsWith('three') && !old.includes('\n'), '作業中のファイルは CRLF');
  const changed = before.replace(`${old}\r\n`, `three ${label}\r\n`);
  writeFileSync(path, changed, 'latin1');
  const mark = posted.length;
  await vscode.commands.executeCommand('amonhen.refresh');
  await waitPosted('wcFiles', mark, (m) => m.files.some((f) => f.group === 'unstaged' && f.path === FILE));

  /** 差分を選び、変わった行が 1 行の書き換えだけであることを確かめて、その行を返す */
  const select = async (group: 'unstaged' | 'staged') => {
    const at = posted.length;
    await wc.handle({ type: 'wcSelect', group, path: FILE });
    const { diff } = await waitPosted('wcDiff', at, (m) => m.group === group && m.diff?.path === FILE);
    const lines = diff!.hunks.flatMap((h) => h.lines).filter((l) => l.kind !== ' ');
    assert.deepEqual(
      lines.map((l) => [l.kind, l.text]),
      [
        ['-', old],
        ['+', `three ${label}`],
      ],
      '変えた 1 行だけが差分になる（改行の違いで全体が書き換えに見えない）',
    );
    const selected = lines.map((l): SelectedLine => (l.kind === '-' ? { kind: '-', old: l.old! } : { kind: '+', new: l.new! }));
    return { group, path: FILE, version: diff!.version, lines: selected };
  };
  const numstat = (...args: string[]) => git('diff', '--numstat', ...args).trim();
  const one = `1\t1\t${FILE}`;

  // 行単位の stage と unstage
  await wc.handle({ type: 'wcStageLines', ...(await select('unstaged')) });
  await until('stage', () => numstat('--cached') === one && numstat() === '');
  await wc.handle({ type: 'wcStageLines', ...(await select('staged')) });
  await until('unstage', () => numstat('--cached') === '' && numstat() === one);

  // 行の破棄（確かめのダイアログには Discard と答える）と、その Undo
  const { version, lines } = await select('unstaged');
  const warning = vscode.window.showWarningMessage;
  (vscode.window as { showWarningMessage: unknown }).showWarningMessage = async () => 'Discard';
  try {
    await wc.handle({ type: 'wcDiscardLines', path: FILE, version, lines });
  } finally {
    (vscode.window as { showWarningMessage: unknown }).showWarningMessage = warning;
  }
  await until('行の破棄', () => git('status', '--porcelain').trim() === '');
  assert.equal(readFileSync(path, 'latin1'), before, '破棄したら元の内容（CRLF のまま）');
  await vscode.commands.executeCommand('amonhen.undo');
  await until('破棄の Undo', () => readFileSync(path, 'latin1') === changed);

  // stage してコミットし、Undo で stage した状態に戻してから、もう一度コミットする
  await wc.handle({ type: 'wcStageLines', ...(await select('unstaged')) });
  await until('stage', () => numstat('--cached') === one);
  const head = git('rev-parse', 'HEAD').trim();
  const commit = async () => {
    const at = posted.length;
    await wc.handle({ type: 'wcCommit', message: `change three (${label})`, amend: false });
    await waitPosted('wcCommitted', at);
    assert.equal(git('log', '-1', '--format=%s').trim(), `change three (${label})`);
    assert.equal(git('rev-parse', 'HEAD~').trim(), head);
    assert.equal(git('status', '--porcelain').trim(), '');
  };
  await commit();
  await vscode.commands.executeCommand('amonhen.undo');
  await until('コミットの Undo', () => git('rev-parse', 'HEAD').trim() === head && numstat('--cached') === one);
  await commit();
  assert.ok(!git('show', `HEAD:${FILE}`).includes('\r'), 'コミットした内容は LF');
  assert.equal(readFileSync(path, 'latin1'), changed, '作業中のファイルは CRLF のまま');
}

/**
 * `.mailmap` を置くと、History の行と詳細が mailmap の名前になり（詳細には記録されたものも付く）、`log.mailmap` を
 * false にすると記録のままに戻る。`mailmap.file` も、その中身を書き換えたときも同じ。どれも開いたまま、知らせ
 * （ここでは amonhen.refresh）で読み直す
 */
async function mailmapFlow(): Promise<void> {
  const panel = await showView('amonhen.showHistory');
  const mailmap = join(root, '.mailmap');
  const shared = join(dirname(root), 'shared-mailmap');
  const show = async (author: string) => {
    const mark = posted.length;
    await vscode.commands.executeCommand('amonhen.refresh');
    const rows = await waitPosted('rows', mark, (m) => m.rows.some((r) => r.subject === 'first' && r.author === author));
    const first = rows.rows.find((r) => r.subject === 'first')!;
    await panel.history.handle({ type: 'select', oid: first.oid });
    return (await waitPosted('details', mark, (m) => m.details.oid === first.oid)).details;
  };
  writeFileSync(mailmap, 'Ann Example <ann@example.com> <a@example.com>\n');
  try {
    const mapped = await show('Ann Example');
    assert.deepEqual([mapped.author.name, mapped.author.email], ['Ann Example', 'ann@example.com']);
    assert.deepEqual(mapped.recordedAuthor, { name: 'a', email: 'a@example.com' });
    git('config', 'log.mailmap', 'false');
    const recorded = await show('a');
    assert.deepEqual([recorded.author.name, recorded.author.email], ['a', 'a@example.com']);
    assert.equal(recorded.recordedAuthor, undefined);
    git('config', '--unset', 'log.mailmap');
    rmSync(mailmap);
    writeFileSync(shared, 'Carol Example <a@example.com>\n');
    git('config', 'mailmap.file', shared);
    assert.equal((await show('Carol Example')).author.name, 'Carol Example');
    // 設定はそのままで、ファイルの中身だけを変える
    writeFileSync(shared, 'Dave Example <a@example.com>\n');
    assert.equal((await show('Dave Example')).author.name, 'Dave Example');
  } finally {
    // 途中で消したものもあるので、あれば消す（ない設定を消すと git は失敗する）
    for (const key of ['log.mailmap', 'mailmap.file']) {
      try {
        git('config', '--unset', key);
      } catch {
        // なかった
      }
    }
    rmSync(mailmap, { force: true });
    rmSync(shared, { force: true });
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

suite('Amon Hen in VSCode', () => {
  suiteSetup(async () => {
    recordSpawns();
    api = await ext.activate();
  });

  test('activates with the bundled engine and the shared daemon', async () => {
    const repo = await until('リポジトリ', () => api.manager.current);
    assert.equal(repo.root.toLowerCase(), root.toLowerCase());
    // 同梱のエンジンが置き場所に置かれ、current がそれを指す
    const data =
      process.platform === 'win32'
        ? join(process.env['LOCALAPPDATA']!, 'amonhen')
        : join(process.env['XDG_DATA_HOME']!, 'amonhen');
    await until('エンジンの置き場所', () => existsSync(join(data, 'current')));
    const link =
      process.platform === 'win32' ? join(data, 'bin', 'amonhen.exe') : join(process.env['HOME']!, '.local', 'bin', 'amonhen');
    // install は current を書いてから CLI を写す（Windows では写すのに時間がかかる）
    await until(`${link} がある`, () => existsSync(link));
  });

  test('shows History, commit details and diffs through the daemon', async () => {
    const mark = posted.length;
    const panel = await showView('amonhen.showHistory');
    const rows = await waitPosted('rows', mark, (m) => m.rows.some((r) => r.subject === 'add crlf.txt'));
    const commit = rows.rows.find((r) => r.subject === 'add crlf.txt')!;
    await panel.history.handle({ type: 'select', oid: commit.oid });
    const { details } = await waitPosted('details', mark, (m) => m.details.oid === commit.oid);
    assert.deepEqual(
      details.files.map((f) => f.path),
      [FILE],
    );
    await panel.history.handle({ type: 'commitDiff', oid: commit.oid, path: FILE });
    const { diff } = await waitPosted('commitDiff', mark, (m) => m.oid === commit.oid);
    assert.deepEqual(
      diff.hunks.flatMap((h) => h.lines).map((l) => [l.kind, l.text]),
      ['one', 'two', 'three', 'four', 'five'].map((t) => ['+', t]),
    );
    // History は共有の daemon が読んでいる
    const status = await until('daemon', () => daemonStatus());
    assert.ok(
      status.repos.some((r) => r.path.toLowerCase() === root.toLowerCase()),
      `daemon が ${root} を開いている: ${JSON.stringify(status.repos.map((r) => r.path))}`,
    );
  });

  test('shows names from the mailmap through the daemon and reloads them when it changes', async () => {
    await mailmapFlow();
  });

  test('stages, undoes, discards lines and commits on a CRLF file through the daemon', async () => {
    const mark = spawned.length;
    await workingCopyFlow('daemon');
    // 行の stage は daemon がする（拡張は git の update-index を起動しない）
    assert.deepEqual(
      spawned.slice(mark).filter((s) => s.args.includes('update-index')),
      [],
    );
  });

  test('shares one daemon with another window and the CLI', async () => {
    const engine = process.env['AMONHEN_E2E_ENGINE']!;
    const before = await until('daemon', () => daemonStatus());
    // もう 1 つの窓の拡張と同じつなぎ方で、同じリポジトリを開く
    const other = await DaemonClient.open(engine, root);
    try {
      const during = daemonStatus()!;
      assert.equal(during.pid, before.pid, '新しい daemon を起動しない');
      assert.equal(during.connections, before.connections + 1);
      // CLI も同じ daemon を通して読む
      const log = JSON.parse(
        childProcess.execFileSync(engine, ['log', '-C', `daemon:${root}`, '-n', '1', '--no-graph', '--json'], {
          encoding: 'utf8',
        }),
      ) as { id: string }[];
      assert.equal(log[0]!.id, git('rev-parse', 'HEAD').trim());
      assert.equal(daemonStatus()!.pid, before.pid, 'CLI も同じ daemon を使う');
    } finally {
      other.dispose();
    }
  });

  test('does the same on the git execution layer with the daemon off', async () => {
    await vscode.workspace.getConfiguration('amonhen').update('daemon.enabled', false, vscode.ConfigurationTarget.Global);
    const mark = spawned.length;
    await workingCopyFlow('git');
    assert.ok(
      spawned.slice(mark).some((s) => s.args.includes('update-index')),
      '行の stage は拡張が git の update-index でする',
    );
  });

  test('shows names from the mailmap on the git execution layer', async () => {
    await mailmapFlow();
  });

  test('runs the git chosen in Amon Hen’s settings, and stops when it isn’t there', async () => {
    const repo = api.manager.current!;
    const settings = settingsFile();
    const choose = (git: string) => {
      mkdirSync(dirname(settings), { recursive: true });
      writeFileSync(settings, JSON.stringify({ git }));
    };
    const pathGit =
      process.platform === 'win32'
        ? childProcess.execFileSync('where', ['git'], { encoding: 'utf8' }).split(/\r?\n/)[0]!
        : childProcess.execFileSync('sh', ['-c', 'command -v git'], { encoding: 'utf8' }).trim();
    const missing = join(dirname(settings), 'no-such-git');
    try {
      for (const daemon of [false, true]) {
        await vscode.workspace.getConfiguration('amonhen').update('daemon.enabled', daemon, vscode.ConfigurationTarget.Global);
        const via = daemon ? 'daemon' : 'git';
        // 選んだ git がなければ、ほかの git で動かさずに止まる
        choose(missing);
        await assert.rejects(repo.backend.createBranch(`missing-${via}`, 'HEAD', false), (e: Error) => e.message.includes(missing));
        assert.equal(git('branch', '--list', `missing-${via}`), '');
        // 選び直せば、再起動もつなぎ直しもなく、その git で動く
        choose(pathGit);
        await repo.backend.createBranch(`chosen-${via}`, 'HEAD', false);
        assert.notEqual(git('branch', '--list', `chosen-${via}`), '');
      }
    } finally {
      rmSync(settings, { force: true });
      await vscode.workspace.getConfiguration('amonhen').update('daemon.enabled', false, vscode.ConfigurationTarget.Global);
    }
  });

  test('follows a change to git.path at once, through the daemon too', async () => {
    const repo = api.manager.current!;
    const gitConfig = vscode.workspace.getConfiguration('git');
    const pathGit =
      process.platform === 'win32'
        ? childProcess.execFileSync('where', ['git'], { encoding: 'utf8' }).split(/\r?\n/)[0]!
        : childProcess.execFileSync('sh', ['-c', 'command -v git'], { encoding: 'utf8' }).trim();
    const missing = join(dirname(settingsFile()), 'no-such-git');
    // 設定を変えたあと、拡張が読み直して daemon に送るまでを待つ
    const settle = () => new Promise((resolve) => setTimeout(resolve, 300));
    try {
      for (const daemon of [false, true]) {
        await vscode.workspace.getConfiguration('amonhen').update('daemon.enabled', daemon, vscode.ConfigurationTarget.Global);
        const via = daemon ? 'daemon' : 'git';
        // Amon Hen の設定で選んでいなければ git.path の git。変えれば、開き直さずに次の操作から使う
        await gitConfig.update('path', missing, vscode.ConfigurationTarget.Global);
        await settle();
        await assert.rejects(repo.backend.createBranch(`path-missing-${via}`, 'HEAD', false), (e: Error) => e.message.includes(missing));
        await gitConfig.update('path', pathGit, vscode.ConfigurationTarget.Global);
        await settle();
        await repo.backend.createBranch(`path-chosen-${via}`, 'HEAD', false);
        assert.notEqual(git('branch', '--list', `path-chosen-${via}`), '');
      }
    } finally {
      await gitConfig.update('path', undefined, vscode.ConfigurationTarget.Global);
      await vscode.workspace.getConfiguration('amonhen').update('daemon.enabled', false, vscode.ConfigurationTarget.Global);
    }
  });

  test('starts every process without a console window', () => {
    assert.ok(spawned.length > 0);
    const shown = spawned.filter((s) => s.options.windowsHide !== true);
    assert.deepEqual(
      shown.map((s) => s.args.join(' ')),
      [],
      'Windows でコンソールの窓が出る',
    );
  });

  test('stops its git processes when the repositories are closed', async () => {
    api.manager.dispose();
    // daemon は拡張と切り離して起動するので除く。常駐の cat-file が残ると、Windows ではリポジトリを消せない（EPERM）
    const own = spawned.filter((s) => !s.options.detached);
    await until(
      'git の終了',
      () => own.every((s) => s.child.exitCode !== null || s.child.signalCode !== null),
      5_000,
    ).catch(() => {
      const alive = own.filter((s) => s.child.exitCode === null && s.child.signalCode === null);
      assert.fail(`終わっていない: ${alive.map((s) => s.args.join(' ')).join(', ')}`);
    });
  });
});
