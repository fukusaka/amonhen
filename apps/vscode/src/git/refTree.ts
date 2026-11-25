/**
 * ref の名前の `/` で分けたフォルダ（アプリのサイドバーのタグと同じ並べ方）。名前の順（コアと同じバイトの順）に
 * 並べ、フォルダは中の最初の名前の位置に出す（フォルダを先にまとめない）。vscode に依存しない
 */

export type RefTree<T> =
  | { kind: 'folder'; label: string; path: string; children: RefTree<T>[] }
  | { kind: 'leaf'; label: string; value: T };

/** 名前の順（UTF-16 の符号の順。ref の名前ではバイトの順とほぼ同じ） */
export function byName(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

export function refTree<T>(items: readonly { name: string; value: T }[]): RefTree<T>[] {
  const root: RefTree<T>[] = [];
  const folders = new Map<string, Extract<RefTree<T>, { kind: 'folder' }>>();
  for (const item of [...items].sort((a, b) => byName(a.name, b.name))) {
    const parts = item.name.split('/');
    const leaf = parts.pop()!;
    let children = root;
    let path = '';
    for (const part of parts) {
      path = path ? `${path}/${part}` : part;
      let folder = folders.get(path);
      if (!folder) {
        folder = { kind: 'folder', label: part, path, children: [] };
        folders.set(path, folder);
        children.push(folder);
      }
      children = folder.children;
    }
    children.push({ kind: 'leaf', label: leaf, value: item.value });
  }
  return root;
}
