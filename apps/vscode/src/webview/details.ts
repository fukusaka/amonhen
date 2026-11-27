/**
 * コミットの詳細の欄（History と Reflog の右側）。上にメッセージ・作者・親、下にファイルごとの差分を出す（アプリと
 * 同じく、差分の見出しが変わったファイルの一覧を兼ね、最初はすべて畳む）。差分は開いて画面に入ったものから順に
 * 拡張ホストに求める。間の仕切りはドラッグでき、メッセージの欄は中身に合わせ（列の半分まで）、⤢ で列いっぱいに広げる
 */
import type { DiffLine, FileDiff } from '../diff/fileDiff.ts';
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
  CommitSummaryView,
  DetailsStyle,
  TagView,
  TreeEntry,
} from '../history/protocol.ts';
import { el, formatDate, getState, post, updateState } from './api.ts';
import { type FileDir, makeFileTree } from '../workingCopy/fileList.ts';
import { COLLAPSE_ABOVE, isGenerated, type LineCounts, lineCounts, renamedPath } from '../history/stackedFiles.ts';
import { renderDiffBody, whitespaceSelect, wrapButton } from './diffView.ts';
import { isImagePath, renderImageDiff } from './imageDiff.ts';
import { type AllFiles, createAllFiles } from './allFiles.ts';
import { type MissingBand, missingBand, missingFilesText } from './missingBand.ts';
import { signatureMark } from './signature.ts';
import { linkPieces, splitTrailers } from '../history/messageParts.ts';

