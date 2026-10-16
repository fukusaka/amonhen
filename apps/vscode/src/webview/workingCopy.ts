/**
 * メインビューの Working Copy 画面。
 * 左に変更ファイルの一覧とコミット欄、右に選んだファイルの差分を出す。
 * 差分の行をクリック（Shift で範囲）して選び、ファイル・hunk・行単位で stage / unstage する。
 */
import type { Change, ChangeGroup } from '../state/changes.ts';
import type { HostMessage } from '../view/protocol.ts';
import type { DiffHunk, FileDiff, SelectedLine } from '../diff/fileDiff.ts';
import { el, post, type View } from './api.ts';
import { lineKey, renderDiffBody } from './diffView.ts';

const GROUPS: { group: ChangeGroup; title: string }[] = [
  { group: 'conflicts', title: 'Conflicts' },
  { group: 'staged', title: 'Staged' },
  { group: 'unstaged', title: 'Changes' },
];

export function createWorkingCopyView(): View {
  const root = el('div', 'view working-copy');
  const split = el('div', 'split');
  const side = el('div', 'wc-side');
  const fileList = el('div', 'wc-files');
  fileList.tabIndex = 0;
  const diffPane = el('div', 'wc-diff');
  split.append(side, diffPane);
  root.append(split);

  // コミット欄
  const commitBox = el('div', 'wc-commit');
  const subject = el('input', 'subject');
  subject.placeholder = 'Summary';
  const body = el('textarea', 'body');
  body.placeholder = 'Description';
  body.rows = 4;
  const commitRow = el('div', 'commit-row');
  const amendLabel = el('label', 'amend');
  const amend = el('input');
  amend.type = 'checkbox';
  amendLabel.append(amend, ' Amend');
  const commitButton = el('button', undefined, 'Commit');
  commitRow.append(amendLabel, commitButton);
  commitBox.append(subject, body, commitRow);
  side.append(fileList, commitBox);

  let files: Change[] = [];
  let lastMessage = '';
  let selected: { group: ChangeGroup; path: string } | undefined;
  let diff: FileDiff | undefined;
  /** diff のファイルが今あるグループ */
  let diffGroup: ChangeGroup | undefined;
  /** 選んでいる行。キーは `old:12` / `new:7` */
  let picked = new Set<string>();
  let anchor: string | undefined;

  // ---- ファイル一覧 ----

  function renderFiles(): void {
    const parts: HTMLElement[] = [];
    for (const { group, title } of GROUPS) {
      const items = files.filter((f) => f.group === group);
      if (items.length === 0) continue;
      const head = el('div', 'wc-group');
      head.append(el('span', 'title', title), el('span', 'count', String(items.length)));
      if (group !== 'conflicts') {
        const op = group === 'staged' ? 'unstage' : 'stage';
        const all = el('button', 'link', group === 'staged' ? 'Unstage All' : 'Stage All');
        all.addEventListener('click', () => stageFiles(op, items));
        head.append(all);
      }
      parts.push(head);
      for (const f of items) parts.push(renderFile(f));
    }
    if (parts.length === 0) {
      parts.push(el('div', 'empty', '変更はありません'));
    }
    fileList.replaceChildren(...parts);
    updateCommitButton();
  }

  function renderFile(f: Change): HTMLElement {
    const row = el('div', 'wc-file');
    if (selected?.group === f.group && selected.path === f.path) row.classList.add('selected');
    row.title = f.origPath ? `${f.origPath} → ${f.path}` : f.path;
    const slash = f.path.lastIndexOf('/');
    row.append(el('span', `status s-${f.letter === '!' ? 'U' : f.letter}`, f.letter));
    row.append(el('span', 'name', f.path.slice(slash + 1)));
    row.append(el('span', 'dir', slash >= 0 ? f.path.slice(0, slash) : ''));
    if (f.group !== 'conflicts') {
      const op = f.group === 'staged' ? 'unstage' : 'stage';
      const button = el('button', 'icon-button', f.group === 'staged' ? '−' : '+');
      button.title = f.group === 'staged' ? 'Unstage' : 'Stage';
      button.addEventListener('click', (e) => {
        e.stopPropagation();
        stageFiles(op, [f]);
      });
      row.append(button);
    }
    if (f.group === 'unstaged') {
      const discard = el('button', 'icon-button', '↺');
      discard.title = 'Discard Changes';
      discard.addEventListener('click', (e) => {
        e.stopPropagation();
        post({ type: 'wcDiscardFiles', paths: [f.path] });
      });
      row.insertBefore(discard, row.lastElementChild);
    }
    row.addEventListener('click', () => select(f));
    row.addEventListener('dblclick', () => post({ type: 'wcOpenFile', path: f.path }));
    return row;
  }

  function select(f: Change): void {
    selected = { group: f.group, path: f.path };
    renderFiles();
    post({ type: 'wcSelect', group: f.group, path: f.path });
  }

  function stageFiles(op: 'stage' | 'unstage', items: Change[]): void {
    post({ type: 'wcStageFiles', op, files: items.map((f) => ({ group: f.group, path: f.path })) });
  }

  fileList.addEventListener('keydown', (e) => {
    const order = GROUPS.flatMap(({ group }) => files.filter((f) => f.group === group));
    const index = order.findIndex((f) => f.group === selected?.group && f.path === selected.path);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const next = order[Math.min(order.length - 1, Math.max(0, index + (e.key === 'ArrowDown' ? 1 : -1)))];
      if (next) select(next);
    } else if ((e.key === ' ' || e.key === 'Enter') && order[index]) {
      e.preventDefault();
      const f = order[index]!;
      if (f.group !== 'conflicts') stageFiles(f.group === 'staged' ? 'unstage' : 'stage', [f]);
    }
  });

  // ---- 差分 ----

  function renderDiff(): void {
    if (!diff) {
      diffPane.replaceChildren(el('div', 'empty', files.length ? 'ファイルを選ぶと差分が出ます' : ''));
      return;
    }
    const d = diff;
    const head = el('div', 'diff-head');
    head.append(el('span', 'path', d.path));
    const actions = el('span', 'actions');
    const group = diffGroup!;
    if (group === 'conflicts') {
      diffPane.replaceChildren(head, renderConflict(d));
      return;
    }
    const staged = group === 'staged';
    if (picked.size > 0) {
      const lines = el(
        'button',
        undefined,
        `${staged ? 'Unstage' : 'Stage'} ${picked.size} Line${picked.size > 1 ? 's' : ''}`,
      );
      lines.addEventListener('click', () => stageLines([...picked]));
      actions.append(lines);
      if (!staged) {
        const discardLines = el('button', 'secondary', `Discard ${picked.size} Line${picked.size > 1 ? 's' : ''}`);
        discardLines.addEventListener('click', () => discard([...picked]));
        actions.append(discardLines);
      }
    }
    const file = el('button', 'secondary', staged ? 'Unstage File' : 'Stage File');
    file.addEventListener('click', () => {
      const f = files.find((x) => x.group === group && x.path === d.path);
      if (f) stageFiles(staged ? 'unstage' : 'stage', [f]);
    });
    const open = el('button', 'secondary', 'Open File');
    open.addEventListener('click', () => post({ type: 'wcOpenFile', path: d.path }));
    actions.append(file);
    if (!staged) {
      const discardFile = el('button', 'secondary', 'Discard File');
      discardFile.addEventListener('click', () => post({ type: 'wcDiscardFiles', paths: [d.path] }));
      actions.append(discardFile);
    }
    actions.append(open);
    head.append(actions);

    const content: HTMLElement[] = [head];
    content.push(
      ...renderDiffBody(d, {
        picked,
        hunkActions: [
          {
            label: staged ? 'Unstage Hunk' : 'Stage Hunk',
            run: (hunk) => stageLines(hunk.lines.filter((l) => l.kind !== ' ').map(lineKey)),
          },
          ...(staged
            ? []
            : [
                {
                  label: 'Discard Hunk',
                  run: (hunk: DiffHunk) => discard(hunk.lines.filter((l) => l.kind !== ' ').map(lineKey)),
                },
              ]),
        ],
      }),
    );
    diffPane.replaceChildren(...content);
  }

  function renderConflict(d: FileDiff): HTMLElement {
    const box = el('div', 'conflict');
    box.append(
      el(
        'p',
        undefined,
        'このファイルはコンフリクトしています。マージエディタで解決するか、どちらかの内容を採用してください。',
      ),
    );
    const buttons = el('div', 'buttons');
    for (const [action, label] of [
      ['merge', 'Open in Merge Editor'],
      ['current', 'Accept Current'],
      ['incoming', 'Accept Incoming'],
    ] as const) {
      const b = el('button', action === 'merge' ? undefined : 'secondary', label);
      b.addEventListener('click', () => post({ type: 'wcResolve', path: d.path, action }));
      buttons.append(b);
    }
    const resolved = el('button', 'secondary', 'Mark as Resolved');
    resolved.addEventListener('click', () => {
      const f = files.find((x) => x.group === 'conflicts' && x.path === d.path);
      if (f) stageFiles('stage', [f]);
    });
    buttons.append(resolved);
    box.append(buttons);
    return box;
  }

  function toLines(keys: string[]): SelectedLine[] {
    return keys.map((k) => {
      const [side, no] = k.split(':') as [string, string];
      return side === 'old' ? { kind: '-', old: Number(no) } : { kind: '+', new: Number(no) };
    });
  }

  function stageLines(keys: string[]): void {
    if (!diff || keys.length === 0) return;
    post({ type: 'wcStageLines', group: diffGroup!, path: diff.path, version: diff.version, lines: toLines(keys) });
    picked = new Set();
  }

  /** 選んだ行を破棄する。確認は拡張ホストで行う */
  function discard(keys: string[]): void {
    if (!diff || keys.length === 0) return;
    post({ type: 'wcDiscardLines', path: diff.path, version: diff.version, lines: toLines(keys) });
  }

  // 行をクリックして選ぶ。Shift で前に選んだ行からの範囲
  diffPane.addEventListener('click', (e) => {
    const row = (e.target as HTMLElement).closest<HTMLElement>('.line');
    const k = row?.dataset.key;
    if (!k) return;
    if (e.shiftKey && anchor) {
      const keys = [...diffPane.querySelectorAll<HTMLElement>('.line[data-key]')].map((r) => r.dataset.key!);
      const [from, to] = [keys.indexOf(anchor), keys.indexOf(k)].sort((x, y) => x - y) as [number, number];
      for (const x of keys.slice(from, to + 1)) picked.add(x);
    } else if (picked.has(k)) {
      picked.delete(k);
    } else {
      picked.add(k);
    }
    anchor = k;
    renderDiff();
  });
  diffPane.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && picked.size > 0) {
      picked = new Set();
      renderDiff();
    }
  });
  diffPane.tabIndex = 0;

  // ---- コミット ----

  function stagedCount(): number {
    return files.filter((f) => f.group === 'staged').length;
  }

  function updateCommitButton(): void {
    const staged = stagedCount();
    commitButton.disabled = !subject.value.trim() || (!amend.checked && staged === 0);
    commitButton.textContent = amend.checked
      ? 'Amend'
      : staged > 0
        ? `Commit ${staged} File${staged > 1 ? 's' : ''}`
        : 'Commit';
  }

  function message(): string {
    const b = body.value.trim();
    return b ? `${subject.value.trim()}\n\n${b}\n` : `${subject.value.trim()}\n`;
  }

  function commit(): void {
    if (commitButton.disabled) return;
    post({ type: 'wcCommit', message: message(), amend: amend.checked });
  }

  subject.addEventListener('input', updateCommitButton);
  commitButton.addEventListener('click', commit);
  for (const input of [subject, body] as HTMLElement[]) {
    input.addEventListener('keydown', (e: KeyboardEvent) => {
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        commit();
      }
    });
  }
  // amend にしたら、空ならいまのコミットのメッセージを入れる
  amend.addEventListener('change', () => {
    if (amend.checked && !subject.value && !body.value && lastMessage) {
      const [s = '', ...rest] = lastMessage.replace(/\n+$/, '').split('\n');
      subject.value = s;
      body.value = rest.join('\n').replace(/^\n+/, '');
    }
    updateCommitButton();
  });

  function onMessage(m: HostMessage): void {
    switch (m.type) {
      case 'wcFiles':
        files = m.files;
        lastMessage = m.lastMessage ?? '';
        if (!selected && files.length > 0) {
          select(files[0]!);
        } else {
          renderFiles();
        }
        break;
      case 'wcDiff':
        if (diff?.version !== m.diff?.version || diff?.path !== m.diff?.path) {
          picked = new Set();
          anchor = undefined;
        }
        diff = m.diff;
        diffGroup = m.group;
        selected = m.diff ? { group: m.group!, path: m.diff.path } : undefined;
        renderFiles();
        renderDiff();
        break;
      case 'wcCommitted':
        subject.value = '';
        body.value = '';
        amend.checked = false;
        updateCommitButton();
        break;
    }
  }

  renderFiles();
  renderDiff();
  return {
    root,
    onMessage,
    onShow() {
      fileList.focus();
    },
  };
}
