/**
 * Working Copy の「すべてのファイル」（All Files）: HEAD の木に status を重ねる（アプリの WorkingTreeFiles と同じ）。
 * 足したファイル・untracked は木に足し、変えたファイルに一覧と同じ印（コンフリクトは ⚠）、消したファイルと名前を
 * 変える前の名前は取り消し線、変更を含むフォルダは ● を付ける。無視したファイルは status に出ないので木にも出ない。
 * vscode と DOM に依存しない
 */
import type { TreeEntry } from '../history/protocol.ts';
import type { Change } from '../state/changes.ts';
import { mixedRows } from './sides.ts';
import type { FileLayout } from './fileList.ts';

/** 変えたファイルの印 */
export interface TreeMark {
  /** 一覧（Mixed）の行と同じ印。コンフリクトは ⚠ */
  letter: string;
  /** コンフリクト */
  conflict: boolean;
  /** 作業ツリーにもうない（消した、名前を変える前の名前） */
  deleted: boolean;
}

/** status を重ねるもの: 変えたファイルの印と、変更を含むフォルダ（根の '' も） */
export interface StatusOverlay {
  marks: Map<string, TreeMark>;
  folders: Set<string>;
}

/** 木の 1 つ */
export interface TreeNode {
  name: string;
  path: string;
  folder: boolean;
  /** submodule（開けない） */
  submodule: boolean;
  /** HEAD の木にある（フォルダなら、中身を HEAD から読む。なければ status で足したものだけ） */
  inHead: boolean;
  mark?: TreeMark;
  /** 変えたファイル・変更を含むフォルダ */
  changes: boolean;
}

function parentOf(path: string): string {
  const at = path.lastIndexOf('/');
  return at < 0 ? '' : path.slice(0, at);
}

/** status から、変えたファイルの印と変更を含むフォルダを作る */
export function statusOverlay(files: readonly Change[]): StatusOverlay {
  const marks = new Map<string, TreeMark>();
  const folders = new Set<string>(['']);
  const addFolders = (path: string) => {
    for (let folder = parentOf(path); folder !== ''; folder = parentOf(folder)) folders.add(folder);
  };
  const rows = mixedRows(files);
  for (const row of rows) {
    const conflict = row.section === 'conflicts';
    // コンフリクトの印が先（同じパスのほかの行では替えない）
    if (marks.get(row.path)?.conflict) continue;
    // 作業ツリーで消した、または消したのを stage した（同じ名前の untracked のファイルがまたあれば、消していない）
    const deleted = !conflict && (row.unstaged?.letter === 'D' || (row.staged?.letter === 'D' && !row.unstaged));
    marks.set(row.path, { letter: conflict ? '⚠' : row.letter, conflict, deleted });
    addFolders(row.path);
  }
  // 名前を変える前の名前は、作業ツリーにはもうない（消したファイルと同じく取り消し線。同じ名前の変更があればそちら）
  for (const row of rows) {
    if (!row.origPath) continue;
    if (!marks.has(row.origPath)) marks.set(row.origPath, { letter: 'D', conflict: false, deleted: true });
    addFolders(row.origPath);
  }
  return { marks, folders };
}

/**
 * フォルダ folder の中身: HEAD の木の entries（HEAD にないフォルダなら空）に、HEAD にない変更のファイル・フォルダを
 * 足し、印を付ける。フォルダが先、それから名前の順（大文字と小文字は区別しない。アプリと同じ）
 */
export function folderChildren(folder: string, entries: readonly TreeEntry[], overlay: StatusOverlay): TreeNode[] {
  const prefix = folder ? `${folder}/` : '';
  const nodes: TreeNode[] = entries.map((e) => ({
    name: e.name,
    path: `${prefix}${e.name}`,
    folder: e.kind === 'folder',
    submodule: e.kind === 'submodule',
    inHead: true,
    changes: false,
  }));
  const names = new Set(nodes.map((n) => n.name));
  for (const path of overlay.marks.keys()) {
    if (parentOf(path) !== folder) continue;
    const name = path.slice(prefix.length);
    if (names.has(name)) continue;
    names.add(name);
    nodes.push({ name, path, folder: false, submodule: false, inHead: false, changes: false });
  }
  for (const path of overlay.folders) {
    if (path === '' || parentOf(path) !== folder) continue;
    const name = path.slice(prefix.length);
    if (names.has(name)) continue;
    names.add(name);
    nodes.push({ name, path, folder: true, submodule: false, inHead: false, changes: false });
  }
  for (const node of nodes) {
    if (node.folder) {
      node.changes = overlay.folders.has(node.path);
    } else {
      const mark = overlay.marks.get(node.path);
      if (mark) node.mark = mark;
      node.changes = mark !== undefined;
    }
  }
  return nodes.sort((a, b) => {
    if (a.folder !== b.folder) return a.folder ? -1 : 1;
    const x = a.name.toLowerCase();
    const y = b.name.toLowerCase();
    return x < y ? -1 : x > y ? 1 : 0;
  });
}

/** リスト（パスの順）とファイル名（名前の順、同じ名前はパスの順）で並べるファイル（アプリと同じ） */
export function flatOrder(files: readonly TreeNode[], layout: FileLayout): TreeNode[] {
  const key = (n: TreeNode) => (layout === 'flat' ? [n.name.toLowerCase(), n.path.toLowerCase()] : [n.path.toLowerCase()]);
  return [...files].sort((a, b) => {
    const x = key(a);
    const y = key(b);
    for (let i = 0; i < x.length; i++) {
      if (x[i]! !== y[i]!) return x[i]! < y[i]! ? -1 : 1;
    }
    return 0;
  });
}
