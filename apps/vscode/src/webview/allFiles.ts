/**
 * History の詳細の「すべてのファイル」。コミットの木（開いたフォルダだけ拡張ホストに求める）と、選んだファイルの
 * その時点の中身を並べる。このコミットで変えたファイルには印を付け、消したファイルは取り消し線で木に残す。中身では、
 * 足した行を緑、変えた行を青（前の行はツールチップ）、消したところに赤い印。画像は拡大縮小でき、SVG は絵・
 * テキスト・モバイル、Markdown は整形とテキストで見る。そのほかのバイナリはエディタで開く（アプリと同じ）
 */
import type { FileView, ImageBlob, SvgCheckView, TreeEntry, ViewLine } from '../history/protocol.ts';
import { el, post } from './api.ts';
import { renderCode } from './diffView.ts';

/** コミットで変えたファイル（木の印） */
export interface ChangedFile {
  path: string;
  status?: string;
}

export interface AllFiles {
  readonly root: HTMLElement;
  showEntries(path: string, entries: TreeEntry[], error?: string): void;
  showFile(path: string, view: FileView): void;
}

/** 中身に並べる行の上限（それより長いファイルはエディタで） */
const MAX_LINES = 20000;

export function createAllFiles(oid: string, changes: readonly ChangedFile[]): AllFiles {
  const root = el('div', 'all-files');
  const tree = el('div', 'tree-pane');
  const pane = el('div', 'file-pane');
  pane.append(el('div', 'empty', 'Select a file to see its contents at this commit'));
  root.append(tree, pane);
  const status = new Map(changes.map((c) => [c.path, c.status ?? 'M']));
  /** フォルダ（パス）の中身を入れる枠 */
  const folders = new Map<string, HTMLElement>();
  let selected: string | undefined;
  let selectedRow: HTMLElement | undefined;

  /** このフォルダの下に、このコミットで変えたファイルがあるか */
  const touches = (dir: string) => changes.some((c) => c.path.startsWith(`${dir}/`));

  function folderBox(path: string, depth: number): HTMLElement {
    const box = el('div', 'tree-children');
    box.dataset.depth = String(depth);
    box.append(el('div', 'tree-note', 'Loading…'));
    folders.set(path, box);
    post({ type: 'treeEntries', oid, path });
    return box;
  }
  tree.append(folderBox('', 0));

  function showEntries(path: string, entries: TreeEntry[], error?: string): void {
    const box = folders.get(path);
    if (!box) return;
    const depth = Number(box.dataset.depth);
    const rows: Node[] = [];
    if (error) rows.push(el('div', 'tree-note', error));
    const prefix = path ? `${path}/` : '';
    // 消したファイルは木にないので、そのフォルダに足す（取り消し線）
    const deleted = changes
      .filter((c) => c.status === 'D' && c.path.startsWith(prefix) && !c.path.slice(prefix.length).includes('/'))
      .map((c) => ({ name: c.path.slice(prefix.length), kind: 'file' as const, deleted: true }));
    const all: (TreeEntry & { deleted?: boolean })[] = [...entries];
    for (const d of deleted) if (!all.some((e) => e.name === d.name)) all.push(d);
    for (const entry of all) {
      const full = `${prefix}${entry.name}`;
      const row = el('div', `tree-row ${entry.kind}`);
      row.style.paddingLeft = `${6 + depth * 14}px`;
      row.title = full;
      if (entry.kind === 'folder') {
        const chevron = el('span', 'chevron', '▸');
        row.append(chevron, el('span', 'name', entry.name));
        if (touches(full)) row.classList.add('touched');
        let children: HTMLElement | undefined;
        row.addEventListener('click', () => {
          if (!children) {
            children = folderBox(full, depth + 1);
            row.after(children);
          } else {
            children.hidden = !children.hidden;
          }
          chevron.textContent = children.hidden ? '▸' : '▾';
        });
        rows.push(row);
        continue;
      }
      const mark = status.get(full);
      row.append(el('span', 'chevron'), el('span', 'name', entry.name));
      if (mark) row.append(el('span', `status s-${mark}`, mark));
      if (entry.deleted) row.classList.add('deleted');
      if (entry.kind === 'submodule') {
        row.classList.add('disabled');
        row.title = `${full} (submodule)`;
      } else {
        row.addEventListener('click', () => {
          selectedRow?.classList.remove('active');
          row.classList.add('active');
          selectedRow = row;
          selected = full;
          pane.replaceChildren(el('div', 'empty', 'Loading…'));
          post({ type: 'treeFile', oid, path: full });
        });
      }
      rows.push(row);
    }
    if (rows.length === 0) rows.push(el('div', 'tree-note', 'Empty'));
    box.replaceChildren(...rows);
  }

  function showFile(path: string, view: FileView): void {
    if (path !== selected) return;
    const head = el('div', 'file-pane-head');
    head.append(el('span', 'path', path));
    const body = el('div', 'file-pane-body');
    pane.replaceChildren(head, body);
    switch (view.kind) {
      case 'error':
        body.append(el('div', 'empty', view.message));
        return;
      case 'binary': {
        body.append(el('div', 'empty', `Binary file · ${sizeText(view.bytes)}`));
        const open = el('button', 'secondary', 'Open in Editor');
        open.title = 'Open the file at this commit in VSCode';
        open.addEventListener('click', () => post({ type: 'openTreeFile', oid, path }));
        body.append(open);
        return;
      }
      case 'image':
        body.append(zoomableImage(view.image));
        return;
      case 'text': {
        const modes: [string, string, () => Node][] = [];
        if (view.markdown) modes.push(['Preview', 'Show the Markdown formatted', () => renderMarkdown(view.markdown!)]);
        if (view.svg) {
          const svg = view.svg;
          modes.push(['Picture', 'Show the SVG as a picture', () => zoomableImage(svg.image)]);
        }
        modes.push(['Text', 'Show the text with this commit’s changes marked', () => renderLines(view.lines, view.removedAtEnd)]);
        if (view.svg?.check) {
          const check = view.svg.check;
          const image = view.svg.image;
          modes.push(['Mobile', 'What breaks on mobile SVG renderers, with an approximation of each', () => renderSvgCheck(image, check)]);
        }
        if (modes.length > 1) {
          const switcher = el('span', 'segmented');
          const pick = (i: number) => {
            for (const [j, b] of [...switcher.children].entries()) b.classList.toggle('on', i === j);
            body.replaceChildren(modes[i]![2]());
          };
          modes.forEach(([label, tip], i) => {
            const b = el('button', 'bar-button', label);
            b.title = tip;
            b.addEventListener('click', () => pick(i));
            switcher.append(b);
          });
          head.append(switcher);
          pick(0);
        } else {
          body.append(modes[0]![2]());
        }
      }
    }
  }

  return { root, showEntries, showFile };
}

