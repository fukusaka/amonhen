import * as vscode from 'vscode';
import { parseDecorationUri } from './uri.ts';

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
    const letter = parseDecorationUri(uri);
    if (!letter) {
      return undefined;
    }
    const color = COLORS[letter];
    return new vscode.FileDecoration(letter, undefined, color ? new vscode.ThemeColor(color) : undefined);
  }
}
