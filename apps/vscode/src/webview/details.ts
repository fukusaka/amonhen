/**
 * コミットの詳細の欄（History と Reflog の右側）。メッセージ・作者・親と、変更したファイルの一覧、ファイルごとの
 * 差分を出す。差分は画面に入ったものから順に拡張ホストに求める。アプリと同じく、メッセージの欄と、ファイルと差分の
 * 欄を上下に分け、間の仕切りをドラッグできる。メッセージの欄は中身に合わせ（列の半分まで）、⤢ で列いっぱいに広げる
 */
import type { FileDiff } from '../diff/fileDiff.ts';
import type { CommitDetails, DetailsExtras, DetailsLayout, PairMode, Person, SelectionView } from '../history/protocol.ts';
import { el, formatDate, getState, post, updateState } from './api.ts';
import { type FileDir, makeFileTree } from '../workingCopy/fileList.ts';
import { COLLAPSE_ABOVE, isGenerated, type LineCounts, lineCounts } from '../history/stackedFiles.ts';
import { renderDiffBody } from './diffView.ts';
import { signatureMark } from './signature.ts';

/** mailmap で置き換えた人。記録と違えば、記録されたものを下に小さく添える */
function withRecorded(text: string, recorded: Person | undefined): Node | string {
  if (!recorded) {
    return text;
  }
  const span = el('span', '', text);
  const note = el('span', 'recorded', `Recorded: ${recorded.name} <${recorded.email}>`);
  note.title = 'The name and email recorded in the commit. The mailmap shows them as above.';
  span.append(note);
  return span;
}

/** ファイルの行と差分の見出しの右クリックメニュー（package.json の webview/context）に渡す情報 */
function fileContext(oid: string, path: string): string {
  return JSON.stringify({ webviewSection: 'commitFile', oid, path, preventDefaultContextMenuItems: true });
}

export interface DetailsPane {
  readonly root: HTMLElement;
  /** 出しているコミット */
  readonly shownOid: string | undefined;
  /** `focus` があれば、そのファイルを選んでその差分の位置に移る（File History）。`viewed` は Viewed にしたファイル */
  show(details: CommitDetails, focus?: string, viewed?: readonly string[]): void;
  showDiff(oid: string, path: string, diff: FileDiff): void;
  /** いくつも選んだコミット。読んでいる間は `view` なし */
  showSelection(state: SelectionState): void;
  /** 詳細の残り（署名・子のコミット）。出しているコミットのものなら足す */
  showExtras(extras: DetailsExtras): void;
  clear(): void;
  /** 次に届いた詳細を、同じコミットでも出し直す */
  forget(): void;
}

/** onParent があれば、親の ID を押すとそれを呼ぶ（History でその行に移る） */
/** いくつも選んだコミットの出し方 */
export interface SelectionState {
  count: number;
  view?: SelectionView;
  error?: string;
  onPair?: (pair: PairMode) => void;
  onCommit?: (row: number) => void;
}

const PAIR_NOTES: Record<PairMode, string> = {
  forward: 'Two non-consecutive commits, so this is the diff from the older to the newer (A..B).',
  reverse: 'The diff of the two non-consecutive commits from the newer to the older (reversed).',
  mergeBase: 'The diff of the two non-consecutive commits from their common ancestor to the newer (A...B: only the changes on the newer side).',
};
const PAIR_NAMES: Record<PairMode, string> = { forward: 'A..B', reverse: 'Reverse', mergeBase: 'From the merge-base (A...B)' };

/** 変わったファイルの見せ方。拡張ホストが覚え、すべての詳細の欄で同じにする */
let layout: DetailsLayout = { tree: false, single: false };
const panes = new Set<(layout: DetailsLayout) => void>();

export function setDetailsLayout(next: DetailsLayout): void {
  layout = next;
  for (const apply of panes) apply(layout);
}

/** 一覧の上に置く、見せ方の切り替え */
function layoutButtons(): HTMLElement {
  const span = el('span', 'layout-buttons');
  const tree = el('button', 'secondary', layout.tree ? 'List' : 'Tree');
  tree.title = layout.tree ? 'Show the changed files as a list above the diffs' : 'Show the changed files as a tree beside the diffs';
  tree.addEventListener('click', () => post({ type: 'detailsLayout', layout: { ...layout, tree: !layout.tree } }));
  const single = el('button', 'secondary', layout.single ? 'All Files' : 'One File');
  single.title = layout.single ? 'Show the diffs of all files one after another' : 'Show only the diff of the selected file';
  single.addEventListener('click', () => post({ type: 'detailsLayout', layout: { ...layout, single: !layout.single } }));
  span.append(tree, single);
  return span;
}

