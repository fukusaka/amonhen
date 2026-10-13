import * as vscode from 'vscode';

export const SCHEME = 'amonhen';

/** サイドバーの変更ファイルに付けるバッジ用の URI。ファイルアイコンはパスから決まる */
export function decorationUri(path: string, letter: string): vscode.Uri {
  return vscode.Uri.file(path).with({ scheme: SCHEME, query: new URLSearchParams({ decoration: letter }).toString() });
}

const COLORS: Record<string, string> = {
  M: 'gitDecoration.modifiedResourceForeground',
  T: 'gitDecoration.modifiedResourceForeground',
  A: 'gitDecoration.addedResourceForeground',
  C: 'gitDecoration.addedResourceForeground',
  D: 'gitDecoration.deletedResourceForeground',
  R: 'gitDecoration.renamedResourceForeground',
  U: 'gitDecoration.untrackedResourceForeground',
  '!': 'gitDecoration.conflictingResourceForeground',
};

export class ChangeDecorationProvider implements vscode.FileDecorationProvider {
  provideFileDecoration(uri: vscode.Uri): vscode.FileDecoration | undefined {
    if (uri.scheme !== SCHEME) {
      return undefined;
    }
    const letter = new URLSearchParams(uri.query).get('decoration');
    if (!letter) {
      return undefined;
    }
    const color = COLORS[letter];
    return new vscode.FileDecoration(letter, undefined, color ? new vscode.ThemeColor(color) : undefined);
  }
}
