/**
 * 「すべてのファイル」の整形した Markdown の図と数式を、絵の手伝い（`amonhen-render`）で描く。エンジンの
 * `markdownBlocks` は Markdown を読むだけで、図のフェンスはコード（`code`）、数式のブロックは TeX（`math`）、文中の
 * 式は原文と `math_tex` で返す。それを手伝いに送り、描いた絵（画面の色を当てたもの）をブロックに書き込む。描け
 * なければコードのまま（手伝いがない・失敗したときも）。vscode に依存しない
 */
import type { PictureColors } from './protocol.ts';

/** 手伝いへの求め（`op` と中身）。答えか、失敗 */
export type Render = (op: string, fields: Record<string, unknown>) => Promise<Record<string, unknown>>;

/** 図として描くフェンスの言葉（コアの `diagram::draw_with` と同じ: Mermaid と Graphviz の DOT） */
const DIAGRAM_FENCES = new Set(['mermaid', 'dot', 'graphviz', 'gv']);

type Obj = Record<string, unknown>;

/** 絵の大きさ（em）と SVG */
const PICTURE_KEYS = ['svg', 'width', 'height', 'depth'] as const;

/**
 * `blocks`（`markdownBlocks` の答え。その場で書き換える）の図と数式を描く。`from` は Markdown のファイルのパス（DOT
 * の図の画像を、そのフォルダから読む）、`utcOffset` は gantt の日付を数える時差（分）
 */
export async function drawMarkdownPictures(
  blocks: unknown[],
  from: string,
  colors: PictureColors,
  utcOffset: number | undefined,
  render: Render,
): Promise<void> {
  const diagrams: Obj[] = [];
  const formulas: { node: Obj; tex: string; display: boolean; block: boolean }[] = [];
  const visit = (value: unknown): void => {
    if (Array.isArray(value)) {
      for (const v of value) visit(v);
      return;
    }
    if (!value || typeof value !== 'object') return;
    const node = value as Obj;
    // エンジンが描いた絵（絵のフィーチャー入りで作ったもの）は色がないので、描き直す
    if (node['kind'] === 'diagram') {
      node['kind'] = 'code';
      for (const key of ['svg', 'config', 'note', 'error']) delete node[key];
    }
    if (node['kind'] === 'code' && typeof node['language'] === 'string' && DIAGRAM_FENCES.has(node['language'].toLowerCase()) && typeof node['text'] === 'string') {
      diagrams.push(node);
      return;
    }
    if (node['kind'] === 'math' && typeof node['text'] === 'string') {
      for (const key of [...PICTURE_KEYS, 'error']) delete node[key];
      formulas.push({ node, tex: node['text'], display: true, block: true });
      return;
    }
    if (typeof node['math_tex'] === 'string') {
      delete node['math'];
      delete node['math_error'];
      formulas.push({ node, tex: node['math_tex'], display: node['math_display'] === true, block: false });
      delete node['math_tex'];
      delete node['math_display'];
    }
    for (const v of Object.values(node)) if (typeof v === 'object') visit(v);
  };
  visit(blocks);
  const jobs: Promise<void>[] = diagrams.map(async (block) => {
    const drawn = await render('diagram', {
      language: block['language'],
      text: block['text'],
      from,
      colors,
      ...(utcOffset !== undefined ? { utcOffset } : {}),
    }).catch(() => undefined);
    if (typeof drawn?.['svg'] === 'string') {
      block['kind'] = 'diagram';
      block['svg'] = drawn['svg'];
      if (typeof drawn['note'] === 'string') block['note'] = drawn['note'];
    } else if (typeof drawn?.['error'] === 'string') {
      block['kind'] = 'diagram';
      block['error'] = drawn['error'];
    }
  });
  if (formulas.length) {
    jobs.push(
      render('math', { color: colors.text, formulas: formulas.map((f) => ({ tex: f.tex, display: f.display })) })
        .then((reply) => {
          const drawn = Array.isArray(reply['formulas']) ? (reply['formulas'] as Obj[]) : [];
          formulas.forEach((f, i) => {
            const picture = drawn[i];
            if (!picture) return;
            const complete = typeof picture['svg'] === 'string' && PICTURE_KEYS.slice(1).every((k) => typeof picture[k] === 'number');
            if (complete) {
              const formula = Object.fromEntries(PICTURE_KEYS.map((k) => [k, picture[k]]));
              if (f.block) Object.assign(f.node, formula);
              else f.node['math'] = formula;
            } else if (typeof picture['error'] === 'string') {
              f.node[f.block ? 'error' : 'math_error'] = picture['error'];
            }
          });
        })
        .catch(() => undefined),
    );
  }
  await Promise.all(jobs);
}
