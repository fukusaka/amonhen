/**
 * VSCode を起動して拡張を動かす結合テスト（`npm run test:e2e`）。
 * 拡張とテストをビルドして一時ディレクトリに .vsix と同じ形で並べ（bin にこのリポジトリのエンジン）、リポジトリを
 * 作って VSCode（@vscode/test-cli）で開き、終わったら daemon を止めて一時ディレクトリを消す。
 * エンジンはルートで `cargo build -p amonhen-cli` したもの（`AMONHEN_DAEMON` で差し替えられる）。
 * `AMONHEN_E2E_GIT` に git を渡すと、リポジトリを作るのにも拡張（VSCode の `git.path`）にもその git を使う
 * （VSCode は PATH ではなく自分で git を探すので、サポートする最低の git で試すときに要る）
 */
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import * as esbuild from 'esbuild';
import { platformName } from '../src/daemon/engine.ts';
import { layout, testEnv } from './env.mjs';

const root = join(import.meta.dirname, '..');
const exe = process.platform === 'win32' ? '.exe' : '';
const engine = process.env.AMONHEN_DAEMON ?? join(root, `../../target/debug/amonhen${exe}`);
if (!existsSync(engine)) {
  console.error(`エンジンがありません: ${engine}（ルートで cargo build -p amonhen-cli）`);
  process.exit(1);
}
const name = platformName();
if (!name) {
  console.error(`このホスト向けのエンジンを作っていません: ${process.platform}-${process.arch}`);
  process.exit(1);
}

// 拡張とテストのビルド
const build = spawnSync(process.execPath, ['esbuild.mjs'], { cwd: root, stdio: 'inherit' });
if (build.status !== 0) {
  process.exit(build.status ?? 1);
}
const dir = realpathSync.native(mkdtempSync(join(tmpdir(), 'amonhen-e2e-')));
const paths = layout(dir);
for (const p of ['package.json', 'dist', 'media']) {
  cpSync(join(root, p), join(paths.ext, p), { recursive: true });
}
mkdirSync(join(paths.ext, 'bin'));
cpSync(engine, join(paths.ext, 'bin', `amonhen-${name}${exe}`));
await esbuild.build({
  entryPoints: [join(root, 'e2e', '*.test.ts')],
  outdir: join(paths.ext, 'e2e'),
  bundle: true,
  sourcemap: true,
  format: 'cjs',
  platform: 'node',
  target: 'node20',
  external: ['vscode', 'mocha'],
  logLevel: 'warning',
});

// 開くリポジトリ。LF で入れたファイルを core.autocrlf=true で clone し、作業中のファイルを CRLF にする
const gitPath = process.env.AMONHEN_E2E_GIT;
const git = (cwd, ...args) => execFileSync(gitPath ?? 'git', args, { cwd, encoding: 'utf8' });
if (gitPath) {
  console.log(`git: ${gitPath} (${git(root, '--version').trim()})`);
}
mkdirSync(paths.home);
// `init -b` は git 2.28 から
git(dir, 'init', '-q', paths.origin);
git(paths.origin, 'symbolic-ref', 'HEAD', 'refs/heads/main');
git(paths.origin, 'config', 'user.name', 'a');
git(paths.origin, 'config', 'user.email', 'a@example.com');
writeFileSync(join(paths.origin, 'a.txt'), 'first\n');
git(paths.origin, 'add', '.');
git(paths.origin, 'commit', '-q', '-m', 'first');
writeFileSync(join(paths.origin, 'crlf.txt'), 'one\ntwo\nthree\nfour\nfive\n');
git(paths.origin, 'add', '.');
git(paths.origin, 'commit', '-q', '-m', 'add crlf.txt');
git(dir, 'clone', '-q', '-c', 'core.autocrlf=true', paths.origin, paths.repo);
git(paths.repo, 'config', 'user.name', 'a');
git(paths.repo, 'config', 'user.email', 'a@example.com');
if (!readFileSync(join(paths.repo, 'crlf.txt'), 'latin1').includes('\r\n')) {
  console.error('core.autocrlf=true で clone したファイルが CRLF になっていません');
  process.exit(1);
}

// VSCode のユーザー設定。テストと関係のない通信（拡張・本体の更新、AI の機能、テレメトリ）を止める
mkdirSync(join(paths.user, 'User'), { recursive: true });
writeFileSync(
  join(paths.user, 'User', 'settings.json'),
  JSON.stringify({
    'extensions.autoUpdate': false,
    'extensions.autoCheckUpdates': false,
    'update.mode': 'none',
    'telemetry.telemetryLevel': 'off',
    'chat.disableAIFeatures': true,
    'workbench.startupEditor': 'none',
    // fsmonitor は試すテストでだけ点ける（リポジトリごとに常駐し、Windows では一時ディレクトリを消せなくなる）
    'amonhen.fsmonitor': false,
    ...(gitPath ? { 'git.path': gitPath } : {}),
  }),
);

