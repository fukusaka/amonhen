/**
 * History の詳細の「すべてのファイル」。コミットの木（開いたフォルダだけ拡張ホストに求める）と、選んだファイルの
 * その時点の中身を並べる。このコミットで変えたファイルには印を付け、消したファイルは取り消し線で木に残す。中身では、
 * 足した行を緑、変えた行を青（前の行はツールチップ）、消したところに赤い印。画像は拡大縮小でき、SVG は絵・
 * テキスト・モバイル、Markdown は整形とテキストで見る。そのほかのバイナリはエディタで開く（アプリと同じ）
 */
import type { FileView, FileViewNote, ImageBlob, PictureColors, SvgCheckView, TreeEntry, ViewLine } from '../history/protocol.ts';
import { el, post } from './api.ts';
import { renderCode } from './diffView.ts';
import { checkerToggle } from './imageDiff.ts';
import { findAll, findCountText } from './textFind.ts';
import { resolvePath } from '../history/repoPath.ts';
import { hexColor } from '../history/cssColor.ts';

/** コミットで変えたファイル（木の印） */
export interface ChangedFile {
  path: string;
  status?: string;
}

export interface AllFiles {
  readonly root: HTMLElement;
  showEntries(path: string, entries: TreeEntry[], error?: string): void;
  showFile(path: string, view: FileView & FileViewNote): void;
  /** リストか木か（詳細の帯の List・Tree） */
  setTree(tree: boolean): void;
}

/** すべてのファイルの持ち主から */
export interface AllFilesOptions {
  /** 木で見せるか（でなければ、すべてのファイルのパスを並べるリスト） */
  tree: boolean;
  /** 「Show This Change」: 変更の表示に戻り、そのファイルの差分を開く */
  onShowChange?: (path: string) => void;
}

/** 見せ方（Markdown・SVG・VectorDrawable・モバイルの部品）は、ファイルを替えても覚えておく（アプリと同じ） */
const lastMode: { markdown?: string; svg?: string; drawable?: string } = {};
let lastRenderer = 'android';

/** 中身に並べる行の上限（それより長いファイルはエディタで） */
const MAX_LINES = 20000;

