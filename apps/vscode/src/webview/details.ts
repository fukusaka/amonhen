/**
 * コミットの詳細の欄（History と Reflog の右側）。メッセージ・作者・親と、変更したファイルの一覧、ファイルごとの
 * 差分を出す。差分は画面に入ったものから順に拡張ホストに求める
 */
import type { FileDiff } from '../diff/fileDiff.ts';
import type { CommitDetails, DetailsExtras, PairMode, Person, SelectionView } from '../history/protocol.ts';
import { el, formatDate, getState, post, updateState } from './api.ts';
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
  let shownOid: string | undefined;
  /** 詳細に並べたファイル */
  let sections = new Map<string, FileSection>();
  let diffObserver: IntersectionObserver | undefined;
  /** 出している詳細の作者などの並び（後から署名・子のコミットを足す） */
  let meta: HTMLElement | undefined;

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

    const head = el('div', 'subject');
    for (const ref of d.refs) {
      head.append(el('span', `ref ${ref.kind}${ref.current ? ' current' : ''}`, ref.name));
    }
    head.append(el('span', '', subject));

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
        `${count} file${count === 1 ? '' : 's'} changed${d.parents.length > 1 ? '（最初の親との差分）' : ''}`,
      ),
    );
    if (count > 0) {
      const all = el('button', 'secondary', 'Open All');
      all.addEventListener('click', () => post({ type: 'open', oid: d.oid }));
      filesHead.append(all);
    }
    files.append(filesHead);
    const diffs = fileSections(d.oid, d.oid, d.files, files, true, viewed);

    const parts: Node[] = [head, list];
    if (body) parts.push(el('pre', 'body', body));
    parts.push(files, diffs);
    root.replaceChildren(...parts);
    root.scrollTop = 0;
    const file = focus ? sections.get(focus) : undefined;
    if (file) {
      file.row.classList.add('active');
      file.section.scrollIntoView({ block: 'start' });
    }
  }

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
          if (entry.isIntersecting && file) request(oid, file);
        }
      },
      { root, rootMargin: '200px' },
    );
    sections = new Map();
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
        open.title = 'エディタで差分を開く';
        open.addEventListener('click', () => post({ type: 'openFile', oid, path: f.path }));
        head.append(open);
      }
      section.append(head, el('div', 'empty', '読み込み中…'));
      diffs.append(section);

      const file: FileSection = { row, section, head, requested: false, chosen: false, viewed: viewedSet.has(f.path) };
      sections.set(f.path, file);
      if (f.added !== undefined && f.removed !== undefined) setCounts(file, { added: f.added, removed: f.removed });
      box.checked = file.viewed;
      setViewed(file, file.viewed);
      // 最初から畳む: Viewed にしたもの、生成物、変わった行が多いもの
      const large = (f.added ?? 0) + (f.removed ?? 0) > COLLAPSE_ABOVE;
      setCollapsed(file, file.viewed || isGenerated(f.path) || large);

      // その差分の位置へ移る
      row.addEventListener('click', () => {
        for (const other of files.querySelectorAll('.file.active')) other.classList.remove('active');
        row.classList.add('active');
        section.scrollIntoView({ block: 'start' });
      });
      head.addEventListener('click', (e) => {
        if ((e.target as Element).closest('button, label')) return;
        file.chosen = true;
        setCollapsed(file, !section.classList.contains('collapsed'));
        if (!section.classList.contains('collapsed')) request(oid, file);
        // 畳んで見出しが上に隠れたら、見出しが見えるところまで戻す
        if (head.getBoundingClientRect().top < root.getBoundingClientRect().top) section.scrollIntoView({ block: 'start' });
      });
      box.addEventListener('change', () => {
        // Viewed にしたら畳み、戻したら開く（GitHub と同じ）
        file.chosen = true;
        setViewed(file, box.checked);
        setCollapsed(file, box.checked);
        if (!box.checked) request(oid, file);
        if (viewedKey) post({ type: 'viewed', key: viewedKey, path: f.path, viewed: box.checked });
        if (box.checked && head.getBoundingClientRect().top < root.getBoundingClientRect().top) section.scrollIntoView({ block: 'start' });
      });
      files.append(row);
      diffObserver.observe(section);
    }
    return diffs;
  }

  /** 見えたファイルの差分を求める（畳んでいれば開くまで待つ） */
  function request(oid: string, file: FileSection): void {
    if (file.requested || file.section.classList.contains('collapsed')) return;
    file.requested = true;
    post({ type: 'commitDiff', oid, path: file.section.dataset.path! });
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
    diffObserver?.disconnect();
    sections = new Map();
    const parts: Node[] = [el('div', 'subject', `${count} commits selected`)];
    if (!view) {
      parts.push(el('div', error ? 'selection-note error' : 'selection-note', error ?? 'Loading the diff…'));
      root.replaceChildren(...parts);
      root.scrollTop = 0;
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
      files.append(el('div', 'files-head', `${n} file${n === 1 ? '' : 's'} changed`));
      parts.push(files, fileSections(view.key, view.viewedKey, view.files, files, false, view.viewed));
    }
    root.replaceChildren(...parts);
    root.scrollTop = 0;
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
    root.replaceChildren(el('div', 'empty', 'コミットを選ぶと、ここに詳細が出ます'));
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
