/** Working Copy の untracked のファイルを .gitignore に足す（コアで読み書きするので、SSH 先でも同じ） */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { ignorePatterns, withIgnorePattern } from '../workingCopy/ignore.ts';
import { showError } from './stageCommands.ts';

export function registerIgnoreCommand(manager: RepositoryManager): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.addToGitignore', async (context: { path: string }) => {
    const repo = manager.current;
    const client = repo && (await daemonFor(repo.root));
    if (!repo || !client) return;
    const patterns = ignorePatterns(context.path);
    const pattern =
      patterns.length > 1
        ? await vscode.window.showQuickPick(patterns, { placeHolder: 'Add to .gitignore' })
        : patterns[0];
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
  });
}
