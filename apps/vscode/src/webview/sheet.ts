/**
 * メインビューに重ねる確認のシート（アプリのシートと同じく、前と後の絵・選ぶもの・見通しの文・コミットの一覧・
 * ボタン）。選ぶもの（ラジオ）を選び直すと、絵と文がそれに替わる。一覧の行を押すと差分を開く（シートは閉じない）。
 * 入力欄があれば（Add Remote）、URL の先が何かの行と、つないでみるボタン（Check Connection）も出せる。
 * 既定のボタンは Enter、Cancel は Esc。答えは sheetAnswer で返す
 */
import { remoteKind } from '../sync/remoteKind.ts';
import type { Sheet } from '../view/protocol.ts';
import { el, post } from './api.ts';
import { drawing } from './drawing.ts';

let open: { id: number; close: () => void; checked?: (text: string, ok: boolean) => void } | undefined;

/** Check Connection の結果を、出しているシートに出す */
export function sheetChecked(id: number, text: string, ok: boolean): void {
  if (open?.id === id) open.checked?.(text, ok);
}

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
    post({
      type: 'sheetAnswer',
      id: sheet.id,
      index,
      choice: sheet.choices ? choice : undefined,
      values: index === undefined || !sheet.fields ? undefined : inputs.map((i) => i.value),
    });
  };
  // 既定のボタンを右端に（アプリのシートと同じ並び）
  const items = sheet.buttons.map((title, index) => {
    const b = el('button', index === 0 ? (sheet.destructive ? 'destructive' : undefined) : 'secondary', title);
    b.addEventListener('click', () => answer(index));
    return b;
  });
  // 入力欄（ラベルと欄を 2 列に並べる。アプリのシートと同じ）
  const inputs: HTMLInputElement[] = [];
  const problem = el('div', 'sheet-problem');
  problem.hidden = true;
  const kind = el('span', 'sheet-field-note');
  let checking = false;
  let checkButton: HTMLButtonElement | undefined;
  const checkResult = el('span', 'sheet-check-result');
  const valid = () => {
    const fields = sheet.fields ?? [];
    let reason: string | undefined;
    fields.forEach((f, i) => {
      const value = inputs[i]!.value.trim();
      if (!reason && f.taken?.includes(value)) reason = `${value} already exists`;
      if (f.required && !value) reason ??= '';
    });
    problem.textContent = reason ?? '';
    problem.hidden = !reason;
    return reason === undefined && !checking;
  };
  const refresh = () => {
    if (sheet.pointsTo !== undefined) kind.textContent = remoteKind(inputs[sheet.pointsTo]?.value.trim() ?? '');
    if (checkButton && sheet.check !== undefined) checkButton.disabled = checking || !inputs[sheet.check]?.value.trim();
    if (items[0] && sheet.fields) items[0].disabled = !valid();
  };
  if (sheet.fields) {
    const grid = el('div', 'sheet-fields');
    sheet.fields.forEach((f, i) => {
      const id = `sheet-${sheet.id}-field-${i}`;
      const label = el('label', undefined, f.label);
      label.htmlFor = id;
      const input = el('input', 'setting-input');
      input.type = 'text';
      input.id = id;
      input.value = f.value;
      input.spellcheck = false;
      if (f.placeholder) input.placeholder = f.placeholder;
      input.addEventListener('input', () => {
        if (i === sheet.check) checkResult.textContent = '';
        refresh();
      });
      inputs.push(input);
      grid.append(label, input);
    });
    if (sheet.pointsTo !== undefined) grid.append(el('span', 'sheet-field-label', 'Points to'), kind);
    box.append(grid);
    if (sheet.check !== undefined) {
      const row = el('div', 'sheet-check');
      const button = el('button', 'secondary', 'Check Connection');
      checkButton = button;
      button.addEventListener('click', () => {
        const url = inputs[sheet.check!]?.value.trim();
        if (!url) return;
        checking = true;
        checkResult.className = 'sheet-check-result';
        checkResult.textContent = 'Connecting…';
        refresh();
        post({ type: 'sheetCheck', id: sheet.id, url });
      });
      row.append(button, checkResult);
      box.append(row);
    }
    box.append(problem);
  }
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
    if (items[0]) items[0].disabled = !!c?.blocked || (sheet.fields !== undefined && !valid());
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
    } else if (e.key === 'Enter' && document.activeElement?.tagName !== 'BUTTON' && !e.isComposing) {
      e.preventDefault();
      e.stopPropagation();
      if (!items[0]?.disabled) answer(0);
    }
  };
  document.addEventListener('keydown', keys, true);
  document.body.append(overlay);
  // 選び直してもシートの大きさが変わらないよう、絵の高さをいちばん高いものに合わせる（アプリと同じ）
  if (sheet.choices?.some((c) => c.drawing)) {
    const shown = choice;
    let tallest = 0;
    sheet.choices.forEach((_, i) => {
      choice = i;
      show();
      tallest = Math.max(tallest, picture.scrollHeight);
    });
    choice = shown;
    show();
    // 低い画面では、これまでどおり絵の中でスクロールする
    picture.style.minHeight = `min(${tallest}px, 40vh)`;
  }
  if (sheet.fields) refresh();
  if (inputs[0]) {
    inputs[0].focus();
    inputs[0].select();
  } else {
    (items[0]?.disabled ? cancel : items[0])?.focus();
  }
  open = {
    id: sheet.id,
    close: () => answer(),
    checked: (text, ok) => {
      checking = false;
      checkResult.className = ok ? 'sheet-check-result ok' : 'sheet-check-result error';
      checkResult.textContent = text;
      refresh();
    },
  };
}
