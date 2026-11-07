/** Working Copy のファイル一覧の絞り込みと木。vscode に依存しない */

/** ファイル一覧の見せ方: パス・ディレクトリの木・ファイル名（親のディレクトリは薄く） */
export type FileLayout = 'list' | 'tree' | 'flat';

/** 木のディレクトリ。子が 1 つのディレクトリだけなら名前をつないで 1 段にする（アプリと同じ） */
export interface FileDir<T> {
  name: string;
  /** リポジトリの中のパス（読み直しても畳んだままにするため） */
  path: string;
  dirs: FileDir<T>[];
  files: T[];
}

/** パスに query を含むものだけ（大文字小文字を区別しない） */
export function filterByPath<T extends { path: string }>(items: readonly T[], query: string): T[] {
  const q = query.trim().toLowerCase();
  return q ? items.filter((item) => item.path.toLowerCase().includes(q)) : [...items];
}

export function makeFileTree<T extends { path: string }>(items: readonly T[]): FileDir<T> {
  const root: FileDir<T> = { name: '', path: '', dirs: [], files: [] };
  for (const item of items) {
    let dir = root;
    const parts = item.path.split('/');
    for (const part of parts.slice(0, -1)) {
      let next = dir.dirs.find((d) => d.name === part);
      if (!next) {
        next = { name: part, path: dir.path ? `${dir.path}/${part}` : part, dirs: [], files: [] };
        dir.dirs.push(next);
      }
      dir = next;
    }
    dir.files.push(item);
  }
  const sort = (dirs: FileDir<T>[]) => dirs.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  const compact = (dir: FileDir<T>) => {
    while (dir.files.length === 0 && dir.dirs.length === 1) {
      const only = dir.dirs[0]!;
      dir.name += `/${only.name}`;
      dir.path = only.path;
      dir.dirs = only.dirs;
      dir.files = only.files;
    }
    sort(dir.dirs);
    dir.dirs.forEach(compact);
  };
  root.dirs.forEach(compact);
  sort(root.dirs);
  return root;
}

/** 配下のすべてのファイル */
export function filesUnder<T>(dir: FileDir<T>): T[] {
  return [...dir.dirs.flatMap(filesUnder), ...dir.files];
}
