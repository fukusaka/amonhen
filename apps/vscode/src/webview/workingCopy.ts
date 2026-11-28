/**
 * メインビューの Working Copy 画面。
 * 左の列いっぱいに変更ファイルの一覧、右の上にコミット欄（件名の行だけにたためる）、その下に選んだファイルの差分を出す
 * （アプリと同じ）。差分の行をクリック（Shift で範囲）して選び、ファイル・hunk・行単位で stage / unstage する。
 * エンジンがあれば、差分で見る側（Unstaged・Staged・All）を帯で選び、一覧を Mixed（1 つの一覧、チェックで次の
 * コミットに入れるか）と Staged・Unstaged の節で切り替える。なければ節の一覧で、差分は行の節の側
 */
import type { Change, ChangeGroup } from '../state/changes.ts';
import type { HostMessage } from '../view/protocol.ts';
import type { DiffHunk, DiffLine, FileDiff, SelectedLine } from '../diff/fileDiff.ts';
import type { Eol, LineEndings } from '../git/eol.ts';
import { ignorePatterns } from '../workingCopy/ignore.ts';
import { addTrailer, commitDisabledReason, composeMessage, lengthLevel, longestLine, messageLength, splitMessage, withType } from '../workingCopy/commitMessage.ts';
import { filesUnder, filterByPath, makeFileTree, type FileDir, type FileLayout } from '../workingCopy/fileList.ts';
import type { DiffMenuAction, FileStat, LineStaging, WorkingCopyLook } from '../workingCopy/protocol.ts';
import {
  changeBlocks,
  gapActions,
  openGap,
  showsWholeFile,
  stageActions,
  toggleWholeFile,
  type FoldedSide,
  type GapAction,
  type GapOpen,
  type StageOp,
} from '../workingCopy/gaps.ts';
import {
  adoptSide,
  changeOf,
  groupCheck,
  groupOf,
  keyOf,
  keyText,
  mixedRows,
  parseKey,
  reselectRows,
  reselectTarget,
  rowCheck,
  sectionRows,
  SIDES,
  sideNoteText,
  toggleRequest,
  type Check,
  type DiffSide,
  type ListRow,
  type RowKey,
  type SideNote,
} from '../workingCopy/sides.ts';
import { el, post, type View } from './api.ts';
import { lineKey, renderDiffBody, whitespaceSelect, wrapButton } from './diffView.ts';
import { gapRow, layoutGapRows } from './gapRow.ts';
import type { DiffWhitespace } from '../history/protocol.ts';

const GROUPS: { group: ChangeGroup; title: string }[] = [
  { group: 'conflicts', title: 'Conflicts' },
  { group: 'staged', title: 'Staged' },
  { group: 'unstaged', title: 'Unstaged' },
];

const LAYOUTS: { layout: FileLayout; label: string; title: string }[] = [
  { layout: 'list', label: 'List', title: 'Show files as a list of paths' },
  { layout: 'tree', label: 'Tree', title: 'Show files as a directory tree' },
  { layout: 'flat', label: 'File Name', title: 'Show the file name first and the parent directory dimmed' },
];

/** たたまないときのコミットの欄の低さの下限と、差分に残す高さ（境目を動かすとき） */
const COMMIT_MIN = 120;
const DIFF_MIN = 200;