function sizeText(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 行番号と印を付けたファイルの中身 */
function renderLines(lines: readonly ViewLine[], removedAtEnd?: number): HTMLElement {
  const box = el('div', 'lines file-lines');
  const shown = lines.slice(0, MAX_LINES);
  shown.forEach((line, i) => {
    const row = el('div', `line${line.mark ? ` ${line.mark}` : ''}${line.removedBefore ? ' removed-before' : ''}`);
    row.append(el('span', 'num', String(i + 1)), renderCode(line.text, line.syntax));
    const tips: string[] = [];
    if (line.removedBefore) tips.push(`${line.removedBefore} line${line.removedBefore === 1 ? '' : 's'} removed above`);
    if (line.mark === 'changed' && line.old !== undefined) tips.push(`Before: ${line.old}`);
    if (line.mark === 'added') tips.push('Added in this commit');
    if (tips.length) row.title = tips.join('\n');
    box.append(row);
  });
  if (removedAtEnd) {
    const end = el('div', 'line removed-before');
    end.title = `${removedAtEnd} line${removedAtEnd === 1 ? '' : 's'} removed at the end`;
    end.append(el('span', 'num'), el('span', 'code', ''));
    box.append(end);
  }
  if (lines.length > MAX_LINES) box.append(el('div', 'empty', `The rest (${lines.length - MAX_LINES} lines) is left out.`));
  return box;
}

/** 拡大縮小できる画像（ボタン、⌘・Ctrl を押してスクロール） */
function zoomableImage(image: ImageBlob): HTMLElement {
  const box = el('div', 'zoom-image');
  const bar = el('div', 'zoom-bar');
  const frame = el('div', 'zoom-frame');
  const checker = el('div', 'checker');
  const img = el('img');
  img.src = image.url;
  checker.append(img);
  frame.append(checker);
  let scale = 0;
  const label = el('span', 'zoom-label');
  const apply = () => {
    if (scale === 0) {
      img.style.width = '';
      img.style.maxWidth = '100%';
      label.textContent = 'Fit';
    } else {
      img.style.maxWidth = 'none';
      img.style.width = `${Math.round(img.naturalWidth * scale)}px`;
      label.textContent = `${Math.round(scale * 100)}%`;
    }
  };
  const current = () => (scale === 0 ? img.clientWidth / (img.naturalWidth || 1) : scale);
  const zoom = (factor: number) => {
    scale = Math.min(32, Math.max(0.05, current() * factor));
    apply();
  };
  const button = (text: string, tip: string, run: () => void) => {
    const b = el('button', 'bar-button', text);
    b.title = tip;
    b.addEventListener('click', run);
    return b;
  };
  bar.append(
    button('−', 'Zoom out', () => zoom(1 / 1.25)),
    label,
    button('+', 'Zoom in', () => zoom(1.25)),
    button('1:1', 'Actual size', () => {
      scale = 1;
      apply();
    }),
    button('Fit', 'Fit the width', () => {
      scale = 0;
      apply();
    }),
  );
  frame.addEventListener(
    'wheel',
    (e) => {
      if (!e.metaKey && !e.ctrlKey) return;
      e.preventDefault();
      zoom(e.deltaY < 0 ? 1.1 : 1 / 1.1);
    },
    { passive: false },
  );
  const info = el('div', 'image-caption');
  img.addEventListener('load', () => {
    info.textContent = `${img.naturalWidth} × ${img.naturalHeight} · ${sizeText(image.bytes)}`;
    apply();
  });
  apply();
  box.append(bar, frame, info);
  return box;
}

const TARGET_NAMES: Record<string, string> = {
  android: 'Android VectorDrawable',
  ios: 'iOS (CoreSVG)',
  flutter: 'flutter_svg',
  reactNative: 'react-native-svg',
};

/** SVG のモバイルの確認: 崩れる機能と、部品ごとに元の絵と崩れ方の近似を並べる */
function renderSvgCheck(image: ImageBlob, check: SvgCheckView): HTMLElement {
  const box = el('div', 'svg-check');
  if (check.issues.length === 0) {
    box.append(el('div', 'empty', 'Nothing here breaks on the mobile SVG renderers.'));
    return box;
  }
  for (const [key, name] of Object.entries(TARGET_NAMES)) {
    const issues = check.issues.filter((i) => i.targets.includes(key));
    const section = el('section', 'svg-target');
    section.append(el('h4', '', `${name}${issues.length ? '' : ' — no problems'}`));
    if (issues.length) {
      const list = el('ul');
      for (const i of issues) list.append(el('li', '', `${i.feature}${i.count > 1 ? ` (×${i.count})` : ''}: ${i.message}`));
      section.append(list);
      const pair = el('div', 'image-side');
      const figure = (label: string, url: string) => {
        const f = el('figure', 'image-figure');
        const frame = el('div', 'checker');
        const img = el('img');
        img.src = url;
        frame.append(img);
        f.append(frame, el('div', 'image-caption', label));
        return f;
      };
      const approx = check.approximations[key];
      pair.append(figure('Original', image.url));
      if (approx) pair.append(figure('Approximation', `data:image/svg+xml;base64,${base64(approx)}`));
      section.append(pair);
    }
    box.append(section);
  }
  return box;
}

function base64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = '';
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary);
}