/** 詳細に並べた 1 ファイル（一覧の行と差分の場所） */
interface FileSection {
  row: HTMLElement;
  section: HTMLElement;
  head: HTMLElement;
  /** 差分を求めたか */
  requested: boolean;
  /** 開く・畳むを自分で選んだか（選んでいれば、大きな差分でも畳まない） */
  chosen: boolean;
  viewed: boolean;
}

/** 足した行と消した行（どちらも 0 なら空） */
function countsLabel(counts: LineCounts): HTMLElement {
  const span = el('span', 'stat');
  if (counts.added) span.append(el('span', 'added', `+${counts.added}`));
  if (counts.removed) span.append(el('span', 'removed', `−${counts.removed}`));
  return span;
}

export function createDetailsPane(onParent?: (oid: string) => void): DetailsPane {
  const root = el('aside', 'details');
  /** メッセージ（件名・作者など・本文）の欄。コミットを 1 つ出すときだけ */
  const head = el('div', 'details-head');
  const divider = el('div', 'details-divider');
  /** ファイルと差分の欄（コミットを 1 つ出すとき以外は、これだけ） */
  const scroller = el('div', 'details-body');
  root.append(head, divider, scroller);
  attachRowSplitter(root, divider, head);
  let shownOid: string | undefined;
  /** 詳細に並べたファイル */
  let sections = new Map<string, FileSection>();
  let diffObserver: IntersectionObserver | undefined;
  /** 出している詳細の作者などの並び（後から署名・子のコミットを足す） */
  let meta: HTMLElement | undefined;
  /** 出しているファイルの一覧（見せ方を変えたら並べ直す） */
  let listed: { files: HTMLElement; diffs: HTMLElement; items: readonly { path: string }[] } | undefined;
  /** 選んでいるファイル（`single` のときに差分を出すもの） */
  let current: string | undefined;

  /** コミットの ID の並び（onParent があれば押すとその行に移る） */
  function commitLinks(oids: readonly string[]): HTMLElement {
    const span = el('span');
    for (const p of oids) {
      if (onParent) {
        const link = el('a', 'parent', p.slice(0, 8));
        link.href = '#';
        link.title = p;
        link.addEventListener('click', (e) => {
          e.preventDefault();
          onParent(p);
        });
        span.append(link, ' ');
      } else {
        const code = el('code', 'parent', p.slice(0, 8));
        code.title = p;
        span.append(code, ' ');
      }
    }
    return span;
  }

  function show(d: CommitDetails, focus?: string, viewed: readonly string[] = []): void {
    shownOid = d.oid;
    const [subject = '', ...rest] = d.message.replace(/\n+$/, '').split('\n');
    const body = rest.join('\n').replace(/^\n+/, '');

    const title = el('div', 'subject');
    for (const ref of d.refs) {
      title.append(el('span', `ref ${ref.kind}${ref.current ? ' current' : ''}`, ref.name));
    }
    title.append(el('span', '', subject));

    const list = el('dl', 'meta');
    meta = list;
    const add = (label: string, value: Node | string) => {
      const dd = el('dd');
      dd.append(value);
      list.append(el('dt', '', label), dd);
    };
    add('Author', withRecorded(`${d.author.name} <${d.author.email}>`, d.recordedAuthor));
    add('Date', formatDate(d.author.date));
    if (d.committer.name !== d.author.name || d.committer.date !== d.author.date) {
      add('Committer', withRecorded(`${d.committer.name}  ${formatDate(d.committer.date)}`, d.recordedCommitter));
    }
    add('Commit', el('code', '', d.oid));
    if (d.parents.length > 0) {
      add(d.parents.length > 1 ? 'Parents' : 'Parent', commitLinks(d.parents));
    }

    const files = el('div', 'files');
    const filesHead = el('div', 'files-head');
    const count = d.files.length;
    filesHead.append(
      el(
        'span',
        '',
        `${count} file${count === 1 ? '' : 's'} changed${d.parents.length > 1 ? ' (against the first parent)' : ''}`,
      ),
    );
    if (count > 0) {
      const all = el('button', 'secondary', 'Open All');
      all.addEventListener('click', () => post({ type: 'open', oid: d.oid }));
      filesHead.append(layoutButtons(), all);
    }
    const diffs = fileSections(d.oid, d.oid, d.files, files, true, viewed);

    const parts: Node[] = [expandButton(), title, list];
    if (body) parts.push(el('pre', 'body', body));
    place(parts, [stack(filesHead, files, diffs)]);
    if (focus && sections.has(focus)) select(focus, true);
  }

  /**
   * 欄に並べる。`message` があれば上のメッセージの欄に（高さは中身に合わせ直し、広げていれば戻す）、なければ
   * 1 つの欄だけにする
   */
  function place(message: Node[] | undefined, rest: Node[]): void {
    root.classList.remove('expanded');
    root.classList.toggle('split', message !== undefined);
    head.style.height = '';
    head.replaceChildren(...(message ?? []));
    scroller.replaceChildren(...rest);
    head.scrollTop = 0;
    scroller.scrollTop = 0;
  }

  /** メッセージの欄を、列いっぱいに広げる・戻すボタン */
  function expandButton(): HTMLElement {
    const button = el('button', 'expand-details', '⤢');
    const label = () => {
      const expanded = root.classList.contains('expanded');
      button.title = expanded ? 'Restore the details' : 'Expand the commit details to fill the right column (click again to restore)';
      button.classList.toggle('on', expanded);
    };
    label();
    button.addEventListener('click', () => {
      root.classList.toggle('expanded');
      label();
    });
    return button;
  }

  /** 一覧の見出し・一覧・差分を並べる枠（木のときは一覧と差分を横に並べる） */
  function stack(head: HTMLElement, files: HTMLElement, diffs: HTMLElement): HTMLElement {
    const box = el('div', 'stack');
    box.append(head, files, diffs);
    return box;
  }

  /** ファイルを選ぶ。`single` ならその差分だけを出し、そうでなければ `scroll` でその差分に移る */
  function select(path: string, scroll: boolean): void {
    const file = sections.get(path);
    if (!file) return;
    current = path;
    for (const [p, f] of sections) {
      f.row.classList.toggle('active', p === path);
      f.section.classList.toggle('current', p === path);
    }
    if (layout.single) {
      request(file);
      if (scroll) file.section.scrollIntoView({ block: 'nearest' });
    } else if (scroll) {
      file.section.scrollIntoView({ block: 'start' });
    }
  }

  /** 一覧の行を、今の見せ方（木・パスの並び）で並べ直す */
  function arrange(): void {
    if (!listed) return;
    const { files, diffs, items } = listed;
    const rows: Node[] = [];
    /** 差分も一覧と同じ順に並べる */
    const order: HTMLElement[] = [];
    if (layout.tree) {
      const walk = (dir: FileDir<{ path: string }>, depth: number, into: Node[]) => {
        for (const sub of dir.dirs) {
          const row = el('div', 'file dir-row');
          row.style.paddingLeft = `${4 + depth * 12}px`;
          row.append(el('span', 'chevron', '▾'), el('span', 'name', sub.name));
          const children = el('div', 'dir-children');
          row.addEventListener('click', () => {
            children.hidden = !children.hidden;
            row.querySelector('.chevron')!.textContent = children.hidden ? '▸' : '▾';
          });
          const inner: Node[] = [];
          walk(sub, depth + 1, inner);
          children.append(...inner);
          into.push(row, children);
        }
        for (const f of dir.files) {
          const file = sections.get(f.path)!;
          file.row.style.paddingLeft = `${4 + depth * 12}px`;
          file.row.querySelector('.dir')!.textContent = '';
          into.push(file.row);
          order.push(file.section);
        }
      };
      walk(makeFileTree(items), 0, rows);
    } else {
      for (const f of items) {
        const file = sections.get(f.path)!;
        const slash = f.path.lastIndexOf('/');
        file.row.style.paddingLeft = '';
        file.row.querySelector('.dir')!.textContent = slash >= 0 ? f.path.slice(0, slash) : '';
        rows.push(file.row);
        order.push(file.section);
      }
    }
    files.replaceChildren(...rows);
    diffs.replaceChildren(...order);
  }

  /** 見せ方を変えた */
  function applyLayout(): void {
    root.classList.toggle('tree', layout.tree);
    root.classList.toggle('single', layout.single);
    for (const old of root.querySelectorAll('.layout-buttons')) old.replaceWith(layoutButtons());
    arrange();
    if (layout.single && sections.size > 0) {
      select(current && sections.has(current) ? current : sections.keys().next().value!, false);
    }
  }
  panes.add(applyLayout);
  applyLayout();

  /** 差分の欄をスクロールしたら、いちばん上に見えているファイルを一覧で選ぶ */
  let following = false;
  scroller.addEventListener('scroll', () => {
    if (layout.single || following || sections.size === 0) return;
    following = true;
    requestAnimationFrame(() => {
      following = false;
      const top = scroller.getBoundingClientRect().top;
      let shown: string | undefined;
      for (const [path, f] of sections) {
        if (f.section.getBoundingClientRect().top > top + 1) break;
        shown = path;
      }
      if (shown && shown !== current) {
        current = shown;
        for (const [p, f] of sections) f.row.classList.toggle('active', p === shown);
      }
    });
  });

  /**
   * ファイルの一覧と差分の枠。差分は画面に入ったものから順に求める（畳んだものは開くまで求めない）。`single` は
   * 1 つのコミット（メニューと Open がある）。`viewedKey` があれば、Viewed をそこに覚える
   */
  function fileSections(
    oid: string,
    viewedKey: string | undefined,
    items: readonly { path: string; origPath?: string; status?: string; added?: number; removed?: number }[],
    files: HTMLElement,
    single: boolean,
    viewed: readonly string[],
  ): HTMLElement {
    diffObserver?.disconnect();
    diffObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const file = sections.get((entry.target as HTMLElement).dataset.path!);
          if (entry.isIntersecting && file) request(file);
        }
      },
      { root: scroller, rootMargin: '200px' },
    );
    sections = new Map();
    current = undefined;
    requestDiff = (path) => post({ type: 'commitDiff', oid, path });
    const viewedSet = new Set(viewed);
    const diffs = el('div', 'commit-diffs');
    for (const f of items) {
      const row = el('div', 'file');
      // 右クリックのメニュー（package.json の webview/context）に渡す情報
      if (single) row.dataset.vscodeContext = fileContext(oid, f.path);
      row.title = f.origPath ? `${f.origPath} → ${f.path}` : f.path;
      if (f.status) row.append(el('span', `status s-${f.status}`, f.status));
      const slash = f.path.lastIndexOf('/');
      row.append(el('span', 'name', f.path.slice(slash + 1)));
      row.append(el('span', 'dir', slash >= 0 ? f.path.slice(0, slash) : ''));
      row.append(el('span', 'viewed-mark', '✓'));

      const section = el('section', 'file-diff');
      section.dataset.path = f.path;
      const head = el('div', 'diff-head');
      head.title = 'Click to collapse or expand';
      head.append(el('span', 'chevron'));
      if (f.status) head.append(el('span', `status s-${f.status}`, f.status));
      head.append(el('span', 'path', f.origPath ? `${f.origPath} → ${f.path}` : f.path));
      const box = el('input');
      box.type = 'checkbox';
      const check = el('label', 'viewed');
      check.title = 'Mark as viewed and collapse';
      check.append(box, ' Viewed');
      head.append(check);
      if (single) {
        head.dataset.vscodeContext = fileContext(oid, f.path);
        const open = el('button', 'secondary', 'Open');
        open.title = 'Open the diff in the editor';
        open.addEventListener('click', () => post({ type: 'openFile', oid, path: f.path }));
        head.append(open);
      }
      section.append(head, el('div', 'empty', 'Loading…'));
      diffs.append(section);

      const file: FileSection = { row, section, head, requested: false, chosen: false, viewed: viewedSet.has(f.path) };
      sections.set(f.path, file);
      if (f.added !== undefined && f.removed !== undefined) setCounts(file, { added: f.added, removed: f.removed });
      box.checked = file.viewed;
      setViewed(file, file.viewed);
      // 最初から畳む: Viewed にしたもの、生成物、変わった行が多いもの
      const large = (f.added ?? 0) + (f.removed ?? 0) > COLLAPSE_ABOVE;
      setCollapsed(file, file.viewed || isGenerated(f.path) || large);

      // その差分の位置へ移る（single ならその差分だけを出す）
      row.addEventListener('click', () => select(f.path, !layout.single));
      head.addEventListener('click', (e) => {
        if ((e.target as Element).closest('button, label')) return;
        file.chosen = true;
        setCollapsed(file, !section.classList.contains('collapsed'));
        if (!section.classList.contains('collapsed')) request(file);
        // 畳んで見出しが上に隠れたら、見出しが見えるところまで戻す
        if (head.getBoundingClientRect().top < scroller.getBoundingClientRect().top) section.scrollIntoView({ block: 'start' });
      });
      box.addEventListener('change', () => {
        // Viewed にしたら畳み、戻したら開く（GitHub と同じ）
        file.chosen = true;
        setViewed(file, box.checked);
        setCollapsed(file, box.checked);
        if (!box.checked) request(file);
        if (viewedKey) post({ type: 'viewed', key: viewedKey, path: f.path, viewed: box.checked });
        if (box.checked && head.getBoundingClientRect().top < scroller.getBoundingClientRect().top) section.scrollIntoView({ block: 'start' });
      });
      diffObserver.observe(section);
    }
    listed = { files, diffs, items };
    applyLayout();
    return diffs;
  }

  /** 出しているコミットの、ファイルの差分を求める */
  let requestDiff: (path: string) => void = () => undefined;

  /** 見えたファイルの差分を求める（畳んでいれば開くまで待つ。single なら選んだものだけ） */
  function request(file: FileSection): void {
    if (file.requested || file.section.classList.contains('collapsed')) return;
    const path = file.section.dataset.path!;
    if (layout.single && path !== current) return;
    file.requested = true;
    requestDiff(path);
  }

  function setCollapsed(file: FileSection, collapsed: boolean): void {
    file.section.classList.toggle('collapsed', collapsed);
    file.head.querySelector('.chevron')!.textContent = collapsed ? '▸' : '▾';
  }

  function setViewed(file: FileSection, viewed: boolean): void {
    file.viewed = viewed;
    file.row.classList.toggle('viewed', viewed);
  }

  /** 行数を一覧の行と見出しに出す */
  function setCounts(file: FileSection, counts: LineCounts): void {
    file.row.querySelector('.stat')?.remove();
    file.head.querySelector('.stat')?.remove();
    file.row.querySelector('.viewed-mark')!.before(countsLabel(counts));
    file.head.querySelector('.viewed')!.before(countsLabel(counts));
  }

  function showSelection({ count, view, error, onPair, onCommit }: SelectionState): void {
    shownOid = view?.key;
    meta = undefined;
    listed = undefined;
    diffObserver?.disconnect();
    sections = new Map();
    const parts: Node[] = [el('div', 'subject', `${count} commits selected`)];
    if (!view) {
      parts.push(el('div', error ? 'selection-note error' : 'selection-note', error ?? 'Loading the diff…'));
      place(undefined, parts);
      return;
    }
    const note =
      view.kind === 'range'
        ? 'They are consecutive, so this is the diff from the parent of the oldest to the newest (the same as squashing them into one).'
        : view.kind === 'pair'
          ? PAIR_NOTES[view.pair]
          : 'They are not contiguous, so each commit’s diff is listed file by file (headed by the path and short ID).';
    parts.push(el('div', 'selection-note', note));
    if (view.kind === 'pair') {
      const modes = el('div', 'selection-pair');
      for (const mode of ['forward', 'reverse', 'mergeBase'] as const) {
        if (mode === view.pair) continue;
        const link = el('button', 'secondary', PAIR_NAMES[mode]);
        link.addEventListener('click', () => onPair?.(mode));
        modes.append(link);
      }
      parts.push(modes);
    }
    const commits = el('div', 'selection-commits');
    for (const c of view.commits) {
      const row = el('div', 'selection-commit');
      row.title = 'Show this commit alone';
      row.append(el('code', '', c.oid.slice(0, 8)), el('span', 'subject-text', c.subject), el('span', 'author', c.author));
      row.addEventListener('click', () => onCommit?.(c.row));
      commits.append(row);
    }
    parts.push(commits);
    if (view.count > view.commits.length) {
      parts.push(el('div', 'selection-note', `…and ${view.count - view.commits.length} more`));
    }
    if (view.files.length === 0) {
      parts.push(el('div', 'empty', 'No changes'));
    } else {
      const files = el('div', 'files');
      const n = view.files.length;
      const filesHead = el('div', 'files-head');
      filesHead.append(el('span', '', `${n} file${n === 1 ? '' : 's'} changed`), layoutButtons());
      const diffs = fileSections(view.key, view.viewedKey, view.files, files, false, view.viewed);
      parts.push(stack(filesHead, files, diffs));
    }
    place(undefined, parts);
  }

  function showDiff(oid: string, path: string, diff: FileDiff): void {
    const file = sections.get(path);
    if (oid !== shownOid || !file) {
      return;
    }
    file.section.replaceChildren(file.head, ...renderDiffBody(diff));
    const counts = lineCounts(diff);
    setCounts(file, counts);
    // 行数は差分を読んで初めて分かることがある。大きければ畳む（自分で開いたものは畳まない）
    if (!file.chosen && counts.added + counts.removed > COLLAPSE_ABOVE) setCollapsed(file, true);
  }

  function showExtras(extras: DetailsExtras): void {
    if (extras.oid !== shownOid || !meta || meta.querySelector('.extras')) return;
    const add = (label: string, value: Node) => {
      const dd = el('dd', 'extras');
      dd.append(value);
      meta!.append(el('dt', 'extras', label), dd);
    };
    const s = extras.signature;
    if (s) {
      // アプリの署名の吹き出しの中身（言葉・説明・署名した人と鍵）
      const value = el('span', 'signature');
      value.append(signatureMark(s), ` ${s.label}`);
      const note = el('span', 'recorded', [s.explanation, s.signer && `Signed by ${s.signer}`, s.key && `Key ${s.key}`].filter(Boolean).join(' · '));
      value.append(note);
      add('Signature', value);
    }
    if (extras.children.length > 0) add(extras.children.length > 1 ? 'Children' : 'Child', commitLinks(extras.children));
  }

  function clear(): void {
    shownOid = undefined;
    meta = undefined;
    listed = undefined;
    sections = new Map();
    place(undefined, [el('div', 'empty', 'Select a commit to see its details here')]);
  }

  clear();
  return {
    root,
    get shownOid() {
      return shownOid;
    },
    show,
    showDiff,
    showExtras,
    clear,
    showSelection,
    forget() {
      shownOid = undefined;
    },
  };
}

