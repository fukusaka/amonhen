/**
 * History の編集モード（docs/ux.md の「History's edit mode」）。選んだコミットから HEAD までを History と同じく新しい順に
 * 並べ、行ごとにすることを選び、ドラッグで並べ替え、ダブルクリックでメッセージを直す。Apply Changes を押すまで何も
 * 変わらず、まとめて 1 回の rebase（コアの `plan`）で行う。Undo 1 回で全部戻る
 */
import {
  ACTION_TITLES,
  ACTION_TOOLTIPS,
  canReorder,
  choices,
  combinedMessage,
  type EditAction,
  type EditRow,
  hasChanges,
  moveRow,
  planSteps,
  subjectOf,
  summary,
  withAction,
} from '../history/editPlan.ts';
import type { EditStacked } from '../history/protocol.ts';
import { el, post } from './api.ts';
import { openMessageEditor } from './messageEditor.ts';

export interface EditMode {
  readonly root: HTMLElement;
  /** 開く（前の編集は捨てる）。`original` は最初の変更を入れる前の並び。`editMessageAt` の行は、すぐメッセージの編集欄を開く */
  open(rows: EditRow[], original: string[], editMessageAt?: number): void;
  preview(key: number, lines: string[], conflicts: string[], stacked?: EditStacked): void;
  /** Apply を終えた（ok なら閉じる） */
  applied(ok: boolean): void;
  readonly isOpen: boolean;
}

