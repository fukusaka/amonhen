/**
 * VSCode を起動して拡張を動かす結合テスト（`npm run test:e2e`）。
 * 拡張とテストをビルドして一時ディレクトリに .vsix と同じ形で並べ（bin にこのリポジトリのエンジン）、リポジトリを
 * 作って VSCode（@vscode/test-cli）で開き、終わったら daemon を止めて一時ディレクトリを消す。
 * エンジンはルートで `cargo build -p amonhen-cli` したもの（`AMONHEN_DAEMON` で差し替えられる）
 */
import { execFileSync, spawnSync } from 'node:child_process';
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
const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8' });
mkdirSync(paths.home);
git(dir, 'init', '-q', '-b', 'main', paths.origin);
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
  }),
);

const env = testEnv(dir, engine);
const start = performance.now();
const run = spawnSync(
  process.execPath,
  [join(root, 'node_modules', '@vscode', 'test-cli', 'out', 'bin.mjs'), '--config', join(root, '.vscode-test.mjs')],
  { cwd: root, stdio: 'inherit', env: { ...process.env, AMONHEN_E2E_DIR: dir, AMONHEN_E2E_ENGINE: engine } },
);
console.log(`VSCode: ${((performance.now() - start) / 1000).toFixed(1)}s`);

// daemon を止めてから消す。Windows では、どこかのプロセスが掴んだままだと消せない（EPERM）
spawnSync(engine, ['daemon', 'stop'], { env: { ...process.env, ...env }, stdio: 'inherit' });
let removed = true;
try {
  rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
} catch (err) {
  removed = false;
  console.error(`一時ディレクトリを消せません（プロセスが掴んだまま）: ${err}`);
}
process.exit(run.status !== 0 ? (run.status ?? 1) : removed ? 0 : 1);
