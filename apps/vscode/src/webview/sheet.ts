/**
 * メインビューに重ねる確認のシート（アプリのシートと同じく、前と後の絵・選ぶもの・見通しの文・コミットの一覧・
 * ボタン）。選ぶもの（ラジオ）を選び直すと、絵と文がそれに替わる。一覧の行を押すと差分を開く（シートは閉じない）。
 * 既定のボタンは Enter、Cancel は Esc。答えは sheetAnswer で返す
 */
import type { Sheet } from '../view/protocol.ts';
import { el, post } from './api.ts';
import { drawing } from './drawing.ts';

let open: { id: number; close: () => void } | undefined;

export function openSheet(sheet: Sheet): void {
  // 前のシートが残っていれば、やめたことにする
  open?.close();
  const overlay = el('div', 'overlay');
  const box = el('div', 'editor sheet');
  box.setAttribute('role', 'dialog');
  box.append(el('div', 'heading', sheet.title));
  // 絵と文（選ぶものがあれば、選んでいるものの）
  const picture = el('div', 'sheet-drawing');
  const text = el('div', 'sheet-lines');
  let choice = sheet.choice ?? 0;
  const buttons = el('div', 'buttons');
  const cancel = el('button', 'secondary', 'Cancel');
  buttons.append(cancel);
  const answer = (index?: number) => {
    overlay.remove();
    document.removeEventListener('keydown', keys, true);
    open = undefined;
    post({ type: 'sheetAnswer', id: sheet.id, index, choice: sheet.choices ? choice : undefined });
  };
  // 既定のボタンを右端に（アプリのシートと同じ並び）
  const items = sheet.buttons.map((title, index) => {
    const b = el('button', index === 0 ? (sheet.destructive ? 'destructive' : undefined) : 'secondary', title);
    b.addEventListener('click', () => answer(index));
    return b;
  });
  const show = () => {
    const c = sheet.choices?.[choice];
    const d = c?.drawing ?? sheet.drawing;
    picture.replaceChildren(...(d ? [drawing(d)] : []));
    picture.hidden = !d;
    const lines = [...sheet.lines, ...(c?.lines ?? [])];
    text.replaceChildren(
      ...lines.map((line) => el('div', undefined, line)),
      ...(sheet.warnings ?? []).map((line) => el('div', 'warning', `⚠︎ ${line}`)),
    );
    text.hidden = text.childElementCount === 0;
    if (items[0]) items[0].disabled = !!c?.blocked;
  };
  box.append(picture);
  if (sheet.choices) {
    const radios = el('div', 'sheet-choices');
    sheet.choices.forEach((c, i) => {
      const label = el('label');
      const radio = el('input');
      radio.type = 'radio';
      radio.name = `sheet-${sheet.id}`;
      radio.checked = i === choice;
      radio.addEventListener('change', () => {
        choice = i;
        show();
      });
      label.append(radio, ` ${c.label}`);
      radios.append(label);
    });
    box.append(radios);
  }
  box.append(text);
  // コミットの一覧（押せる行は、通して数えた番号で知らせる）
  let n = 0;
  for (const list of sheet.lists ?? []) {
    const section = el('div', list.tone === 'red' ? 'sheet-list red' : 'sheet-list');
    section.append(el('div', 'sheet-list-title', list.title));
    for (const item of list.items) {
      const row = el(item.open ? 'button' : 'div', 'sheet-list-item');
      row.append(el('span', 'label', item.label));
      if (item.description) row.append(el('span', 'description', item.description));
      const at = n++;
      if (item.open) row.addEventListener('click', () => post({ type: 'sheetOpen', id: sheet.id, item: at }));
      section.append(row);
    }
    if (list.more) section.append(el('div', 'sheet-list-more', list.more));
    box.append(section);
  }
  buttons.append(...items.slice(1).reverse(), ...items.slice(0, 1));
  cancel.addEventListener('click', () => answer());
  box.append(buttons);
  overlay.append(box);
  show();
  const keys = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      answer();
    } else if (e.key === 'Enter' && document.activeElement?.tagName !== 'BUTTON') {
      e.preventDefault();
      e.stopPropagation();
      if (!items[0]?.disabled) answer(0);
    }
  };
  document.addEventListener('keydown', keys, true);
  document.body.append(overlay);
  (items[0]?.disabled ? cancel : items[0])?.focus();
  open = { id: sheet.id, close: () => answer() };
}
