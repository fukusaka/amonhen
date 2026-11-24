/**
 * コミットの詳細の欄（History と Reflog の右側）。上にメッセージ・作者・親、下にファイルごとの差分を出す（アプリと
 * 同じく、差分の見出しが変わったファイルの一覧を兼ね、最初はすべて畳む）。差分は開いて画面に入ったものから順に
 * 拡張ホストに求める。間の仕切りはドラッグでき、メッセージの欄は中身に合わせ（列の半分まで）、⤢ で列いっぱいに広げる
 */
import type { FileDiff } from '../diff/fileDiff.ts';
import type {
  CommitDetails,
  DetailsExtras,
  DetailsLayout,
  DiffChoices,
  FileView,
  ImageBlob,
  PairMode,
  Person,
  SelectionView,
  TreeEntry,
} from '../history/protocol.ts';
import { el, formatDate, getState, post, updateState } from './api.ts';
import { type FileDir, makeFileTree } from '../workingCopy/fileList.ts';
import { COLLAPSE_ABOVE, isGenerated, type LineCounts, lineCounts, renamedPath } from '../history/stackedFiles.ts';
import { renderDiffBody, whitespaceSelect, wrapButton } from './diffView.ts';
import { isImagePath, renderImageDiff } from './imageDiff.ts';
import { type AllFiles, createAllFiles } from './allFiles.ts';
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

/**
 * ファイルの差分の見出しの右クリックメニュー（package.json の webview/context）に渡す情報。`wide` なら Show Whole
 * File も出す。このコミットで消したファイルには「この版に戻す」を出さない（この版にはない）。名前を変えたファイルは、
 * 前の版に戻すと元の名前に戻す（origPath）
 */
function fileContext(oid: string, path: string, wide: boolean, status?: string, origPath?: string): string {
  return JSON.stringify({
    webviewSection: 'commitFile',
    oid,
    path,
    ...(origPath ? { origPath } : {}),
    commitFileWide: wide,
    commitFileDeleted: status === 'D',
    preventDefaultContextMenuItems: true,
  });
}

export interface DetailsPane {
  readonly root: HTMLElement;
  /** 出しているコミット */
  readonly shownOid: string | undefined;
  /**
   * `focus` があれば、そのファイルの差分を開いてその位置に移る（File History）。`choices` があれば、無視する空白と
   * 前後の行を選べる（エンジンだけ）
   */
  show(details: CommitDetails, focus?: string, choices?: DiffChoices): void;
  showDiff(oid: string, path: string, diff: FileDiff): void;
  /** 「すべてのファイル」のフォルダの中身とファイルの中身 */
  showTreeEntries(oid: string, path: string, entries: TreeEntry[], error?: string): void;
  showTreeFile(oid: string, path: string, view: FileView): void;
  /** 画像の前と後（画像の差分） */
  showImage(oid: string, path: string, before?: ImageBlob, after?: ImageBlob, error?: string): void;
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
  choices?: DiffChoices;
}

const PAIR_NOTES: Record<PairMode, string> = {
  forward: 'Two non-consecutive commits, so this is the diff from the older to the newer (A..B).',
  reverse: 'The diff of the two non-consecutive commits from the newer to the older (reversed).',
  mergeBase: 'The diff of the two non-consecutive commits from their common ancestor to the newer (A...B: only the changes on the newer side).',
};
const PAIR_NAMES: Record<PairMode, string> = { forward: 'A..B', reverse: 'Reverse', mergeBase: 'From the merge-base (A...B)' };

/** 変わったファイルの見せ方（リストか木か）。拡張ホストが覚え、すべての詳細の欄で同じにする */
let layout: DetailsLayout = { tree: false };
const panes = new Set<(layout: DetailsLayout) => void>();

export function setDetailsLayout(next: DetailsLayout): void {
  layout = next;
  for (const apply of panes) apply(layout);
}

/** 右クリックの Show Whole File: そのコミットを出している欄が、ファイル全体を頼む */
const wholeFiles = new Set<(oid: string, path: string) => void>();

export function showWholeFile(oid: string, path: string): void {
  for (const show of wholeFiles) show(oid, path);
}


