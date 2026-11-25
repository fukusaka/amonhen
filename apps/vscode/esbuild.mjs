import * as esbuild from 'esbuild';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';

const production = process.argv.includes('--production');
const watch = process.argv.includes('--watch');

const common = {
  bundle: true,
  minify: production,
  sourcemap: !production,
  logLevel: 'info',
};

// どの FE でも同じ形のアイコン（crates/icons/svg、docs/icons.md）を、VSCode が求めるライト・ダークの 2 枚にする。
// 拡張の SVG は画像として描かれ、まわりの文字の色を受け継がないので、色を書き込む（VSCode の icon.foreground と
// charts.green の既定の色）。
const ICON_COLORS = {
  light: { '': '#424242', '-green': '#388A34' },
  dark: { '': '#C5C5C5', '-green': '#89D185' },
};
const iconSource = new URL('../../crates/icons/svg/', import.meta.url);
for (const [theme, colors] of Object.entries(ICON_COLORS)) {
  const dir = new URL(`media/icons/${theme}/`, import.meta.url);
  mkdirSync(dir, { recursive: true });
  for (const file of readdirSync(iconSource).filter((f) => f.endsWith('.svg'))) {
    const svg = readFileSync(new URL(file, iconSource), 'utf8');
    for (const [suffix, color] of Object.entries(colors)) {
      writeFileSync(new URL(file.replace(/\.svg$/, `${suffix}.svg`), dir), svg.replaceAll('currentColor', color));
    }
  }
}

const contexts = await Promise.all([
  // 拡張ホスト
  esbuild.context({
    ...common,
    entryPoints: ['src/extension.ts'],
    outfile: 'dist/extension.js',
    external: ['vscode'],
    format: 'cjs',
    platform: 'node',
    target: 'node20',
  }),
  // Webview
  esbuild.context({
    ...common,
    entryPoints: ['src/webview/main.ts'],
    outfile: 'dist/webview/main.js',
    format: 'iife',
    platform: 'browser',
    target: 'es2022',
  }),
]);

if (watch) {
  await Promise.all(contexts.map((c) => c.watch()));
} else {
  await Promise.all(contexts.map((c) => c.rebuild()));
  await Promise.all(contexts.map((c) => c.dispose()));
}