/** コアの Markdown のブロック */
interface MdRun {
  text: string;
  bold?: boolean;
  italic?: boolean;
  strike?: boolean;
  code?: boolean;
  html?: boolean;
  link?: string;
  image?: string;
  footnote?: string;
  /** GitHub 独自の絵文字（`:octocat:` などの画像だけのもの）。`text` を薄い札で見せる */
  custom_emoji?: string;
  line_break?: boolean;
}
type MdBlock =
  | { kind: 'heading'; level: number; runs: MdRun[] }
  | { kind: 'paragraph'; runs: MdRun[] }
  | { kind: 'code'; language: string; text: string; syntax?: [number, number, number][][] }
  | { kind: 'quote'; alert?: string; blocks: MdBlock[] }
  | { kind: 'list'; start?: number; items: { task?: boolean; blocks: MdBlock[] }[] }
  | { kind: 'table'; aligns: string[]; head: MdRun[][]; rows: MdRun[][][] }
  | { kind: 'rule' }
  | { kind: 'html'; text: string }
  | { kind: 'footnote'; label: string; blocks: MdBlock[] };

/** Markdown を整形して見せる（コアが読んだブロックから組み立てる。外からは何も読まない） */
function renderMarkdown(blocks: readonly unknown[]): HTMLElement {
  const box = el('div', 'markdown');
  box.append(...(blocks as MdBlock[]).map(renderBlock));
  return box;
}

