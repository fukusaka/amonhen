/** Working Copy の untracked のファイルを .gitignore に足す（コアで読み書きするので、SSH 先でも同じ） */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { ignorePatterns, withIgnorePattern } from '../workingCopy/ignore.ts';
import { showError } from './stageCommands.ts';

export function registerIgnoreCommand(manager: RepositoryManager): vscode.Disposable {
  /** `which` 番目の候補（0 はそのパス、1 は拡張子のパターン）を足す。アプリと同じく、右クリックで別々の項目 */
  const add = (which: 0 | 1) => async (context: { path: string }) => {
    const repo = manager.current;
    const client = repo && (await daemonFor(repo.root));
    if (!repo || !client) return;
    const pattern = ignorePatterns(context.path)[which];
    if (!pattern) return;
    try {
      const run = async (command: Record<string, unknown>) => {
        const reply = (await client.call({ op: 'run', command })).json;
        if (reply['ok'] !== true) throw new Error(String(reply['error'] ?? 'Can’t save .gitignore'));
        return reply['info'] as { gitignore?: string; exclude?: string } | undefined;
      };
      const files = await run({ op: 'ignoreFiles' });
      const gitignore = withIgnorePattern(files?.gitignore ?? '', pattern);
      if (gitignore !== undefined) await run({ op: 'saveIgnoreFiles', gitignore, exclude: files?.exclude ?? '' });
    } catch (err) {
      showError(err);
    } finally {
      await repo.refresh();
    }
  };
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.addToGitignore', add(0)),
    vscode.commands.registerCommand('amonhen.addExtensionToGitignore', add(1)),
  );
}
