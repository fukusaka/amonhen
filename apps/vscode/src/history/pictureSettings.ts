/**
 * 「すべてのファイル」の図と数式の設定（アプリの Settings > Theme の Diagrams・Math と同じ選択肢と既定）。設定の値を
 * 確かめ、絵の手伝いに渡す形にする。vscode に依存しない（設定の値とファイルの読み手は呼ぶ側が渡す）
 */
import { basename, isAbsolute, join } from 'node:path';
import type { PictureOptions } from './markdownPictures.ts';

/** 図の配色（`amonhen.diagrams.colors`）。`imported` は `amonhen.diagrams.mermaidTheme` のファイル（アプリの読み込んだテーマ） */
export const DIAGRAM_SCHEMES = [
  'mermaid',
  'syntax',
  'pele',
  'mermaid-default',
  'mermaid-neutral',
  'mermaid-dark',
  'mermaid-forest',
  'mermaid-base',
  'imported',
] as const;

/** 数式の大きさ（本文の字に対する倍率。コアの `math::FONT_SCALES`、既定は KaTeX と同じ 1.21） */
export const MATH_SCALES = [1.0, 1.1, 1.21, 1.3, 1.4, 1.5] as const;
export const DEFAULT_MATH_SCALE = 1.21;

/** 1 つのファイルで式を描く時間の上限（秒。0 は上限なし。コアの `math::TIME_LIMITS`、既定は 2 秒） */
export const MATH_TIME_LIMITS = [1, 2, 5, 10, 0] as const;
export const DEFAULT_MATH_TIME_LIMIT = 2;

/** 設定の値（読んだまま。型は確かめていない） */
export interface RawPictureSettings {
  colors?: unknown;
  mermaidTheme?: unknown;
  timeZone?: unknown;
  formulaSize?: unknown;
  timeLimit?: unknown;
}

/** 手伝いに渡す設定と、Webview に渡す数式の大きさ。`themeError` はテーマのファイルが読めなかったわけ */
export interface PictureSettings {
  options: PictureOptions;
  mathScale: number;
  themeError?: string;
}

/**
 * 設定 `raw` を手伝いに渡す形にする。`localZone` は「OS の時間帯」（設定が空）のときの時間帯の名前（Webview の
 * 時間帯。なければ手伝いの OS の時間帯）、`root` はテーマのファイルの相対パスの起点（リポジトリの根）、`home` は `~`、
 * `read` はテーマのファイルを読む
 */
export async function pictureSettings(
  raw: RawPictureSettings,
  context: { root?: string; home: string; localZone?: string; read: (path: string) => Promise<string> },
): Promise<PictureSettings> {
  const colors = DIAGRAM_SCHEMES.find((s) => s === raw.colors) ?? 'mermaid';
  const options: PictureOptions = {};
  let themeError: string | undefined;
  if (colors === 'imported') {
    const file = typeof raw.mermaidTheme === 'string' ? raw.mermaidTheme.trim() : '';
    const path = file ? themePath(file, context.root, context.home) : undefined;
    options.scheme = `mermaid-import:${path ? basename(path) : ''}`;
    if (!path) {
      themeError = 'No Mermaid theme file is set (amonhen.diagrams.mermaidTheme)';
    } else {
      try {
        options.importedTheme = await context.read(path);
      } catch (err) {
        themeError = `Can’t read the Mermaid theme ${file}: ${err instanceof Error ? err.message : String(err)}`;
      }
    }
  } else {
    options.scheme = colors;
  }
  const zone = typeof raw.timeZone === 'string' ? raw.timeZone.trim() : '';
  options.timeZone = zone || context.localZone || '';
  options.timeLimit = MATH_TIME_LIMITS.find((s) => s === raw.timeLimit) ?? DEFAULT_MATH_TIME_LIMIT;
  const mathScale = MATH_SCALES.find((s) => s === raw.formulaSize) ?? DEFAULT_MATH_SCALE;
  return { options, mathScale, ...(themeError ? { themeError } : {}) };
}

/** テーマのファイルのパス（`~` は家、相対パスはリポジトリの根から。根がなければ読まない） */
function themePath(file: string, root: string | undefined, home: string): string | undefined {
  if (file === '~' || file.startsWith('~/')) return join(home, file.slice(1));
  if (isAbsolute(file)) return file;
  return root ? join(root, file) : undefined;
}
