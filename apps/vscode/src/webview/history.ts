/**
 * メインビューの History 画面。左にコミットの一覧とグラフ、右に選んだコミットの詳細を出す。
 * 一覧は見えている行だけを描く仮想スクロールで、グラフは行ごとの canvas に描く。
 * git で読むときは、末尾に近づいたら次のページを要求する。daemon の History は行の数が分かっているので、見えている
 * ところのまだない行をまとめて要求する（離れた行へもすぐ移れる）。
 */
import type { FileDiff } from '../diff/fileDiff.ts';
import { type LaneMetrics, lanePositions } from '../history/lanes.ts';
import type { CommitDetails, MessageEdit, Person, RowData, Scope, SearchState } from '../history/protocol.ts';
import type { HostMessage } from '../view/protocol.ts';
import { el, formatDate, getState, post, updateState, type View } from './api.ts';
import { renderDiffBody } from './diffView.ts';

const ROW_HEIGHT = 24;
const LANE_WIDTH = 14;
/** グラフの列の幅の上限。線が多い履歴でも、文字を押し出さない（アプリと同じ） */
const GRAPH_MAX_WIDTH = 120;
/** グラフの左右の余白の和 */
const GRAPH_INSET = 4;
const LANES: LaneMetrics = {
  full: LANE_WIDTH,
  thin: 3,
  minFull: 7,
  minThin: 0,
  available: GRAPH_MAX_WIDTH - GRAPH_INSET,
};
/** 見えている範囲の外に余分に描く行数 */
const OVERSCAN = 8;
/** 末尾からこの行数以内に来たら次を読む */
const PREFETCH = 50;
/** daemon の History で、1 回に頼む行の数（拡張ホストの PAGE と同じ） */
const CHUNK = 200;
const PALETTE = [
  '--vscode-charts-blue',
  '--vscode-charts-green',
  '--vscode-charts-orange',
  '--vscode-charts-purple',
  '--vscode-charts-red',
  '--vscode-charts-yellow',
  '--vscode-terminal-ansiCyan',
  '--vscode-terminal-ansiMagenta',
];

/** グラフの列の幅と、レーンごとの中心の横位置（左の余白を除く） */
interface GraphColumn {
  width: number;
  centers: readonly number[];
}

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

