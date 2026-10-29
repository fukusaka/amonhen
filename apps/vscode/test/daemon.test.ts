import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { Changes, clientEnv, controlDaemon, DaemonClient } from '../src/daemon/client.ts';
import { readWorkingDiff, selectedRows } from '../src/daemon/diff.ts';
import { platformName, prepareEngine } from '../src/daemon/engine.ts';
import { readRows } from '../src/daemon/history.ts';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { UndoLog } from '../src/state/undo.ts';
import { withDaemonWrites } from '../src/daemon/writes.ts';
import { initMain } from './git.ts';

// このリポジトリのエンジン（ルートで `cargo build -p amonhen-cli`。CLI と daemon を兼ねる）。なければ飛ばす
const exe = process.platform === 'win32' ? '.exe' : '';
const daemon = process.env['AMONHEN_DAEMON'] ?? join(import.meta.dirname, `../../../target/debug/amonhen${exe}`);
const skip = existsSync(daemon) ? false : `エンジンがありません: ${daemon}`;

let dir = '';
let repo = '';

function git(...args: string[]): string {
  return execFileSync('git', args, { cwd: repo, encoding: 'utf8' });
}

before(() => {
  // ソケットのパス（このフォルダ/s/amonhend-<指紋の頭 8 文字>.sock）は macOS では 103 バイトまで。macOS の一時フォルダ
  // （/private/var/folders/…/T/）は長いので、名前は短くする
  dir = realpathSync.native(mkdtempSync(join(tmpdir(), 'ah-')));
  // daemon のソケットと History のキャッシュは試験の中に置く（手元の daemon とアプリに混ぜない）
  process.env['AMONHEN_DAEMON_DIR'] = join(dir, 's');
  process.env['HOME'] = dir;
  process.env['LOCALAPPDATA'] = dir;
  delete process.env['XDG_DATA_HOME'];
  repo = join(dir, 'repo');
  initMain(repo);
  git('config', 'user.name', 'a');
  git('config', 'user.email', 'a@example.com');
  writeFileSync(join(repo, 'a.txt'), 'first\n');
  git('add', '.');
  git('commit', '-q', '-m', 'first');
  git('switch', '-q', '-c', 'topic');
  writeFileSync(join(repo, 't.txt'), 'topic\n');
  git('add', '.');
  git('commit', '-q', '-m', 'topic work');
  git('switch', '-q', 'main');
  writeFileSync(join(repo, 'm.txt'), 'main\n');
  git('add', '.');
  git('commit', '-q', '-m', 'main work');
  git('merge', '-q', '--no-ff', '-m', 'merge topic', 'topic');
});