export function createWorkingCopyView(): View {
  const root = el('div', 'view working-copy');
  const split = el('div', 'split');
  const left = el('div', 'wc-side');
  const fileList = el('div', 'wc-files');
  fileList.tabIndex = 0;
  const right = el('div', 'wc-right');
  const diffPane = el('div', 'wc-diff');
  split.append(left, right);
  root.append(split);

  // コミット欄（右の上、差分の上。何を書くかと何が入るかを隣に置く。アプリと同じ）
  const commitBox = el('div', 'wc-commit');
  const collapseButton = el('button', 'wc-collapse');
  collapseButton.addEventListener('click', () => {
    commitCollapsed = !commitCollapsed;
    applyCollapsed();
    post({ type: 'wcLook', commitCollapsed });
    if (commitCollapsed && document.activeElement === body) subject.focus();
  });
  const summaryRow = el('div', 'summary-row');
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
  const commitMenu = el('button', 'commit-menu', '▾');
  commitMenu.title = 'Commit and Push (⌘⇧↩), Amend';
  commitMenu.setAttribute('aria-label', 'More Commit Actions');
  commitMenu.addEventListener('click', () =>
    post({ type: 'wcCommitMenu', canCommit: !commitButton.disabled, message: message(), amend: amend.checked }),
  );
  const commitActions = el('div', 'commit-actions');
  commitActions.append(commitButton, commitMenu);
  // コミット欄と差分の境目。動かした高さは覚え、ダブルクリックで中身の高さに戻す
  const commitDivider = el('div', 'wc-commit-divider');
  commitDivider.setAttribute('role', 'separator');
  commitDivider.title = 'Drag to change the height of the commit message; double-click to fit it again';
  right.append(commitBox, commitDivider, diffPane);

  /** コミット欄をたたんでいる（件名の行に文字数と Commit を移し、本文とボタンの行を隠す。覚える） */
  let commitCollapsed = false;
  /** たたんでいないときのコミット欄の高さ（境目を動かしたとき） */
  let commitHeight: number | undefined;

  function applyCollapsed(): void {
    collapseButton.textContent = commitCollapsed ? '▸' : '▾';
    collapseButton.title = commitCollapsed ? 'Show the description and the commit options' : 'Collapse to the summary line';
    collapseButton.setAttribute('aria-label', commitCollapsed ? 'Show Description' : 'Hide Description');
    collapseButton.setAttribute('aria-expanded', String(!commitCollapsed));
    commitBox.classList.toggle('collapsed', commitCollapsed);
    if (commitCollapsed) {
      summaryRow.replaceChildren(collapseButton, subject, lengthLabel, commitActions);
      commitBox.replaceChildren(summaryRow);
      commitBox.style.height = '';
    } else {
      summaryRow.replaceChildren(collapseButton, subject);
      commitTools.replaceChildren(recentButton, insertButton, lengthLabel);
      commitRow.replaceChildren(amendLabel, commitActions);
      commitBox.replaceChildren(summaryRow, body, commitTools, commitRow);
      commitBox.style.height = commitHeight ? `${commitHeight}px` : '';
    }
    commitDivider.hidden = commitCollapsed;
  }

  commitDivider.addEventListener('dblclick', () => {
    commitHeight = undefined;
    commitBox.style.height = '';
    post({ type: 'wcLook', commitHeight: 0 });
  });
  commitDivider.addEventListener('mousedown', (e) => {
    if (commitCollapsed) return;
    e.preventDefault();
    const top = commitBox.getBoundingClientRect().top;
    const onMove = (m: MouseEvent) => {
      const max = Math.max(COMMIT_MIN, right.clientHeight - DIFF_MIN);
      commitHeight = Math.round(Math.min(max, Math.max(COMMIT_MIN, m.clientY - top)));
      commitBox.style.height = `${commitHeight}px`;
    };
    const onUp = () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
      document.body.classList.remove('resizing-rows');
      if (commitHeight) post({ type: 'wcLook', commitHeight });
    };
    document.body.classList.add('resizing-rows');
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  });

  // 一覧の絞り込みと見せ方
  const tools = el('div', 'wc-tools');
  const filter = el('input');
  filter.type = 'search';
  filter.placeholder = 'Filter by path';
  filter.setAttribute('aria-label', 'Filter by path');
  // Mixed（1 つの一覧）と Staged・Unstaged の節の切り替え（エンジンがあるとき）
  const sectionsButton = el('button', 'wc-sections', '⊟');
  sectionsButton.title = 'Split the list into Staged and Unstaged, like git status (a partly staged file is in both)';
  sectionsButton.setAttribute('aria-label', 'Staged and Unstaged');
  sectionsButton.addEventListener('click', () => {
    sectionsWanted = !sectionsWanted;
    post({ type: 'wcLook', sections: sectionsWanted });
    relist();
  });
  const layoutButtons = el('div', 'wc-layout');
  tools.append(filter, sectionsButton, layoutButtons);
  let layout: FileLayout = 'list';
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
  // 一覧の見出し: すべてのチェックと、次のコミットに入るファイルの数 / 変わったファイルの数（エンジンがあるとき）
  const listHead = el('div', 'wc-head');
  const allCheck = el('input', 'wc-check');
  allCheck.type = 'checkbox';
  allCheck.title = 'Include or exclude all listed files in the next commit';
  allCheck.setAttribute('aria-label', 'Include All');
  allCheck.addEventListener('click', (e) => {
    e.preventDefault();
    toggle(listed);
  });
  const countLabel = el('span', 'count');
  countLabel.title = 'Files in the next commit / changed files';
  listHead.append(allCheck, countLabel);
  // sparse checkout なら、一覧の上にそう出す（取り出していないフォルダのファイルは変更に出ない）
  const sparseNote = el('button', 'wc-sparse');
  sparseNote.hidden = true;
  sparseNote.addEventListener('click', () => post({ type: 'wcShowFolders' }));
  left.append(tools, sparseNote, listHead, fileList);

  let files: Change[] = [];
  let lastMessage = '';
  /** エンジンで読んでいる（見る側の帯・Mixed の一覧・空白の無視はエンジンがあるときだけ） */
  let engine = false;
  /** 一覧を節に分けたい（覚える。エンジンがなければいつも節） */
  let sectionsWanted = false;
  /** 見た目の好みを、拡張から最初に受け取った（後は Webview で変えたものが正しい） */
  let lookApplied = false;
  const sectioned = () => !engine || sectionsWanted;
  /** 差分で見る側（エンジンがあるとき。Working Copy で 1 つ、ファイルを替えても同じ） */
  let side: DiffSide = 'unstaged';
  /** 差分に出している行 */
  let selected: RowKey | undefined;
  /**
   * いくつも選んだ行（⌘・Ctrl クリックで足し引き、Shift で範囲。アプリと同じ）。キーはパスと節。1 つだけのときは
   * selected だけ。差分は最後に押したもの（selected）
   */
  let marked = new Set<string>();
  let fileAnchor: string | undefined;
  const isSelected = (row: RowKey) => selected !== undefined && keyText(selected) === keyText(row);
  /** 操作の相手: いくつも選んでいて row がその中なら、選んだものすべて。でなければ row だけ */
  const targetsOf = (row: ListRow): ListRow[] =>
    marked.size > 1 && marked.has(keyText(row)) ? listed.filter((x) => marked.has(keyText(x))) : [row];
  let diff: FileDiff | undefined;
  /** diff の側のグループ（All なら、そのファイルが今あるグループ） */
  let diffGroup: ChangeGroup | undefined;
  /** 選んだファイルの差分を読めなかった理由（読めたら undefined） */
  let diffError: string | undefined;
  /** 選んだ側の差分を出さないわけ（その側に変更がない・All で出せない） */
  let diffNote: { path: string; group: ChangeGroup; note: SideNote } | undefined;
  /** 差分で無視している空白（エンジンで読んだときだけ。なければ選ぶ欄を出さない） */
  let whitespace: DiffWhitespace | undefined;
  /** 選んでいる行。キーは `old:12` / `new:7` */
  let picked = new Set<string>();
  let anchor: string | undefined;
  /** 行の Stage の形（設定。行を選んでから隙間の行の操作で・変更の帯と行番号を押して） */
  let lineStaging: LineStaging = 'select';
  /** たたんだ差分（diff.folded）の隙間の開き方（拡張が覚えているもの） */
  let gapOpen: GapOpen[] = [];
  /** 木で畳んだディレクトリ（節とパス）。読み直しても畳んだままにする */
  const collapsed = new Set<string>();
  /** 一覧に出ている行（上から順。絞り込みと畳んだディレクトリの分を除く） */
  let listed: ListRow[] = [];
  /** ファイルごとの追加・削除の行数（HEAD → 作業ツリー） */
  let stats = new Map<string, { added: number | null; removed: number | null }>();
  /** 節ごとの行数（Staged は HEAD → index、Unstaged は index → 作業ツリー。節に分けたときに読む） */
  let splitStats: { staged: Map<string, FileStat>; unstaged: Map<string, FileStat> } | undefined;

  // ---- ファイル一覧 ----

  function renderFiles(): void {
    for (const button of layoutButtons.children as HTMLCollectionOf<HTMLElement>) {
      button.classList.toggle('active', button.dataset.layout === layout);
    }
    sectionsButton.hidden = !engine;
    sectionsButton.classList.toggle('active', sectionsWanted);
    sectionsButton.setAttribute('aria-pressed', String(sectionsWanted));
    listed = [];
    const parts: HTMLElement[] = [];
    const groups = sectioned()
      ? GROUPS.map(({ group, title }) => ({ group, title, rows: sectionRows(files, group) }))
      : (() => {
          const rows = mixedRows(files);
          return [
            { group: 'conflicts' as const, title: 'Conflicts', rows: rows.filter((r) => r.section === 'conflicts') },
            { group: null, title: '', rows: rows.filter((r) => r.section !== 'conflicts') },
          ];
        })();
    for (const { group, title, rows } of groups) {
      const items = filterByPath(rows, filter.value);
      if (items.length === 0) continue;
      // Mixed の 1 つの一覧には節の見出しを付けない（見出しのチェックが上にある）
      if (group !== null) parts.push(groupHead(group, title, items));
      if (layout === 'tree') {
        renderDir(makeFileTree(items), group, 0, parts);
      } else {
        for (const row of items) {
          listed.push(row);
          parts.push(renderFile(row, 0));
        }
      }
    }
    if (parts.length === 0) {
      parts.push(el('div', 'empty', files.length > 0 ? 'No matching files' : 'No changes'));
    }
    fileList.replaceChildren(...parts);
    renderHead();
    updateCommitButton();
  }

  /** 一覧の見出し（エンジンがあるとき）: すべてのチェックと、含める数 / 全体の数（アプリと同じ） */
  function renderHead(): void {
    const all = mixedRows(files);
    listHead.hidden = !engine || all.length === 0;
    setCheck(allCheck, groupCheck(listed));
    allCheck.disabled = groupCheck(listed) === undefined;
    const included = all.filter((r) => r.section === null && r.staged).length;
    countLabel.textContent = `${included} / ${all.length}`;
  }

  function setCheck(box: HTMLInputElement, check: Check | undefined): void {
    box.checked = check === 'on';
    box.indeterminate = check === 'mixed';
  }

  /** 節の見出し: 名前と数、Stage All・Unstage All（コンフリクトは、マーカーのないものをまとめて解決済みに） */
  function groupHead(group: ChangeGroup, title: string, items: ListRow[]): HTMLElement {
    const head = el('div', 'wc-group');
    head.append(el('span', 'title', title), el('span', 'count', String(items.length)));
    if (group !== 'conflicts') {
      const all = el('button', 'link', group === 'staged' ? 'Unstage All' : 'Stage All');
      all.addEventListener('click', () =>
        stageFiles(group === 'staged' ? 'unstage' : 'stage', items.flatMap((r) => r[group] ?? [])),
      );
      head.append(all);
    }
    // マーカーの残っていないコンフリクトは、まとめて解決済みにできる
    const resolved = items.flatMap((r) => (r.conflict?.markers === 0 ? [r.conflict] : []));
    if (group === 'conflicts' && resolved.length > 0) {
      const all = el('button', 'link', `Mark ${resolved.length} as Resolved`);
      all.title = 'Mark the conflicted files that have no conflict markers left as resolved (rerere or another editor resolved them)';
      all.addEventListener('click', () => stageFiles('stage', resolved));
      head.append(all);
    }
    return head;
  }

  /** 木の 1 つのディレクトリの中身を parts に足す（ディレクトリが先、ファイルが後）。group が null なら Mixed */
  function renderDir(dir: FileDir<ListRow>, group: ChangeGroup | null, depth: number, parts: HTMLElement[]): void {
    for (const child of dir.dirs) {
      const key = `${group ?? 'mixed'}:${child.path}`;
      const open = !collapsed.has(key);
      const row = el('div', 'wc-file wc-dir');
      row.style.paddingLeft = `${12 + depth * 14}px`;
      row.title = child.path;
      row.append(el('span', 'chevron', open ? '▾' : '▸'));
      if (group === null) {
        // Mixed: 配下のすべてを入れる・外すチェック
        const below = filesUnder(child);
        const check = groupCheck(below);
        if (check !== undefined) {
          const box = el('input', 'wc-check');
          box.type = 'checkbox';
          setCheck(box, check);
          box.title = 'Include or exclude everything in this folder';
          box.addEventListener('click', (e) => {
            e.preventDefault();
            e.stopPropagation();
            toggle(below);
          });
          row.append(box);
        }
      }
      row.append(el('span', 'name', `${child.name}/`), el('span', 'dir'));
      if (group === 'staged' || group === 'unstaged') {
        const button = el('button', 'icon-button', group === 'staged' ? '−' : '+');
        button.title = group === 'staged' ? 'Unstage Folder' : 'Stage Folder';
        button.addEventListener('click', (e) => {
          e.stopPropagation();
          stageFiles(group === 'staged' ? 'unstage' : 'stage', filesUnder(child).flatMap((r) => r[group] ?? []));
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

  /** 行の追加・削除の行数（節の行はその側の、Mixed の行は HEAD → 作業ツリーの） */
  function statOf(row: ListRow): { added: number | null; removed: number | null } | undefined {
    if (row.section === 'staged' || row.section === 'unstaged') return splitStats?.[row.section].get(row.path);
    return stats.get(row.path);
  }

  function renderFile(f: ListRow, depth: number): HTMLElement {
    const change = changeOf(f);
    const row = el('div', 'wc-file');
    if (depth > 0) row.style.paddingLeft = `${12 + depth * 14}px`;
    if (isSelected(f)) row.classList.add('selected');
    const many = marked.size > 1 && marked.has(keyText(f));
    if (many) row.classList.add('marked');
    row.title = f.origPath ? `${f.origPath} → ${f.path}` : f.path;
    // Webview の右クリックメニュー（package.json の webview/context）に渡す情報
    row.dataset.vscodeContext = JSON.stringify({
      webviewSection: 'wcFile',
      path: f.path,
      // Stage・Discard…（作業ツリーの側があれば）と Unstage（index の側があれば）を出し分ける
      ...(f.unstaged ? { wcUnstaged: true } : {}),
      ...(f.staged ? { wcStaged: true } : {}),
      // いくつも選んでいれば、その中で使えるもの（Stash…）だけを、選んだものすべてに
      ...(many ? { wcMulti: true, paths: [...new Set(targetsOf(f).map((x) => x.path))] } : {}),
      // コンフリクトのファイル（Stash… を出さない）
      ...(f.conflict ? { wcConflict: true } : {}),
      // まだ履歴のないファイル（.gitignore に足せる。拡張子があれば、そのパターンも）
      ...(change.letter === 'U' ? { wcUntracked: true, ...(ignorePatterns(f.path).length > 1 ? { wcExtension: true } : {}) } : {}),
      // 作業ツリーにないファイル（Blame・開く・ファイルマネージャーで見せるを出さない）
      ...(change.letter === 'D' ? { wcDeleted: true } : {}),
      // rerere が解決を当てたコンフリクトのファイル（Forget Resolution を出す）
      ...(f.conflict?.rerere ? { wcRerere: true } : {}),
      preventDefaultContextMenuItems: true,
    });
    // Mixed: 次のコミットに入れるかのチェック（index の状態そのもの）。コンフリクトはチェックの代わりに ⚠
    if (f.section === null) {
      const check = rowCheck(f)!;
      const box = el('input', 'wc-check');
      box.type = 'checkbox';
      setCheck(box, check);
      box.title =
        check === 'on'
          ? 'Included in the next commit (click to exclude)'
          : check === 'mixed'
            ? 'Only some lines are included (click to include all)'
            : 'Not included in the next commit (click to include)';
      box.addEventListener('click', (e) => {
        e.preventDefault();
        e.stopPropagation();
        // 押したファイルが入っていれば、選んだものすべてを外す。でなければ、すべて入れる
        const targets = targetsOf(f).filter((x) => x.section !== 'conflicts');
        if (check === 'on') stageFiles('unstage', targets.flatMap((x) => x.staged ?? []));
        else stageFiles('stage', targets.flatMap((x) => x.unstaged ?? []));
      });
      row.append(box);
    } else if (f.section === 'conflicts' && engine && !sectioned()) {
      row.append(el('span', 'wc-check conflict', '⚠'));
    }
    const slash = f.path.lastIndexOf('/');
    const name = layout === 'list' ? f.path : f.path.slice(slash + 1);
    if (f.conflict?.markers === 0) {
      // マーカーが残っていない。解決できていそうなことを印と短い注記で示す
      row.title += `\n${withoutMarkersMessage(f.conflict)}`;
      row.append(el('span', 'status s-resolved', '✓'));
      row.append(el('span', 'name', name));
      row.append(el('span', 'dir', f.conflict.rerere ? 'rerere' : 'no markers'));
    } else {
      row.append(el('span', `status s-${f.letter === '!' ? 'U' : f.letter}`, f.letter));
      row.append(el('span', 'name', name));
      row.append(el('span', 'dir', layout === 'flat' && slash >= 0 ? f.path.slice(0, slash) : ''));
    }
    const stat = statOf(f);
    if (stat && (stat.added || stat.removed)) {
      const counts = el('span', 'stat');
      if (stat.added) counts.append(el('span', 'added', `+${stat.added}`));
      if (stat.removed) counts.append(el('span', 'removed', `−${stat.removed}`));
      row.append(counts);
    }
    // 節の行: Stage・Unstage のボタン
    if (f.section === 'staged' || f.section === 'unstaged') {
      const group = f.section;
      const button = el('button', 'icon-button', group === 'staged' ? '−' : '+');
      button.title = group === 'staged' ? 'Unstage' : 'Stage';
      button.addEventListener('click', (e) => {
        e.stopPropagation();
        stageFiles(group === 'staged' ? 'unstage' : 'stage', targetsOf(f).flatMap((x) => (x.section === group ? (x[group] ?? []) : [])));
      });
      row.append(button);
    }
    // 作業ツリーの側の変更を捨てる（ホバーで出す）
    if (f.unstaged && f.section !== 'staged') {
      const discard = el('button', 'icon-button', '↺');
      discard.title = 'Discard Changes';
      discard.addEventListener('click', (e) => {
        e.stopPropagation();
        post({ type: 'wcDiscardFiles', paths: unstagedPaths(targetsOf(f)) });
      });
      if (f.section === 'unstaged') row.insertBefore(discard, row.lastElementChild);
      else row.append(discard);
    }
    row.addEventListener('click', (e) => {
      if (e.metaKey || e.ctrlKey) toggleMark(f);
      else if (e.shiftKey) markRange(f);
      else select(f);
    });
    row.addEventListener('dblclick', () => post({ type: 'wcOpenFile', path: f.path }));
    return row;
  }

  /** 作業ツリーの側の変更があるファイル（捨てられるもの。節分けでは Unstaged の節の行だけ） */
  function unstagedPaths(rows: ListRow[]): string[] {
    return [...new Set(rows.filter((r) => r.unstaged && r.section !== 'staged').map((r) => r.path))];
  }

  function select(row: ListRow, keepMarks = false): void {
    selected = keyOf(row);
    if (!keepMarks) {
      marked = new Set([keyText(row)]);
      fileAnchor = keyText(row);
    }
    // 節の行を 1 つだけ選んだら、見る側をその節にそろえる（アプリと同じ）
    const before = side;
    if (engine && marked.size <= 1) side = adoptSide(row, side);
    renderFiles();
    if (side !== before) renderDiff();
    post({ type: 'wcSelect', group: groupOf(row), path: row.path, ...(side !== before ? { side } : {}) });
  }

  /** ⌘・Ctrl クリック: 選んだものに足す・外す（差分は押したもの） */
  function toggleMark(f: ListRow): void {
    const k = keyText(f);
    if (marked.has(k) && marked.size > 1) marked.delete(k);
    else marked.add(k);
    fileAnchor = k;
    select(f, true);
  }

  /** Shift: 前に押したものから f までを選ぶ（一覧の並びで） */
  function markRange(f: ListRow): void {
    const from = listed.findIndex((x) => keyText(x) === fileAnchor);
    const to = listed.indexOf(f);
    if (from < 0 || to < 0) return select(f);
    marked = new Set(listed.slice(Math.min(from, to), Math.max(from, to) + 1).map(keyText));
    select(f, true);
  }

  /**
   * 一覧を作り直して、選んでいた行を選び直す（読み直した・節分けを切り替えたとき）。いくつか選んでいれば残っている
   * ものすべて、別の節へ移ったファイルはその節の行。差分に出していた行がなくなれば、選んでいたほかの行、それも
   * なければ見る側の節の最初の行（アプリの WorkingCopyReselect と同じ）
   */
  function relist(): void {
    const keptKeys = [...marked].map(parseKey);
    renderFiles();
    const rows = listed.map(keyOf);
    const kept = reselectRows(keptKeys, rows);
    marked = new Set(kept.map((i) => keyText(rows[i]!)));
    const target = reselectTarget(selected, rows, kept, side);
    const row = target === undefined ? undefined : listed[target];
    if (!row || isSelected(row)) {
      renderFiles();
      return;
    }
    const pathChanged = selected?.path !== row.path;
    const key = keyText(row);
    if (marked.size === 0) marked = new Set([key]);
    else marked.add(key);
    fileAnchor = marked.has(fileAnchor ?? '') ? fileAnchor : key;
    selected = keyOf(row);
    const before = side;
    if (engine && marked.size <= 1) side = adoptSide(row, side);
    renderFiles();
    // 同じファイルで見る側も同じなら、拡張が読み直した差分を送る
    if (pathChanged || side !== before) {
      if (side !== before) renderDiff();
      post({ type: 'wcSelect', group: groupOf(row), path: row.path, ...(side !== before ? { side } : {}) });
    }
  }

  function stageFiles(op: 'stage' | 'unstage', items: Change[]): void {
    if (items.length === 0) return;
    post({ type: 'wcStageFiles', op, files: items.map((f) => ({ group: f.group, path: f.path })) });
  }

  /** チェックを押す・Space: すべて入っていれば外し、でなければすべて入れる（アプリと同じ） */
  function toggle(rows: ListRow[]): void {
    const request = toggleRequest(rows);
    if (request) stageFiles(request.op, request.files);
  }

  fileList.addEventListener('keydown', (e) => {
    const order = listed;
    const index = order.findIndex(isSelected);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const next = order[Math.min(order.length - 1, Math.max(0, index + (e.key === 'ArrowDown' ? 1 : -1)))];
      if (next && e.shiftKey) markRange(next);
      else if (next) select(next);
    } else if ((e.key === ' ' || e.key === 'Enter') && order[index]) {
      e.preventDefault();
      // 選んだものすべて: どれかが入っていなければ入れ、すべて入っていれば外す（アプリと同じ）
      toggle(targetsOf(order[index]!));
    } else if ((e.key === 'Backspace' || e.key === 'Delete') && order[index]) {
      e.preventDefault();
      const paths = unstagedPaths(targetsOf(order[index]!));
      if (paths.length > 0) post({ type: 'wcDiscardFiles', paths });
    }
  });

  // J・K で次・前の hunk、⌥J・⌥K で次・前のファイル（文字を打つ欄の中では効かない）
  root.addEventListener('keydown', (e) => {
    if (e.metaKey || e.ctrlKey || e.shiftKey || (e.code !== 'KeyJ' && e.code !== 'KeyK')) return;
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    e.preventDefault();
    const forward = e.code === 'KeyJ';
    if (e.altKey) {
      const index = listed.findIndex(isSelected);
      const next = listed[forward ? index + 1 : Math.max(0, index - 1)];
      if (next) select(next);
      return;
    }
    if (diff?.folded) {
      moveBlock(forward);
      return;
    }
    const top = diffPane.getBoundingClientRect().top;
    const heads = [...diffPane.querySelectorAll<HTMLElement>('.hunk-head')].map((h) => h.getBoundingClientRect().top - top);
    // 見出しは差分の見出しの下に止まるので、少し余裕を見て比べる
    const offset = diffPane.querySelector<HTMLElement>('.diff-head')?.offsetHeight ?? 0;
    const target = forward
      ? heads.find((y) => y > offset + 2)
      : heads.filter((y) => y < offset - 2).at(-1);
    if (target !== undefined) diffPane.scrollBy({ top: target - offset });
  });

  filter.addEventListener('input', () => {
    renderFiles();
    // 選んでいたファイルが隠れたら、残っている最初のものへ
    const shown = listed.some(isSelected);
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

  /** 拡張が選んだファイル（コンフリクトで止まったとき）・別の側の差分を送ってきたら、一覧でもその行を選ぶ */
  function followShown(path: string, group: ChangeGroup): void {
    if (selected?.path === path) return;
    const row = listed.find((r) => r.path === path && r.section === group) ?? listed.find((r) => r.path === path);
    selected = row ? keyOf(row) : { path, section: sectioned() ? group : null };
    marked = new Set([keyText(selected)]);
    fileAnchor = keyText(selected);
  }

  /** 差分の帯の、見る側の切り替え（エンジンがあって、コンフリクトでないとき） */
  function sideBand(): HTMLElement {
    const band = el('div', 'wc-sides');
    band.setAttribute('role', 'group');
    band.setAttribute('aria-label', 'Diff Side');
    for (const s of SIDES) {
      const button = el('button', s.side === side ? 'active' : undefined, s.label);
      button.title = s.title;
      button.setAttribute('aria-pressed', String(s.side === side));
      button.addEventListener('click', () => chooseSide(s.side));
      band.append(button);
    }
    return band;
  }

  function chooseSide(next: DiffSide): void {
    if (next === side) return;
    side = next;
    // 節分けでは、一覧の選択もその側の節の行に移す（その節になければ、そのまま。アプリと同じ）
    if (sectioned() && selected && marked.size <= 1) {
      const row = listed.find((r) => r.path === selected!.path && r.section === next);
      if (row) {
        selected = keyOf(row);
        marked = new Set([keyText(row)]);
        fileAnchor = keyText(row);
      }
    }
    renderFiles();
    renderDiff();
    post({ type: 'wcSide', side: next });
  }
  // ---- 差分 ----

  /** 改行を変換している（作業ツリーと git で違う）・改行が混ざっているファイルの印。差分には出ないので */
  function lineEndingsBadge(le: LineEndings): HTMLElement {
    const name = (e: Eol) => (e === 'lf' ? 'LF' : e === 'crlf' ? 'CRLF' : e === 'mixed' ? 'Mixed' : 'None');
    // 作業ツリーの形 → git に入る形
    const text = le.worktree !== le.index && le.index !== 'none' ? `${name(le.worktree)} → ${name(le.index)}` : name(le.worktree);
    const badge = el('span', le.warning ? 'eol warning' : 'eol', le.warning ? `⚠ ${text}` : text);
    const tip = [`Line endings: working tree ${name(le.worktree)}, git ${name(le.index)}, checkout ${name(le.checkout)}`];
    if (le.rule) tip.push(`Conversion: ${le.rule}`);
    if (le.warning) tip.push('The next commit or checkout may change the line endings of the whole file');
    badge.title = tip.join('\n');
    return badge;
  }

  /** 差分の見出し: パス、見る側の帯（エンジンがあれば）、改行の印、空白、折り返し */
  function diffHead(path: string, group: ChangeGroup, lineEndings?: LineEndings): HTMLElement {
    const head = el('div', 'diff-head');
    const label = el('span', 'path', path);
    // ボタンの列を次の段に送る前にパスに残す幅。短いパスには、その長さの分だけ取る（上限は main.css の 8em）
    label.style.flexBasis = `min(${[...path].length}ch, 8em)`;
    head.append(label);
    if (engine && group !== 'conflicts') head.append(sideBand());
    if (lineEndings?.notable) {
      head.append(lineEndingsBadge(lineEndings));
    }
    if (whitespace && group !== 'conflicts') {
      head.append(whitespaceSelect(whitespace, (w) => post({ type: 'wcWhitespace', whitespace: w })));
    }
    head.append(wrapButton());
    return head;
  }

  function renderDiff(): void {
    if (!diff) {
      // 選んだ側に変更がない・All で出せないときは、帯を残してそう言う（アプリと同じ）
      if (diffNote && diffNote.path === selected?.path) {
        diffPane.replaceChildren(diffHead(diffNote.path, diffNote.group), el('div', 'empty side-note', sideNoteText(diffNote.note)));
        return;
      }
      // 読めなかったときは、空の差分にせず理由を出す（アプリと同じ）
      const text = diffError ? `Can’t read the diff: ${diffError}` : files.length ? 'Select a file to see its diff' : '';
      diffPane.replaceChildren(el('div', 'empty', text));
      return;
    }
    const d = diff;
    const group = diffGroup!;
    const head = diffHead(d.path, group, d.lineEndings);
    const actions = el('span', 'actions');
    if (group === 'conflicts') {
      diffPane.replaceChildren(head, renderConflict(d));
      return;
    }
    if (d.folded) {
      renderFolded(d, head, actions);
      return;
    }
    if (d.combined) {
      renderCombined(d, head, actions);
      return;
    }
    const staged = group === 'staged';
    // 空白を無視した差分の行は中身をそのまま表していないので、行・hunk ごとには stage・破棄しない（macOS 版と同じ）
    const exact = (whitespace ?? 'exact') === 'exact';
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
      if (!exact) {
        for (const button of actions.querySelectorAll('button')) {
          button.disabled = true;
          button.title = 'Can’t stage or discard lines while whitespace is ignored';
        }
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
        hunkActions: !exact ? [] : [
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

  // ---- たたんだ差分（エンジンがあるとき） ----

  /** 出している差分の側（たたんだ差分の hunk の操作の名前を決める） */
  function foldedSide(d: FileDiff): FoldedSide {
    return d.combined ? 'all' : diffGroup === 'staged' ? 'staged' : 'unstaged';
  }

  /** 差分の変更の行（上から順）と、lineKey → 行・その hunk の位置・変更のかたまり */
  function changesOf(d: FileDiff): { lines: DiffLine[]; byKey: Map<string, { line: DiffLine; hunk: number; block: number }> } {
    const byKey = new Map<string, { line: DiffLine; hunk: number; block: number }>();
    const blockOf = new Map<DiffLine, number>();
    changeBlocks(d.hunks).forEach((block, i) => block.forEach((l) => blockOf.set(l, i)));
    const lines: DiffLine[] = [];
    d.hunks.forEach((h, hunk) => {
      for (const line of h.lines) {
        if (line.kind === ' ') continue;
        lines.push(line);
        byKey.set(lineKey(line), { line, hunk, block: blockOf.get(line)! });
      }
    });
    return { lines, byKey };
  }

  /** 空白を無視していない（行・hunk を stage・破棄できる） */
  const exactDiff = () => (whitespace ?? 'exact') === 'exact';

  /**
   * 変更のない所を隙間の行にたたんだ差分（エンジンがあるとき。アプリと同じ）。hunk の見出しの行はなく、隙間の行の
   * 右端に hunk の操作を置く。「変更の帯」の形では、行の左の帯と行番号で stage する。All では帯が stage 済み（塗り）
   * か、まだか（枠だけ）を示す
   */
  function renderFolded(d: FileDiff, head: HTMLElement, actions: HTMLElement): void {
    const fside = foldedSide(d);
    const exact = exactDiff();
    const bar = lineStaging === 'bar';
    const { lines: changed, byKey } = changesOf(d);
    const pickedLines = changed.filter((l) => picked.has(lineKey(l)));
    // 「変更の帯」の形では、選んだ行の操作を帯に置く（「選んでから」の形では隙間の行と右クリック・Space で）
    if (bar) {
      const ops =
        pickedLines.length > 0
          ? stageActions(fside, pickedLines, 'Lines')
          : [{ op: fside === 'staged' ? ('unstage' as const) : ('stage' as const), label: fside === 'staged' ? 'Unstage Lines' : 'Stage Lines' }];
      for (const { op, label } of ops) {
        const button = el('button', op === 'discard' ? 'secondary' : undefined, label);
        button.disabled = pickedLines.length === 0 || !exact;
        if (!exact) button.title = 'Can’t stage or discard lines while whitespace is ignored';
        else if (pickedLines.length === 0) button.title = 'Select lines to stage them (or click the change bar or line numbers)';
        button.addEventListener('click', () => runOp(op, pickedLines));
        actions.append(button);
      }
    }
    const unstaged = files.find((x) => x.group === 'unstaged' && x.path === d.path);
    const stagedFile = files.find((x) => x.group === 'staged' && x.path === d.path);
    if (unstaged && fside !== 'staged') {
      const stageFile = el('button', 'secondary', 'Stage File');
      stageFile.addEventListener('click', () => stageFiles('stage', [unstaged]));
      actions.append(stageFile);
    }
    if (stagedFile && fside !== 'unstaged') {
      const unstageFile = el('button', 'secondary', 'Unstage File');
      unstageFile.addEventListener('click', () => stageFiles('unstage', [stagedFile]));
      actions.append(unstageFile);
    }
    if (unstaged && fside !== 'staged') {
      const discardFile = el('button', 'secondary', 'Discard File');
      discardFile.addEventListener('click', () => post({ type: 'wcDiscardFiles', paths: [d.path] }));
      actions.append(discardFile);
    }
    const open = el('button', 'secondary', 'Open File');
    open.addEventListener('click', () => post({ type: 'wcOpenFile', path: d.path }));
    actions.append(open);
    // Show Whole File（すべての隙間を開く・閉じる。帯の ↩︎ の左）
    const whole = showsWholeFile(d.hunks);
    const wholeButton = el('button', whole ? 'bar-button whole-file on' : 'bar-button whole-file', '⇳');
    wholeButton.title = `${whole ? 'Showing the whole file' : 'Show Whole File'}\nOpen every unchanged part of the file (each part also opens from its own row)`;
    wholeButton.setAttribute('aria-label', 'Show Whole File');
    wholeButton.setAttribute('aria-pressed', String(whole));
    wholeButton.addEventListener('click', () => setGaps(d, toggleWholeFile(d.hunks)));
    head.insertBefore(wholeButton, head.querySelector('.wrap-button'));
    head.append(actions);
    const body = renderDiffBody(d, {
      picked,
      ...(bar || fside === 'all' ? { bar: (line: DiffLine) => (fside === 'all' ? (line.staged ? 'staged' : 'unstaged') : 'mark') } : {}),
      gapRow: (hunk, index) => {
        const inHunk = hunk.lines.filter((l) => l.kind !== ' ');
        const selected = bar ? [] : inHunk.filter((l) => picked.has(lineKey(l)));
        return gapRow({
          hunk,
          index,
          selected: selected.length,
          ops: !exact || bar ? [] : stageActions(fside, selected.length > 0 ? selected : inHunk, selected.length > 0 ? 'Lines' : 'Hunk'),
          onGap: (action) => applyGap(d, hunk.gap!.index, action),
          onOp: (op) => runOp(op, selected.length > 0 ? selected : inHunk),
          onMenu: (row, pill) => {
            // 札の下に、開く・畳むだけのメニュー（package.json の webview/context）
            pillMenu = true;
            const r = pill.getBoundingClientRect();
            row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left, clientY: r.bottom, button: 2 }));
            pillMenu = false;
          },
        });
      },
    });
    diffPane.classList.toggle('bar-style', bar);
    diffPane.replaceChildren(head, ...body);
    folded = { diff: d, byKey };
    layoutGaps();
  }

  /** 今のたたんだ差分（マウス・右クリック・J・K が使う） */
  let folded: { diff: FileDiff; byKey: Map<string, { line: DiffLine; hunk: number; block: number }> } | undefined;
  /** 札を押してメニューを開いている（開く・畳むだけを出す） */
  let pillMenu = false;

  /** 隙間の行の札とボタンを置き直す（描いた後と、幅が変わったとき） */
  function layoutGaps(): void {
    const box = diffPane.querySelector<HTMLElement>('.hunk.folded');
    if (!box) return;
    const code = box.querySelector<HTMLElement>('.line .code');
    const gutter = code ? code.getBoundingClientRect().left - box.getBoundingClientRect().left : 0;
    box.style.setProperty('--gap-gutter', `${gutter}px`);
    layoutGapRows(box, gutter);
  }
  let layoutFrame = 0;
  new ResizeObserver(() => {
    cancelAnimationFrame(layoutFrame);
    layoutFrame = requestAnimationFrame(layoutGaps);
  }).observe(diffPane);

  /** 隙間の開き方を変えて、拡張に覚えてもらう（差分を読み直して送ってくる） */
  function setGaps(d: FileDiff, open: GapOpen[]): void {
    gapOpen = open;
    post({ type: 'wcGaps', path: d.path, side: foldedSide(d), open });
  }

  function applyGap(d: FileDiff, gap: number, action: GapAction): void {
    setGaps(d, openGap(gapOpen, gap, action));
  }

  /** 変更の行 lines を、出している側で stage・unstage・破棄する（All は入れる・外す） */
  function runOp(op: StageOp, lines: DiffLine[]): void {
    if (!diff || lines.length === 0 || !exactDiff()) return;
    if (diff.combined) {
      if (op !== 'discard') checkLines(lines, op === 'stage');
      else discard(lines.filter((l) => !l.staged).map(lineKey));
      return;
    }
    if (op === 'discard') discard(lines.map(lineKey));
    else stageLines(lines.map(lineKey));
  }

  /**
   * 差分の右クリック（package.json の webview/context、wcDiff）に渡す情報。隙間の行では開く・畳む（札を押したときは
   * それだけ）、それから選んだ行・その hunk・その行の Stage・Unstage・Discard（アプリと同じ）
   */
  function diffMenuContext(target: HTMLElement): { row: HTMLElement; context: Record<string, unknown> } | undefined {
    if (!folded || !diff?.folded) return undefined;
    const { diff: d, byKey } = folded;
    const gapEl = target.closest<HTMLElement>('.gap-row');
    const lineEl = gapEl ? undefined : target.closest<HTMLElement>('.line');
    const row = gapEl ?? lineEl;
    if (!row) return undefined;
    const fside = foldedSide(d);
    const context: Record<string, unknown> = { webviewSection: 'wcDiff', preventDefaultContextMenuItems: true };
    const flag = (prefix: string, ops: { op: StageOp }[]) => {
      for (const { op } of ops) context[`${prefix}${op[0]!.toUpperCase()}${op.slice(1)}`] = true;
    };
    const hunk = Number((gapEl ?? lineEl?.closest<HTMLElement>('.lines'))?.dataset.hunk);
    const h = d.hunks[hunk];
    if (h) context['hunk'] = hunk;
    const bar = lineStaging === 'bar';
    if (gapEl && h?.gap) {
      context['gap'] = h.gap.index;
      const selectedHere = bar ? 0 : h.lines.filter((l) => l.kind !== ' ' && picked.has(lineKey(l))).length;
      for (const { action } of gapActions(h.gap, selectedHere > 0)) context[`wcGap${action[0]!.toUpperCase()}${action.slice(1)}`] = true;
      if (pillMenu) return { row, context };
    }
    if (exactDiff()) {
      const pickedLines = [...picked].flatMap((k) => byKey.get(k)?.line ?? []);
      flag('wcSel', stageActions(fside, pickedLines, 'Lines'));
      if (h) flag('wcHunk', stageActions(fside, h.lines.filter((l) => l.kind !== ' '), 'Hunk'));
      const key = lineEl?.dataset.key;
      const line = key ? byKey.get(key)?.line : undefined;
      if (key && line) {
        context['line'] = key;
        flag('wcLine', stageActions(fside, [line], 'Lines'));
      }
    }
    return { row, context };
  }

  // 右クリックした行の情報を、VSCode がメニューを出す前に（window で受ける前に）その行に置く
  diffPane.addEventListener(
    'contextmenu',
    (e) => {
      const found = diffMenuContext(e.target as HTMLElement);
      if (found) found.row.dataset.vscodeContext = JSON.stringify(found.context);
    },
    true,
  );

  /** 右クリックの項目を選んだ（拡張から頼み返される） */
  function diffAction(action: DiffMenuAction, at: { gap?: number; hunk?: number; line?: string }): void {
    if (!folded || folded.diff !== diff) return;
    const { diff: d, byKey } = folded;
    const gapAction = ({ gapDown: 'down', gapUp: 'up', gapAll: 'all', gapCollapse: 'collapse' } as const)[action as 'gapDown'];
    if (gapAction) {
      if (at.gap !== undefined) applyGap(d, at.gap, gapAction);
      return;
    }
    const op: StageOp = action.startsWith('discard') ? 'discard' : action.startsWith('unstage') ? 'unstage' : 'stage';
    let lines: DiffLine[] = [];
    if (action.endsWith('Selected')) lines = [...picked].flatMap((k) => byKey.get(k)?.line ?? []);
    else if (action.endsWith('Hunk')) lines = d.hunks[at.hunk ?? -1]?.lines.filter((l) => l.kind !== ' ') ?? [];
    else if (at.line) lines = byKey.get(at.line) ? [byKey.get(at.line)!.line] : [];
    runOp(op, lines);
  }

  /**
   * HEAD → 作業ツリーの 1 つの差分（エンジンがあるとき）。変更の行と hunk の見出しのチェックが「次のコミットに
   * 入れるか」で、押すとその行・hunk を入れる・外す。選んだ行はまとめて入れる・外す・捨てる（捨てるのは入れて
   * いない行だけ）
   */
  function renderCombined(d: FileDiff, head: HTMLElement, actions: HTMLElement): void {
    const changed = d.hunks.flatMap((h) => h.lines.filter((l) => l.kind !== ' '));
    const pickedLines = changed.filter((l) => picked.has(lineKey(l)));
    if (pickedLines.length > 0) {
      const n = pickedLines.length;
      const allStaged = pickedLines.every((l) => l.staged);
      const toggle = el('button', undefined, `${allStaged ? 'Unstage' : 'Stage'} ${n} Line${n > 1 ? 's' : ''}`);
      toggle.addEventListener('click', () => checkLines(pickedLines, !allStaged));
      actions.append(toggle);
      const loose = pickedLines.filter((l) => !l.staged);
      if (loose.length > 0) {
        const discardLines = el('button', 'secondary', `Discard ${loose.length} Line${loose.length > 1 ? 's' : ''}`);
        discardLines.addEventListener('click', () => discard(loose.map(lineKey)));
        actions.append(discardLines);
      }
    }
    const unstaged = files.find((x) => x.group === 'unstaged' && x.path === d.path);
    const stagedFile = files.find((x) => x.group === 'staged' && x.path === d.path);
    if (unstaged) {
      const stageFile = el('button', 'secondary', 'Stage File');
      stageFile.addEventListener('click', () => stageFiles('stage', [unstaged]));
      actions.append(stageFile);
    }
    if (stagedFile) {
      const unstageFile = el('button', 'secondary', 'Unstage File');
      unstageFile.addEventListener('click', () => stageFiles('unstage', [stagedFile]));
      actions.append(unstageFile);
    }
    if (unstaged) {
      const discardFile = el('button', 'secondary', 'Discard File');
      discardFile.addEventListener('click', () => post({ type: 'wcDiscardFiles', paths: [d.path] }));
      actions.append(discardFile);
    }
    const open = el('button', 'secondary', 'Open File');
    open.addEventListener('click', () => post({ type: 'wcOpenFile', path: d.path }));
    actions.append(open);
    head.append(actions);
    diffPane.replaceChildren(
      head,
      ...renderDiffBody(d, {
        picked,
        check: checkLines,
        hunkActions: [
          {
            label: 'Discard Hunk',
            run: (hunk: DiffHunk) => discard(hunk.lines.filter((l) => l.kind !== ' ' && !l.staged).map(lineKey)),
          },
        ],
      }),
    );
  }

  /** HEAD → 作業ツリーの差分で、lines を入れる・外す。すぐにチェックを変えて描き直し、status が来たら合わせる */
  function checkLines(lines: DiffLine[], stage: boolean): void {
    if (!diff || lines.length === 0) return;
    post({
      type: 'wcStageLines',
      group: diffGroup!,
      path: diff.path,
      version: diff.version,
      lines: toLines(lines.map(lineKey)),
      stage,
      combined: true,
    });
    for (const line of lines) line.staged = stage;
    picked = new Set();
    renderDiff();
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
          : 'This file has conflicts. Resolve them in the merge editor, or take one side’s version.',
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
      ['current', 'Take Current File'],
      ['incoming', 'Take Incoming File'],
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
      ? 'git rerere applied an earlier resolution. Check the result, then Mark as Resolved.'
      : 'No conflict markers are left. Check the result, then Mark as Resolved.';
  }

  function toLines(keys: string[]): SelectedLine[] {
    return keys.map((k) => {
      const [side, no] = k.split(':') as [string, string];
      return side === 'old' ? { kind: '-', old: Number(no) } : { kind: '+', new: Number(no) };
    });
  }

  function stageLines(keys: string[]): void {
    if (!diff || keys.length === 0 || (whitespace ?? 'exact') !== 'exact') return;
    post({ type: 'wcStageLines', group: diffGroup!, path: diff.path, version: diff.version, lines: toLines(keys) });
    picked = new Set();
  }

  /** 選んだ行を破棄する。確認は拡張ホストで行う */
  function discard(keys: string[]): void {
    if (!diff || keys.length === 0 || (whitespace ?? 'exact') !== 'exact') return;
    post({ type: 'wcDiscardLines', path: diff.path, version: diff.version, lines: toLines(keys), ...(diff.combined ? { combined: true } : {}) });
  }

  /**
   * 行を押して選ぶ（アプリと同じ）: 押した行だけ、⌘・Ctrl で足し引き、Shift で前に押した行からの範囲。押したまま
   * 動かすと、押した行からの範囲（⌘・Ctrl なら前に選んでいた行に足す）
   */
  let rowDrag: { anchor: number; base: Set<string>; rows: HTMLElement[] } | undefined;
  /** 「変更の帯」の形で、行番号の上をドラッグしている（離したら、その間の変更の行を stage する） */
  let numberDrag: { anchor: number; rows: HTMLElement[]; keys: string[] } | undefined;
  /** 「変更の帯」の形で、マウスが乗っている帯のかたまり */
  let hotBlock: number | undefined;

  /** 今描いている差分の行（上から順） */
  const lineRows = () => [...diffPane.querySelectorAll<HTMLElement>('.line')];

  /** 行 first〜last（lineRows の位置）の、変更の行の lineKey */
  const keysBetween = (rows: HTMLElement[], a: number, b: number) =>
    rows.slice(Math.min(a, b), Math.max(a, b) + 1).flatMap((r) => (r.dataset.key ? [r.dataset.key] : []));

  /** 選んだ行の印だけ付け直す（ドラッグの間。離したら描き直す） */
  function markPicked(rows: HTMLElement[]): void {
    for (const r of rows) if (r.dataset.key) r.classList.toggle('picked', picked.has(r.dataset.key));
  }

  /** 帯のかたまり block の行に印を付ける（undefined なら外す） */
  function markBlock(block: number | undefined): void {
    if (block === hotBlock) return;
    hotBlock = block;
    for (const r of diffPane.querySelectorAll<HTMLElement>('.line.hot')) r.classList.remove('hot');
    if (block === undefined || !folded) return;
    for (const r of diffPane.querySelectorAll<HTMLElement>('.line[data-key]')) {
      if (folded.byKey.get(r.dataset.key!)?.block === block) r.classList.add('hot');
    }
  }

  diffPane.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    if (target.closest('.gap-row, .diff-head')) return;
    const row = target.closest<HTMLElement>('.line');
    const k = row?.dataset.key;
    if (!row || !k) return;
    const info = folded?.diff === diff ? folded?.byKey.get(k) : undefined;
    if (info && diff?.folded) {
      const bar = lineStaging === 'bar';
      // 帯: 「変更の帯」の形ならかたまりごと、All ならその行を入れる・外す
      if (target.closest('.bar') && (bar || diff.combined)) {
        e.preventDefault();
        const lines = bar ? [...folded!.byKey.values()].filter((x) => x.block === info.block).map((x) => x.line) : [info.line];
        runOp(stageOpFor(lines), lines);
        return;
      }
      // 行番号（「変更の帯」の形）: 離したときに、押した行からドラッグした行までを stage する
      if (bar && target.closest('.no')) {
        e.preventDefault();
        const rows = lineRows();
        const at = rows.indexOf(row);
        numberDrag = { anchor: at, rows, keys: [k] };
        row.classList.add('hot');
        return;
      }
    }
    e.preventDefault();
    diffPane.focus();
    const rows = lineRows();
    const at = rows.indexOf(row);
    if (e.shiftKey && anchor && rows.some((r) => r.dataset.key === anchor)) {
      const from = rows.findIndex((r) => r.dataset.key === anchor);
      picked = new Set(keysBetween(rows, from, at));
      rowDrag = { anchor: from, base: new Set(), rows };
    } else if (e.metaKey || e.ctrlKey) {
      const before = new Set(picked);
      if (picked.has(k)) picked.delete(k);
      else picked.add(k);
      before.delete(k);
      anchor = k;
      rowDrag = { anchor: at, base: before, rows };
    } else {
      picked = new Set([k]);
      anchor = k;
      rowDrag = { anchor: at, base: new Set(), rows };
    }
    markPicked(rows);
  });

  diffPane.addEventListener('mousemove', (e) => {
    const target = e.target as HTMLElement;
    const row = target.closest<HTMLElement>('.line');
    if (numberDrag && e.buttons & 1) {
      const at = row ? numberDrag.rows.indexOf(row) : -1;
      if (at < 0) return;
      numberDrag.keys = keysBetween(numberDrag.rows, numberDrag.anchor, at);
      for (const r of numberDrag.rows) r.classList.toggle('hot', r.dataset.key !== undefined && numberDrag.keys.includes(r.dataset.key));
      return;
    }
    if (rowDrag && e.buttons & 1) {
      const at = row ? rowDrag.rows.indexOf(row) : -1;
      if (at < 0) return;
      picked = new Set([...rowDrag.base, ...keysBetween(rowDrag.rows, rowDrag.anchor, at)]);
      markPicked(rowDrag.rows);
      return;
    }
    // 「変更の帯」の形: 帯の上なら、そのかたまりに印を付ける（押すとかたまりごと）
    const key = row?.dataset.key;
    const onBar = lineStaging === 'bar' && diff?.folded && key && target.closest('.bar');
    markBlock(onBar ? folded?.byKey.get(key)?.block : undefined);
  });
  diffPane.addEventListener('mouseleave', () => markBlock(undefined));

  window.addEventListener('mouseup', () => {
    if (numberDrag) {
      const keys = numberDrag.keys;
      numberDrag = undefined;
      const lines = keys.flatMap((k) => folded?.byKey.get(k)?.line ?? []);
      for (const r of diffPane.querySelectorAll<HTMLElement>('.line.hot')) r.classList.remove('hot');
      hotBlock = undefined;
      if (lines.length > 0) runOp(stageOpFor(lines), lines);
      return;
    }
    if (rowDrag) {
      rowDrag = undefined;
      renderDiff();
    }
  });

  /** 帯・行番号・Space で lines を: Staged なら unstage、All ならすべて入っていれば外し、でなければ入れる */
  function stageOpFor(lines: DiffLine[]): StageOp {
    if (diff?.combined) return lines.every((l) => l.staged) ? 'unstage' : 'stage';
    return diffGroup === 'staged' ? 'unstage' : 'stage';
  }

  diffPane.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && picked.size > 0) {
      picked = new Set();
      renderDiff();
    } else if (e.key === ' ' && diff?.combined && !diff.folded && picked.size > 0) {
      // 選んだ行をまとめて入れる・外す（すべて入っていれば外す）
      e.preventDefault();
      const lines = diff.hunks.flatMap((h) => h.lines.filter((l) => l.kind !== ' ' && picked.has(lineKey(l))));
      checkLines(lines, !lines.every((l) => l.staged));
    } else if (e.key === ' ' && diff?.folded && picked.size > 0) {
      // 選んだ行を stage・unstage（All ではすべて入っていれば外す。アプリと同じ）
      e.preventDefault();
      const lines = diff.hunks.flatMap((h) => h.lines.filter((l) => l.kind !== ' ' && picked.has(lineKey(l))));
      runOp(stageOpFor(lines), lines);
    }
  });

  /** J・K（たたんだ差分）: 次・前の変更のかたまりを選び、上から 3 分の 1 あたりに出す（アプリと同じ） */
  function moveBlock(forward: boolean): void {
    if (!diff) return;
    const blocks = changeBlocks(diff.hunks);
    if (blocks.length === 0) return;
    const indexOf = (key: string) => blocks.findIndex((b) => b.some((l) => lineKey(l) === key));
    let target: number;
    const current = [...picked].map(indexOf).filter((i) => i >= 0);
    if (current.length > 0) {
      target = (forward ? Math.max(...current) : Math.min(...current)) + (forward ? 1 : -1);
    } else {
      // 選んでいなければ、見えている一番上から: J はそこ以降の最初、K はそれより前の最後
      const top = diffPane.getBoundingClientRect().top + (diffPane.querySelector<HTMLElement>('.diff-head')?.offsetHeight ?? 0);
      const firstBelow = blocks.findIndex((b) => {
        const r = diffPane.querySelector<HTMLElement>(`.line[data-key="${lineKey(b[0]!)}"]`);
        return r !== null && r.getBoundingClientRect().top >= top;
      });
      target = forward ? (firstBelow < 0 ? blocks.length : firstBelow) : (firstBelow < 0 ? blocks.length : firstBelow) - 1;
    }
    const block = blocks[target];
    if (!block) return;
    picked = new Set(block.map(lineKey));
    anchor = lineKey(block[0]!);
    renderDiff();
    const first = diffPane.querySelector<HTMLElement>(`.line[data-key="${anchor}"]`);
    const last = diffPane.querySelector<HTMLElement>(`.line[data-key="${lineKey(block.at(-1)!)}"]`);
    if (!first || !last) return;
    const pane = diffPane.getBoundingClientRect();
    const headHeight = diffPane.querySelector<HTMLElement>('.diff-head')?.offsetHeight ?? 0;
    if (first.getBoundingClientRect().top < pane.top + headHeight || last.getBoundingClientRect().bottom > pane.bottom) {
      diffPane.scrollBy({ top: first.getBoundingClientRect().top - pane.top - diffPane.clientHeight / 3 });
    }
  }
  diffPane.tabIndex = 0;

  // ---- コミット ----

  function stagedCount(): number {
    return files.filter((f) => f.group === 'staged').length;
  }

  /** コミットしている間（2 回目の ⌘↩ が、もう 1 つ git commit を始めないように） */
  let committing = false;

  function updateCommitButton(): void {
    const staged = stagedCount();
    const reason = commitDisabledReason({ staged, amend: amend.checked, message: message(), committing });
    commitButton.disabled = reason !== undefined;
    // 押せないときは、ツールチップでわけを言う（アプリと同じ）
    commitButton.title = reason ?? 'Commit the checked files (⌘↩)';
    commitButton.textContent = amend.checked
      ? 'Amend Commit'
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

  /** Summary と Description をつないだメッセージ（どちらかだけでもコミットできる。アプリと同じ） */
  function message(): string {
    return composeMessage({ summary: subject.value, body: body.value });
  }

  function commit(push = false): void {
    if (commitButton.disabled) return;
    committing = true;
    updateCommitButton();
    post({ type: 'wcCommit', message: message(), amend: amend.checked, ...(push ? { push } : {}) });
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
  commitButton.addEventListener('click', () => commit());
  for (const input of [subject, body] as HTMLElement[]) {
    input.addEventListener('keydown', (e: KeyboardEvent) => {
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        commit(e.shiftKey);
      }
    });
  }
  // amend にしたら、空ならいまのコミットのメッセージを入れる
  function amendChanged(): void {
    if (amend.checked && !subject.value && !body.value && lastMessage) {
      const [s = '', ...rest] = lastMessage.replace(/\n+$/, '').split('\n');
      subject.value = s;
      body.value = rest.join('\n').replace(/^\n+/, '');
    }
    updateCommitButton();
  }
  amend.addEventListener('change', amendChanged);

  /** コミットの hooks の出力や失敗の理由を、長くても読めるように出す */
  function showCommitOutput(title: string, text: string): void {
    const overlay = el('div', 'overlay');
    const sheet = el('div', 'editor commit-output');
    const close = el('button', undefined, 'Close');
    const row = el('div', 'buttons');
    row.append(close);
    sheet.append(el('div', 'title', title), el('pre', undefined, text), row);
    overlay.append(sheet);
    const dismiss = () => {
      overlay.remove();
      subject.focus();
    };
    close.addEventListener('click', dismiss);
    overlay.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') dismiss();
    });
    root.append(overlay);
    close.focus();
  }

  function onMessage(m: HostMessage): void {
    switch (m.type) {
      case 'wcFiles':
        files = m.files;
        lastMessage = m.lastMessage ?? '';
        layout = m.layout;
        engine = m.engine;
        lineStaging = m.lineStaging;
        if (!lookApplied) applyLook(m);
        // 選び直す（stage して節が変わったものは、その節の行。消えたファイルは選びから外す）
        relist();
        break;
      case 'wcDiff':
        if (!m.diff) {
          // 変更がなくなって外したファイルが、もう別のファイルを選んだ後なら、そのまま
          if (m.gone !== undefined && selected !== undefined && selected.path !== m.gone) break;
          picked = new Set();
          anchor = undefined;
          diff = undefined;
          diffNote = undefined;
          diffError = undefined;
          selected = undefined;
          renderFiles();
          renderDiff();
          break;
        }
        if (diff?.version !== m.diff.version || diff?.path !== m.diff.path) {
          picked = new Set();
          anchor = undefined;
        }
        diffError = undefined;
        diffNote = undefined;
        diff = m.diff;
        diffGroup = m.group;
        whitespace = m.whitespace;
        gapOpen = m.open ?? [];
        if (m.side) side = m.side;
        followShown(m.diff.path, m.group);
        renderFiles();
        renderDiff();
        break;
      case 'wcDiffNote':
        picked = new Set();
        anchor = undefined;
        diff = undefined;
        diffError = undefined;
        diffGroup = m.group;
        diffNote = { path: m.path, group: m.group, note: m.note };
        whitespace = m.whitespace;
        side = m.side;
        followShown(m.path, m.group);
        renderFiles();
        renderDiff();
        break;
      case 'wcDiffFailed':
        picked = new Set();
        anchor = undefined;
        diff = undefined;
        diffNote = undefined;
        diffGroup = m.group;
        diffError = m.error;
        followShown(m.path, m.group);
        renderFiles();
        renderDiff();
        break;
      case 'wcLineStaging':
        lineStaging = m.lineStaging;
        renderDiff();
        break;
      case 'wcDiffAction':
        diffAction(m.action, m);
        break;
      case 'wcSplitStats':
        splitStats = {
          staged: new Map(m.staged.map((s) => [s.path, s])),
          unstaged: new Map(m.unstaged.map((s) => [s.path, s])),
        };
        renderFiles();
        break;
      case 'wcRecent':
        recent = m.messages;
        recentCursor = -1;
        break;
      case 'wcUseMessage':
        recentCursor = -1;
        useMessage(m.message);
        break;
      case 'wcStartAmend':
        amend.checked = true;
        amendChanged();
        break;
      case 'wcStartFixup':
        // fixup! は件名だけ（元のメッセージを残す）、squash! は本文に書き足せる
        amend.checked = false;
        replaceField(subject, `${m.kind}! ${m.subject}`);
        if (m.kind === 'fixup') replaceField(body, '');
        (m.kind === 'fixup' ? subject : body).focus();
        updateCommitButton();
        break;
      case 'wcCommitOutput':
        showCommitOutput(m.title, m.text);
        break;
      case 'wcStats':
        stats = new Map(m.stats.map((s) => [s.path, s]));
        renderFiles();
        break;
      case 'wcSparse':
        sparseNote.hidden = !m.sparse;
        if (m.sparse) {
          sparseNote.textContent = m.sparse.cone
            ? `Only some folders are checked out (${m.sparse.folders.length})`
            : 'Only some files are checked out';
          sparseNote.title = `${m.sparse.folders.join('\n')}\nClick to change them in Repository Settings`;
        }
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
      case 'wcCommitting':
        committing = m.committing;
        updateCommitButton();
        break;
      case 'wcCommitNow':
        // 押せないときは、ボタンのツールチップと同じわけを返す（拡張が知らせる）
        if (commitButton.disabled) post({ type: 'wcCommitRefused', reason: commitButton.title });
        else commit(m.push);
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

  /** 覚えている見た目（最初に受け取ったときだけ。後は Webview で変えたものが正しい） */
  function applyLook(look: WorkingCopyLook): void {
    lookApplied = true;
    sectionsWanted = look.sections;
    commitCollapsed = look.commitCollapsed;
    commitHeight = look.commitHeight;
    applyCollapsed();
  }

  applyCollapsed();
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
