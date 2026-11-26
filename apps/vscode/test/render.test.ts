import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { RenderSession } from '../src/daemon/render.ts';
import { drawMarkdownPictures } from '../src/history/markdownPictures.ts';

// このリポジトリの絵の手伝い（ルートで `cargo build -p amonhen-render`）。なければ飛ばす
const exe = process.platform === 'win32' ? '.exe' : '';
const helper = process.env['AMONHEN_RENDER'] ?? join(import.meta.dirname, `../../../target/debug/amonhen-render${exe}`);
const skip = existsSync(helper) ? false : `絵の手伝いがありません: ${helper}`;

const colors = { dark: true, text: '#abcdef', background: '#101010', kinds: { keyword: '#ff0000' } };

/** 同じコミットのファイル（フォルダは中のファイルから作る） */
function files(contents: Record<string, string>) {
  const asked: string[] = [];
  return {
    asked,
    read: async (path: string) => {
      asked.push(`read ${path}`);
      return contents[path] === undefined ? undefined : new TextEncoder().encode(contents[path]);
    },
    list: async (folder: string) => {
      asked.push(`list ${folder}`);
      const prefix = folder ? `${folder}/` : '';
      const names = new Map<string, boolean>();
      for (const path of Object.keys(contents)) {
        if (!path.startsWith(prefix)) continue;
        const [name, ...rest] = path.slice(prefix.length).split('/');
        names.set(name!, rest.length > 0);
      }
      return [...names].map(([name, folder]) => ({ name, folder }));
    },
  };
}

test('the helper draws a Markdown file’s diagrams and formulas, reading DOT images from the commit', { skip }, async () => {
  const source = files({ 'docs/img/a.svg': '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8"/></svg>' });
  const session = new RenderSession(helper, source);
  const blocks: unknown[] = [
    { kind: 'code', language: 'mermaid', text: 'flowchart LR\n  A --> B' },
    { kind: 'code', language: 'dot', text: 'digraph { a [image="img/a.svg" shape=box] }' },
    { kind: 'math', text: 'x^2' },
    { kind: 'paragraph', runs: [{ text: '$y$', code: true, math_tex: 'y' }] },
  ];
  try {
    await drawMarkdownPictures(blocks, 'docs/a.md', colors, { scheme: 'syntax', timeZone: 'UTC', timeLimit: 2 }, (op, fields) =>
      session.request(op, fields),
    );
  } finally {
    session.close();
  }
  const [mermaid, dot, math, paragraph] = blocks as Record<string, unknown>[];
  assert.equal(mermaid!['kind'], 'diagram');
  assert.match(String(mermaid!['svg']), /#abcdef/);
  assert.equal(dot!['kind'], 'diagram');
  assert.match(String(dot!['svg']), /<image/);
  assert.deepEqual(source.asked, ['read docs/img/a.svg']);
  assert.match(String(math!['svg']), /#abcdef/);
  assert.equal(typeof math!['depth'], 'number');
  const run = (paragraph!['runs'] as Record<string, unknown>[])[0]!;
  assert.match(String((run['math'] as Record<string, unknown>)['svg']), /<svg/);
});

test('the helper draws a VectorDrawable with colors from the commit’s resources', { skip }, async () => {
  const xml =
    '<vector xmlns:android="http://schemas.android.com/apk/res/android" android:width="24dp" android:height="24dp" android:viewportWidth="24" android:viewportHeight="24"><path android:fillColor="@color/brand" android:pathData="M0,0h24v24h-24z"/></vector>';
  const source = files({ 'res/drawable/icon.xml': xml, 'res/values/colors.xml': '<resources><color name="brand">#FF112233</color></resources>' });
  const session = new RenderSession(helper, source);
  try {
    const reply = await session.request('vectorDrawable', { path: 'res/drawable/icon.xml', data: Buffer.from(xml).toString('base64') });
    assert.match(String(reply['svg']), /#112233/);
    assert.ok(source.asked.includes('read res/values/colors.xml'), source.asked.join(', '));
    // 失敗はその求めだけ
    await assert.rejects(session.request('svgCheck', { data: '*' }));
    const check = await session.request('svgCheck', { data: Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 10 10"><circle cx="5" cy="5" r="5"/></svg>').toString('base64') });
    assert.ok(Array.isArray(check['issues']));
    const previews = check['previews'] as Record<string, { svg?: string; title: string; caption: string; exact: boolean }>;
    assert.match(String(previews['android']?.svg), /<svg/);
    assert.equal(previews['android']?.exact, true);
    assert.equal(previews['ios']?.exact, false);
    assert.ok(previews['flutter']?.title && previews['flutter'].caption);
  } finally {
    session.close();
  }
});

test('a missing helper fails the requests instead of hanging', async () => {
  const session = new RenderSession(join(import.meta.dirname, 'no-such-helper'), files({}));
  await assert.rejects(session.request('math', { color: '#000000', formulas: [] }));
  session.close();
});