/** 詳細に並べた 1 ファイル（差分の見出しが、変わったファイルの一覧を兼ねる） */
interface FileSection {
  section: HTMLElement;
  head: HTMLElement;
  path: string;
  origPath?: string;
  /** 差分を求めたか */
  requested: boolean;
  /** 開く・畳むを自分で選んだか（選んでいれば、大きな差分でも畳まない） */
  chosen: boolean;
  /** 生成物か、変わった行が多い（すべて開くでも畳んだまま） */
  heavy: boolean;
  /** 画像の比べる欄（求めたら置く） */
  image?: HTMLElement;
  counts?: LineCounts;
}

/** 足した行と消した行（どちらも 0 なら空） */
function countsLabel(counts: LineCounts): HTMLElement {
  const span = el('span', 'stat');
  if (counts.added) span.append(el('span', 'added', `+${counts.added}`));
  if (counts.removed) span.append(el('span', 'removed', `−${counts.removed}`));
  return span;
}

/** `images` なら、画像の前と後を拡張ホストに求めて比べる（History だけ） */
export function createDetailsPane(onParent?: (oid: string) => void, images = false): DetailsPane {
  const root = el('aside', 'details');
  /** メッセージ（件名・作者など・本文）の欄。いくつも選んだときは、その説明 */
  const head = el('div', 'details-head');
  const divider = el('div', 'details-divider');
  /** 差分の上の帯（数・リストと木・すべて開く・畳む） */
  const bar = el('div', 'files-bar');
  /** ファイルと差分の欄 */
  const scroller = el('div', 'details-body');
  root.append(head, divider, bar, scroller);
  attachRowSplitter(root, divider, head);
  let shownOid: string | undefined;
  /** 詳細に並べたファイル（並べた順） */
  let sections = new Map<string, FileSection>();
  let diffObserver: IntersectionObserver | undefined;
  /** 出している詳細の作者などの並び（後から署名・子のコミットを足す） */
  let meta: HTMLElement | undefined;
  /** 差分を並べる枠と、その中身（見せ方を変えたら並べ直す） */
  let diffs: HTMLElement | undefined;
  /** 木で畳んだフォルダ（パス。見せ方を変えても、同じコミットのうちは保つ） */
  let foldedDirs = new Set<string>();
  /** 1 つのコミットなら、その ID（帯の Open All） */
  let openAll: string | undefined;
  /** 画像を比べるか（1 つのコミットを出しているとき） */
  let imagesFor = false;
  /** 変更（差分を並べる）か、すべてのファイル（その時点の木と中身）か。コミットを替えても保つ */
  let page: 'changes' | 'all' = 'changes';
  /** 出している「すべてのファイル」（1 つのコミットで、エンジンがあるときだけ作る） */
  let allFiles: AllFiles | undefined;
  /** 出しているコミットの変わったファイル（「すべてのファイル」の木の印） */
  let changedFiles: { path: string; status?: string }[] = [];
  /** 無視する空白と前後の行を選べるか（エンジンだけ）。今の選び方 */
  let choices: DiffChoices | undefined;
  /** ファイルごとの前後の行（3 から広げたもの。all はファイル全体） */
  let contexts = new Map<string, number | 'all'>();
  wholeFiles.add((oid, path) => {
    const file = sections.get(path);
    if (oid !== shownOid || !file || !choices) return;
    widen(file, 'all');
  });

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

  function show(d: CommitDetails, focus?: string, next?: DiffChoices): void {
    // 同じコミットを出し直す（空白を選び直した）なら、開いていたファイルは開いたまま
    const reopen = d.oid === shownOid ? openPaths() : [];
    shownOid = d.oid;
    choices = next;
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

    const parts: Node[] = [expandButton(), title, list];
    if (body) parts.push(el('pre', 'body', body));
    if (d.parents.length > 1) parts.push(el('div', 'selection-note', 'The changes are shown against the first parent.'));
    place(parts);
    fileSections(d.oid, d.files, true, d.oid);
    reopenFiles(reopen);
    if (focus) {
      const file = sections.get(focus);
      if (file) {
        file.chosen = true;
        setCollapsed(file, false);
        request(file);
        file.section.scrollIntoView({ block: 'start' });
      }
    }
  }

  /** 上の欄に並べる（高さは中身に合わせ直し、広げていれば戻す） */
  function place(message: Node[]): void {
    root.classList.remove('expanded');
    root.classList.toggle('split', message.length > 0);
    const kept = getState().detailsHeadHeight;
    head.style.height = kept ? `${kept}px` : '';
    head.replaceChildren(...message);
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

  /** 帯のボタン（`on` なら押した見た目） */
  function barButton(text: string, tip: string, on: boolean, click: () => void): HTMLElement {
    const button = el('button', `bar-button${on ? ' on' : ''}`, text);
    button.title = tip;
    button.addEventListener('click', click);
    return button;
  }

  /** 差分の上の帯を作り直す（数と行数の合計、リストと木、フォルダ・差分をすべて開く・畳む、Open All） */
  function renderBar(): void {
    const files = [...sections.values()];
    if (files.length === 0) {
      bar.replaceChildren();
      bar.hidden = true;
      return;
    }
    bar.hidden = false;
    const canAll = imagesFor && choices !== undefined;
    const pages = el('span', 'segmented');
    if (canAll) {
      pages.append(
        barButton('Changes', 'The changes in this commit', page === 'changes', () => setPage('changes')),
        barButton('All Files', 'All files at this commit, with the changes marked', page === 'all', () => setPage('all')),
      );
    }
    if (canAll && page === 'all') {
      const spacer = el('span', 'spacer');
      bar.replaceChildren(pages, spacer, wrapButton());
      return;
    }
    const total = el('span', 'files-total', `${files.length} file${files.length === 1 ? '' : 's'}`);
    if (files.every((f) => f.counts)) {
      total.append(' ', countsLabel(files.reduce((sum, f) => ({ added: sum.added + f.counts!.added, removed: sum.removed + f.counts!.removed }), { added: 0, removed: 0 })));
    }
    const spacer = el('span', 'spacer');
    const switcher = el('span', 'segmented');
    switcher.append(
      barButton('List', 'Show the changed files as a list of paths', !layout.tree, () => post({ type: 'detailsLayout', layout: { tree: false } })),
      barButton('Tree', 'Show the changed files in their folders', layout.tree, () => post({ type: 'detailsLayout', layout: { tree: true } })),
    );
    const items: Node[] = canAll ? [pages, total, spacer, switcher] : [total, spacer, switcher];
    if (choices) {
      const select = whitespaceSelect(choices.whitespace, (whitespace) => post({ type: 'whitespace', whitespace }));
      items.push(select);
    }
    items.push(wrapButton());
    if (layout.tree && diffs?.querySelector('.dir-head')) {
      const anyFolded = foldedDirs.size > 0;
      items.push(
        barButton(anyFolded ? 'Expand Folders' : 'Collapse Folders', anyFolded ? 'Expand all folders' : 'Collapse all folders', false, () => {
          if (anyFolded) foldedDirs.clear();
          else for (const d of diffs?.querySelectorAll<HTMLElement>('.dir-head') ?? []) foldedDirs.add(d.dataset.dir!);
          applyFolds();
          renderBar();
        }),
      );
    }
    const closed = files.filter((f) => !f.heavy && f.section.classList.contains('collapsed'));
    const expand = closed.length > 0;
    items.push(
      barButton(
        expand ? 'Expand All' : 'Collapse All',
        expand ? 'Expand every diff (generated files and very large changes stay folded)' : 'Collapse every diff',
        false,
        () => {
          for (const f of files) {
            if (expand && f.heavy) continue;
            f.chosen = true;
            setCollapsed(f, !expand);
          }
          if (expand) foldedDirs.clear();
          applyFolds();
          for (const f of files) request(f);
          renderBar();
        },
      ),
    );
    if (openAll) {
      const oid = openAll;
      const all = barButton('Open All', 'Open every changed file in the multi-file diff editor', false, () => post({ type: 'open', oid }));
      items.push(all);
    }
    bar.replaceChildren(...items);
  }

  /** 差分を、今の見せ方（パスの並びか木）で並べ直す */
  function arrange(): void {
    if (!diffs) return;
    const nodes: Node[] = [];
    if (layout.tree) {
      const walk = (dir: FileDir<FileSection>, depth: number, into: Node[]) => {
        for (const sub of dir.dirs) {
          const row = el('div', 'dir-head');
          row.dataset.dir = sub.path;
          row.style.paddingLeft = `${12 + depth * 14}px`;
          row.title = sub.path;
          row.append(el('span', 'chevron'), el('span', 'name', sub.name));
          row.addEventListener('click', () => {
            if (foldedDirs.has(sub.path)) foldedDirs.delete(sub.path);
            else foldedDirs.add(sub.path);
            applyFolds();
            renderBar();
          });
          const children = el('div', 'dir-children');
          children.dataset.dir = sub.path;
          const inner: Node[] = [];
          walk(sub, depth + 1, inner);
          children.append(...inner);
          into.push(row, children);
        }
        for (const file of dir.files) {
          file.head.style.paddingLeft = `${12 + depth * 14}px`;
          setPath(file, dir.path);
          into.push(file.section);
        }
      };
      walk(makeFileTree([...sections.values()]), 0, nodes);
    } else {
      for (const file of sections.values()) {
        file.head.style.paddingLeft = '';
        setPath(file, undefined);
        nodes.push(file.section);
      }
    }
    diffs.replaceChildren(...nodes);
    applyFolds();
  }

  /** 見出しのパス。木なら、そのフォルダの中の名前だけ（`dir` はフォルダのパス） */
  function setPath(file: FileSection, dir: string | undefined): void {
    const label = file.head.querySelector('.path')!;
    const inside = dir === undefined ? undefined : dir ? `${dir}/` : '';
    const relative = (p: string) => (inside !== undefined && p.startsWith(inside) ? p.slice(inside.length) : p);
    label.replaceChildren();
    if (file.origPath) {
      const moved = inside !== undefined && !file.origPath.startsWith(inside);
      label.append(moved ? `${file.origPath} → ${relative(file.path)}` : renamedPath(relative(file.origPath), relative(file.path)));
    } else if (inside !== undefined) {
      label.append(relative(file.path));
    } else {
      const slash = file.path.lastIndexOf('/');
      if (slash >= 0) label.append(el('span', 'dir', file.path.slice(0, slash + 1)));
      label.append(file.path.slice(slash + 1));
    }
  }

  /** 畳んだフォルダの中身を隠す */
  function applyFolds(): void {
    if (!diffs) return;
    for (const row of diffs.querySelectorAll<HTMLElement>('.dir-head')) {
      const folded = foldedDirs.has(row.dataset.dir!);
      row.classList.toggle('folded', folded);
      row.querySelector('.chevron')!.textContent = folded ? '▸' : '▾';
      (row.nextElementSibling as HTMLElement).hidden = folded;
    }
  }

  /** 見せ方を変えた */
  function applyLayout(): void {
    arrange();
    renderBar();
  }
  panes.add(applyLayout);

  /**
   * 差分を並べる。差分は画面に入ったものから順に求める（畳んだものは開くまで求めない）。`single` は 1 つのコミット
   * （右クリックのメニューと Open がある）
   */
  function fileSections(
    oid: string,
    items: readonly { path: string; origPath?: string; status?: string; added?: number; removed?: number }[],
    single: boolean,
    allOid?: string,
  ): void {
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
    foldedDirs = new Set();
    contexts = new Map();
    openAll = items.length > 0 ? allOid : undefined;
    imagesFor = single && images;
    requestDiff = (path) => post({ type: 'commitDiff', oid, path });
    diffs = el('div', 'commit-diffs');
    for (const f of items) {
      const section = el('section', 'file-diff');
      section.dataset.path = f.path;
      const head = el('div', 'diff-head');
      head.title = f.origPath ? `${f.origPath} → ${f.path}` : f.path;
      head.append(el('span', 'chevron'));
      if (f.status) head.append(el('span', `status s-${f.status}`, f.status));
      head.append(el('span', 'path'));
      if (single) {
        head.dataset.vscodeContext = fileContext(oid, f.path, choices !== undefined, f.status, f.origPath);
        const open = el('button', 'head-button', 'Open');
        open.title = 'Open the diff in the editor';
        open.addEventListener('click', () => post({ type: 'openFile', oid, path: f.path }));
        // 右クリックと同じメニュー（Blame・File History・戻す・パスのコピー）
        const more = el('button', 'head-button more', '⋯');
        more.title = 'More actions for this file';
        more.addEventListener('click', (e) => {
          e.stopPropagation();
          const r = more.getBoundingClientRect();
          head.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left, clientY: r.bottom, button: 2 }));
        });
        head.append(open, more);
      }
      section.append(head, el('div', 'empty', 'Loading…'));

      const large = (f.added ?? 0) + (f.removed ?? 0) > COLLAPSE_ABOVE;
      const file: FileSection = { section, head, path: f.path, origPath: f.origPath, requested: false, chosen: false, heavy: isGenerated(f.path) || large };
      sections.set(f.path, file);
      if (f.added !== undefined && f.removed !== undefined) setCounts(file, { added: f.added, removed: f.removed });
      // 最初はすべて畳む（見出しが変わったファイルの一覧を兼ねる。アプリと同じ）
      setCollapsed(file, true);

      head.addEventListener('click', (e) => {
        if ((e.target as Element).closest('button')) return;
        file.chosen = true;
        setCollapsed(file, !section.classList.contains('collapsed'));
        request(file);
        // 畳んで見出しが上に隠れたら、見出しが見えるところまで戻す
        if (head.getBoundingClientRect().top < scroller.getBoundingClientRect().top) section.scrollIntoView({ block: 'start' });
        renderBar();
      });
      diffObserver.observe(section);
    }
    changedFiles = items.map((f) => ({ path: f.path, ...(f.status ? { status: f.status } : {}) }));
    allFiles = undefined;
    showPage();
    applyLayout();
  }

  /** 変更とすべてのファイルを切り替える */
  function setPage(next: 'changes' | 'all'): void {
    page = next;
    showPage();
    renderBar();
  }

  /** 今のページを欄に出す（すべてのファイルは、初めて開くときに作る） */
  function showPage(): void {
    const canAll = imagesFor && choices !== undefined && shownOid !== undefined;
    if (page === 'all' && canAll) {
      allFiles ??= createAllFiles(shownOid!, changedFiles);
      scroller.replaceChildren(allFiles.root);
      root.classList.add('all-files-page');
    } else {
      if (diffs) scroller.replaceChildren(diffs);
      root.classList.remove('all-files-page');
    }
  }

  /** 出しているコミットの、ファイルの差分を求める */
  let requestDiff: (path: string) => void = () => undefined;

  /** 開いているファイルの差分を求める */
  function request(file: FileSection): void {
    if (file.requested || file.section.classList.contains('collapsed')) return;
    file.requested = true;
    requestDiff(file.path);
  }

  /** そのファイルの前後の行を広げる（`to` がなければ 20 行増やす）。差分は求め直す */
  function widen(file: FileSection, to?: 'all'): void {
    const now = contexts.get(file.path) ?? 3;
    if (now === 'all') return;
    const context = to ?? now + 20;
    contexts.set(file.path, context);
    file.chosen = true;
    setCollapsed(file, false);
    file.requested = true;
    post({ type: 'fileContext', oid: shownOid!, path: file.path, context });
    renderBar();
  }

  /** 開いているファイル */
  function openPaths(): string[] {
    return [...sections.values()].filter((f) => !f.section.classList.contains('collapsed')).map((f) => f.path);
  }

  /** 出し直した後で、前に開いていたファイルを開き直す */
  function reopenFiles(paths: readonly string[]): void {
    for (const path of paths) {
      const file = sections.get(path);
      if (!file) continue;
      file.chosen = true;
      setCollapsed(file, false);
      request(file);
    }
    if (paths.length > 0) renderBar();
  }

  function setCollapsed(file: FileSection, collapsed: boolean): void {
    file.section.classList.toggle('collapsed', collapsed);
    file.head.querySelector('.chevron')!.textContent = collapsed ? '▸' : '▾';
  }

  /** 行数を見出しに出す */
  function setCounts(file: FileSection, counts: LineCounts): void {
    file.counts = counts;
    file.head.querySelector('.stat')?.remove();
    const before = file.head.querySelector('.head-button');
    if (before) before.before(countsLabel(counts));
    else file.head.append(countsLabel(counts));
  }

  function showSelection({ count, view, error, onPair, onCommit, choices: next }: SelectionState): void {
    const reopen = view && view.key === shownOid ? openPaths() : [];
    shownOid = view?.key;
    choices = view ? next : undefined;
    meta = undefined;
    const parts: Node[] = [el('div', 'subject', `${count} commits selected`)];
    if (!view) {
      parts.push(el('div', error ? 'selection-note error' : 'selection-note', error ?? 'Loading the diff…'));
      place(parts);
      fileSections('', [], false);
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
    place(parts);
    fileSections(view.key, view.files, false);
    reopenFiles(reopen);
    if (view.files.length === 0) scroller.replaceChildren(el('div', 'empty', 'No changes'));
  }

  function showDiff(oid: string, path: string, diff: FileDiff): void {
    const file = sections.get(path);
    if (oid !== shownOid || !file) {
      return;
    }
    const all = contexts.get(path) === 'all';
    const body = renderDiffBody(diff, choices && !all ? { widen: () => widen(file) } : {});
    // 画像は前と後を比べる（1 つのコミットだけ。SVG は文字の差分も下に出す）。画像は別に求める
    if (imagesFor && isImagePath(path)) {
      const slot = file.image ?? el('div', 'empty', 'Loading the image…');
      if (!file.image) {
        file.image = slot;
        post({ type: 'commitImage', oid, path });
      }
      file.section.replaceChildren(file.head, slot, ...(diff.binary ? [] : body));
    } else {
      file.section.replaceChildren(file.head, ...body);
    }
    const counts = lineCounts(diff);
    const had = file.counts;
    setCounts(file, counts);
    // 行数は差分を読んで初めて分かることがある。大きければ畳む（自分で開いたものは畳まない）
    if (counts.added + counts.removed > COLLAPSE_ABOVE) {
      file.heavy = true;
      if (!file.chosen) setCollapsed(file, true);
    }
    if (!had) renderBar();
  }

  function showTreeEntries(oid: string, path: string, entries: TreeEntry[], error?: string): void {
    if (oid === shownOid) allFiles?.showEntries(path, entries, error);
  }

  function showTreeFile(oid: string, path: string, view: FileView): void {
    if (oid === shownOid) allFiles?.showFile(path, view);
  }

  function showImage(oid: string, path: string, before?: ImageBlob, after?: ImageBlob, error?: string): void {
    const file = sections.get(path);
    if (oid !== shownOid || !file?.image) return;
    const panel = error ? el('div', 'empty', error) : renderImageDiff(before, after);
    file.image.replaceWith(panel);
    file.image = panel;
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
    place([]);
    fileSections('', [], false);
    scroller.replaceChildren(el('div', 'empty', 'Select a commit to see its details here'));
  }

  clear();
  return {
    root,
    get shownOid() {
      return shownOid;
    },
    show,
    showDiff,
    showImage,
    showTreeEntries,
    showTreeFile,
    showExtras,
    clear,
    showSelection,
    forget() {
      shownOid = undefined;
    },
  };
}

/**
 * メッセージの欄と、ファイルと差分の欄の仕切りをドラッグして、メッセージの欄の高さを変える。変えた高さはどの
 * コミットでも保ち、仕切りをダブルクリックすると中身に合わせ直す（アプリの「高さを自動で決める」）
 */
function attachRowSplitter(root: HTMLElement, divider: HTMLElement, head: HTMLElement): void {
  divider.title = 'Drag to keep this height for every commit; double-click to fit the commit details again';
  divider.addEventListener('dblclick', () => {
    updateState({ detailsHeadHeight: undefined });
    head.style.height = '';
  });
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
      if (head.style.height) updateState({ detailsHeadHeight: parseFloat(head.style.height) });
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