/**
 * メッセージの欄と、ファイルと差分の欄の仕切りをドラッグして、メッセージの欄の高さを変える（次のコミットを出すと
 * 中身に合わせ直す。アプリと同じ）
 */
function attachRowSplitter(root: HTMLElement, divider: HTMLElement, head: HTMLElement): void {
  divider.addEventListener('mousedown', (e) => {
    e.preventDefault();
    const top = head.getBoundingClientRect().top;
    const onMove = (m: MouseEvent) => {
      const max = Math.max(48, root.clientHeight - 80);
      head.style.height = `${Math.min(max, Math.max(32, m.clientY - top))}px`;
    };
    const onUp = () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
      document.body.classList.remove('resizing-rows');
    };
    document.body.classList.add('resizing-rows');
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  });
}

/** 左右の境目をドラッグして、詳細の幅を変える。幅は覚えておき、どの画面でも同じにする */
export function attachSplitter(body: HTMLElement, splitter: HTMLElement, pane: HTMLElement, onDone: () => void): void {
  function setWidth(width: number): void {
    const max = Math.max(240, body.clientWidth - 240);
    const clamped = Math.min(max, Math.max(240, width));
    pane.style.width = `${clamped}px`;
    updateState({ detailsWidth: clamped });
  }
  splitter.addEventListener('mousedown', (e) => {
    e.preventDefault();
    const onMove = (m: MouseEvent) => setWidth(body.getBoundingClientRect().right - m.clientX);
    const onUp = () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
      document.body.classList.remove('resizing');
      onDone();
    };
    document.body.classList.add('resizing');
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  });
  const saved = getState().detailsWidth;
  if (saved) pane.style.width = `${saved}px`;
}
