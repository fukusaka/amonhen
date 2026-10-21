// VSCode を起動する結合テスト（e2e/）の設定。`npm run test:e2e`（e2e/run.mjs）が一時ディレクトリを用意して渡す
import { readdirSync } from 'node:fs';
import { join } from 'node:path';
import { defineConfig } from '@vscode/test-cli';
import { layout, testEnv } from './e2e/env.mjs';

const dir = process.env.AMONHEN_E2E_DIR;
if (!dir) {
  throw new Error('npm run test:e2e から走らせてください');
}
const paths = layout(dir);

export default defineConfig({
  label: 'e2e',
  // package.json の engines の下限
  version: process.env.AMONHEN_E2E_VSCODE ?? '1.120.0',
  extensionDevelopmentPath: paths.ext,
  // 一時ディレクトリの中は glob で拾えないので、並べて渡す
  files: readdirSync(join(paths.ext, 'e2e'))
    .filter((f) => f.endsWith('.test.js'))
    .map((f) => join(paths.ext, 'e2e', f)),
  workspaceFolder: paths.repo,
  launchArgs: [
    // 裏に回った窓でも Webview の読み込みを間引かせない（間引かれると開くのに数十秒かかる）。VSCode の知らない
    // オプションは次の引数を値として取るので、VSCode のオプションの前に置く
    '--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows',
    '--disable-background-timer-throttling',
    // Linux（Ubuntu 24.04 の CI）は、展開しただけの VSCode では Chromium のサンドボックスが使えない
    ...(process.platform === 'linux' ? ['--no-sandbox'] : []),
    '--user-data-dir',
    paths.user,
    '--disable-workspace-trust',
    '--skip-welcome',
    '--skip-release-notes',
  ],
  env: testEnv(dir, process.env.AMONHEN_E2E_ENGINE),
  mocha: { ui: 'tdd', timeout: 60_000 },
});