const env = testEnv(dir, engine);
// Windows の CI で拡張ホストが止まる件の手がかり: 10 秒ごとに、その間に CPU を多く使ったプロセスを残し、終わりに出す
const cpuLog = join(dir, 'cpu.log');
const sampler =
  process.platform === 'win32'
    ? spawn(
        'powershell',
        [
          '-NoProfile',
          // 引用符を気にしなくて済むように、UTF-16LE の base64 で渡す
          '-EncodedCommand',
          // svchost はサービス名、Code はプロセスの種類（--type）を添える
          `$names = @{}; function Label($p) { if (-not $names.ContainsKey($p.Id)) { $extra = ''; ` +
            `if ($p.ProcessName -eq 'svchost') { $extra = (Get-CimInstance Win32_Service -Filter "ProcessId=$($p.Id)").Name -join '+' } ` +
            `elseif ($p.ProcessName -eq 'Code') { $c = (Get-CimInstance Win32_Process -Filter "ProcessId=$($p.Id)").CommandLine; ` +
            `$extra = if ($c -match '--type=(\\S+)') { $Matches[1] } else { 'main' } } ` +
            `$names[$p.Id] = '{0}({1}{2})' -f $p.ProcessName, $p.Id, $(if ($extra) { ' ' + $extra } else { '' }) }; $names[$p.Id] }; ` +
            `$prev = @{}; while ($true) { $now = @{}; $rows = foreach ($p in Get-Process) { $now[$p.Id] = $p.CPU; ` +
            `$d = $p.CPU - $(if ($prev.ContainsKey($p.Id)) { $prev[$p.Id] } else { 0 }); ` +
            `if ($prev.Count -gt 0 -and $d -ge 1) { [pscustomobject]@{ N = '{0} {1:N1}s' -f (Label $p), $d; D = $d } } }; ` +
            `if ($rows) { (Get-Date -Format 'HH:mm:ss') + ' ' + (($rows | Sort-Object D -Descending | Select-Object -First 6).N -join ', ') | ` +
            `Add-Content -LiteralPath '${cpuLog}' }; $prev = $now; Start-Sleep -Seconds 10 }`,
        ].map((a, i) => (i === 2 ? Buffer.from(a, 'utf16le').toString('base64') : a)),
        { stdio: 'ignore' },
      )
    : undefined;
const start = performance.now();
const run = spawnSync(
  process.execPath,
  [join(root, 'node_modules', '@vscode', 'test-cli', 'out', 'bin.mjs'), '--config', join(root, '.vscode-test.mjs')],
  { cwd: root, stdio: 'inherit', env: { ...process.env, AMONHEN_E2E_DIR: dir, AMONHEN_E2E_ENGINE: engine } },
);
console.log(`VSCode: ${((performance.now() - start) / 1000).toFixed(1)}s`);
if (sampler) {
  sampler.kill();
  if (existsSync(cpuLog)) console.log(`CPU を 1 秒以上使ったプロセス（10 秒ごと）:\n${readFileSync(cpuLog, 'utf8').trimEnd()}`);
}

// daemon を止めてから消す。Windows では、どこかのプロセスが掴んだままだと消せない（EPERM）
spawnSync(engine, ['daemon', 'stop'], { env: { ...process.env, ...env }, stdio: 'inherit' });
// Repository Settings の署名の鍵を集めると gpg が gpg-agent を起こし、一時ディレクトリの HOME を掴んだまま残る
// （Windows では消せない）。gpg がなければ何もしない
spawnSync('gpgconf', ['--kill', 'all'], { env: { ...process.env, ...env }, stdio: 'ignore' });
let removed = true;
try {
  rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
} catch (err) {
  removed = false;
  console.error(`一時ディレクトリを消せません（プロセスが掴んだまま）: ${err}`);
  // 掴んでいるのが誰かを残す（Windows。残っている git とその引数）
  if (process.platform === 'win32') {
    const left = spawnSync(
      'powershell',
      [
        '-NoProfile',
        '-Command',
        "Get-CimInstance Win32_Process -Filter 'Name = ''git.exe''' | ForEach-Object { '{0} {1} {2}' -f $_.ProcessId, $_.ParentProcessId, $_.CommandLine }",
      ],
      { encoding: 'utf8' },
    );
    console.error(`残っている git:\n${left.stdout?.trim() || 'なし'}`);
  }
}
process.exit(run.status !== 0 ? (run.status ?? 1) : removed ? 0 : 1);
