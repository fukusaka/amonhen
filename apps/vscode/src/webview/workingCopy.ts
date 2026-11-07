/**
 * メインビューの Working Copy 画面。
 * 左に変更ファイルの一覧とコミット欄、右に選んだファイルの差分を出す。
 * 差分の行をクリック（Shift で範囲）して選び、ファイル・hunk・行単位で stage / unstage する。
 */
import type { Change, ChangeGroup } from '../state/changes.ts';
import type { HostMessage } from '../view/protocol.ts';
import type { DiffHunk, FileDiff, SelectedLine } from '../diff/fileDiff.ts';
import type { Eol, LineEndings } from '../git/eol.ts';
import { addTrailer, lengthLevel, longestLine, messageLength, splitMessage, withType } from '../workingCopy/commitMessage.ts';
import { filesUnder, filterByPath, makeFileTree, type FileDir, type FileLayout } from '../workingCopy/fileList.ts';
import { el, post, type View } from './api.ts';
import { lineKey, renderDiffBody } from './diffView.ts';

const GROUPS: { group: ChangeGroup; title: string }[] = [
  { group: 'conflicts', title: 'Conflicts' },
  { group: 'staged', title: 'Staged' },
  { group: 'unstaged', title: 'Changes' },
];

const LAYOUTS: { layout: FileLayout; label: string; title: string }[] = [
  { layout: 'list', label: 'List', title: 'Show files as a list of paths' },
  { layout: 'tree', label: 'Tree', title: 'Show files as a directory tree' },
  { layout: 'flat', label: 'Name', title: 'Show the file name first and the parent directory dimmed' },
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
  const lengthLabel = el('span', 'length');
  const recentButton = el('button', 'link', 'Recent…');
  recentButton.title = 'Insert the message of a recent commit (⌥↑ and ⌥↓ in the Summary)';
  recentButton.addEventListener('click', () => post({ type: 'wcPickRecent' }));
  const insertButton = el('button', 'link', 'Insert…');
  insertButton.title = 'Insert a Conventional Commits type, Co-authored-by or Signed-off-by';
  insertButton.addEventListener('click', () => post({ type: 'wcPickInsert' }));
  const commitTools = el('div', 'commit-tools');
  commitTools.append(recentButton, insertButton, lengthLabel);
  commitRow.append(amendLabel, commitButton);
  commitBox.append(subject, body, commitTools, commitRow);

  // 一覧の絞り込みと見せ方
  const tools = el('div', 'wc-tools');
  const filter = el('input');
  filter.type = 'search';
  filter.placeholder = 'Filter by path';
  filter.setAttribute('aria-label', 'Filter by path');
  const layoutButtons = el('div', 'wc-layout');
  tools.append(filter, layoutButtons);
  let layout: FileLayout = 'flat';
  for (const { layout: value, label, title } of LAYOUTS) {
    const button = el('button', undefined, label);
    button.title = title;
    button.dataset.layout = value;
    button.addEventListener('click', () => {
      layout = value;
      post({ type: 'wcLayout', layout: value });
      renderFiles();
    });
    layoutButtons.append(button);
  }
  side.append(tools, fileList, commitBox);

  let files: Change[] = [];
  let lastMessage = '';
  let selected: { group: ChangeGroup; path: string } | undefined;
  let diff: FileDiff | undefined;
  /** diff のファイルが今あるグループ */
  let diffGroup: ChangeGroup | undefined;
  /** 選んでいる行。キーは `old:12` / `new:7` */
  let picked = new Set<string>();
  let anchor: string | undefined;
  /** 木で畳んだディレクトリ（グループとパス）。読み直しても畳んだままにする */
  const collapsed = new Set<string>();
  /** 一覧に出ているファイル（上から順。絞り込みと畳んだディレクトリの分を除く） */
  let listed: Change[] = [];

  // ---- ファイル一覧 ----

  function renderFiles(): void {
    for (const button of layoutButtons.children as HTMLCollectionOf<HTMLElement>) {
      button.classList.toggle('active', button.dataset.layout === layout);
    }
    listed = [];
    const parts: HTMLElement[] = [];
    for (const { group, title } of GROUPS) {
      const items = filterByPath(
        files.filter((f) => f.group === group),
        filter.value,
      );
      if (items.length === 0) continue;
      const head = el('div', 'wc-group');
      head.append(el('span', 'title', title), el('span', 'count', String(items.length)));
      if (group !== 'conflicts') {
        const op = group === 'staged' ? 'unstage' : 'stage';
        const all = el('button', 'link', group === 'staged' ? 'Unstage All' : 'Stage All');
        all.addEventListener('click', () => stageFiles(op, items));
        head.append(all);
      }
      // マーカーの残っていないコンフリクトは、まとめて解決済みにできる
      const resolved = items.filter((f) => f.markers === 0);
      if (group === 'conflicts' && resolved.length > 0) {
        const all = el('button', 'link', `Mark ${resolved.length} as Resolved`);
        all.title = 'マーカーの残っていないファイル（rerere やほかのエディタで解決したもの）を解決済みにする';
        all.addEventListener('click', () => stageFiles('stage', resolved));
        head.append(all);
      }
      parts.push(head);
      if (layout === 'tree') {
        renderDir(makeFileTree(items), group, 0, parts);
      } else {
        for (const f of items) {
          listed.push(f);
          parts.push(renderFile(f, 0));
        }
      }
    }
    if (parts.length === 0) {
      parts.push(el('div', 'empty', files.length > 0 ? 'No matching files' : '変更はありません'));
    }
    fileList.replaceChildren(...parts);
    updateCommitButton();
  }

  /** 木の 1 つのディレクトリの中身を parts に足す（ディレクトリが先、ファイルが後） */
  function renderDir(dir: FileDir<Change>, group: ChangeGroup, depth: number, parts: HTMLElement[]): void {
    for (const child of dir.dirs) {
      const key = `${group}:${child.path}`;
      const open = !collapsed.has(key);
      const row = el('div', 'wc-file wc-dir');
      row.style.paddingLeft = `${12 + depth * 14}px`;
      row.title = child.path;
      row.append(el('span', 'chevron', open ? '▾' : '▸'), el('span', 'name', `${child.name}/`), el('span', 'dir'));
      if (group !== 'conflicts') {
        const button = el('button', 'icon-button', group === 'staged' ? '−' : '+');
        button.title = group === 'staged' ? 'Unstage Folder' : 'Stage Folder';
        button.addEventListener('click', (e) => {
          e.stopPropagation();
          stageFiles(group === 'staged' ? 'unstage' : 'stage', filesUnder(child));
        });
        row.append(button);
      }
      row.addEventListener('click', () => {
        if (open) collapsed.add(key);
        else collapsed.delete(key);
        renderFiles();
      });
      parts.push(row);
      if (open) renderDir(child, group, depth + 1, parts);
    }
    for (const f of dir.files) {
      listed.push(f);
      parts.push(renderFile(f, depth));
    }
  }

  function renderFile(f: Change, depth: number): HTMLElement {
    const row = el('div', 'wc-file');
    if (depth > 0) row.style.paddingLeft = `${12 + depth * 14}px`;
    if (selected?.group === f.group && selected.path === f.path) row.classList.add('selected');
    row.title = f.origPath ? `${f.origPath} → ${f.path}` : f.path;
    // Webview の右クリックメニュー（package.json の webview/context）に渡す情報
    row.dataset.vscodeContext = JSON.stringify({
      webviewSection: 'wcFile',
      path: f.path,
      // コンフリクトのファイル（Stash… を出さない）
      ...(f.group === 'conflicts' ? { wcConflict: true } : {}),
      // rerere が解決を当てたコンフリクトのファイル（Forget Resolution を出す）
      ...(f.group === 'conflicts' && f.rerere ? { wcRerere: true } : {}),
      preventDefaultContextMenuItems: true,
    });
    const slash = f.path.lastIndexOf('/');
    const name = layout === 'list' ? f.path : f.path.slice(slash + 1);
    if (f.markers === 0) {
      // マーカーが残っていない。解決できていそうなことを印と短い注記で示す
      row.title += `\n${withoutMarkersMessage(f)}`;
      row.append(el('span', 'status s-resolved', '✓'));
      row.append(el('span', 'name', name));
      row.append(el('span', 'dir', f.rerere ? 'rerere' : 'マーカーなし'));
    } else {
      row.append(el('span', `status s-${f.letter === '!' ? 'U' : f.letter}`, f.letter));
      row.append(el('span', 'name', name));
      row.append(el('span', 'dir', layout === 'flat' && slash >= 0 ? f.path.slice(0, slash) : ''));
    }
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
    const order = listed;
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

  filter.addEventListener('input', () => {
    renderFiles();
    // 選んでいたファイルが隠れたら、残っている最初のものへ
    const shown = listed.some((f) => f.group === selected?.group && f.path === selected.path);
    if (selected && !shown && listed[0]) select(listed[0]);
  });
  filter.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      filter.value = '';
      renderFiles();
      fileList.focus();
    }
  });

  // ---- 差分 ----

  /** 改行を変換している（作業ツリーと git で違う）・改行が混ざっているファイルの印。差分には出ないので */
  function lineEndingsBadge(le: LineEndings): HTMLElement {
    const name = (e: Eol) => (e === 'lf' ? 'LF' : e === 'crlf' ? 'CRLF' : e === 'mixed' ? '混在' : 'なし');
    // 作業ツリーの形 → git に入る形
    const text = le.worktree !== le.index && le.index !== 'none' ? `${name(le.worktree)} → ${name(le.index)}` : name(le.worktree);
    const badge = el('span', le.warning ? 'eol warning' : 'eol', le.warning ? `⚠ ${text}` : text);
    const tip = [`改行: 作業ツリー ${name(le.worktree)}、git ${name(le.index)}、checkout ${name(le.checkout)}`];
    if (le.rule) tip.push(`変換: ${le.rule}`);
    if (le.warning) tip.push('次の commit や checkout で、ファイル全体の改行が変わることがあります');
    badge.title = tip.join('\n');
    return badge;
  }

  function renderDiff(): void {
    if (!diff) {
      diffPane.replaceChildren(el('div', 'empty', files.length ? 'ファイルを選ぶと差分が出ます' : ''));
      return;
    }
    const d = diff;
    const head = el('div', 'diff-head');
    head.append(el('span', 'path', d.path));
    if (d.lineEndings?.notable) {
      head.append(lineEndingsBadge(d.lineEndings));
    }
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
    const f = files.find((x) => x.group === 'conflicts' && x.path === d.path);
    // マーカーが残っていなければ、解決済みにするのを先に勧める
    const clean = f?.markers === 0;
    box.append(
      el(
        'p',
        undefined,
        clean && f
          ? withoutMarkersMessage(f)
          : 'このファイルはコンフリクトしています。マージエディタで解決するか、どちらかの内容を採用してください。',
      ),
    );
    const buttons = el('div', 'buttons');
    const resolved = el('button', clean ? undefined : 'secondary', 'Mark as Resolved');
    resolved.addEventListener('click', () => {
      if (f) stageFiles('stage', [f]);
    });
    if (clean) buttons.append(resolved);
    for (const [action, label] of [
      ['merge', 'Open in Merge Editor'],
      ['current', 'Accept Current'],
      ['incoming', 'Accept Incoming'],
    ] as const) {
      const b = el('button', action === 'merge' && !clean ? undefined : 'secondary', label);
      b.addEventListener('click', () => post({ type: 'wcResolve', path: d.path, action }));
      buttons.append(b);
    }
    if (!clean) buttons.append(resolved);
    box.append(buttons);
    return box;
  }

  /** マーカーの残っていないコンフリクトの説明 */
  function withoutMarkersMessage(f: Change): string {
    return f.rerere
      ? 'git の rerere が前の解決を適用しました。結果を確かめて「Mark as Resolved」を押してください。'
      : 'コンフリクトのマーカーは残っていません。結果を確かめて「Mark as Resolved」を押してください。';
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
    updateLength();
  }

  /** 件名の文字数と、72 文字を超える本文の行（git の慣習） */
  function updateLength(): void {
    const length = messageLength(subject.value);
    const line = longestLine(body.value);
    lengthLabel.replaceChildren();
    if (length > 0) lengthLabel.append(el('span', `level-${lengthLevel(length)}`, String(length)));
    if (lengthLevel(line) === 'over') lengthLabel.append(el('span', 'level-over', ` Body ${line}`));
    lengthLabel.title = 'Length of the summary (yellow over 50, orange over 72), and of a description line over 72';
  }

  function message(): string {
    const b = body.value.trim();
    return b ? `${subject.value.trim()}\n\n${b}\n` : `${subject.value.trim()}\n`;
  }

  function commit(): void {
    if (commitButton.disabled) return;
    post({ type: 'wcCommit', message: message(), amend: amend.checked });
  }

  /** 欄の中身を入れ替える。打ったのと同じく Undo で戻せるように、編集として入れる */
  function replaceField(field: HTMLInputElement | HTMLTextAreaElement, text: string): void {
    if (field.value === text) return;
    field.focus();
    field.select();
    if (!document.execCommand(text ? 'insertText' : 'delete', false, text)) {
      field.value = text;
      field.dispatchEvent(new Event('input', { bubbles: true }));
    }
  }

  function useMessage(text: string): void {
    const parts = splitMessage(text);
    replaceField(body, parts.body);
    replaceField(subject, parts.summary);
  }

  // ⌥↑・⌥↓ で最近のメッセージを順に呼び出す。一番新しいものの先は、呼び出す前の中身
  let recent: string[] = [];
  let recentCursor = -1;
  let beforeRecall = { summary: '', body: '' };
  let recalling = false;
  subject.addEventListener('keydown', (e) => {
    if (!e.altKey || (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') || recent.length === 0) return;
    e.preventDefault();
    if (recentCursor < 0) beforeRecall = { summary: subject.value, body: body.value };
    recentCursor = Math.max(-1, Math.min(recent.length - 1, recentCursor + (e.key === 'ArrowUp' ? 1 : -1)));
    const parts = recentCursor < 0 ? beforeRecall : splitMessage(recent[recentCursor]!);
    recalling = true;
    replaceField(body, parts.body);
    replaceField(subject, parts.summary);
    recalling = false;
    subject.focus();
  });

  /** 打ち終わるのを少し待って、下書きを覚えてもらう */
  let draftTimer: ReturnType<typeof setTimeout> | undefined;
  function messageChanged(): void {
    updateCommitButton();
    clearTimeout(draftTimer);
    // 呼び出しただけでは下書きにしない（呼び出す前の中身が下書きのまま）。手で直したら、それが下書き
    if (recalling) return;
    recentCursor = -1;
    draftTimer = setTimeout(() => post({ type: 'wcDraft', summary: subject.value, body: body.value }), 300);
  }
  subject.addEventListener('input', messageChanged);
  body.addEventListener('input', messageChanged);
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
        layout = m.layout;
        renderFiles();
        if (!selected && listed[0]) select(listed[0]);
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
      case 'wcRecent':
        recent = m.messages;
        recentCursor = -1;
        break;
      case 'wcUseMessage':
        recentCursor = -1;
        useMessage(m.message);
        break;
      case 'wcInsertType':
        replaceField(subject, withType(subject.value, m.commitType));
        subject.focus();
        break;
      case 'wcInsertTrailer':
        replaceField(body, addTrailer(body.value, m.line));
        body.focus();
        break;
      case 'wcMessage':
        clearTimeout(draftTimer);
        subject.value = m.summary;
        body.value = m.body;
        updateCommitButton();
        break;
      case 'wcCommitted':
        clearTimeout(draftTimer);
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
