/**
 * コミットの詳細の欄（History と Reflog の右側）。メッセージ・作者・親と、変更したファイルの一覧、ファイルごとの
 * 差分を出す。差分は画面に入ったものから順に拡張ホストに求める
 */
import type { FileDiff } from '../diff/fileDiff.ts';
import type { CommitDetails, DetailsExtras, Person } from '../history/protocol.ts';
import { el, formatDate, getState, post, updateState } from './api.ts';
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
  /** `focus` があれば、そのファイルを選んでその差分の位置に移る（File History） */
  show(details: CommitDetails, focus?: string): void;
  showDiff(oid: string, path: string, diff: FileDiff): void;
  /** 詳細の残り（署名・子のコミット）。出しているコミットのものなら足す */
  showExtras(extras: DetailsExtras): void;
  clear(): void;
  /** 次に届いた詳細を、同じコミットでも出し直す */
  forget(): void;
}

/** onParent があれば、親の ID を押すとそれを呼ぶ（History でその行に移る） */
export function createDetailsPane(onParent?: (oid: string) => void): DetailsPane {
  const root = el('aside', 'details');
  let shownOid: string | undefined;
  /** 詳細に並べたファイルごとの差分の場所 */
  let sections = new Map<string, HTMLElement>();
  let requestedDiffs = new Set<string>();
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

  function show(d: CommitDetails, focus?: string): void {
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
    const rows = new Map<string, HTMLElement>();
    for (const f of d.files) {
      const row = el('div', 'file');
      rows.set(f.path, row);
      // 右クリックのメニュー（package.json の webview/context）に渡す情報
      row.dataset.vscodeContext = fileContext(d.oid, f.path);
      row.title = f.origPath ? `${f.origPath} → ${f.path}` : f.path;
      row.append(el('span', `status s-${f.status}`, f.status));
      const slash = f.path.lastIndexOf('/');
      row.append(el('span', 'name', f.path.slice(slash + 1)));
      if (slash >= 0) row.append(el('span', 'dir', f.path.slice(0, slash)));
      // その差分の位置へ移る
      row.addEventListener('click', () => {
        for (const other of files.querySelectorAll('.file.active')) other.classList.remove('active');
        row.classList.add('active');
        sections.get(f.path)?.scrollIntoView({ block: 'start' });
      });
      files.append(row);
    }

    // ファイルごとの差分。画面に入ったものから順に求める
    diffObserver?.disconnect();
    diffObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const path = (entry.target as HTMLElement).dataset.path!;
          if (entry.isIntersecting && !requestedDiffs.has(path)) {
            requestedDiffs.add(path);
            post({ type: 'commitDiff', oid: d.oid, path });
          }
        }
      },
      { root, rootMargin: '200px' },
    );
    requestedDiffs = new Set();
    sections = new Map();
    const diffs = el('div', 'commit-diffs');
    for (const f of d.files) {
      const section = el('section', 'file-diff');
      section.dataset.path = f.path;
      const sectionHead = el('div', 'diff-head');
      sectionHead.dataset.vscodeContext = fileContext(d.oid, f.path);
      sectionHead.append(el('span', `status s-${f.status}`, f.status));
      sectionHead.append(el('span', 'path', f.origPath ? `${f.origPath} → ${f.path}` : f.path));
      const open = el('button', 'secondary', 'Open');
      open.title = 'エディタで差分を開く';
      open.addEventListener('click', () => post({ type: 'openFile', oid: d.oid, path: f.path }));
      sectionHead.append(open);
      section.append(sectionHead, el('div', 'empty', '読み込み中…'));
      diffs.append(section);
      sections.set(f.path, section);
      diffObserver.observe(section);
    }

    const parts: Node[] = [head, list];
    if (body) parts.push(el('pre', 'body', body));
    parts.push(files, diffs);
    root.replaceChildren(...parts);
    root.scrollTop = 0;
    const section = focus ? sections.get(focus) : undefined;
    if (focus && section) {
      rows.get(focus)?.classList.add('active');
      section.scrollIntoView({ block: 'start' });
    }
  }

  function showDiff(oid: string, path: string, diff: FileDiff): void {
    const section = sections.get(path);
    if (oid !== shownOid || !section) {
      return;
    }
    section.replaceChildren(section.firstElementChild!, ...renderDiffBody(diff));
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
