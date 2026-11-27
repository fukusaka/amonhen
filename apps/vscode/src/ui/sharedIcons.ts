import * as vscode from 'vscode';

// どの FE でも同じ形のアイコン（crates/icons/svg、docs/icons.md）。形はフォント（media/amonhen-icons.ttf）の字で、
// package.json の contributes.icons に `amonhen-<名前>` で登録してある（`cargo run -p amonhen-icons --example vscode`
// で作る）。codicon と同じように描かれ、テーマのアイコンの色・ハイコントラスト・選んだ行の色に従う。

/** 状態の色（VSCode のテーマの色）。 */
export type SharedIconColor = 'green' | 'yellow' | 'orange' | 'red' | 'warning';

const THEME_COLORS: Record<SharedIconColor, string> = {
  green: 'charts.green',
  yellow: 'charts.yellow',
  orange: 'charts.orange',
  red: 'charts.red',
  warning: 'list.warningForeground',
};

/** 共通のアイコン `name`（`color` があればその色）。 */
export function sharedIcon(name: string, color?: SharedIconColor | false): vscode.ThemeIcon {
  return new vscode.ThemeIcon(`amonhen-${name}`, color ? new vscode.ThemeColor(THEME_COLORS[color]) : undefined);
}