after(async () => {
  if (!skip) {
    await controlDaemon(daemon, 'stop').catch(() => undefined);
  }
  rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

test('reads History rows and the graph through the daemon', { skip }, async () => {
  const client = await DaemonClient.open(daemon, repo);
  try {
    const history = await client.call({ op: 'history' });
    assert.equal(history.json['rows'], 4);
    const reply = await client.call({ op: 'historyRows', history: history.json['history'], start: 0, count: 10 });
    const rows = readRows(reply);
    assert.deepEqual(
      rows.map((r) => r.subject),
      ['merge topic', 'main work', 'topic work', 'first'],
    );
    assert.equal(rows[0]!.author, 'a');
    assert.equal(rows[0]!.oid, git('rev-parse', 'HEAD').trim());
    // merge は 2 つの親を持ち、topic の側は別のレーンに出る
    assert.equal(rows[0]!.parents.length, 2);
    assert.notEqual(rows[2]!.graph.col, rows[1]!.graph.col);
    assert.ok(rows.every((r) => r.graph.segs.length % 5 === 0));
    assert.ok(rows[0]!.graph.segs.length > 0);

    const found = await client.call({ op: 'historySearch', history: history.json['history'], query: 'topic' });
    assert.deepEqual(Array.from(found.u32('rows')), [0, 2]);
  } finally {
    client.dispose();
  }
});

test('shows names from the mailmap through the daemon', { skip }, async () => {
  const client = await DaemonClient.open(daemon, repo);
  writeFileSync(join(repo, '.mailmap'), 'Ann Example <ann@example.com> <a@example.com>\n');
  try {
    // History は作り直さずに、行の作者名・検索・詳細が置き換わる
    const history = await client.call({ op: 'history' });
    const reply = await client.call({ op: 'historyRows', history: history.json['history'], start: 0, count: 1 });
    assert.equal(readRows(reply)[0]!.author, 'Ann Example');
    for (const query of ['author:ann', 'author:a']) {
      const found = await client.call({ op: 'historySearch', history: history.json['history'], query });
      assert.equal(found.u32('rows').length, 4, query);
    }
    const oid = git('rev-parse', 'HEAD').trim();
    const info = (await client.call({ op: 'run', command: { op: 'commitInfo', oid } })).json['info'] as Record<string, unknown>;
    assert.deepEqual([info['author'], info['authorEmail']], ['Ann Example', 'ann@example.com']);
    assert.deepEqual(info['recordedAuthor'], { name: 'a', email: 'a@example.com' });
  } finally {
    rmSync(join(repo, '.mailmap'));
    client.dispose();
  }
});

test('stages lines, commits and hears changes through the daemon', { skip }, async () => {
  const client = await DaemonClient.open(daemon, repo);
  try {
    const heard = new Promise<number>((resolve) => {
      const d = client.onDidChange((c) => {
        if (c & Changes.worktree) {
          d.dispose();
          resolve(c);
        }
      });
    });
    writeFileSync(join(repo, 'a.txt'), 'FIRST\nsecond\n');
    await heard;
    const status = await client.call({ op: 'status' });
    assert.deepEqual(
      (status.json['entries'] as { path: string }[]).map((e) => e.path),
      ['a.txt'],
    );
    const diff = await client.call({ op: 'workingDiff', path: 'a.txt', mode: 'combined' });
    const rows = diff.u32('rows');
    const lengths = diff.u32('textLengths');
    const text = diff.bytes('text');
    const line = (i: number) => new TextDecoder().decode(text.subarray(rows[i * 4 + 3]!, rows[i * 4 + 3]! + lengths[i]!));
    const picked = [];
    for (let i = 0; i < lengths.length; i++) {
      if (line(i) === 'second') picked.push(i);
    }
    assert.equal(picked.length, 1);
    await client.call({ op: 'stageRows', diff: diff.json['diff'], rows: picked, stage: true });
    assert.match(git('diff', '--cached'), /\+second/);
    assert.doesNotMatch(git('diff', '--cached'), /FIRST/);
    await client.call({ op: 'commit', message: 'add second' });
    assert.equal(git('log', '-1', '--format=%s').trim(), 'add second');
    // Undo はコアの操作ログで（アプリ・CLI と同じ）
    const undo = await client.call({ op: 'run', command: { op: 'undo' } });
    assert.equal(undo.json['ok'], true);
    assert.equal(git('log', '-1', '--format=%s').trim(), 'merge topic');
  } finally {
    client.dispose();
  }
});

test('shows working diffs as hunks and discards lines that the extension can undo', { skip }, async () => {
  const client = await DaemonClient.open(daemon, repo);
  try {
    writeFileSync(join(repo, 'm.txt'), 'MAIN\nmore\n');
    const d = readWorkingDiff(await client.call({ op: 'workingDiff', path: 'm.txt', mode: 'unstaged' }));
    assert.equal(d.diff.hunks.length, 1);
    assert.equal(d.diff.hunks[0]!.header, '@@ -1 +1,2 @@');
    assert.deepEqual(
      d.diff.hunks[0]!.lines.map((l) => `${l.kind}${l.text}`),
      ['-main', '+MAIN', '+more'],
    );
    // 改行の情報も届く（普段のファイルなので見せない）
    assert.equal(d.diff.lineEndings?.worktree, 'lf');
    assert.equal(d.diff.lineEndings?.notable, false);
    // 「more」の行だけを捨てる。記録は拡張と同じ undo.json に入り、拡張の Undo で戻せる
    const rows = selectedRows(d.rows, [{ kind: '+', new: 2 }]);
    assert.equal(rows.length, 1);
    await client.call({ op: 'discardRows', diff: d.handle, rows });
    assert.equal(git('show', ':m.txt'), 'main\n');
    assert.equal(readFileSync(join(repo, 'm.txt'), 'utf8'), 'MAIN\n');
    const backend = await CliBackend.open(new GitExecutor('git'), repo);
    try {
      const entry = await new UndoLog(backend).undo();
      assert.ok(entry, '拡張の Undo で、daemon の操作が戻せる');
    } finally {
      await backend.dispose();
    }
    assert.equal(readFileSync(join(repo, 'm.txt'), 'utf8'), 'MAIN\nmore\n');
  } finally {
    client.dispose();
    git('checkout', '--', 'm.txt');
  }
});

test('writes through the daemon record one undo entry each', { skip }, async () => {
  const client = await DaemonClient.open(daemon, repo);
  const inner = await CliBackend.open(new GitExecutor('git'), repo);
  const backend = withDaemonWrites(inner, async () => client);
  const undo = new UndoLog(backend);
  const count = () => {
    try {
      return (JSON.parse(readFileSync(join(repo, '.git/amonhen/undo.json'), 'utf8')) as unknown[]).length;
    } catch {
      return 0;
    }
  };
  try {
    const before = count();
    // ブランチを作る（コアが記録する）→ 拡張の記録は増やさない
    await undo.record('Create branch side', ['refs/heads/side'], 'keep', () => backend.createBranch('side', 'main', false));
    assert.equal(git('rev-parse', 'side').trim(), git('rev-parse', 'main').trim());
    // checkout（コアは記録しない）→ 拡張の側で記録しないまま
    await backend.switchBranch('side');
    assert.equal(git('branch', '--show-current').trim(), 'side');
    await backend.switchBranch('main');
    // コミット（コアが記録する）
    writeFileSync(join(repo, 'w.txt'), 'w\n');
    await backend.stage(['w.txt']);
    await undo.record('Commit "w"', ['refs/heads/main'], 'soft', () => backend.commit('w'));
    assert.equal(git('log', '-1', '--format=%s').trim(), 'w');
    assert.equal(count(), before + 2, 'ブランチとコミットの 2 つだけ（二重に記録しない）');
    // 拡張の Undo で、コアが記録したコミットとブランチを順に戻せる
    await undo.undo();
    assert.equal(git('log', '-1', '--format=%s').trim(), 'merge topic');
    await undo.undo();
    assert.equal(git('branch', '--list', 'side').trim(), '');
  } finally {
    await inner.dispose();
    client.dispose();
    git('reset', '-q');
    rmSync(join(repo, 'w.txt'), { force: true });
  }
});

test('takes a new environment on an open connection from the next command', { skip }, async () => {
  const client = await DaemonClient.open(daemon, repo);
  const missing = process.platform === 'win32' ? 'C:\\no\\such\\git.exe' : '/no/such/git';
  const setName = { op: 'run', command: { op: 'configSet', key: 'amonhen.test', value: 'x' } };
  // `run` は失敗を `{ ok: false, error }` で返す
  const result = async (): Promise<unknown> => (await client.call(setName)).json['error'] ?? 'ok';
  try {
    assert.equal(await result(), 'ok');
    process.env['AMONHEN_GIT'] = missing;
    await client.sendEnvironment();
    assert.ok(String(await result()).includes(missing));
    delete process.env['AMONHEN_GIT'];
    await client.sendEnvironment();
    assert.equal(await result(), 'ok');
  } finally {
    delete process.env['AMONHEN_GIT'];
    client.dispose();
  }
});

test('names the host platform like the engine files', () => {
  assert.equal(platformName('darwin', 'arm64'), 'darwin-aarch64');
  assert.equal(platformName('linux', 'x64'), 'linux-x86_64');
  assert.equal(platformName('win32', 'x64'), 'windows-x86_64');
  assert.equal(platformName('freebsd', 'x64'), undefined);
  assert.equal(platformName('linux', 'ia32'), undefined);
});

test('installs the bundled engine and picks it for the daemon', { skip }, async () => {
  // 拡張の bin と同じ形（amonhen-<os>-<arch>）に、試験の amonhen を置く
  const bin = join(dir, 'ext-bin');
  mkdirSync(bin, { recursive: true });
  copyFileSync(daemon, join(bin, `amonhen-${platformName()}${exe}`));
  const path = await prepareEngine(bin);
  // 置き場所は OS ごと（Windows は %LOCALAPPDATA%\\amonhen で、current はハッシュを書いたファイル）
  if (process.platform === 'win32') {
    assert.match(path!, /amonhen[\\/]engine[\\/][0-9a-f]{16}[\\/]amonhen\.exe$/);
  } else {
    assert.equal(path, join(dir, '.local/share/amonhen/current/amonhen'));
  }
  assert.match(execFileSync(path!, ['--version'], { encoding: 'utf8' }), /\(build [0-9a-f]{16}\)/);
});

test('sends the daemon only the environment git and ssh use', () => {
  const env = clientEnv({ PATH: '/bin', HOME: '/home/x', GIT_SSH_COMMAND: 'ssh -v', SECRET_TOKEN: 'x', SSH_AUTH_SOCK: '/a.sock' });
  assert.deepEqual(env.split('\0').sort(), ['GIT_SSH_COMMAND=ssh -v', 'PATH=/bin', 'SSH_AUTH_SOCK=/a.sock']);
  // Windows では PATH が `Path` で入っている
  assert.equal(clientEnv({ Path: 'C:\\fe', HOME: 'x' }, true), 'Path=C:\\fe');
  assert.equal(clientEnv({ Path: 'C:\\fe' }, false), '');
  // AMONHEN_PASS_ENV で足した名前も送る
  const added = clientEnv({ PATH: '/bin', AMONHEN_PASS_ENV: 'GH_HOST, HTTPS_PROXY', GH_HOST: 'example.com', NO_PROXY: 'x' }, false);
  assert.deepEqual(added.split('\0').sort(), ['AMONHEN_PASS_ENV=GH_HOST, HTTPS_PROXY', 'GH_HOST=example.com', 'PATH=/bin']);
  // git.path は、Amon Hen の設定で git を決めていないときの git として送る
  assert.equal(clientEnv({ AMONHEN_FALLBACK_GIT: '/vscode/git' }, false), 'AMONHEN_FALLBACK_GIT=/vscode/git');
});
