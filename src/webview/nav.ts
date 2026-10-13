/**
 * メインビューの左のナビ。ツールバー、画面の切り替え、ブランチ・リモート・タグ・stash の一覧。
 * ブランチはドラッグ＆ドロップで merge / rebase / push でき、選ぶとその履歴を History に出す。
 */
import type {
  DragSource,
  DropTarget,
  NavRef,
  NavRefs,
  RepoSummary,
  ToolbarCommand,
  ViewName,
} from '../view/protocol.ts';
import { el, getState, post, updateState } from './api.ts';

const VIEWS: { view: ViewName; label: string; icon: string }[] = [
  { view: 'workingCopy', label: 'Working Copy', icon: '●' },
  { view: 'history', label: 'History', icon: '◷' },
];

const TOOLBAR: { command: ToolbarCommand; label: string; title: string }[] = [
  { command: 'fetch', label: 'Fetch', title: 'すべてのリモートから取得する' },
  { command: 'pull', label: 'Pull', title: '現在のブランチに upstream を取り込む' },
  { command: 'push', label: 'Push', title: '現在のブランチを push する' },
  { command: 'undo', label: 'Undo', title: '直前の操作を戻す' },
];

type Section = 'branches' | 'remotes' | 'tags' | 'stashes';

const DRAG_TYPE = 'application/x-amonhen-ref';

export interface Nav {
  readonly root: HTMLElement;
  setRepo(repo: RepoSummary | undefined): void;
  setRefs(refs: NavRefs): void;
  /** 選んでいる画面、またはブランチなど */
  setActive(active: { view: ViewName } | { ref: string }): void;
}