function renderRuns(runs: readonly MdRun[]): Node[] {
  return runs.map((run) => {
    if (run.line_break) return el('br');
    if (run.image) {
      // コミットの中の絵は拡張ホストが data URL にしてある。外の URL は読まず、代わりの文を出す
      if (run.image.startsWith('data:')) {
        const img = el('img', 'md-img');
        img.src = run.image;
        img.alt = run.text;
        return img;
      }
      const alt = el('span', 'md-image', `🖼 ${run.text || run.image}`);
      alt.title = run.image;
      return alt;
    }
    if (run.footnote) return el('sup', 'md-footnote', `[${run.footnote}]`);
    if (run.custom_emoji) {
      // GitHub 独自の絵文字は画像を持たないので、名前を薄い札で見せる（アプリと同じ）
      const chip = el('span', 'md-emoji-chip', run.text);
      chip.title = 'GitHub’s own emoji, shown by name (its image isn’t included)';
      return chip;
    }
    let node: HTMLElement = el(run.code ? 'code' : 'span', run.html ? 'md-html' : '', run.text);
    if (run.bold) node = wrapIn('strong', node);
    if (run.italic) node = wrapIn('em', node);
    if (run.strike) node = wrapIn('s', node);
    if (run.link) {
      const a = el('a');
      a.href = run.link;
      a.title = run.link;
      a.append(node);
      node = a;
    }
    return node;
  });
}

function wrapIn(tag: 'strong' | 'em' | 's', child: Node): HTMLElement {
  const e = el(tag);
  e.append(child);
  return e;
}

function renderBlock(block: MdBlock): HTMLElement {
  switch (block.kind) {
    case 'heading': {
      const h = el(`h${Math.min(6, Math.max(1, block.level))}` as 'h1');
      h.append(...renderRuns(block.runs));
      return h;
    }
    case 'paragraph': {
      const p = el('p');
      p.append(...renderRuns(block.runs));
      return p;
    }
    case 'code': {
      const pre = el('pre', 'md-code');
      const code = el('code');
      const lines = block.text.replace(/\n$/, '').split('\n');
      lines.forEach((line, i) => {
        if (i > 0) code.append('\n');
        code.append(block.syntax ? renderCode(line, block.syntax[i]) : line);
      });
      pre.append(code);
      return pre;
    }
    case 'quote': {
      const q = el('blockquote', block.alert ? `md-alert md-${block.alert}` : '');
      if (block.alert) q.append(el('div', 'md-alert-title', block.alert[0]!.toUpperCase() + block.alert.slice(1)));
      q.append(...block.blocks.map(renderBlock));
      return q;
    }
    case 'list': {
      const list = block.start !== undefined ? el('ol') : el('ul');
      if (block.start !== undefined && list instanceof HTMLOListElement) list.start = block.start;
      for (const item of block.items) {
        const li = el('li', item.task !== undefined ? 'md-task' : '');
        if (item.task !== undefined) {
          const box = el('input');
          box.type = 'checkbox';
          box.checked = item.task;
          box.disabled = true;
          li.append(box, ' ');
        }
        li.append(...item.blocks.map(renderBlock));
        list.append(li);
      }
      return list;
    }
    case 'table': {
      const table = el('table', 'md-table');
      const head = el('tr');
      block.head.forEach((cell, i) => {
        const th = el('th');
        th.style.textAlign = block.aligns[i] && block.aligns[i] !== 'none' ? block.aligns[i]! : '';
        th.append(...renderRuns(cell));
        head.append(th);
      });
      table.append(head);
      for (const row of block.rows) {
        const tr = el('tr');
        row.forEach((cell, i) => {
          const td = el('td');
          td.style.textAlign = block.aligns[i] && block.aligns[i] !== 'none' ? block.aligns[i]! : '';
          td.append(...renderRuns(cell));
          tr.append(td);
        });
        table.append(tr);
      }
      return table;
    }
    case 'rule':
      return el('hr');
    case 'html':
      return el('pre', 'md-html', block.text);
    case 'footnote': {
      const div = el('div', 'md-footnote-body');
      div.append(el('sup', '', `[${block.label}]`), ...block.blocks.map(renderBlock));
      return div;
    }
  }
}
