/**
 * メインビューに重ねる確認のシート（アプリのシートと同じく、前と後の絵・見通しの文・ボタン）。既定のボタンは Enter、
 * Cancel は Esc。答えは sheetAnswer で返す
 */
import type { Sheet } from '../view/protocol.ts';
import { el, post } from './api.ts';
import { rewriteDrawing } from './rewriteDrawing.ts';

let open: { id: number; close: () => void } | undefined;

export function openSheet(sheet: Sheet): void {
  // 前のシートが残っていれば、やめたことにする
  open?.close();
  const overlay = el('div', 'overlay');
  const box = el('div', 'editor sheet');
  box.setAttribute('role', 'dialog');
  box.append(el('div', 'heading', sheet.title));
  const drawing = sheet.preview && rewriteDrawing(sheet.preview);
  if (drawing) box.append(drawing);
  if (sheet.lines.length > 0) {
    const lines = el('div', 'sheet-lines');
    for (const line of sheet.lines) lines.append(el('div', line.startsWith('⚠︎') ? 'warning' : undefined, line));
    box.append(lines);
  }
  const buttons = el('div', 'buttons');
  const cancel = el('button', 'secondary', 'Cancel');
  buttons.append(cancel);
  const answer = (index?: number) => {
    overlay.remove();
    document.removeEventListener('keydown', keys, true);
    open = undefined;
    post({ type: 'sheetAnswer', id: sheet.id, index });
  };
  // 既定のボタンを右端に（アプリのシートと同じ並び）
  const items = sheet.buttons.map((title, index) => {
    const b = el('button', index === 0 ? undefined : 'secondary', title);
    b.addEventListener('click', () => answer(index));
    return b;
  });
  buttons.append(...items.slice(1).reverse(), ...items.slice(0, 1));
  cancel.addEventListener('click', () => answer());
  box.append(buttons);
  overlay.append(box);
  const keys = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      answer();
    } else if (e.key === 'Enter' && document.activeElement?.tagName !== 'BUTTON') {
      e.preventDefault();
      e.stopPropagation();
      answer(0);
    }
  };
  document.addEventListener('keydown', keys, true);
  document.body.append(overlay);
  items[0]?.focus();
  open = { id: sheet.id, close: () => answer() };
}