export function createAllFiles(oid: string, changes: readonly ChangedFile[], options: AllFilesOptions = { tree: true }): AllFiles {
  const root = el('div', 'all-files');
  let tree_ = options.tree;
  /** リストのために読んだ、すべてのファイル（パス → 種類） */
  const listed = new Map<string, TreeEntry['kind']>();
  /** 読んでいる途中のフォルダの数（リストで、すべて読み終えたら並べる） */
  let loading = 0;
  const tree = el('div', 'tree-pane');
  const pane = el('div', 'file-pane');
  pane.append(el('div', 'empty', 'Select a file to see its contents at this commit'));
  root.append(tree, pane);
  const status = new Map(changes.map((c) => [c.path, c.status ?? 'M']));
  /** フォルダ（パス）の中身を入れる枠 */
  const folders = new Map<string, HTMLElement>();
  let selected: string | undefined;
  let selectedRow: HTMLElement | undefined;
  /** 開いているファイルの絵（図・数式・VectorDrawable）が、画面の色で描いたものか（テーマが変わったら求め直す） */
  let themed = false;
  // テーマが変わったら、画面の色で描いた絵を描き直す（アプリと同じ）
  const observer = new MutationObserver(() => {
    if (!root.isConnected) {
      observer.disconnect();
      return;
    }
    if (themed && selected) requestFile(selected);
  });
  observer.observe(document.body, { attributes: true, attributeFilter: ['class'] });

  /** このフォルダの下に、このコミットで変えたファイルがあるか */
  const touches = (dir: string) => changes.some((c) => c.path.startsWith(`${dir}/`));

  function folderBox(path: string, depth: number): HTMLElement {
    const box = el('div', 'tree-children');
    box.dataset.depth = String(depth);
    box.append(el('div', 'tree-note', 'Loading…'));
    folders.set(path, box);
    loading++;
    post({ type: 'treeEntries', oid, path });
    return box;
  }

  /** 木（根から）かリスト（すべてのフォルダを読んで、パスの順に）を作り直す */
  function rebuild(): void {
    folders.clear();
    listed.clear();
    loading = 0;
    tree.replaceChildren(folderBox('', 0));
  }
  rebuild();

  /** リスト: 読んだファイルをパスの順に並べる（読み終えるまでは数だけ） */
  function renderList(): void {
    if (loading > 0) {
      tree.replaceChildren(el('div', 'tree-note', `Loading… (${listed.size} files)`));
      return;
    }
    const deleted = changes.filter((c) => c.status === 'D').map((c) => c.path);
    const paths = [...new Set([...listed.keys(), ...deleted])].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()));
    tree.replaceChildren(
      ...paths.map((full) => {
        const row = el('div', 'tree-row file');
        row.style.paddingLeft = '6px';
        row.title = full;
        const mark = status.get(full);
        row.append(el('span', 'chevron'), el('span', 'name', full));
        if (mark) row.append(el('span', `status s-${mark}`, mark));
        if (mark === 'D') row.classList.add('deleted');
        if (listed.get(full) === 'submodule') row.classList.add('disabled');
        else row.addEventListener('click', () => openFile(full, row));
        if (full === selected) {
          row.classList.add('active');
          selectedRow = row;
        }
        return row;
      }),
    );
  }

  /** ファイルを開く（行を選んだ印にして、中身を拡張ホストに求める） */
  function openFile(full: string, row?: HTMLElement): void {
    selectedRow?.classList.remove('active');
    row?.classList.add('active');
    selectedRow = row;
    selected = full;
    themed = false;
    pane.replaceChildren(el('div', 'empty', 'Loading…'));
    requestFile(full);
  }

  /** ファイルの中身を求める（図・数式・VectorDrawable の絵は、今の画面の色と時間帯で描かせる） */
  function requestFile(full: string): void {
    post({ type: 'treeFile', oid, path: full, colors: pictureColors(), utcOffset: -new Date().getTimezoneOffset() });
  }

  function showEntries(path: string, entries: TreeEntry[], error?: string): void {
    const box = folders.get(path);
    if (!box) return;
    loading = Math.max(0, loading - 1);
    if (!tree_) {
      // リスト: フォルダはすべて読み、ファイルを覚える
      const prefix = path ? `${path}/` : '';
      for (const e of entries) {
        if (e.kind === 'folder') folderBox(`${prefix}${e.name}`, 0);
        else listed.set(`${prefix}${e.name}`, e.kind);
      }
      renderList();
      return;
    }
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
        const toggle = () => {
          if (!children) {
            children = folderBox(full, depth + 1);
            row.after(children);
          } else {
            children.hidden = !children.hidden;
          }
          chevron.textContent = children.hidden ? '▸' : '▾';
        };
        row.addEventListener('click', toggle);
        rows.push(row);
        // このコミットで変えたファイルのあるフォルダは、最初から開く（アプリと同じ）
        if (touches(full)) queueMicrotask(toggle);
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
        row.addEventListener('click', () => openFile(full, row));
      }
      rows.push(row);
    }
    if (rows.length === 0) rows.push(el('div', 'tree-note', 'Empty'));
    box.replaceChildren(...rows);
  }

  function showFile(path: string, view: FileView & FileViewNote): void {
    if (path !== selected) return;
    themed = view.kind === 'text' && (view.drawable !== undefined || (view.markdown !== undefined && hasPictures(view.markdown)));
    const head = el('div', 'file-pane-head');
    // パスとコミット（アプリの「パス @ 短い ID」）。消したファイルは前の版を出すので、そう添える
    head.append(el('span', 'path', path), el('span', 'at', `@ ${oid.slice(0, 7)}`));
    if (view.deleted) head.append(el('span', 'note', '(deleted in this commit)'));
    const tools = el('span', 'file-tools');
    if (status.has(path) && options.onShowChange) {
      const change = el('button', 'bar-button', 'Show This Change');
      change.title = 'Show this file’s change in Changes';
      change.addEventListener('click', () => options.onShowChange?.(path));
      tools.append(change);
    }
    if (view.kind !== 'error' && view.kind !== 'binary') {
      const open = el('button', 'bar-button', 'Open in Editor');
      open.title = 'Open the file at this commit in VSCode';
      open.addEventListener('click', () => post({ type: 'openTreeFile', oid, path }));
      tools.append(open);
    }
    head.append(tools);
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
        // Markdown の中の相対リンクは、このコミットのそのファイルを開く（アプリと同じ）
        if (view.markdown) modes.push(['Preview', 'Show the Markdown formatted', () => renderMarkdown(view.markdown!, path, (to) => openFile(to))]);
        if (view.svg) {
          const svg = view.svg;
          modes.push(['Picture', 'Show the SVG as a picture', () => zoomableImage(svg.image)]);
          if (svg.check) {
            const check = svg.check;
            modes.push(['Mobile', 'What breaks on a mobile SVG renderer, beside an approximation of it', () => renderSvgCheck(svg.image, check)]);
          }
        }
        if (view.drawable) {
          const drawable = view.drawable;
          modes.push(['Picture', 'Draw the VectorDrawable', () => drawablePicture(drawable.image, drawable.notes)]);
        }
        modes.push([
          'Text',
          view.drawable ? 'Show the XML source' : 'Show the text with this commit’s changes marked',
          () => renderLines(view.lines, view.removedAtEnd),
        ]);
        const kind = view.markdown ? 'markdown' : view.svg ? 'svg' : view.drawable ? 'drawable' : undefined;
        if (modes.length > 1) {
          const switcher = el('span', 'segmented');
          const pick = (i: number) => {
            for (const [j, b] of [...switcher.children].entries()) b.classList.toggle('on', i === j);
            if (kind) lastMode[kind] = modes[i]![0];
            body.replaceChildren(modes[i]![2]());
          };
          modes.forEach(([label, tip], i) => {
            const b = el('button', 'bar-button', label);
            b.title = tip;
            b.addEventListener('click', () => pick(i));
            switcher.append(b);
          });
          head.insertBefore(switcher, tools);
          // 前に選んだ見せ方で開く（アプリと同じ）
          const remembered = kind ? modes.findIndex((m) => m[0] === lastMode[kind]) : -1;
          pick(Math.max(0, remembered));
        } else {
          body.append(modes[0]![2]());
        }
      }
    }
  }

  function setTree(next: boolean): void {
    if (next === tree_) return;
    tree_ = next;
    rebuild();
  }

  return { root, showEntries, showFile, setTree };
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

