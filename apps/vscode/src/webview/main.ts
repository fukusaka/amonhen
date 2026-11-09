/**
 * メインビュー。Working Copy・History・Reflog・Range Diff・File History・Blame などのどれかを出す。どれを出すかはサイドバーのナビ
 * （File History・Blame はファイルの右クリック）で選ぶ。
 * どの画面も作っておき、表示だけを切り替えるので、切り替えたときに作り直さない。
 */
import type { HostMessage, ViewName } from '../view/protocol.ts';
import { el, getState, post, updateState, type View } from './api.ts';
import { createBlameView } from './blame.ts';
import { setDetailsLayout } from './details.ts';
import { createFileHistoryView } from './fileHistory.ts';
import { createHistoryView } from './history.ts';
import { createRangeDiffView } from './rangeDiff.ts';
import { createReflogView } from './reflog.ts';
import { createSettingsView } from './settings.ts';
import { createWorkingCopyView } from './workingCopy.ts';

const views: Record<ViewName, View> = {
  workingCopy: createWorkingCopyView(),
  history: createHistoryView(),
  reflog: createReflogView(),
  rangeDiff: createRangeDiffView(),
  fileHistory: createFileHistoryView(),
  blame: createBlameView(),
  settings: createSettingsView(),
};

const content = el('div', 'content');
for (const view of Object.values(views)) {
  content.append(view.root);
}
document.body.append(content);

let current: ViewName | undefined;

function show(view: ViewName): void {
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
  if (message.type === 'show') {
    show(message.view);
    return;
  }
  if (message.type === 'detailsLayout') {
    setDetailsLayout(message.layout);
    return;
  }
  for (const v of Object.values(views)) {
    v.onMessage(message);
  }
});

show(getState().view ?? 'workingCopy');
post({ type: 'ready' });