/** onClose は閉じたとき（History に戻す）、onSelect は行を選んだとき（詳細に出す） */
export function createEditMode(onClose: () => void, onSelect: (oid: string) => void): EditMode {
  const root = el('div', 'edit-mode');
  root.hidden = true;
  const heading = el('div', 'edit-heading', 'Edit Commits');
  const hint = el('div', 'edit-hint');
  const list = el('div', 'edit-rows');
  list.tabIndex = 0;
  const bar = el('div', 'edit-bar');
  const summaryText = el('div', 'edit-summary');
  const previewText = el('div', 'edit-preview');
  const stackedBox = el('input');
  stackedBox.type = 'checkbox';
  const stackedLabel = el('label', 'edit-stacked');
  stackedLabel.append(stackedBox, ' Also move the stacked branches');
  stackedLabel.hidden = true;
  const cancel = el('button', 'secondary', 'Cancel');
  const apply = el('button', '', 'Apply Changes');
  const texts = el('div', 'edit-texts');
  texts.append(summaryText, previewText);
  const buttons = el('div', 'edit-buttons');
  buttons.append(stackedLabel, cancel, apply);
  bar.append(texts, buttons);
  root.append(heading, hint, list, bar);

  let rows: EditRow[] = [];
  let original: string[] = [];
  let selected = 0;
  let conflicts = new Set<string>();
  let previewKey = 0;
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  /** 積み重ねたブランチの扱いを、この編集で触ったか（触っていなければ git の設定のとおり） */
  let touchedStacked = false;
  let applying = false;
  let dragging: number | undefined;

  function close(): void {
    clearTimeout(previewTimer);
    root.hidden = true;
    onClose();
  }

  function open(next: EditRow[], before: string[], editMessageAt?: number): void {
    original = before;
    rows = next;
    selected = editMessageAt ?? 0;
    conflicts = new Set();
    touchedStacked = false;
    stackedLabel.hidden = true;
    previewText.textContent = '';
    applying = false;
    root.hidden = false;
    hint.textContent = canReorder(rows)
      ? 'Choose what to do with each commit and drag rows to reorder them. Nothing changes until Apply Changes.'
      : 'Choose what to do with each commit. Merges are recreated as they are, so rows can’t be reordered across them. Nothing changes until Apply Changes.';
    update();
    list.focus();
    if (rows[selected]) onSelect(rows[selected]!.oid);
    if (editMessageAt !== undefined) editMessage(editMessageAt);
  }

  function update(): void {
    render();
    summaryText.textContent = summary(rows, original);
    apply.disabled = applying;
    cancel.disabled = applying;
    requestPreview();
  }

  function render(): void {
    const reorder = canReorder(rows);
    list.replaceChildren(
      ...rows.map((row, i) => {
        const item = el('div', `edit-row a-${row.action}${row.merge ? ' merge' : ''}${i === selected ? ' selected' : ''}`);
        item.dataset.index = String(i);
        item.draggable = reorder;
        const combining = row.action === 'squash' || row.action === 'fixup';
        item.append(el('span', 'mark', row.merge ? '⑃' : combining ? '↳' : '●'));
        const select = el('select');
        select.disabled = row.merge === true || applying;
        for (const action of choices(rows, i)) {
          const option = el('option', '', ACTION_TITLES[action]);
          option.value = action;
          option.title = ACTION_TOOLTIPS[action];
          select.append(option);
        }
        select.value = row.action;
        select.title = row.merge ? 'Merges are recreated as they are.' : ACTION_TOOLTIPS[row.action];
        select.addEventListener('change', () => setAction(i, select.value as EditAction));
        const subject = el('span', 'subject', subjectOf(row));
        if (row.newMessage !== undefined && (row.action === 'reword' || row.action === 'squash')) subject.classList.add('changed');
        item.append(select, subject);
        if (conflicts.has(row.oid)) {
          const warn = el('span', 'warn', '⚠︎');
          warn.title = 'Expected to conflict here';
          item.append(warn);
        }
        item.append(el('span', 'author', row.author), el('code', 'oid', row.oid.slice(0, 8)));
        return item;
      }),
    );
  }

  function setAction(i: number, action: EditAction): void {
    rows = withAction(rows, i, action);
    update();
    // メッセージを直すことにしたら、そのまま編集欄を開く
    if (action === 'reword') editMessage(i);
  }

  /** i の行のメッセージを直す（まとめる行なら、まとめたメッセージ） */
  function editMessage(i: number): void {
    const row = rows[i];
    if (!row || row.merge || applying) return;
    const combining = row.action === 'squash';
    const initial = row.newMessage ?? (combining ? combinedMessage(rows, i) : row.message);
    openMessageEditor(
      combining ? 'Combined Message' : 'Edit Message',
      initial,
      (text) => {
        const action = row.action === 'squash' || row.action === 'reword' ? row.action : 'reword';
        rows = rows.map((r, j) => (j === i ? { ...r, action, newMessage: text } : r));
        update();
      },
      () => list.focus(),
    );
  }

  function requestPreview(): void {
    clearTimeout(previewTimer);
    const key = ++previewKey;
    if (!hasChanges(rows, original)) {
      // 変更をすべて戻したら、前の見込みも消す
      previewText.textContent = '';
      stackedLabel.hidden = true;
      if (conflicts.size > 0) {
        conflicts = new Set();
        render();
      }
      return;
    }
    previewTimer = setTimeout(() => {
      post({ type: 'editPreview', key, steps: planSteps(rows), ...(touchedStacked ? { updateRefs: stackedBox.checked } : {}) });
    }, 300);
  }

  function preview(key: number, lines: string[], marks: string[], stacked?: EditStacked): void {
    if (key !== previewKey || root.hidden) return;
    previewText.textContent = lines.join(' ');
    previewText.classList.toggle('warning', marks.length > 0);
    stackedLabel.hidden = !stacked;
    if (stacked && !touchedStacked) {
      stackedBox.checked = stacked.configured;
      stackedBox.disabled = !stacked.available;
      stackedLabel.title = stacked.available
        ? 'For this edit only (git’s update-ref). Starts from rebase.updateRefs.'
        : 'Moving stacked branches needs git 2.38 or newer.';
    }
    const next = new Set(marks);
    if ([...next].sort().join() !== [...conflicts].sort().join()) {
      conflicts = next;
      render();
    }
  }

  function applied(ok: boolean): void {
    applying = false;
    if (ok) close();
    else update();
  }

  stackedBox.addEventListener('change', () => {
    touchedStacked = true;
    requestPreview();
  });
  cancel.addEventListener('click', close);
  apply.addEventListener('click', () => {
    if (!hasChanges(rows, original)) {
      close();
      return;
    }
    applying = true;
    update();
    post({ type: 'editApply', steps: planSteps(rows), ...(touchedStacked ? { updateRefs: stackedBox.checked } : {}) });
  });

  const indexOf = (e: Event): number | undefined => {
    const item = (e.target as Element).closest<HTMLElement>('.edit-row');
    return item ? Number(item.dataset.index) : undefined;
  };
  list.addEventListener('click', (e) => {
    const i = indexOf(e);
    if (i === undefined || (e.target as Element).closest('select') || i === selected) return;
    selected = i;
    render();
    onSelect(rows[i]!.oid);
  });
  list.addEventListener('dblclick', (e) => {
    const i = indexOf(e);
    if (i !== undefined && !(e.target as Element).closest('select')) editMessage(i);
  });
  list.addEventListener('keydown', (e) => {
    if ((e.target as Element).closest('select')) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const next = Math.min(rows.length - 1, Math.max(0, selected + (e.key === 'ArrowDown' ? 1 : -1)));
      if (next !== selected) {
        selected = next;
        render();
        onSelect(rows[next]!.oid);
      }
    } else if (e.key === 'Enter') {
      e.preventDefault();
      editMessage(selected);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      if (!applying) close();
    }
  });

  // ドラッグで並べ替える（merge がなければ）
  const clearDrop = () => {
    for (const item of list.querySelectorAll('.drop-above, .drop-below')) item.classList.remove('drop-above', 'drop-below');
  };
  const upper = (e: DragEvent, item: HTMLElement) => {
    const rect = item.getBoundingClientRect();
    return e.clientY < rect.top + rect.height / 2;
  };
  list.addEventListener('dragstart', (e) => {
    dragging = indexOf(e);
    if (dragging === undefined || applying || !e.dataTransfer) {
      e.preventDefault();
      return;
    }
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', rows[dragging]!.oid);
  });
  list.addEventListener('dragover', (e) => {
    const item = (e.target as Element).closest<HTMLElement>('.edit-row');
    if (dragging === undefined || !item) return;
    e.preventDefault();
    clearDrop();
    item.classList.add(upper(e, item) ? 'drop-above' : 'drop-below');
  });
  list.addEventListener('dragleave', clearDrop);
  list.addEventListener('drop', (e) => {
    e.preventDefault();
    const item = (e.target as Element).closest<HTMLElement>('.edit-row');
    clearDrop();
    if (dragging === undefined || !item) return;
    const target = Number(item.dataset.index);
    const to = upper(e, item) ? target : target + 1;
    const from = dragging;
    dragging = undefined;
    if (to === from || to === from + 1) return;
    rows = moveRow(rows, from, to);
    selected = from < to ? to - 1 : to;
    update();
  });
  list.addEventListener('dragend', () => {
    dragging = undefined;
    clearDrop();
  });

  return {
    root,
    open,
    preview,
    applied,
    get isOpen() {
      return !root.hidden;
    },
  };
}