export function createNav(onView: (view: ViewName) => void, onRef: (ref: string) => void): Nav {
  const root = el('nav', 'nav');
  const head = el('div', 'nav-head');
  const repoName = el('div', 'repo-name');
  const repoBranch = el('div', 'repo-branch');
  head.append(repoName, repoBranch);

  const toolbar = el('div', 'nav-toolbar');
  for (const { command, label, title } of TOOLBAR) {
    const b = el('button', 'secondary', label);
    b.title = title;
    b.addEventListener('click', () => post({ type: 'command', command }));
    toolbar.append(b);
  }

  const viewItems = new Map<ViewName, { item: HTMLElement; badge: HTMLElement }>();
  const views = el('div', 'nav-views');
  for (const { view, label, icon } of VIEWS) {
    const item = el('button', 'nav-item');
    item.type = 'button';
    const badge = el('span', 'badge');
    item.append(el('span', 'icon', icon), el('span', 'label', label), badge);
    item.addEventListener('click', () => onView(view));
    views.append(item);
    viewItems.set(view, { item, badge });
  }

  const refsBox = el('div', 'nav-refs');
  root.append(head, toolbar, views, refsBox);

  let refs: NavRefs = { branches: [], remotes: [] };
  let active: { view: ViewName } | { ref: string } = { view: 'workingCopy' };
  const collapsed = new Set<string>(getState().collapsed ?? ['remotes', 'tags', 'stashes']);

  function toggle(key: string): void {
    if (collapsed.has(key)) {
      collapsed.delete(key);
      if (key === 'tags' || key === 'stashes') post({ type: 'navExpand', section: key });
    } else {
      collapsed.add(key);
    }
    updateState({ collapsed: [...collapsed] });
    renderRefs();
  }

  function sectionHead(key: string, title: string, count?: number): HTMLElement {
    const h = el('button', 'nav-section');
    h.type = 'button';
    h.append(el('span', 'twisty', collapsed.has(key) ? '▸' : '▾'), el('span', 'label', title));
    if (count !== undefined) h.append(el('span', 'count', String(count)));
    h.addEventListener('click', () => toggle(key));
    return h;
  }

  function refItem(r: NavRef, kind: 'branch' | 'remoteBranch' | 'tag', remote?: string): HTMLElement {
    const item = el('div', 'nav-ref');
    item.classList.toggle('active', 'ref' in active && active.ref === r.ref);
    item.classList.toggle('current', !!r.current);
    item.title = r.upstream ? `${r.name} → ${r.upstream.replace(/^refs\/remotes\//, '')}` : r.name;
    item.draggable = true;
    item.dataset.vscodeContext = JSON.stringify({
      webviewSection: 'navRef',
      ref: r.ref,
      refKind: kind,
      current: !!r.current,
      preventDefaultContextMenuItems: true,
    });
    const icon = kind === 'tag' ? '⌑' : r.current ? '✓' : '⑂';
    // リモートブランチはリモート名を省いて出す
    const name = remote ? r.name.slice(remote.length + 1) : r.name;
    item.append(el('span', 'icon', icon), el('span', 'label', name));
    const sync: string[] = [];
    if (r.ahead) sync.push(`↑${r.ahead}`);
    if (r.behind) sync.push(`↓${r.behind}`);
    if (r.gone) sync.push('gone');
    if (sync.length) item.append(el('span', 'sync', sync.join(' ')));

    item.addEventListener('click', () => onRef(r.ref));
    if (kind !== 'tag') item.addEventListener('dblclick', () => post({ type: 'checkout', ref: r.ref }));
    item.addEventListener('dragstart', (e) => {
      const source: DragSource = { ref: r.ref, local: kind === 'branch', upstream: r.upstream };
      e.dataTransfer?.setData(DRAG_TYPE, JSON.stringify(source));
      e.dataTransfer!.effectAllowed = 'copyMove';
    });
    if (kind === 'branch') dropZone(item, { kind: 'branch', ref: r.ref }, r.ref);
    if (kind === 'remoteBranch') dropZone(item, { kind: 'remoteBranch', remote: remote!, ref: r.ref }, r.ref);
    return item;
  }

  /** ドロップを受け付ける。自分自身へのドロップは受けない */
  function dropZone(target: HTMLElement, drop: DropTarget, self?: string): void {
    target.addEventListener('dragover', (e) => {
      if (!e.dataTransfer?.types.includes(DRAG_TYPE)) return;
      e.preventDefault();
      target.classList.add('drop');
    });
    target.addEventListener('dragleave', () => target.classList.remove('drop'));
    target.addEventListener('drop', (e) => {
      e.preventDefault();
      target.classList.remove('drop');
      const data = e.dataTransfer?.getData(DRAG_TYPE);
      if (!data) return;
      const source = JSON.parse(data) as DragSource;
      if (source.ref !== self) post({ type: 'dropRef', source, target: drop });
    });
  }

  function renderRefs(): void {
    const parts: HTMLElement[] = [];
    const section = (key: Section, title: string, count: number | undefined, items: () => HTMLElement[]) => {
      parts.push(sectionHead(key, title, count));
      if (!collapsed.has(key)) parts.push(...items());
    };
    section('branches', 'Branches', refs.branches.length, () => refs.branches.map((b) => refItem(b, 'branch')));
    section('remotes', 'Remotes', refs.remotes.length, () =>
      refs.remotes.flatMap((remote) => {
        const key = `remote:${remote.name}`;
        const h = sectionHead(key, remote.name);
        h.classList.add('nested');
        dropZone(h, { kind: 'remote', remote: remote.name });
        return collapsed.has(key) ? [h] : [h, ...remote.branches.map((b) => refItem(b, 'remoteBranch', remote.name))];
      }),
    );
    section('tags', 'Tags', refs.tags?.length, () => (refs.tags ?? []).map((t) => refItem(t, 'tag')));
    section('stashes', 'Stashes', refs.stashes?.length, () =>
      (refs.stashes ?? []).map((s) => {
        const item = el('div', 'nav-ref stash');
        item.title = s.subject;
        item.append(el('span', 'icon', '≡'), el('span', 'label', s.subject));
        return item;
      }),
    );
    refsBox.replaceChildren(...parts);
  }

  // 開いた状態で始まる Tags / Stashes は最初から取得する
  for (const key of ['tags', 'stashes'] as const) {
    if (!collapsed.has(key)) post({ type: 'navExpand', section: key });
  }

  return {
    root,
    setRepo(repo) {
      repoName.textContent = repo?.name ?? 'No Repository';
      repoBranch.textContent = repo
        ? repo.sequence
          ? `${repo.sequence} in progress`
          : (repo.branch ?? 'detached HEAD')
        : '';
      repoBranch.classList.toggle('warning', !!repo?.sequence);
      viewItems.get('workingCopy')!.badge.textContent = repo?.changes ? String(repo.changes) : '';
    },
    setRefs(next) {
      refs = next;
      renderRefs();
    },
    setActive(next) {
      active = next;
      for (const [view, { item }] of viewItems) {
        item.classList.toggle('active', 'view' in active && active.view === view);
      }
      renderRefs();
    },
  };
}
