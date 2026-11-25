import * as vscode from 'vscode';

// どの FE でも同じ形のアイコン（crates/icons/svg、docs/icons.md）。media/icons/{light,dark}/ に、色を書き込み画素に
// 合わせた SVG を置く（`cargo run -p amonhen-icons --example vscode` で作る。拡張の SVG は画像として描かれ、文字の色を
// 受け継がないため）。

let root: vscode.Uri | undefined;

/** 拡張の置き場所（activate で一度だけ）。 */
export function setSharedIconsRoot(extensionUri: vscode.Uri): void {
  root = extensionUri;
}

/** 共通のアイコン `name` の、ライト・ダークの 2 枚。`green` はほかの worktree のブランチの色（charts.green）。 */
export function sharedIcon(name: string, green = false): { light: vscode.Uri; dark: vscode.Uri } | undefined {
  if (!root) return undefined;
  const file = `${name}${green ? '-green' : ''}.svg`;
  return {
    light: vscode.Uri.joinPath(root, 'media', 'icons', 'light', file),
    dark: vscode.Uri.joinPath(root, 'media', 'icons', 'dark', file),
  };
}