export function createHistoryView(): View {
  const root = el('div', 'view history');
  const toolbar = el('div', 'toolbar');
  const scopeSelect = el('select');
  scopeSelect.setAttribute('aria-label', 'Scope');
  for (const [value, label] of [
    ['head', 'Current Branch'],
    ['all', 'All Branches'],
  ] as const) {
    const option = el('option', undefined, label);
    option.value = value;
    scopeSelect.append(option);
  }
  // ナビでブランチなどを選んだときは、その ref の選択肢を足す
  const refOption = el('option');
  function setScope(scope: Scope): void {
    if (scope.startsWith('ref:')) {
      refOption.value = scope;
      refOption.textContent = scope.slice('ref:'.length).replace(/^refs\/(heads|remotes|tags)\//, '');
      if (!refOption.parentElement) scopeSelect.append(refOption);
    } else {
      refOption.remove();
    }
    scopeSelect.value = scope;
  }
  // 検索（daemon の History のときだけ）。構文はコアのもの
  const search = el('div', 'search');
  search.hidden = true;
  const searchInput = el('input');
  searchInput.type = 'search';
  searchInput.placeholder = 'Search';
  searchInput.setAttribute('aria-label', 'Search History');
  searchInput.title =
    'Words, author:, subject:, commit:, path:, contents:, after:, before:, AND, OR and ( ).\nEnter: next match, Shift+Enter: previous match, Escape: clear';
  const searchCount = el('span', 'search-count');
  const previousMatch = el('button', 'icon-button', '∧');
  previousMatch.title = 'Previous Match (Shift+Enter)';
  const nextMatch = el('button', 'icon-button', '∨');
  nextMatch.title = 'Next Match (Enter)';
  const matchingOnlyLabel = el('label', 'matching-only');
  const matchingOnly = el('input');
  matchingOnly.type = 'checkbox';
  matchingOnlyLabel.append(matchingOnly, 'Matching Only');
  matchingOnlyLabel.title = 'Show only the matching commits';
  search.append(searchInput, searchCount, previousMatch, nextMatch, matchingOnlyLabel);
  const status = el('span', 'status');
  toolbar.append(scopeSelect, search, status);
  const body = el('div', 'split');
  const list = el('div', 'list');
  list.tabIndex = 0;
  const spacer = el('div', 'spacer');
  list.append(spacer);
  const splitter = el('div', 'splitter');
  splitter.setAttribute('role', 'separator');
  const detailsPane = el('aside', 'details');
  body.append(list, splitter, detailsPane);
  root.append(toolbar, body);

  /** 行。daemon の History では行の数だけの長さで、まだ届いていない行は空 */
  let rows: (RowData | undefined)[] = [];
  /** daemon の History（行の数が分かっていて、飛び飛びに頼む） */
  let sparse = false;
  /** 頼んで届いていない、daemon の History の区切り（CHUNK 行ずつの始め） */
  let pendingChunks = new Set<number>();
  let done = false;
  let requested = false;
  let selected = -1;
  /** 検索の様子と、一致した行（一致だけの History では空。すべての行が一致） */
  let searchState: SearchState | undefined;
  let matchSet = new Set<number>();
  /** 最後に頼んだ問い合わせ。同じなら Enter で次の一致へ */
  let submitted = '';
  let colors: string[] = [];
  let frame = 0;

  function resolveColors(): void {
    const style = getComputedStyle(document.body);
    colors = PALETTE.map((v) => style.getPropertyValue(v).trim() || '#888');
  }

  function scheduleRender(): void {
    if (!frame) {
      frame = requestAnimationFrame(() => {
        frame = 0;
        render();
      });
    }
  }

  function render(): void {
    spacer.style.height = `${rows.length * ROW_HEIGHT}px`;
    const first = Math.max(0, Math.floor(list.scrollTop / ROW_HEIGHT) - OVERSCAN);
    const last = Math.min(rows.length, Math.ceil((list.scrollTop + list.clientHeight) / ROW_HEIGHT) + OVERSCAN);
    // レーンは描く範囲に合わせて並べ（点のあるレーンは広く、線だけのレーンは細く）、列は上限までに収める。
    // まだ届いていない行は飛ばす
    const lanes = lanePositions(
      rows.slice(first, last).map((r) => r?.graph),
      LANES,
    );
    const graph = {
      width: Math.min(GRAPH_MAX_WIDTH, GRAPH_INSET + Math.max(lanes.width, LANE_WIDTH)),
      centers: lanes.centers,
    };
    const elements: HTMLElement[] = [];
    for (let i = first; i < last; i++) {
      const row = rows[i];
      if (row) {
        elements.push(renderRow(row, i, graph));
      } else if (sparse) {
        // まだ届いていない行は、その区切りを頼む
        const chunk = Math.floor(i / CHUNK) * CHUNK;
        if (!pendingChunks.has(chunk)) {
          pendingChunks.add(chunk);
          post({ type: 'range', start: chunk, count: CHUNK });
        }
      }
    }
    spacer.replaceChildren(...elements);

    if (!done && !requested && last >= rows.length - PREFETCH) {
      requested = true;
      post({ type: 'more' });
    }
    status.textContent = done ? `${rows.length} commits` : `${rows.length}+ commits`;
  }

  function renderRow(row: RowData, index: number, graph: GraphColumn): HTMLElement {
    const el = document.createElement('div');
    el.className = index === selected ? 'row selected' : 'row';
    if (matchSet.has(index)) el.classList.add('match');
    el.style.top = `${index * ROW_HEIGHT}px`;
    el.dataset.index = String(index);
    // Webview の右クリックメニュー（package.json の webview/context）に渡す情報
    el.dataset.vscodeContext = JSON.stringify({
      webviewSection: 'commit',
      oid: row.oid,
      commitEditable: row.editable,
      commitCanSquash: row.canSquash,
      preventDefaultContextMenuItems: true,
    });
    // 編集できるコミットはドラッグで並べ替えられる
    if (row.editable) {
      el.draggable = true;
    }

    el.append(drawGraph(row, graph));
    const text = document.createElement('div');
    text.className = 'text';
    for (const ref of row.refs) {
      const badge = document.createElement('span');
      badge.className = `ref ${ref.kind}${ref.current ? ' current' : ''}`;
      badge.textContent = ref.name;
      text.append(badge);
    }
    const subject = document.createElement('span');
    subject.className = 'subject';
    subject.textContent = row.subject;
    text.append(subject);
    el.append(text);

    for (const [cls, value] of [
      ['author', row.author],
      ['date', formatDate(row.date)],
      ['oid', row.oid.slice(0, 8)],
    ] as const) {
      const span = document.createElement('span');
      span.className = cls;
      span.textContent = value;
      el.append(span);
    }
    return el;
  }

  function drawGraph(row: RowData, { width, centers }: GraphColumn): HTMLCanvasElement {
    const canvas = document.createElement('canvas');
    const ratio = window.devicePixelRatio || 1;
    canvas.width = width * ratio;
    canvas.height = ROW_HEIGHT * ratio;
    canvas.style.width = `${width}px`;
    canvas.style.height = `${ROW_HEIGHT}px`;
    const ctx = canvas.getContext('2d')!;
    ctx.scale(ratio, ratio);
    ctx.lineWidth = 1.5;
    const x = (col: number) => GRAPH_INSET / 2 + (centers[col] ?? col * LANE_WIDTH + LANE_WIDTH / 2);
    const y = (pos: number) => (pos * ROW_HEIGHT) / 2;
    const { segs } = row.graph;
    for (let i = 0; i < segs.length; i += 5) {
      const [x1, y1, x2, y2, color] = segs.slice(i, i + 5) as [number, number, number, number, number];
      ctx.strokeStyle = colors[color % colors.length]!;
      ctx.beginPath();
      ctx.moveTo(x(x1), y(y1));
      if (x1 === x2) {
        ctx.lineTo(x(x2), y(y2));
      } else {
        // 列をまたぐ線はなめらかに曲げる
        const mid = (y(y1) + y(y2)) / 2;
        ctx.bezierCurveTo(x(x1), mid, x(x2), mid, x(x2), y(y2));
      }
      ctx.stroke();
    }
    ctx.fillStyle = colors[row.graph.color % colors.length]!;
    ctx.beginPath();
    ctx.arc(x(row.graph.col), y(1), 4, 0, Math.PI * 2);
    ctx.fill();
    return canvas;
  }

  let selectTimer = 0;
  let shownOid: string | undefined;
  /** History を切り替えたあと、最初の行が届いたら選んだ行を見せる */
  let revealOnRows = false;

  function select(index: number, reveal: boolean): void {
    if (index < 0 || index >= rows.length) {
      return;
    }
    selected = index;
    requestDetails();
    if (reveal) {
      const top = index * ROW_HEIGHT;
      const far = top + ROW_HEIGHT < list.scrollTop - list.clientHeight || top > list.scrollTop + 2 * list.clientHeight;
      if (far) {
        // 離れた行（検索の一致など）は、真ん中に出す
        spacer.style.height = `${rows.length * ROW_HEIGHT}px`;
        list.scrollTop = top - (list.clientHeight - ROW_HEIGHT) / 2;
      } else if (top < list.scrollTop) {
        list.scrollTop = top;
      } else if (top + ROW_HEIGHT > list.scrollTop + list.clientHeight) {
        list.scrollTop = top + ROW_HEIGHT - list.clientHeight;
      }
    }
    scheduleRender();
  }

  /** 一致した行（一致だけの History では、すべての行） */
  function matchRows(): readonly number[] | undefined {
    return searchState && !searchState.error && !searchState.matchingOnly ? searchState.rows : undefined;
  }

  /** 次（step が 1）か前（-1）の一致へ。今選んでいる行の下（上）で、いちばん近いもの。端では反対の端へ */
  function moveMatch(step: number): void {
    if (searchState?.matchingOnly) {
      if (rows.length > 0) select((selected + step + rows.length) % rows.length, true);
      return;
    }
    const matches = matchRows();
    if (!matches || matches.length === 0) {
      return;
    }
    let target: number | undefined;
    if (step > 0) {
      target = matches.find((r) => r > selected) ?? matches[0];
    } else {
      for (let i = matches.length - 1; i >= 0; i--) {
        if (matches[i]! < selected) {
          target = matches[i];
          break;
        }
      }
      target ??= matches[matches.length - 1];
    }
    if (target !== undefined) select(target, true);
    showSearchCount();
  }

  /** 件数の欄: 探している途中なら分かった数と割合、終われば何番目か */
  function showSearchCount(): void {
    const s = searchState;
    searchCount.title = '';
    searchCount.classList.toggle('error', !!s?.error);
    if (!s || !s.query) {
      searchCount.textContent = '';
    } else if (s.error) {
      searchCount.textContent = s.error;
      searchCount.title = s.error;
    } else if (s.searching) {
      const found = s.rows.length > 0 ? `${s.rows.length.toLocaleString()} found` : 'Searching…';
      searchCount.textContent = s.done > 0 ? `${found} (${Math.floor(s.done * 100)}%)` : found;
    } else {
      const total = s.matchingOnly ? rows.length : s.rows.length;
      if (total === 0) {
        searchCount.textContent = 'No matches';
      } else {
        const index = s.matchingOnly ? selected : s.rows.indexOf(selected);
        searchCount.textContent =
          index >= 0 ? `${(index + 1).toLocaleString()} of ${total.toLocaleString()}` : `${total.toLocaleString()} found`;
      }
    }
    const navigable = !!s && !s.error && !s.searching && (s.matchingOnly ? rows.length > 0 : s.rows.length > 0);
    previousMatch.disabled = !navigable;
    nextMatch.disabled = !navigable;
  }

  function submitSearch(step: number): void {
    const query = searchInput.value.trim();
    if (query && query === submitted && searchState && !searchState.searching && !searchState.error) {
      moveMatch(step);
      return;
    }
    submitted = query;
    post({ type: 'search', query, matchingOnly: matchingOnly.checked, selected: Math.max(0, selected) });
  }

  function onSearchState(state: SearchState): void {
    // 探し終えたところか（途中の答えのあと、または一致だけの切り替え）
    const finished =
      !state.searching && (!searchState || searchState.searching || searchState.query !== state.query);
    searchState = state;
    if (!state.query) submitted = '';
    matchSet = state.matchingOnly ? new Set() : new Set(state.rows);
    // 探し終えたら、選んでいる行から下で、いちばん近い一致へ
    if (finished && !state.error && !state.matchingOnly && state.rows.length > 0 && !matchSet.has(selected)) {
      moveMatch(1);
    }
    showSearchCount();
    scheduleRender();
  }

  /** メッセージの編集欄。保存は Ctrl/Cmd+Enter、取り消しは Esc */
  function editMessage(oid: string, kind: MessageEdit, title: string, message: string): void {
    const overlay = document.createElement('div');
    overlay.className = 'overlay';
    const box = document.createElement('div');
    box.className = 'editor';
    const heading = document.createElement('div');
    heading.className = 'heading';
    heading.textContent = title;
    const textarea = document.createElement('textarea');
    textarea.value = message;
    textarea.spellcheck = false;
    const buttons = document.createElement('div');
    buttons.className = 'buttons';
    const save = document.createElement('button');
    save.textContent = 'Save';
    const cancel = document.createElement('button');
    cancel.textContent = 'Cancel';
    cancel.className = 'secondary';
    buttons.append(cancel, save);
    box.append(heading, textarea, buttons);
    overlay.append(box);
    document.body.append(overlay);
    textarea.focus();
    textarea.setSelectionRange(
      0,
      textarea.value.indexOf('\n') < 0 ? textarea.value.length : textarea.value.indexOf('\n'),
    );

    const close = () => {
      overlay.remove();
      list.focus();
    };
    const submit = () => {
      if (!textarea.value.trim()) {
        textarea.focus();
        return;
      }
      post({ type: 'saveMessage', oid, kind, message: textarea.value });
      close();
    };
    save.addEventListener('click', submit);
    cancel.addEventListener('click', close);
    textarea.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        submit();
      } else if (e.key === 'Escape') {
        e.preventDefault();
        close();
      }
    });
  }

  /** 選択が落ち着いてから詳細を求める。矢印キーを押し続けても要求が溜まらないように */
  function requestDetails(): void {
    const oid = rows[selected]?.oid;
    if (!oid || oid === shownOid) {
      return;
    }
    if (searchState?.query) showSearchCount();
    clearTimeout(selectTimer);
    selectTimer = window.setTimeout(() => post({ type: 'select', oid }), 60);
  }

  /** 詳細に並べたファイルごとの差分の場所 */
  let sections = new Map<string, HTMLElement>();
  let requestedDiffs = new Set<string>();
  let diffObserver: IntersectionObserver | undefined;

  function showDetails(d: CommitDetails): void {
    if (d.oid !== rows[selected]?.oid) {
      return;
    }
    shownOid = d.oid;
    const [subject = '', ...rest] = d.message.replace(/\n+$/, '').split('\n');
    const body = rest.join('\n').replace(/^\n+/, '');

    const head = el('div', 'subject');
    for (const ref of d.refs) {
      head.append(el('span', `ref ${ref.kind}${ref.current ? ' current' : ''}`, ref.name));
    }
    head.append(el('span', '', subject));

    const meta = el('dl', 'meta');
    const add = (label: string, value: Node | string) => {
      const dd = el('dd');
      dd.append(value);
      meta.append(el('dt', '', label), dd);
    };
    add('Author', withRecorded(`${d.author.name} <${d.author.email}>`, d.recordedAuthor));
    add('Date', formatDate(d.author.date));
    if (d.committer.name !== d.author.name || d.committer.date !== d.author.date) {
      add('Committer', withRecorded(`${d.committer.name}  ${formatDate(d.committer.date)}`, d.recordedCommitter));
    }
    add('Commit', el('code', '', d.oid));
    if (d.parents.length > 0) {
      const parents = el('span');
      for (const p of d.parents) {
        const link = el('a', 'parent', p.slice(0, 8));
        link.href = '#';
        link.title = p;
        link.addEventListener('click', (e) => {
          e.preventDefault();
          const index = rows.findIndex((r) => r?.oid === p);
          if (index >= 0) select(index, true);
        });
        parents.append(link, ' ');
      }
      add(d.parents.length > 1 ? 'Parents' : 'Parent', parents);
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
    for (const f of d.files) {
      const row = el('div', 'file');
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
      { root: detailsPane, rootMargin: '200px' },
    );
    requestedDiffs = new Set();
    sections = new Map();
    const diffs = el('div', 'commit-diffs');
    for (const f of d.files) {
      const section = el('section', 'file-diff');
      section.dataset.path = f.path;
      const sectionHead = el('div', 'diff-head');
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

    const parts: Node[] = [head, meta];
    if (body) parts.push(el('pre', 'body', body));
    parts.push(files, diffs);
    detailsPane.replaceChildren(...parts);
    detailsPane.scrollTop = 0;
  }

  function showCommitDiff(oid: string, path: string, diff: FileDiff): void {
    const section = sections.get(path);
    if (oid !== shownOid || !section) {
      return;
    }
    section.replaceChildren(section.firstElementChild!, ...renderDiffBody(diff));
  }

  function clearDetails(): void {
    shownOid = undefined;
    detailsPane.replaceChildren(el('div', 'empty', 'コミットを選ぶと、ここに詳細が出ます'));
  }

  // 左右の境目をドラッグして、詳細の幅を変える。幅は覚えておく
  function setDetailsWidth(width: number): void {
    const max = Math.max(240, body.clientWidth - 240);
    const clamped = Math.min(max, Math.max(240, width));
    detailsPane.style.width = `${clamped}px`;
    updateState({ detailsWidth: clamped });
  }
  splitter.addEventListener('mousedown', (e) => {
    e.preventDefault();
    const onMove = (m: MouseEvent) => setDetailsWidth(body.getBoundingClientRect().right - m.clientX);
    const onUp = () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
      document.body.classList.remove('resizing');
      scheduleRender();
    };
    document.body.classList.add('resizing');
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  });

  function rowIndex(event: Event): number | undefined {
    const row = (event.target as HTMLElement).closest<HTMLElement>('.row');
    return row ? Number(row.dataset.index) : undefined;
  }

  list.addEventListener('scroll', scheduleRender);
  window.addEventListener('resize', scheduleRender);
  list.addEventListener('mousedown', (e) => {
    const index = rowIndex(e);
    if (index !== undefined) select(index, false);
  });
  list.addEventListener('dblclick', (e) => {
    const index = rowIndex(e);
    if (index !== undefined) post({ type: 'open', oid: rows[index]!.oid });
  });
  list.addEventListener('keydown', (e) => {
    const page = Math.max(1, Math.floor(list.clientHeight / ROW_HEIGHT) - 1);
    const moves: Record<string, number> = { ArrowDown: 1, ArrowUp: -1, PageDown: page, PageUp: -page };
    if (e.key in moves) {
      e.preventDefault();
      select(Math.min(rows.length - 1, Math.max(0, selected + moves[e.key]!)), true);
    } else if (e.key === 'Enter' && rows[selected]) {
      post({ type: 'open', oid: rows[selected]!.oid });
    } else if (e.key === 'F3' && searchState?.query) {
      e.preventDefault();
      moveMatch(e.shiftKey ? -1 : 1);
    }
  });
  scopeSelect.addEventListener('change', () => post({ type: 'scope', scope: scopeSelect.value as Scope }));
  searchInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      submitSearch(e.shiftKey ? -1 : 1);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      searchInput.value = '';
      submitSearch(1);
      list.focus();
    }
  });
  // × で消したとき
  searchInput.addEventListener('search', () => {
    if (!searchInput.value && submitted) submitSearch(1);
  });
  previousMatch.addEventListener('click', () => moveMatch(-1));
  nextMatch.addEventListener('click', () => moveMatch(1));
  matchingOnly.addEventListener('change', () => {
    if (submitted) {
      post({ type: 'search', query: submitted, matchingOnly: matchingOnly.checked, selected: Math.max(0, selected) });
    }
  });

  // ドラッグでの並べ替え。編集できるコミットの上にだけ落とせる
  let dragging: RowData | undefined;
  let dropTarget: HTMLElement | undefined;
  const clearDropTarget = () => {
    dropTarget?.classList.remove('drop-above', 'drop-below');
    dropTarget = undefined;
  };
  list.addEventListener('dragstart', (e) => {
    const index = rowIndex(e);
    dragging = index === undefined ? undefined : rows[index];
    if (!dragging?.editable || !e.dataTransfer) {
      e.preventDefault();
      return;
    }
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', dragging.oid);
  });
  list.addEventListener('dragover', (e) => {
    const index = rowIndex(e);
    const target = index === undefined ? undefined : rows[index];
    if (!dragging || target?.chain === undefined || target.oid === dragging.oid) {
      clearDropTarget();
      return;
    }
    e.preventDefault();
    const el = (e.target as HTMLElement).closest<HTMLElement>('.row')!;
    const above = isUpperHalf(e, el);
    clearDropTarget();
    dropTarget = el;
    el.classList.add(above ? 'drop-above' : 'drop-below');
  });
  list.addEventListener('dragleave', clearDropTarget);
  list.addEventListener('drop', (e) => {
    e.preventDefault();
    const index = rowIndex(e);
    const target = index === undefined ? undefined : rows[index];
    const el = dropTarget;
    clearDropTarget();
    if (!dragging || dragging.chain === undefined || target?.chain === undefined || !el) {
      return;
    }
    // 上半分なら target の上（より新しい側）、下半分なら下に置く
    const above = isUpperHalf(e, el);
    let to = above ? target.chain : target.chain + 1;
    // 自分より下に動かすときは、自分が抜けた分だけ詰まる
    if (to > dragging.chain) to -= 1;
    if (to !== dragging.chain) {
      post({ type: 'move', oid: dragging.oid, to });
    }
    dragging = undefined;
  });
  list.addEventListener('dragend', () => {
    dragging = undefined;
    clearDropTarget();
  });

  function isUpperHalf(e: MouseEvent, el: HTMLElement): boolean {
    const rect = el.getBoundingClientRect();
    return e.clientY < rect.top + rect.height / 2;
  }

  // テーマが変わったら色を取り直す
  new MutationObserver(() => {
    resolveColors();
    scheduleRender();
  }).observe(document.body, { attributes: true, attributeFilter: ['class'] });

  function onMessage(message: HostMessage): void {
    switch (message.type) {
      case 'reset':
        rows = [];
        sparse = false;
        pendingChunks = new Set();
        done = false;
        requested = true;
        matchSet = new Set();
        setScope(message.scope);
        if (message.select !== undefined) {
          // 一致だけの History との切り替え: 同じコミットを選んだまま、行が届いたら詳細を出し直す
          selected = message.select;
          shownOid = undefined;
          revealOnRows = true;
        } else {
          selected = -1;
          clearDetails();
          list.scrollTop = 0;
        }
        break;
      case 'rows':
        if (message.total !== undefined) {
          if (!sparse || rows.length !== message.total) {
            rows = new Array<RowData | undefined>(message.total);
            sparse = true;
          }
          message.rows.forEach((row, i) => (rows[message.start + i] = row));
          pendingChunks.delete(message.start);
        } else {
          rows.splice(message.start, rows.length - message.start, ...message.rows);
        }
        done = message.done;
        requested = false;
        if (selected < 0 && rows.length > 0) {
          selected = 0;
        }
        if (revealOnRows && selected >= 0) {
          revealOnRows = false;
          select(Math.min(selected, rows.length - 1), true);
        }
        requestDetails();
        break;
      case 'searchable':
        search.hidden = !message.searchable;
        break;
      case 'search':
        onSearchState(message.state);
        break;
      case 'busy':
        root.classList.toggle('busy', message.busy);
        break;
      case 'details':
        showDetails(message.details);
        return;
      case 'commitDiff':
        showCommitDiff(message.oid, message.path, message.diff);
        return;
      case 'editMessage':
        editMessage(message.oid, message.kind, message.title, message.message);
        return;
      default:
        return;
    }
    scheduleRender();
  }

  resolveColors();
  clearDetails();
  const savedWidth = getState().detailsWidth;
  if (savedWidth) detailsPane.style.width = `${savedWidth}px`;

  return {
    root,
    onMessage,
    onShow() {
      scheduleRender();
      list.focus();
    },
  };
}
