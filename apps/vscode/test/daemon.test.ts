import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { Changes, controlDaemon, DaemonClient } from '../src/daemon/client.ts';
import { readWorkingDiff, selectedRows } from '../src/daemon/diff.ts';
import { platformName, prepareEngine } from '../src/daemon/engine.ts';
import { readRows } from '../src/daemon/history.ts';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { UndoLog } from '../src/state/undo.ts';
import { withDaemonWrites } from '../src/daemon/writes.ts';

// このリポジトリのエンジン（ルートで `cargo build -p amonhen-cli`。CLI と daemon を兼ねる）。なければ飛ばす
const daemon = process.env['AMONHEN_DAEMON'] ?? join(import.meta.dirname, '../../../target/debug/amonhen');
const skip = existsSync(daemon) ? false : `エンジンがありません: ${daemon}`;

let dir = '';
let repo = '';

function git(...args: string[]): string {
  return execFileSync('git', args, { cwd: repo, encoding: 'utf8' });
}

before(() => {
  dir = realpathSync(mkdtempSync(join(tmpdir(), 'amonhen-daemon-')));
  // daemon のソケットと History のキャッシュは試験の中に置く（手元の daemon とアプリに混ぜない）
  process.env['AMONHEN_DAEMON_DIR'] = join(dir, 's');
  process.env['HOME'] = dir;
  delete process.env['XDG_DATA_HOME'];
  repo = join(dir, 'repo');
  execFileSync('git', ['init', '-q', '-b', 'main', repo]);
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
  rmSync(dir, { recursive: true, force: true });
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
    // 「more」の行だけを捨てる。記録は拡張と同じ undo.json に入り、拡張の Undo で戻せる
    const rows = selectedRows(d.rows, [{ kind: '+', new: 2 }]);
    assert.equal(rows.length, 1);
    await client.call({ op: 'discardRows', diff: d.handle, rows });
    assert.equal(git('show', ':m.txt'), 'main\n');
    assert.equal(execFileSync('cat', [join(repo, 'm.txt')], { encoding: 'utf8' }), 'MAIN\n');
    const backend = await CliBackend.open(new GitExecutor('git'), repo);
    try {
      const entry = await new UndoLog(backend).undo();
      assert.ok(entry, '拡張の Undo で、daemon の操作が戻せる');
    } finally {
      backend.dispose();
    }
    assert.equal(execFileSync('cat', [join(repo, 'm.txt')], { encoding: 'utf8' }), 'MAIN\nmore\n');
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
      return (JSON.parse(execFileSync('cat', [join(repo, '.git/amonhen/undo.json')], { encoding: 'utf8' })) as unknown[]).length;
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
    inner.dispose();
    client.dispose();
    git('reset', '-q');
    rmSync(join(repo, 'w.txt'), { force: true });
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
  execFileSync('mkdir', ['-p', bin]);
  execFileSync('cp', [daemon, join(bin, `amonhen-${platformName()}`)]);
  const path = await prepareEngine(bin);
  assert.equal(path, join(dir, '.local/share/amonhen/current/amonhen'));
  assert.match(execFileSync(path!, ['--version'], { encoding: 'utf8' }), /\(protocol \d+\)/);
});
