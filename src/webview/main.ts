/**
 * メインビュー。左のナビで画面を切り替え、右に出す。
 * 切り替えはこの中だけで行い、拡張ホストとの往復を待たない。
 */
import type { HostMessage, RepoSummary, ViewName } from '../view/protocol.ts';
import { el, getState, post, updateState, type View } from './api.ts';
import { createHistoryView } from './history.ts';

const NAV: { view: ViewName; label: string; icon: string }[] = [
  { view: 'workingCopy', label: 'Working Copy', icon: '●' },
  { view: 'history', label: 'History', icon: '◷' },
];

function createWorkingCopyView(): View {
  const root = el('div', 'view placeholder');
  root.append(el('p', undefined, 'Working Copy は次のステップでこの画面に移します。'));
  return { root, onMessage() {}, onShow() {} };
}

const views: Record<ViewName, View> = {
  workingCopy: createWorkingCopyView(),
  history: createHistoryView(),
};

const app = el('div', 'app');
const nav = el('nav', 'nav');
const repoName = el('div', 'repo-name');
const repoBranch = el('div', 'repo-branch');
const navHead = el('div', 'nav-head');
navHead.append(repoName, repoBranch);
nav.append(navHead);

const navItems = new Map<ViewName, { item: HTMLElement; badge: HTMLElement }>();
for (const { view, label, icon } of NAV) {
  const item = el('button', 'nav-item');
  item.type = 'button';
  const badge = el('span', 'badge');
  item.append(el('span', 'icon', icon), el('span', 'label', label), badge);
  item.addEventListener('click', () => show(view));
  nav.append(item);
  navItems.set(view, { item, badge });
}

const content = el('div', 'content');
for (const view of Object.values(views)) {
  content.append(view.root);
}
app.append(nav, content);
document.body.append(app);

let current: ViewName | undefined;

function show(view: ViewName): void {
  if (view === current) {
    return;
  }
  current = view;
  for (const [name, { item }] of navItems) {
    item.classList.toggle('active', name === view);
  }
  for (const [name, v] of Object.entries(views)) {
    v.root.classList.toggle('active', name === view);
  }
  updateState({ view });
  post({ type: 'view', view });
  views[view].onShow();
}

function showRepo(repo: RepoSummary | undefined): void {
  repoName.textContent = repo?.name ?? 'No Repository';
  repoBranch.textContent = repo ? (repo.sequence ? `${repo.sequence} in progress` : (repo.branch ?? 'detached HEAD')) : '';
  repoBranch.classList.toggle('warning', !!repo?.sequence);
  const changes = navItems.get('workingCopy')!.badge;
  changes.textContent = repo?.changes ? String(repo.changes) : '';
}

window.addEventListener('message', (event: MessageEvent<HostMessage>) => {
  const message = event.data;
  switch (message.type) {
    case 'repo':
      showRepo(message.repo);
      break;
    case 'show':
      show(message.view);
      break;
    default:
      for (const v of Object.values(views)) {
        v.onMessage(message);
      }
  }
});

show(getState().view ?? 'workingCopy');
post({ type: 'ready' });
