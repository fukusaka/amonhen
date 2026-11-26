import assert from 'node:assert/strict';
import { test } from 'node:test';
import { drawMarkdownPictures, type Render } from '../src/history/markdownPictures.ts';

const colors = { dark: false, text: '#333333', background: '#ffffff', kinds: { keyword: '#0000ff' } };

/** markdownBlocks の答え（絵を描かないエンジンの形） */
function blocks(): unknown[] {
  return [
    { kind: 'code', language: 'mermaid', text: 'flowchart LR\n  A --> B' },
    { kind: 'code', language: 'DOT', text: 'digraph { a -> b }' },
    { kind: 'code', language: 'mermaid', text: 'not a diagram' },
    { kind: 'code', language: 'rust', text: 'fn main() {}' },
    { kind: 'math', text: 'x^2' },
    {
      kind: 'list',
      items: [{ blocks: [{ kind: 'paragraph', runs: [{ text: 'Area ' }, { text: '$\\pi r^2$', code: true, math_tex: '\\pi r^2' }] }] }],
    },
    { kind: 'heading', level: 1, runs: [{ text: '$$y$$', code: true, math_tex: 'y', math_display: true }] },
  ];
}

test('diagrams and formulas are drawn by the helper and written into the blocks', async () => {
  const asked: [string, Record<string, unknown>][] = [];
  const render: Render = async (op, fields) => {
    asked.push([op, fields]);
    if (op === 'diagram') {
      if (fields['text'] === 'not a diagram') return {};
      if (fields['language'] === 'DOT') return { error: 'syntax error' };
      return { svg: '<svg>d</svg>', note: 'approx' };
    }
    return {
      formulas: [
        { svg: '<svg>1</svg>', width: 1, height: 0.8, depth: 0.2 },
        { error: 'bad' },
        { svg: '<svg>3</svg>', width: 0.5, height: 0.7, depth: 0 },
      ],
    };
  };
  const b = blocks();
  await drawMarkdownPictures(b, 'docs/a.md', colors, { scheme: 'mermaid-forest', timeZone: 'Asia/Tokyo', timeLimit: 5 }, render);
  const diagrams = asked.filter(([op]) => op === 'diagram').map(([, f]) => f);
  assert.equal(diagrams.length, 3);
  assert.deepEqual(diagrams[0], {
    language: 'mermaid',
    text: 'flowchart LR\n  A --> B',
    from: 'docs/a.md',
    colors,
    scheme: 'mermaid-forest',
    timeZone: 'Asia/Tokyo',
  });
  const math = asked.find(([op]) => op === 'math')?.[1];
  assert.deepEqual(math, {
    color: '#333333',
    timeLimit: 5,
    formulas: [
      { tex: 'x^2', display: true },
      { tex: '\\pi r^2', display: false },
      { tex: 'y', display: true },
    ],
  });
  assert.deepEqual(b[0], { kind: 'diagram', language: 'mermaid', text: 'flowchart LR\n  A --> B', svg: '<svg>d</svg>', note: 'approx' });
  assert.deepEqual(b[1], { kind: 'diagram', language: 'DOT', text: 'digraph { a -> b }', error: 'syntax error' });
  assert.deepEqual(b[2], { kind: 'code', language: 'mermaid', text: 'not a diagram' });
  assert.deepEqual(b[3], { kind: 'code', language: 'rust', text: 'fn main() {}' });
  assert.deepEqual(b[4], { kind: 'math', text: 'x^2', svg: '<svg>1</svg>', width: 1, height: 0.8, depth: 0.2 });
  const list = b[5] as { items: { blocks: { runs: Record<string, unknown>[] }[] }[] };
  assert.deepEqual(list.items[0]!.blocks[0]!.runs[1], { text: '$\\pi r^2$', code: true, math_error: 'bad' });
  const heading = b[6] as { runs: Record<string, unknown>[] };
  assert.deepEqual(heading.runs[0], { text: '$$y$$', code: true, math: { svg: '<svg>3</svg>', width: 0.5, height: 0.7, depth: 0 } });
});

test('without the helper, diagrams and formulas stay as their code', async () => {
  const b = blocks();
  // エンジンが描いた（色のない）図も、コードに戻す
  b.push({ kind: 'diagram', language: 'mermaid', text: 'pie', svg: '<svg>var(--pele-fg)</svg>' });
  await drawMarkdownPictures(b, 'a.md', colors, {}, () => Promise.reject(new Error('No drawing helper')));
  assert.deepEqual(b[0], { kind: 'code', language: 'mermaid', text: 'flowchart LR\n  A --> B' });
  assert.deepEqual(b[4], { kind: 'math', text: 'x^2' });
  const heading = b[6] as { runs: Record<string, unknown>[] };
  assert.deepEqual(heading.runs[0], { text: '$$y$$', code: true });
  assert.deepEqual(b[7], { kind: 'code', language: 'mermaid', text: 'pie' });
});

test('a file without diagrams or formulas never starts the helper', async () => {
  let started = false;
  await drawMarkdownPictures([{ kind: 'paragraph', runs: [{ text: 'x' }] }], 'a.md', colors, {}, async () => {
    started = true;
    return {};
  });
  assert.equal(started, false);
});

test('an imported Mermaid theme the helper can’t read is reported', async () => {
  const b = blocks();
  const drawn = await drawMarkdownPictures(b, 'a.md', colors, { scheme: 'mermaid-import:mine', importedTheme: 'not json' }, async (op, fields) => {
    if (op !== 'diagram') return { formulas: [] };
    assert.equal(fields['importedTheme'], 'not json');
    return { svg: '<svg/>', themeError: 'expected value' };
  });
  assert.deepEqual(drawn, { themeError: 'expected value' });
});