/** VectorDrawable の絵（大きさは dp）と、絵の下に添える説明（引けなかった色など。アプリと同じ） */
function drawablePicture(image: ImageBlob, notes: readonly string[]): HTMLElement {
  const box = zoomableImage(image, 'dp');
  if (notes.length) box.append(el('div', 'image-footnote', notes.join('\n')));
  return box;
}

/** 拡大縮小できる画像（ボタン、⌘・Ctrl を押してスクロール）。`unit` は大きさの単位（VectorDrawable の dp） */
function zoomableImage(image: ImageBlob, unit = ''): HTMLElement {
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
    // 市松模様の明暗（透けた画素があるときだけ。アプリと同じく拡大縮小の左）
    checkerToggle(box, [img]),
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
    info.textContent = `${img.naturalWidth} × ${img.naturalHeight}${unit ? ` ${unit}` : ''} · ${sizeText(image.bytes)}`;
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

/**
 * SVG のモバイルの確認: 部品（Android・iOS・Flutter・React Native）を選び、その部品で崩れる機能と、元の絵と崩れ方の
 * 近似を並べる（アプリと同じ。選んだ部品は覚える）
 */
function renderSvgCheck(image: ImageBlob, check: SvgCheckView): HTMLElement {
  const box = el('div', 'svg-check');
  if (check.issues.length === 0) {
    box.append(el('div', 'empty', 'Nothing here breaks on the mobile SVG renderers.'));
    return box;
  }
  const switcher = el('span', 'segmented');
  const body = el('div');
  const SHORT: Record<string, string> = { android: 'Android', ios: 'iOS', flutter: 'Flutter', reactNative: 'React Native' };
  const pick = (key: string) => {
    lastRenderer = key;
    for (const b of switcher.querySelectorAll<HTMLElement>('button')) b.classList.toggle('on', b.dataset.key === key);
    const target = renderTarget(key);
    body.replaceChildren(target);
    const toggle = checkerToggle(box, [...target.querySelectorAll<HTMLImageElement>('.checker img')]);
    head.querySelector('.checker-toggle')?.remove();
    head.append(toggle);
  };
  for (const key of Object.keys(TARGET_NAMES)) {
    const b = el('button', 'bar-button', SHORT[key] ?? key);
    b.dataset.key = key;
    b.title = TARGET_NAMES[key]!;
    b.addEventListener('click', () => pick(key));
    switcher.append(b);
  }
  // 市松模様の明暗は部品を選ぶ帯に（既定は元の絵と、選んだ部品の近似の色から）
  const head = el('div', 'svg-check-bar');
  head.append(switcher);
  box.append(head, body);
  pick(TARGET_NAMES[lastRenderer] ? lastRenderer : 'android');
  return box;

  function renderTarget(key: string): HTMLElement {
    const name = TARGET_NAMES[key]!;
    const section = el('section', 'svg-target');
    const issues = check.issues.filter((i) => i.targets.includes(key));
    section.append(el('h4', '', `${name}${issues.length ? '' : ' — no problems'}`));
    if (issues.length) {
      const list = el('ul');
      for (const i of issues) list.append(el('li', '', `${i.feature}${i.count > 1 ? ` (×${i.count})` : ''}: ${i.message}`));
      section.append(list);
    }
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
    if (approx) pair.append(figure(`How ${name} draws it (approximation)`, `data:image/svg+xml;base64,${base64(approx)}`));
    section.append(pair);
    return section;
  }
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
  /** HTML の `<kbd>`・`<sub>`・`<sup>`・`<ins>` */
  kbd?: boolean;
  sub?: boolean;
  sup?: boolean;
  underline?: boolean;
  link?: string;
  image?: string;
  /** `<picture>` の、暗い外観・明るい外観で見せる画像（合うものがなければ `image`） */
  image_dark?: string;
  image_light?: string;
  /** 画像の HTML の `width`・`height` */
  width?: number;
  height?: number;
  /** 画像の幅の割合（`width="50%"`、1〜100）。`width` とは同時に付かない */
  width_percent?: number;
  footnote?: string;
  /** GitHub 独自の絵文字（`:octocat:` などの画像だけのもの）。`text` を薄い札で見せる */
  custom_emoji?: string;
  line_break?: boolean;
  /** 文中の数式の絵（`text` は書いたままの原文で、`code` の印つき） */
  math?: MdFormula;
  /** 文中の数式を描けなかった理由（`text` は書いたままの原文で、`code` の印つき） */
  math_error?: string;
}
/** コアが描いた数式（色は当ててある）。大きさは em（本文の字の大きさ × `MATH_SCALE`） */
interface MdFormula {
  svg: string;
  width: number;
  height: number;
  depth: number;
}
type MdAlign = 'left' | 'center' | 'right';
interface MdCell {
  row: number;
  column: number;
  rowspan: number;
  colspan: number;
  header?: boolean;
  align?: MdAlign;
  runs: MdRun[];
}
type MdBlock =
  | { kind: 'heading'; level: number; runs: MdRun[]; align?: MdAlign }
  | { kind: 'paragraph'; runs: MdRun[]; align?: MdAlign }
  | { kind: 'code'; language: string; text: string; syntax?: [number, number, number][][] }
  | { kind: 'diagram'; language: string; text: string; svg?: string; note?: string; error?: string }
  | ({ kind: 'math'; text: string; error?: string } & Partial<MdFormula>)
  | { kind: 'quote'; alert?: string; blocks: MdBlock[] }
  | { kind: 'list'; start?: number; items: { task?: boolean; blocks: MdBlock[] }[] }
  | { kind: 'table'; aligns: string[]; head: MdRun[][]; rows: MdRun[][][] }
  | { kind: 'rule' }
  | { kind: 'footnote'; label: string; blocks: MdBlock[] }
  | { kind: 'details'; summary: MdRun[]; open?: boolean; blocks: MdBlock[] }
  | { kind: 'htmlTable'; columns: number; rows: number; cells: MdCell[] };

/** Markdown を整形して見せる（コアが読んだブロックから組み立てる。外からは何も読まない） */
function renderMarkdown(blocks: readonly unknown[], from?: string, onOpen?: (path: string) => void): HTMLElement {
  const box = el('div', 'markdown');
  box.append(...(blocks as MdBlock[]).map(renderBlock));
  // 相対リンク（スキームのないもの）は、このコミットの中のファイルとして開く。外の URL はふつうに開く
  if (from !== undefined && onOpen) {
    box.addEventListener('click', (e) => {
      const a = (e.target as Element).closest('a');
      const href = a?.getAttribute('href');
      if (!href || /^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith('#')) return;
      e.preventDefault();
      const path = resolvePath(from, href.split('#')[0]!.split('?')[0]!);
      if (path) onOpen(path);
    });
  }
  return withFind(box);
}

const mac = navigator.platform.toUpperCase().includes('MAC');

/**
 * 整形した Markdown を探す帯（アプリと同じ）。⌘F（Ctrl+F）で開き、見えている文字（キーの札・表のマスも）から大文字・
 * 小文字を区別せずに探す。絵と閉じた `<details>` の中は探さない。一致の地を塗り、「n / m」を出し、Enter・Shift+Enter・
 * ⌘G・⇧⌘G（F3・Shift+F3）で進む・戻る。帯を閉じているときの ⌘F は VSCode の検索に渡さない
 */
function withFind(box: HTMLElement): HTMLElement {
  const view = el('div', 'markdown-view');
  // 押すと欄に入り、⌘F を受けられる
  view.tabIndex = -1;
  const bar = el('div', 'md-find-bar');
  bar.hidden = true;
  const field = el('input');
  field.type = 'search';
  field.placeholder = 'Search';
  field.setAttribute('aria-label', 'Search');
  const count = el('span', 'search-count');
  const previous = el('button', 'icon-button', '∧');
  previous.title = 'Previous';
  const next = el('button', 'icon-button', '∨');
  next.title = 'Next';
  const done = el('button', 'bar-button', 'Done');
  bar.append(field, count, previous, next, done);
  view.append(bar, box);
  clearHighlights();

  /** 一致（文書の順） */
  let matches: Range[] = [];
  let current: number | undefined;

  const scroller = () => view.closest<HTMLElement>('.file-pane-body') ?? document.documentElement;

  /** 探す文字と、その場所の文字のノード。ブロックの境目には改行を入れる（段落をまたいで当たらない） */
  const collect = () => {
    let text = '';
    const nodes: { node: Text; start: number }[] = [];
    let lastBlock: Element | null = null;
    const walker = document.createTreeWalker(box, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => (visible(n, box) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
    });
    for (let n = walker.nextNode() as Text | null; n; n = walker.nextNode() as Text | null) {
      if (!n.data) continue;
      const block = n.parentElement?.closest(BLOCKS) ?? null;
      if (block !== lastBlock && text) text += '\n';
      lastBlock = block;
      nodes.push({ node: n, start: text.length });
      text += n.data;
    }
    return { text, nodes };
  };

  const update = (reveal: boolean) => {
    const { text, nodes } = collect();
    const at = (offset: number, end: boolean): [Text, number] | undefined => {
      // 終わりは、その文字で終わるノード（次のノードの頭でなく）
      let lo = 0;
      let hi = nodes.length - 1;
      let found = -1;
      while (lo <= hi) {
        const mid = (lo + hi) >> 1;
        const start = nodes[mid]!.start;
        if (end ? start < offset : start <= offset) {
          found = mid;
          lo = mid + 1;
        } else {
          hi = mid - 1;
        }
      }
      const hit = nodes[found];
      return hit ? [hit.node, Math.min(offset - hit.start, hit.node.length)] : undefined;
    };
    matches = [];
    for (const [s, e] of findAll(text, field.value)) {
      const from = at(s, false);
      const to = at(e, true);
      if (!from || !to) continue;
      const range = document.createRange();
      range.setStart(from[0], from[1]);
      range.setEnd(to[0], to[1]);
      matches.push(range);
    }
    // 今の一致は、見えている所より後の最初のもの（アプリと同じ）
    const top = scroller().getBoundingClientRect().top + bar.offsetHeight;
    const first = matches.findIndex((r) => r.getBoundingClientRect().bottom > top);
    current = matches.length === 0 ? undefined : Math.max(0, first);
    paint();
    if (reveal) revealCurrent();
  };

  const paint = () => {
    count.textContent = findCountText(field.value, current, matches.length);
    const highlights = highlightRegistry();
    if (!highlights) return;
    highlights.set(FIND_HIGHLIGHT, new Highlight(...matches.filter((_, i) => i !== current)));
    highlights.set(CURRENT_HIGHLIGHT, new Highlight(...(current === undefined ? [] : [matches[current]!])));
  };

  const revealCurrent = () => {
    if (current === undefined) return;
    const range = matches[current]!;
    const s = scroller();
    const area = s.getBoundingClientRect();
    const rect = range.getBoundingClientRect();
    const top = area.top + bar.offsetHeight;
    if (rect.top < top || rect.bottom > area.bottom) s.scrollTop += rect.top - top - (area.bottom - top) / 3;
  };

  const move = (step: number) => {
    if (bar.hidden) show();
    if (matches.length === 0) return;
    current = ((current ?? -step) + step + matches.length) % matches.length;
    paint();
    revealCurrent();
  };

  const show = () => {
    const selected = window.getSelection()?.toString() ?? '';
    if (selected && !selected.includes('\n')) field.value = selected;
    bar.hidden = false;
    field.focus();
    field.select();
    update(true);
  };

  const hide = () => {
    bar.hidden = true;
    matches = [];
    current = undefined;
    clearHighlights();
    count.textContent = '';
    view.focus();
  };

  field.addEventListener('input', () => update(true));
  previous.addEventListener('click', () => move(-1));
  next.addEventListener('click', () => move(1));
  done.addEventListener('click', hide);
  // `<details>` を開け閉めしたら、見えている文字が変わる（toggle は泡立たないので捕まえる側で）
  box.addEventListener('toggle', () => (bar.hidden ? undefined : update(false)), true);
  view.addEventListener('keydown', (e) => {
    const command = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
    const key = e.key.toLowerCase();
    let handled = true;
    if (command && !e.altKey && key === 'f' && !e.shiftKey) {
      show();
    } else if (!bar.hidden && ((command && !e.altKey && key === 'g') || (e.key === 'F3' && !command))) {
      // 帯を閉じているときは、ほか（History の検索）に任せる
      move(e.shiftKey ? -1 : 1);
    } else if (e.target === field && e.key === 'Enter') {
      move(e.shiftKey ? -1 : 1);
    } else if (e.target === field && e.key === 'Escape') {
      hide();
    } else {
      handled = false;
    }
    if (handled) {
      // VSCode の検索の窓（enableFindWidget）に渡さない
      e.preventDefault();
      e.stopPropagation();
    }
  });
  return view;
}

/** 段落の区切りになる要素 */
const BLOCKS = 'p, li, td, th, h1, h2, h3, h4, h5, h6, pre, summary, blockquote, div, figure';

/** 見えている文字か（閉じた `<details>` の中は見出しのほか見えない） */
function visible(n: Node, root: HTMLElement): boolean {
  const parent = n.parentElement;
  if (!parent) return false;
  for (let d = parent.closest('details'); d && root.contains(d); d = d.parentElement?.closest('details') ?? null) {
    if (d.open) continue;
    const summary = d.querySelector(':scope > summary');
    if (!summary?.contains(n)) return false;
  }
  return true;
}

const FIND_HIGHLIGHT = 'amonhen-md-find';
const CURRENT_HIGHLIGHT = 'amonhen-md-find-current';

function highlightRegistry(): HighlightRegistry | undefined {
  return typeof CSS !== 'undefined' && 'highlights' in CSS ? CSS.highlights : undefined;
}

function clearHighlights(): void {
  const highlights = highlightRegistry();
  highlights?.delete(FIND_HIGHLIGHT);
  highlights?.delete(CURRENT_HIGHLIGHT);
}


/** 本文の字の大きさに対する数式の 1 em（コアの `math::FONT_SCALE`。KaTeX と同じ） */
const MATH_SCALE = 1.21;

/** 図・数式の絵があるか（テーマが変わったら描き直す） */
function hasPictures(blocks: readonly unknown[]): boolean {
  return JSON.stringify(blocks).includes('"svg":');
}

/** 図・数式・VectorDrawable の絵に当てる、今の画面の色（文字・地と、構文の種類の色） */
function pictureColors(): PictureColors {
  const probe = el('span');
  probe.style.display = 'none';
  document.body.append(probe);
  const color = (className: string) => {
    probe.className = className;
    return hexColor(getComputedStyle(probe).color);
  };
  const text = color('');
  probe.className = '';
  probe.style.backgroundColor = 'var(--vscode-editor-background)';
  const background = hexColor(getComputedStyle(probe).backgroundColor) ?? hexColor(getComputedStyle(document.body).backgroundColor);
  probe.style.backgroundColor = '';
  const kinds: Record<string, string> = {};
  for (const kind of ['keyword', 'string', 'comment', 'number', 'function', 'type', 'constant', 'attribute', 'tag']) {
    const c = color(`syn-${kind}`);
    if (c) kinds[kind] = c;
  }
  probe.remove();
  const dark = darkTheme();
  return { dark, text: text ?? (dark ? '#cccccc' : '#333333'), background: background ?? (dark ? '#1e1e1e' : '#ffffff'), kinds };
}

/** 数式の絵（`display` ならブロックの組み方で中央に、でなければ文中で基線にそろえる） */
function formulaImage(formula: MdFormula, tex: string, display: boolean): HTMLElement {
  const img = el('img', display ? 'md-math-block' : 'md-math');
  img.src = `data:image/svg+xml;base64,${base64(formula.svg)}`;
  img.alt = tex;
  img.title = tex;
  img.style.width = `${formula.width * MATH_SCALE}em`;
  img.style.height = `${(formula.height + formula.depth) * MATH_SCALE}em`;
  if (!display) img.style.verticalAlign = `-${formula.depth * MATH_SCALE}em`;
  return img;
}

/** 描けなかった図・数式の理由（誤りの位置を ^ で指すので等幅で。アプリと同じく赤） */
function pictureError(error: string): HTMLElement {
  return el('pre', 'md-picture-error', error);
}

/** 今の外観が暗いか（`<picture>` の画像を選ぶ） */
function darkTheme(): boolean {
  return document.body.classList.contains('vscode-dark') || document.body.classList.contains('vscode-high-contrast');
}

/** リンクで包む */
function linked(node: HTMLElement, link: string | undefined): HTMLElement {
  if (!link) return node;
  const a = el('a');
  a.href = link;
  a.title = link;
  a.append(node);
  return a;
}

function renderRuns(runs: readonly MdRun[]): Node[] {
  return runs.map((run) => {
    if (run.line_break) return el('br');
    const image = (darkTheme() ? run.image_dark : run.image_light) ?? run.image;
    if (image) {
      // コミットの中の絵は拡張ホストが data URL にしてある。外の URL は読まず、代わりの文を出す
      if (image.startsWith('data:')) {
        const img = el('img', 'md-img');
        img.src = image;
        img.alt = run.text;
        if (run.width) img.width = run.width;
        // 割合は GitHub（ブラウザ）と同じく、入っている欄（表ならマス）の幅に対して
        else if (run.width_percent) img.style.width = `${run.width_percent}%`;
        if (run.height) img.height = run.height;
        return linked(img, run.link);
      }
      const alt = el('span', 'md-image', `🖼 ${run.text || image}`);
      alt.title = image;
      return linked(alt, run.link);
    }
    if (run.footnote) return el('sup', 'md-footnote', `[${run.footnote}]`);
    // 文中の数式: 基線を本文にそろえる。原文は浮かぶ説明に（アプリと同じ）
    if (run.math) return linked(formulaImage(run.math, run.text, false), run.link);
    if (run.custom_emoji) {
      // GitHub 独自の絵文字は画像を持たないので、名前を薄い札で見せる（アプリと同じ）
      const chip = el('span', 'md-emoji-chip', run.text);
      chip.title = 'GitHub’s own emoji, shown by name (its image isn’t included)';
      return chip;
    }
    let node: HTMLElement = el(run.kbd ? 'kbd' : run.code ? 'code' : 'span', '', run.text);
    if (run.math_error) {
      // 描けなかった文中の数式は原文をコードとして見せ、理由を浮かぶ説明に（アプリと同じ）
      node.classList.add('md-math-error');
      node.title = run.math_error;
    }
    if (run.bold) node = wrapIn('strong', node);
    if (run.italic) node = wrapIn('em', node);
    if (run.strike) node = wrapIn('s', node);
    if (run.underline) node = wrapIn('ins', node);
    if (run.sub) node = wrapIn('sub', node);
    if (run.sup) node = wrapIn('sup', node);
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

/** コードとして見せる図・数式と、描けなかった理由 */
function codeWithError(language: string, text: string, error: string | undefined): HTMLElement {
  const pre = el('pre', 'md-code');
  pre.title = language;
  pre.append(el('code', '', text));
  if (!error) return pre;
  const box = el('div');
  box.append(pre, pictureError(error));
  return box;
}

function wrapIn(tag: 'strong' | 'em' | 's' | 'ins' | 'sub' | 'sup', child: Node): HTMLElement {
  const e = el(tag);
  e.append(child);
  return e;
}

function renderBlock(block: MdBlock): HTMLElement {
  switch (block.kind) {
    case 'heading': {
      const h = el(`h${Math.min(6, Math.max(1, block.level))}` as 'h1');
      if (block.align) h.style.textAlign = block.align;
      h.append(...renderRuns(block.runs));
      return h;
    }
    case 'paragraph': {
      const p = el('p');
      if (block.align) p.style.textAlign = block.align;
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
    case 'diagram': {
      if (block.svg) {
        // コアが描いた図（列より広い図は縮める）。近いもので描いた図には、そのことを絵の下に添える（アプリと同じ）
        const figure = el('div', 'md-diagram');
        const img = el('img');
        img.src = `data:image/svg+xml;base64,${base64(block.svg)}`;
        img.alt = block.text;
        figure.append(img);
        if (block.note) figure.append(el('div', 'md-diagram-note', block.note));
        return figure;
      }
      // 描けない図（描かない古いエンジンも）はコードとして見せ、理由を添える
      return codeWithError(block.language, block.text, block.error);
    }
    case 'math': {
      if (block.svg !== undefined && block.width !== undefined && block.height !== undefined && block.depth !== undefined) {
        // 数式のブロックは中央に（原文は浮かぶ説明に）
        const p = el('div', 'md-math-display');
        p.append(formulaImage({ svg: block.svg, width: block.width, height: block.height, depth: block.depth }, block.text, true));
        return p;
      }
      // 描けない数式（描かない古いエンジンも）は TeX をコードとして見せ、理由を添える
      return codeWithError('math', block.text, block.error);
    }
    case 'rule':
      return el('hr');
    case 'details': {
      // HTML の <details>（押すと開く・閉じる。見出しがなければ GitHub と同じ「Details」）
      const details = el('details', 'md-details');
      details.open = block.open === true;
      const summary = el('summary');
      if (block.summary.length > 0) summary.append(...renderRuns(block.summary));
      else summary.textContent = 'Details';
      details.append(summary, ...block.blocks.map(renderBlock));
      return details;
    }
    case 'htmlTable': {
      // HTML の表（マスは行・列の順で、colspan・rowspan を解いてある）
      const table = el('table', 'md-table');
      let tr: HTMLTableRowElement | undefined;
      let row = -1;
      for (const cell of block.cells) {
        if (cell.row !== row || !tr) {
          tr = el('tr');
          table.append(tr);
          row = cell.row;
        }
        const td = el(cell.header ? 'th' : 'td');
        if (cell.colspan > 1) td.colSpan = cell.colspan;
        if (cell.rowspan > 1) td.rowSpan = cell.rowspan;
        if (cell.align) td.style.textAlign = cell.align;
        td.append(...renderRuns(cell.runs));
        tr.append(td);
      }
      return table;
    }
    case 'footnote': {
      const div = el('div', 'md-footnote-body');
      div.append(el('sup', '', `[${block.label}]`), ...block.blocks.map(renderBlock));
      return div;
    }
  }
}
