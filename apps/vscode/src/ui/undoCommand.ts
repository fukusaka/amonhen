/**
 * Undo。知らせの Undo のボタン（操作の直後に押す）は尋ねずに戻し、サイドバーの上の Undo とコマンドパレットは、何を
 * 戻すかを見せて確かめてから戻す（アプリと同じ）。Undo History… は記録を新しい順に並べ、選んだところまで戻す
 */
import * as vscode from 'vscode';
import { moveDrawing, type Drawing, type MovePreview } from '../history/drawing.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { Repository } from '../state/repository.ts';
import type { UndoEntry } from '../state/undo.ts';
import { confirmOperation } from './confirm.ts';
import { MainPanel } from './mainPanel.ts';
import { showError } from './stageCommands.ts';

export function registerUndoCommand(manager: RepositoryManager): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.undo', () => undo(manager, 1)),
    vscode.commands.registerCommand('amonhen.undoWithConfirmation', () => undoWithConfirmation(manager)),
    vscode.commands.registerCommand('amonhen.undoHistory', () => undoHistory(manager)),
  );
}

/** 何を戻すか（記録の種類による。アプリの言い方） */
export function undoWhat(entry: UndoEntry): string {
  if (entry.files) return 'Puts the files back as they were. HEAD and the index stay.';
  if (entry.stash) return 'Brings back the dropped stash.';
  if (entry.rename) return 'Gives the branch its earlier name back.';
  return 'Returns HEAD and the branches to where they were.';
}

async function undoWithConfirmation(manager: RepositoryManager): Promise<void> {
  const repo = manager.current;
  if (!repo) return;
  await repo.undo.settlePending().catch(() => false);
  const entry = await repo.undo.peek();
  if (!entry) {
    void vscode.window.showInformationMessage('Nothing to Undo');
    return;
  }
  const title = `Undo “${entry.label}”?`;
  // メインビューが開いていれば、アプリと同じく、ブランチが戻る前と後の絵を見せて確かめる
  const ok = MainPanel.active?.canAsk
    ? await confirmOperation(title, [undoWhat(entry)], 'Undo', { drawing: () => undoDrawing(repo, entry) })
    : (await vscode.window.showWarningMessage(title, { modal: true, detail: undoWhat(entry) }, 'Undo')) === 'Undo';
  if (ok) await undo(manager, 1);
}

/** 戻すブランチ（今のブランチを先に。作られた・消されたブランチは描かない）の前と後の絵 */
async function undoDrawing(repo: Repository, entry: UndoEntry): Promise<Drawing | undefined> {
  if (entry.files || entry.stash || entry.rename) return undefined;
  const moves = Object.entries(entry.after.refs).flatMap(([ref, now]) => {
    const back = entry.before.refs[ref];
    return now && back && now !== back && ref.startsWith('refs/heads/') ? [{ ref, now, back }] : [];
  });
  const move = moves.find((m) => m.ref === entry.after.head.branch) ?? moves[0];
  if (!move) return undefined;
  const preview = await repo.backend.graphPreview<MovePreview>({ op: 'movePreview', from: move.now, to: move.back });
  return moveDrawing(preview, { moving: shortName(move.ref), kind: 'local', afterTitle: 'After the undo', lostNote: 'taken off' });
}

async function undoHistory(manager: RepositoryManager): Promise<void> {
  const repo = manager.current;
  if (!repo) return;
  await repo.undo.settlePending().catch(() => false);
  const entries = await repo.undo.list();
  if (entries.length === 0) {
    void vscode.window.showInformationMessage('Nothing to Undo');
    return;
  }
  const picked = await vscode.window.showQuickPick(
    entries.map((e, i) => ({
      label: e.label,
      description: new Date(e.time).toLocaleString(),
      detail: i === 0 ? 'Undo this operation' : `Undo the ${i + 1} latest operations, down to and including this one`,
      count: i + 1,
    })),
    {
      title: 'Undo History',
      placeHolder: 'Operations you can Undo (newest first). It stops where another tool has moved a ref.',
    },
  );
  if (!picked) return;
  const answer = await vscode.window.showWarningMessage(
    picked.count === 1 ? `Undo “${picked.label}”?` : `Undo the last ${picked.count} operations?`,
    { modal: true, detail: picked.count === 1 ? undoWhat(entries[0]!) : 'Returns HEAD and the branches to where they were.' },
    'Undo',
  );
  if (answer === 'Undo') await undo(manager, picked.count);
}

async function undo(manager: RepositoryManager, count: number): Promise<void> {
  const repo = manager.current;
  if (!repo) {
    return;
  }
  try {
    let last: UndoEntry | undefined;
    for (let i = 0; i < count; i++) {
      const entry = await vscode.window.withProgress({ location: vscode.ProgressLocation.Window, title: 'Undo' }, () =>
        repo.undo.undo(),
      );
      if (!entry) break;
      last = entry;
      // 次の Undo は、戻した後の ref で確かめる
      if (i + 1 < count) await repo.refresh();
    }
    if (last) {
      vscode.window.setStatusBarMessage(`$(discard) Undo: ${count > 1 ? `${count} operations` : last.label}`, 5000);
    } else {
      void vscode.window.showInformationMessage('Nothing to Undo');
    }
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}