/** mailmap で置き換えた人。記録と違えば、記録されたものを下に小さく添える */
function withRecorded(text: Node | string, recorded: Person | undefined): Node | string {
  if (!recorded) {
    return text;
  }
  const span = el('span');
  span.append(text);
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
function fileContext(oid: string, path: string, wide: boolean, status?: string, origPath?: string, state: { collapsed?: boolean; whole?: boolean } = {}): string {
  return JSON.stringify({
    webviewSection: 'commitFile',
    oid,
    path,
    ...(origPath ? { origPath } : {}),
    commitFileWide: wide,
    // 開く・畳む（アプリの ⋯ の最初の項目）と、ファイル全体を出しているか（Show Whole File を戻す）
    commitFileCollapsed: state.collapsed === true,
    commitFileWhole: state.whole === true,
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
  /** partial clone で手元になかった中身を取り終えた（`path` があれば「すべてのファイル」のそのファイル） */
  contentsFetched(oid: string, path: string | undefined, error: string | undefined): void;
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

/** コミット情報の見せ方（簡潔・標準・詳細）。拡張ホストが覚え、すべての詳細の欄で同じにする */
let style: DetailsStyle = 'standard';
const restylers = new Set<() => void>();

export function setDetailsStyle(next: DetailsStyle): void {
  if (next === style) return;
  style = next;
  for (const restyle of restylers) restyle();
}

/** 表示の設定で選んだこと（高さを中身に合わせ直す・コミット情報だけを表示する）を、見えている詳細の欄に */
const viewActions = new Set<(action: 'fit' | 'expand') => void>();

export function detailsViewAction(action: 'fit' | 'expand'): void {
  for (const act of viewActions) act(action);
}

/** 右クリックの Show Whole File: そのコミットを出している欄が、ファイル全体を頼む */
const wholeFiles = new Set<(oid: string, path: string) => void>();

export function showWholeFile(oid: string, path: string): void {
  for (const show of wholeFiles) show(oid, path);
}

/** 見出しのメニューの Expand・Collapse と Show Changes Only */
const fileActions = new Set<(oid: string, path: string, action: 'toggle' | 'changesOnly') => void>();

export function fileAction(oid: string, path: string, action: 'toggle' | 'changesOnly'): void {
  for (const act of fileActions) act(oid, path, action);
}

/** Open File at This Commit（見出しと差分の行の右クリック）。見えている欄が「すべてのファイル」で開けば true */
const atCommits = new Set<(oid: string, path: string, line?: number) => boolean>();

/**
 * その時点のファイルを開く。出している欄に「すべてのファイル」があればそこで（行があればその行へ。アプリと同じ）、
 * なければ（エンジンがない・History でない）エディタで開く
 */
export function openAtCommit(oid: string, path: string, line?: number): void {
  for (const open of atCommits) if (open(oid, path, line)) return;
  post({ type: 'openTreeFile', oid, path, ...(line ? { line } : {}) });
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
  /** partial clone で中身が手元になく、差分を取っていない（「取ってくる」の帯で取るまで） */
  missing?: boolean;
  /** SVG を文字の差分で見ている（Show as Text）。読んだ差分（絵と文字を行き来するため） */
  svgText?: boolean;
  diff?: FileDiff;
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
/** 詳細の欄を持つ画面ごとの違い */
export interface DetailsOptions {
  /** 件名・本文・トレーラー・notes で強調する語（History の検索） */
  highlight?: () => readonly string[];
  /** 右クリックのメニュー（package.json の webview/context）に足す情報（History なら編集できるか） */
  context?: (oid: string) => Record<string, unknown>;
  /** 作者・コミッターの名前を押したとき（History はその人で絞り込む） */
  onPerson?: (name: string) => void;
}

export function createDetailsPane(onParent?: (oid: string) => void, images = false, options: DetailsOptions = {}): DetailsPane {
  const root = el('aside', 'details');
  /** メッセージ（件名・作者など・本文）の欄。いくつも選んだときは、その説明 */
  const head = el('div', 'details-head');
  const divider = el('div', 'details-divider');
  /** 差分の上の帯（数・リストと木・すべて開く・畳む） */
  const bar = el('div', 'files-bar');
  /** partial clone で中身が手元にないときの「取ってくる」の帯（差分の上の帯の下。変更の側だけ） */
  const bandSlot = el('div', 'missing-band-slot');
  let band: MissingBand | undefined;
  /** ファイルと差分の欄 */
  const scroller = el('div', 'details-body');
  root.append(head, divider, bar, bandSlot, scroller);
  attachRowSplitter(root, divider, head);
  let shownOid: string | undefined;
  /** 詳細に並べたファイル（並べた順） */
  let sections = new Map<string, FileSection>();
  let diffObserver: IntersectionObserver | undefined;
  /** 出している詳細の作者などの並び（後から署名・子のコミットを足す） */
  let meta: HTMLElement | undefined;
  /** 出している詳細と、届いた残り（見せ方を変えたら出し直す） */
  let shownDetails: { d: CommitDetails; next?: DiffChoices } | undefined;
  let shownExtras: DetailsExtras | undefined;
  restylers.add(() => {
    if (!shownDetails || shownDetails.d.oid !== shownOid) return;
    const extras = shownExtras;
    renderHead(shownDetails.d);
    if (extras) showExtras(extras);
  });
  viewActions.add((action) => {
    if (!root.isConnected || root.offsetParent === null) return;
    if (action === 'fit') {
      updateState({ detailsHeadHeight: undefined });
      head.style.height = '';
    } else {
      head.querySelector<HTMLElement>('.expand-details')?.click();
    }
  });
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
  atCommits.add((oid, path, line) => {
    if (oid !== shownOid || !root.closest('.view.active') || !canAll()) return false;
    setPage('all');
    allFiles?.open(path, line);
    return true;
  });
  fileActions.add((oid, path, action) => {
    const file = sections.get(path);
    if (oid !== shownOid || !file) return;
    if (action === 'toggle') {
      file.chosen = true;
      setCollapsed(file, !file.section.classList.contains('collapsed'));
      request(file);
      renderBar();
    } else if (contexts.get(path) === 'all') {
      // ファイル全体をやめて、変えたところ（前後 3 行）に戻す
      contexts.delete(path);
      file.requested = true;
      post({ type: 'fileContext', oid, path, context: 3 });
    }
  });

  /**
   * メッセージの文: URL とコミット ID をリンクにし（ID は onParent があれば押すとその行に移る）、検索の語を強調する
   * （アプリと同じ）
   */
  function richText(text: string): Node[] {
    const terms = (options.highlight?.() ?? []).filter(Boolean).map((t) => t.toLowerCase());
    const marked = (s: string): Node[] => {
      if (terms.length === 0) return [document.createTextNode(s)];
      const out: Node[] = [];
      const lower = s.toLowerCase();
      let at = 0;
      while (at < s.length) {
        let next = -1;
        let len = 0;
        for (const t of terms) {
          const i = lower.indexOf(t, at);
          if (i >= 0 && (next < 0 || i < next)) [next, len] = [i, t.length];
        }
        if (next < 0) break;
        if (next > at) out.push(document.createTextNode(s.slice(at, next)));
        out.push(el('mark', 'match', s.slice(next, next + len)));
        at = next + len;
      }
      if (at < s.length) out.push(document.createTextNode(s.slice(at)));
      return out;
    };
    return linkPieces(text).flatMap((p): Node[] => {
      if ('url' in p) {
        const a = el('a', 'url', p.url);
        a.href = p.url;
        a.title = p.url;
        return [a];
      }
      if ('commit' in p) {
        if (!onParent) return marked(p.commit);
        const a = el('a', 'parent', p.commit);
        a.href = '#';
        a.title = `Go to ${p.commit}`;
        a.addEventListener('click', (e) => {
          e.preventDefault();
          onParent(p.commit);
        });
        return [a];
      }
      return marked(p.text);
    });
  }

  /** コミットの ID の並び（onParent があれば押すとその行に移る）。取っていないもの（missing）は薄く「(not fetched)」 */
  function commitLinks(oids: readonly string[], missing: readonly string[] = []): HTMLElement {
    const span = el('span');
    for (const p of oids) {
      if (missing.includes(p)) {
        const code = el('code', 'parent missing', p.slice(0, 8));
        code.title = `${p} isn’t fetched (shallow clone)`;
        span.append(code, el('span', 'recorded', ' (not fetched) '));
        continue;
      }
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

  /** 名前（押せばその人で絞り込む）と、その後ろに続ける文字 */
  function person(name: string, rest: string): Node {
    const span = el('span');
    if (options.onPerson) {
      const link = el('a', 'person', name);
      link.href = '#';
      link.title = `Show commits by ${name}`;
      link.addEventListener('click', (e) => {
        e.preventDefault();
        options.onPerson?.(name);
      });
      span.append(link, rest);
    } else {
      span.append(name + rest);
    }
    return span;
  }

  function show(d: CommitDetails, focus?: string, next?: DiffChoices): void {
    // 同じコミットを出し直す（空白を選び直した・中身を取ってきた）なら、開いていたファイルは開いたまま、同じ位置に
    const same = d.oid === shownOid;
    const reopen = same ? openPaths() : [];
    const top = same ? scroller.scrollTop : 0;
    shownOid = d.oid;
    choices = next;
    shownDetails = { d, ...(next ? { next } : {}) };
    shownExtras = undefined;
    renderHead(d);
    const missing = new Set(d.missingFiles ?? []);
    fileSections(d.oid, missing.size > 0 ? d.files.map((f) => (missing.has(f.path) ? { ...f, missing: true } : f)) : d.files, true, d.oid);
    reopenFiles(reopen);
    if (same && top > 0) scroller.scrollTop = top;
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

  /**
   * コミット情報の欄（見せ方による。アプリと同じ）。簡潔: 作者・日時・ref・親の 1 行と件名。標準: 作者・ID・署名・
   * タグと本文。詳細: さらに子・HEAD からの道・含むブランチとタグ・前のタグ・push 済みか・取り込んだコミット・関係する
   * コミット
   */
  function renderHead(d: CommitDetails): void {
    const [subject = '', ...rest] = d.message.replace(/\n+$/, '').split('\n');
    const body = rest.join('\n').replace(/^\n+/, '');

    if (style === 'brief') {
      const line = el('div', 'brief-line');
      line.append(person(d.author.name, ''), `  ${formatDate(d.author.date)}`);
      for (const ref of d.refs) line.append('  ', el('span', `ref ${ref.kind}${ref.current ? ' current' : ''}`, ref.name));
      if (d.parents.length > 0) line.append(d.parents.length > 1 ? '  Parents ' : '  Parent ', commitLinks(d.parents));
      const title = el('div', 'subject');
      title.append(...richText(subject));
      meta = undefined;
      place([expandButton(), line, title]);
      head.dataset.vscodeContext = JSON.stringify({ webviewSection: 'commitDetails', oid: d.oid, ...options.context?.(d.oid) });
      return;
    }
    const title = el('div', 'subject');
    for (const ref of d.refs) {
      title.append(el('span', `ref ${ref.kind}${ref.current ? ' current' : ''}`, ref.name));
    }
    const subjectText = el('span');
    subjectText.append(...richText(subject));
    title.append(subjectText);

    const list = el('dl', 'meta');
    meta = list;
    const add = (label: string, value: Node | string) => {
      const dd = el('dd');
      dd.append(value);
      list.append(el('dt', '', label), dd);
    };
    add('Author', withRecorded(person(d.author.name, ` <${d.author.email}>`), d.recordedAuthor));
    add('Date', formatDate(d.author.date));
    if (d.committer.name !== d.author.name || d.committer.date !== d.author.date) {
      add('Committer', withRecorded(person(d.committer.name, `  ${formatDate(d.committer.date)}`), d.recordedCommitter));
    }
    add('Commit', el('code', '', d.oid));
    if (d.parents.length > 0) {
      add(d.parents.length > 1 ? 'Parents' : 'Parent', commitLinks(d.parents, d.missingParents));
    }

    const parts: Node[] = [expandButton(), title, list];
    // 本文（トレーラーは最後の段落から分けて、名前と値に並べる。アプリと同じ）
    const { body: text, trailers } = splitTrailers(body);
    if (text) {
      const pre = el('pre', 'body');
      pre.append(...richText(text));
      parts.push(pre);
    }
    if (trailers.length > 0) {
      const list = el('div', 'trailers');
      for (const t of trailers) {
        const line = el('div', 'trailer');
        line.append(el('span', 'trailer-key', `${t.key}:`), ' ', ...richText(t.value));
        list.append(line);
      }
      parts.push(list);
    }
    if (d.parents.length > 1) {
      parts.push(el('div', 'selection-note', `This is a merge: the changes below are against its first parent (${d.parents[0]!.slice(0, 7)}).`));
    }
    place(parts);
    // 見出しの右クリック: ID・件名・メッセージ・git show のコピー、編集できるコミットなら Edit Message…（アプリと同じ）。
    // 選んだ文字のコピーも残す（既定の項目を消さない）
    head.dataset.vscodeContext = JSON.stringify({ webviewSection: 'commitDetails', oid: d.oid, ...options.context?.(d.oid) });
  }

  /** 上の欄に並べる（高さは中身に合わせ直し、広げていれば戻す） */
  function place(message: Node[]): void {
    // 1 つのコミットの詳細（show）でなければ、見出しの右クリックは出さない。show が付け直す
    delete head.dataset.vscodeContext;
    // コミットを替えても、コミット情報だけを表示したまま（アプリと同じ）。何も出さないときだけ戻す
    if (message.length === 0) root.classList.remove('expanded');
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
      button.title = expanded ? 'Restore Commit Details' : 'Show Only Commit Details\nExpand the commit details to fill the right column (click again to restore)';
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

  /** 帯の右端の表示の設定（アプリの View Options。拡張ホストが一覧を出す） */
  function viewOptionsButton(): HTMLElement {
    return barButton('⚙︎', 'View Options', false, () =>
      post({ type: 'detailsViewOptions', kept: getState().detailsHeadHeight !== undefined, expanded: root.classList.contains('expanded') }),
    );
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
    const switcher = el('span', 'segmented');
    switcher.append(
      barButton('List', 'Show the files as a list of paths', !layout.tree, () => post({ type: 'detailsLayout', layout: { tree: false } })),
      barButton('Tree', 'Show the files in their folders', layout.tree, () => post({ type: 'detailsLayout', layout: { tree: true } })),
    );
    if (canAll && page === 'all') {
      const spacer = el('span', 'spacer');
      bar.replaceChildren(pages, spacer, switcher, wrapButton(), viewOptionsButton());
      return;
    }
    const total = el('span', 'files-total', `${files.length} file${files.length === 1 ? '' : 's'}`);
    // 中身が手元にないファイルは、行数を数えていない（取ってくるまで合計に入れない）
    const counted = files.filter((f) => !f.missing);
    if (counted.every((f) => f.counts)) {
      total.append(' ', countsLabel(counted.reduce((sum, f) => ({ added: sum.added + f.counts!.added, removed: sum.removed + f.counts!.removed }), { added: 0, removed: 0 })));
    }
    const spacer = el('span', 'spacer');
    const items: Node[] = canAll ? [pages, total, spacer, switcher] : [total, spacer, switcher];
    if (choices) {
      const select = whitespaceSelect(choices.whitespace, (whitespace) => post({ type: 'whitespace', whitespace }));
      items.push(select);
    }
    items.push(wrapButton());
    if (layout.tree && diffs?.querySelector('.dir-head')) {
      const anyFolded = foldedDirs.size > 0;
      items.push(
        barButton(anyFolded ? 'Expand All Folders' : 'Collapse All Folders', anyFolded ? 'Expand all folders' : 'Collapse all folders', false, () => {
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
        expand ? 'Expand All Diffs' : 'Collapse All Diffs',
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
    items.push(viewOptionsButton());
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
  // すべてのファイルもリストと木を切り替える（アプリと同じ）
  panes.add((next) => allFiles?.setTree(next.tree));

  /**
   * 差分を並べる。差分は画面に入ったものから順に求める（畳んだものは開くまで求めない）。`single` は 1 つのコミット
   * （右クリックのメニューと Open がある）
   */
  function fileSections(
    oid: string,
    items: readonly { path: string; origPath?: string; status?: string; added?: number; removed?: number; missing?: boolean }[],
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
        // 右クリックの直前に、今の様子（畳んでいるか・ファイル全体か）を入れ直す
        head.addEventListener(
          'contextmenu',
          () => {
            head.dataset.vscodeContext = fileContext(oid, f.path, choices !== undefined, f.status, f.origPath, {
              collapsed: section.classList.contains('collapsed'),
              whole: contexts.get(f.path) === 'all',
            });
          },
          true,
        );
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
      } else {
        // いくつも選んだときの見出しにも、パスで使える項目（File History・Copy Path。アプリと同じ）
        head.dataset.vscodeContext = JSON.stringify({ webviewSection: 'selectionFile', path: f.path, preventDefaultContextMenuItems: true });
        const more = el('button', 'head-button more', '⋯');
        more.title = 'More actions for this file';
        more.addEventListener('click', (e) => {
          e.stopPropagation();
          const r = more.getBoundingClientRect();
          head.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left, clientY: r.bottom, button: 2 }));
        });
        head.append(more);
      }
      section.append(head, el('div', 'empty', 'Loading…'));

      const large = (f.added ?? 0) + (f.removed ?? 0) > COLLAPSE_ABOVE;
      const file: FileSection = { section, head, path: f.path, origPath: f.origPath, requested: false, chosen: false, heavy: isGenerated(f.path) || large };
      sections.set(f.path, file);
      if (f.missing) {
        // 中身が手元にない（行数も数えていない）: 行数の代わりに添え書き（アプリと同じ）
        file.missing = true;
        const note = el('span', 'missing-note', 'Not here yet');
        note.title = 'This file’s contents aren’t here yet (partial clone).';
        const before = head.querySelector('.head-button');
        if (before) before.before(note);
        else head.append(note);
      } else if (f.added !== undefined && f.removed !== undefined) setCounts(file, { added: f.added, removed: f.removed });
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
    const missing = items.filter((f) => f.missing).length;
    const key = oid;
    band =
      missing > 0
        ? missingBand(missingFilesText(missing), () => post({ type: 'fetchContents', oid: key }))
        : undefined;
    bandSlot.replaceChildren(...(band ? [band.root] : []));
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
  /** 「すべてのファイル」を出せるか（1 つのコミットで、エンジンがあるとき） */
  function canAll(): boolean {
    return imagesFor && choices !== undefined && shownOid !== undefined;
  }

  function showPage(): void {
    if (page === 'all' && canAll()) {
      allFiles ??= createAllFiles(shownOid!, changedFiles, {
        tree: layout.tree,
        // Show This Change: 変更に戻り、そのファイルの差分を開いて見せる（アプリと同じ）
        onShowChange: (path) => {
          setPage('changes');
          const file = sections.get(path);
          if (!file) return;
          file.chosen = true;
          setCollapsed(file, false);
          request(file);
          file.section.scrollIntoView({ block: 'start' });
        },
      });
      scroller.replaceChildren(allFiles.root);
      root.classList.add('all-files-page');
    } else {
      if (diffs) scroller.replaceChildren(diffs);
      root.classList.remove('all-files-page');
    }
    // 差分の「取ってくる」の帯は変更の側だけ（すべてのファイルでは、開いたファイルごとに聞く。アプリと同じ）
    bandSlot.hidden = !band || (page === 'all' && canAll());
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
    const same = view !== undefined && view.key === shownOid;
    const reopen = same ? openPaths() : [];
    const top = same ? scroller.scrollTop : 0;
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
    if (same && top > 0) scroller.scrollTop = top;
    if (view.files.length === 0) scroller.replaceChildren(el('div', 'empty', 'No changes'));
  }

  function showDiff(oid: string, path: string, diff: FileDiff): void {
    const file = sections.get(path);
    if (oid !== shownOid || !file) {
      return;
    }
    if (file.missing) {
      // 中身が手元にない: 取りに行かずに、帯で取ってくるまで待つ（画像も求めない）
      file.section.replaceChildren(file.head, el('div', 'empty', 'This file’s contents aren’t here yet.'));
      return;
    }
    const all = contexts.get(path) === 'all';
    // 1 つのコミットなら、行の右クリックに Open File at This Commit（その行で開く。アプリと同じ）
    const lineContext = imagesFor
      ? (line: DiffLine) => ({
          webviewSection: 'commitLine',
          oid,
          path,
          ...(line.new ? { commitLine: line.new } : {}),
          commitFileDeleted: changedFiles.some((c) => c.path === path && c.status === 'D'),
          preventDefaultContextMenuItems: true,
        })
      : undefined;
    const body = renderDiffBody(diff, { ...(choices && !all ? { widen: () => widen(file) } : {}), ...(lineContext ? { lineContext } : {}) });
    // 画像は前と後を比べる（1 つのコミットだけ）。画像は別に求める。SVG は絵を出し、「Show as Text」で文字の差分に
    // 替える（アプリと同じ）
    if (imagesFor && isImagePath(path) && !file.svgText) {
      const slot = file.image ?? el('div', 'empty', 'Loading the image…');
      if (!file.image) {
        file.image = slot;
        post({ type: 'commitImage', oid, path });
      }
      file.diff = diff;
      file.section.replaceChildren(file.head, slot);
    } else if (file.svgText) {
      const back = el('button', 'bar-button svg-picture', 'Show as Picture');
      back.title = 'Show the SVG as a picture again';
      back.addEventListener('click', () => {
        file.svgText = false;
        showDiff(oid, path, diff);
      });
      file.section.replaceChildren(file.head, back, ...body);
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
    const svg = /\.svg$/i.test(path);
    const panel = error
      ? el('div', 'empty', error)
      : renderImageDiff(before, after, {
          ...(svg
            ? {
                onShowText: () => {
                  file.svgText = true;
                  if (file.diff) showDiff(oid, path, file.diff);
                },
              }
            : {}),
        });
    file.image.replaceWith(panel);
    file.image = panel;
  }

  function showExtras(extras: DetailsExtras): void {
    if (extras.oid === shownOid) shownExtras = extras;
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
      const note = el('span', 'recorded', [s.explanation, s.signer && `Signed by ${s.signer}`, s.format, s.key && `Key ${s.key}`].filter(Boolean).join(' · '));
      if (s.fingerprint) note.title = `Fingerprint ${s.fingerprint}`;
      value.append(note);
      add('Signature', value);
    }
    // このコミットを指すタグと、この merge が取り込んだタグ（注釈付きならタガー・日時・件名と署名。アプリと同じ）
    const tagLine = (t: TagView): Node => {
      const span = el('span', 'tag-line');
      span.append(el('span', 'ref tag', t.name));
      const about = [t.tagger, t.time ? formatDate(t.time) : undefined].filter(Boolean).join(', ');
      if (about || t.subject) span.append(el('span', 'recorded', [about, t.subject].filter(Boolean).join(' — ')));
      if (t.signature) {
        const sig = el('span', 'signature');
        sig.append(signatureMark(t.signature), ` ${t.signature.label}`);
        sig.title = [t.signature.explanation, t.signature.signer && `Signed by ${t.signature.signer}`, t.signature.format, t.signature.key && `Key ${t.signature.key}`]
          .filter(Boolean)
          .join('\n');
        span.append(' ', sig);
      }
      return span;
    };
    for (const t of extras.tags ?? []) add('Tag', tagLine(t));
    // 詳細（Full）だけの項目（アプリの renderDetailed と同じ並び）
    const full = style === 'full' ? extras.full : undefined;
    const text = (s: string) => document.createTextNode(s);
    if (full) {
      if (extras.children.length > 0) add(extras.children.length > 1 ? 'Children' : 'Child', commitLinks(extras.children));
      if (full.headRev) add('Rev', el('code', '', full.headRev));
      if (full.branches.length > 0) add('Branches', text(full.branches.join(', ')));
      if (full.tagCount > 0) {
        const more = full.tagCount > 12 ? ` and ${full.tagCount - 12} more tags` : '';
        add('Tags', text(full.tags.slice(0, 12).join(', ') + more));
      }
      if (full.previousTag) add('Previous Tag', text(full.previousTag));
      if (full.pushed) {
        add('Pushed', full.pushed.length > 0 ? text(full.pushed.join(', ')) : el('span', 'unpushed', 'Not pushed (not on any remote branch)'));
      }
    }
    for (const t of extras.mergedTags ?? []) add('Merged Tag', tagLine(t));
    const summary = (c: CommitSummaryView): Node => {
      const line = el('div', 'summary-line');
      line.append(commitLinks([c.oid]), c.subject, ...(c.author ? [el('span', 'recorded', ` — ${c.author}`)] : []));
      return line;
    };
    // 取り込んだコミット: 標準は数だけ、詳細は 20 まで並べる
    const mergedCount = extras.mergedCount ?? extras.full?.mergedCount ?? 0;
    if (mergedCount > 0) {
      const value = el('div');
      value.append(`${mergedCount} commit${mergedCount === 1 ? '' : 's'}`);
      if (full) {
        value.append(...full.merged.map(summary));
        if (mergedCount > full.merged.length) value.append(el('div', 'recorded', `and ${mergedCount - full.merged.length} more`));
      }
      add('Merged', value);
    }
    for (const r of full?.related ?? []) {
      const value = el('span');
      value.append(el('span', 'recorded', `${r.kind}  `), summary(r.commit));
      add('Related', value);
    }
    // git notes（refs/notes/commits）は、メッセージの後に
    if (extras.notes) {
      const notes = el('div', 'notes extras');
      const pre = el('pre', 'body');
      pre.append(...richText(extras.notes.replace(/\s+$/, '')));
      notes.append(el('div', 'notes-head', 'Notes'), pre);
      const after = head.querySelector('.selection-note') ?? head.querySelector('.trailers') ?? head.querySelector('pre.body') ?? meta;
      after.after(notes);
    }
  }

  function contentsFetched(oid: string, path: string | undefined, error: string | undefined): void {
    if (oid !== shownOid) return;
    if (path !== undefined) {
      allFiles?.contentsFetched(path, error);
      return;
    }
    // 取れたときは、拡張ホストが詳細（か選んだ行の差分）を送り直して帯を作り直す
    if (error !== undefined) band?.failed(error);
  }

  function clear(): void {
    shownOid = undefined;
    meta = undefined;
    place([]);
    fileSections('', [], false);
    scroller.replaceChildren(el('div', 'empty', 'Select a commit to see its changes.'));
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
    contentsFetched,
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
