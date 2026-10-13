/**
 * メインビュー。左のナビで画面を切り替え、右に出す。
 * 切り替えはこの中だけで行い、拡張ホストとの往復を待たない。
 */
import type { HostMessage, ViewName } from '../view/protocol.ts';
import { el, getState, post, updateState, type View } from './api.ts';
import { createHistoryView } from './history.ts';
import { createNav } from './nav.ts';
import { createWorkingCopyView } from './workingCopy.ts';

const views: Record<ViewName, View> = {
  workingCopy: createWorkingCopyView(),
  history: createHistoryView(),
};

const nav = createNav(
  (view) => show(view),
  (ref) => {
    // ブランチなどを選んだら、その履歴を History に出す
    show('history', ref);
    post({ type: 'showRef', ref });
  },
);

const app = el('div', 'app');
const content = el('div', 'content');
for (const view of Object.values(views)) {
  content.append(view.root);
}
app.append(nav.root, content);
document.body.append(app);

let current: ViewName | undefined;

/** 画面を切り替える。ref を渡すと、ナビではその ref を選んだ状態にする */
function show(view: ViewName, ref?: string): void {
  nav.setActive(ref ? { ref } : { view });
  if (view === current) {
    return;
  }
  current = view;
  for (const [name, v] of Object.entries(views)) {
    v.root.classList.toggle('active', name === view);
  }
  updateState({ view });
  post({ type: 'view', view });
  views[view].onShow();
}

window.addEventListener('message', (event: MessageEvent<HostMessage>) => {
  const message = event.data;
  switch (message.type) {
    case 'repo':
      nav.setRepo(message.repo);
      break;
    case 'refs':
      nav.setRefs(message.refs);
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
