/**
 * メインビューの Repository Settings 画面（アプリの Repository Settings と同じ形）。左にページの並びと、このリポジトリを
 * 扱う git・リポジトリの形、右に選んだページの行。行は、質問とその下の説明、効いている値の欄、このリポジトリが
 * 上書きしている間だけ ↩︎、設定元、ⓘ（キーと値とファイル、上書きしなければ効く値）
 */
import { ConfigView, normalizedBool, pullMethod, pullWrites, type PullMethod } from '../settings/model.ts';
import { PAGES, type SettingRow } from '../settings/pages.ts';
import type { FetchingAction, SettingsFacts } from '../settings/protocol.ts';
import type { HostMessage } from '../view/protocol.ts';
import { el, getState, post, updateState, type View } from './api.ts';

const PULL_LABELS: Record<PullMethod, string> = { merge: 'Merge', rebase: 'Rebase', ffOnly: 'Fast-forward' };

export function createSettingsView(): View {
  const root = el('div', 'view settings');
  const nav = el('nav', 'settings-nav');
  const pages = el('div', 'settings-pages');
  const gitLine = el('div', 'settings-git');
  nav.append(pages, gitLine);
  const content = el('div', 'settings-content');
  root.append(nav, content);

  let page = PAGES.find((p) => p.id === getState().settingsPage) ?? PAGES[0]!;
  let config = new ConfigView([]);
  let facts: SettingsFacts = { updateRefs: true, rerereCache: false, form: [], branches: [], exclude: '' };
  /** 書きかけの exclude（読み直しで消さない） */
  let excludeDraft: string | undefined;
  let error: string | undefined;

  const set = (writes: [string, string | null][]) => post({ type: 'settingsSet', writes });

  function renderNav(): void {
    pages.replaceChildren(
      ...PAGES.map((p) => {
        const item = el('button', p === page ? 'settings-page selected' : 'settings-page', p.title);
        item.addEventListener('click', () => {
          page = p;
          updateState({ settingsPage: p.id });
          render();
        });
        return item;
      }),
    );
    // このリポジトリを動かしている git（変えるのはその機械の設定。ここでは見せるだけ）と、リポジトリの形
    const version = el('div', 'settings-git-version', `git ${facts.gitVersion ?? '?'}`);
    version.title = 'The git that runs this repository';
    gitLine.replaceChildren(
      version,
      ...facts.form.map((f) => {
        const item = el('div', 'settings-form', f.title);
        item.title = f.help;
        return item;
      }),
    );
  }

  /** 行の共通の形。`keys` の先頭のキーの設定元を出し、↩︎ はすべての上書きを消す */
  function row(question: string, help: string | undefined, keys: string[], control: HTMLElement, describe: (v: string) => string, disabled?: string): HTMLElement {
    const div = el('div', 'setting-row');
    const head = el('div', 'setting-head');
    head.append(el('span', 'setting-question', question), control);
    const overriding = keys.some((k) => config.local(k) !== undefined);
    const revert = el('button', 'icon-button setting-revert', '↩︎');
    revert.title = 'Stop overriding here. The value above takes over.';
    revert.style.visibility = overriding ? 'visible' : 'hidden';
    revert.addEventListener('click', () => set(keys.filter((k) => config.local(k) !== undefined).map((k) => [k, null])));
    const origin = config.origin(keys[0]!);
    const source = el('span', 'setting-source', origin.label);
    source.title = origin.file ?? 'Nothing is set anywhere, so git’s own default applies.';
    const info = el('span', 'setting-info', 'ⓘ');
    info.title = keys
      .map((k) => {
        const e = config.effective(k);
        const lines = e ? [`${k} = ${e.value}`, e.origin.replace(/^file:/, '')] : [`${k} is not set`, 'git’s default'];
        const above = config.local(k) !== undefined ? config.inherited(k) : undefined;
        if (above) lines.push(`Without the override here: ${describe(above.value)}`);
        return lines.join('\n');
      })
      .join('\n\n');
    head.append(revert, source, info);
    div.append(head);
    if (help) div.append(el('div', 'setting-help', help));
    if (disabled) {
      control.querySelectorAll('select, input').forEach((c) => ((c as HTMLInputElement).disabled = true));
      if (control instanceof HTMLSelectElement || control instanceof HTMLInputElement) control.disabled = true;
      div.append(el('div', 'setting-warning', `⚠ ${disabled}`));
    }
    return div;
  }

  function select(options: { value: string; label: string }[], current: string, onChange: (value: string) => void): HTMLSelectElement {
    const s = el('select');
    for (const o of options) {
      const option = el('option', '', o.label);
      option.value = o.value;
      s.append(option);
    }
    // 選択肢にない値（手で書いたもの）は、そのまま出す
    if (!options.some((o) => o.value === current)) {
      const option = el('option', '', current);
      option.value = current;
      s.append(option);
    }
    s.value = current;
    s.addEventListener('change', () => onChange(s.value));
    return s;
  }

  /** 既定のブランチ。選ばなければ判定したもの（Branches の並べ方の土台） */
  function defaultBranchRow(): HTMLElement {
    const key = 'amonhen.defaultBranch';
    const chosen = config.local(key);
    const options = [
      { value: '', label: chosen ? 'Detected' : `Detected (${facts.base ?? 'none'})` },
      ...facts.branches.map((b) => ({ value: b, label: b })),
    ];
    const control = select(options, chosen ?? '', (v) => set([[key, v || null]]));
    const div = row('Default branch', 'Branches measures the others against it. Detected from origin/HEAD, main or master; choose one when there is none.', [key], control, (v) => v);
    if (chosen && !facts.branches.includes(chosen)) {
      div.append(el('div', 'setting-warning', `⚠ There is no branch named ${chosen}, so the detected branch is used.`));
    }
    return div;
  }

  /** `.git/info/exclude`（1 行に 1 つのパターン）。Revert・Save で */
  function excludeRow(): HTMLElement {
    const div = el('div', 'setting-row');
    div.append(el('div', 'setting-help', '.git/info/exclude, one pattern per line. Shared patterns go in .gitignore.'));
    const text = el('textarea', 'setting-text');
    text.value = excludeDraft ?? facts.exclude;
    text.rows = 10;
    text.spellcheck = false;
    const buttons = el('div', 'setting-buttons');
    const revert = el('button', 'secondary', 'Revert');
    const save = el('button', '', 'Save');
    const update = () => {
      const changed = text.value !== facts.exclude;
      revert.disabled = !changed;
      save.disabled = !changed;
    };
    text.addEventListener('input', () => {
      excludeDraft = text.value;
      update();
    });
    revert.addEventListener('click', () => {
      excludeDraft = undefined;
      text.value = facts.exclude;
      update();
    });
    save.addEventListener('click', () => {
      excludeDraft = undefined;
      post({ type: 'settingsSaveExclude', exclude: text.value });
    });
    update();
    buttons.append(revert, save);
    div.append(text, buttons);
    return div;
  }

  /** ボタンの行（題・説明と、右にボタン）。`disabled` は要る git の版 */
  function actionRow(title: string, help: string, controls: HTMLElement[], disabled?: string): HTMLElement {
    const div = el('div', 'setting-row');
    const head = el('div', 'setting-head');
    head.append(el('span', 'setting-question', title), ...controls);
    div.append(head, el('div', 'setting-help', help));
    if (disabled) {
      for (const c of controls) (c as HTMLButtonElement | HTMLInputElement).disabled = true;
      div.append(el('div', 'setting-warning', `⚠ ${disabled}`));
    }
    return div;
  }

  function button(label: string, action: FetchingAction, title?: string): HTMLButtonElement {
    const b = el('button', 'secondary', label);
    if (title) b.title = title;
    b.addEventListener('click', () => post({ type: 'settingsRun', ...action }));
    return b;
  }

  const tooOld = (needed: string) => `Needs git ${needed} or later. This repository’s git is ${facts.gitVersion ?? '?'}.`;

  /** Fetching and Folders（アプリと同じく、リポジトリの形に合わせて History・Contents・Folders・Maintenance） */
  function fetchingRows(): HTMLElement {
    const form = facts.repositoryForm;
    const box = el('div');
    if (!form) return box;
    const section = (title: string, ...rows: HTMLElement[]) => {
      const s = el('section', 'settings-section');
      s.append(el('h3', '', title), ...rows);
      box.append(s);
    };
    if (form.shallow) {
      section(
        'History',
        actionRow('Older history isn’t fetched (shallow clone)', 'Fetch more of it from the remote, as at the end of History.', [
          button('Fetch More…', { action: 'fetchMore' }, 'Fetch older commits from the remote (git fetch --deepen)'),
          button('Fetch All', { action: 'fetchAll' }, 'Fetch the whole history from the remote (git fetch --unshallow)'),
        ]),
      );
    }
    if (form.partial) {
      section(
        'Contents',
        actionRow(
          `Contents fetched when needed${form.partial.filter ? ` (${form.partial.filter})` : ''}`,
          `Diffs, blame and contents: searches fetch what they need from ${form.partial.remote}.`,
          [button('Fetch All Contents…', { action: 'fetchContents' })],
          form.canFetchAllContents ? undefined : tooOld('2.36'),
        ),
      );
    }
    if (form.workingTree) {
      const sparseOld = form.canSparseCheckout ? undefined : tooOld('2.25');
      const rows: HTMLElement[] = [];
      if (form.sparse?.cone) {
        rows.push(
          actionRow(
            form.sparse.folders.length === 0 ? 'Only the files at the top are checked out' : 'Only these folders are checked out',
            form.sparse.folders.length === 0
              ? 'Add the folders to check out with +.'
              : 'Files at the top, and directly in the folders above these, are checked out too.',
            [button('Check Out All Folders…', { action: 'checkOutAll' })],
            sparseOld,
          ),
        );
        for (const folder of form.sparse.folders) {
          const item = el('div', 'setting-folder');
          const remove = button('−', { action: 'removeFolder', folder }, 'Stop checking out this folder');
          remove.disabled = !!sparseOld;
          item.append(el('code', '', folder), remove);
          rows.push(item);
        }
        const add = button('+', { action: 'addFolder' }, 'Check out one more folder');
        add.disabled = !!sparseOld;
        rows.push(add);
      } else if (form.sparse) {
        rows.push(
          actionRow(
            'Only the files matching these patterns are checked out',
            'The patterns aren’t in cone mode, so Amon Hen doesn’t change them.',
            [button('Check Out All Folders…', { action: 'checkOutAll' })],
            sparseOld,
          ),
          ...form.sparse.folders.map((p) => el('div', 'setting-folder', p)),
        );
      } else {
        rows.push(
          actionRow(
            'All folders are checked out',
            'In a large repository, you can check out only the folders you work in (sparse checkout).',
            [button('Check Out Only Some Folders…', { action: 'addFolder' })],
            sparseOld,
          ),
        );
      }
      section('Folders', ...rows);
    }
    if (!form.shallow && !form.partial && !form.workingTree) {
      box.append(el('div', 'setting-help', 'This repository has all of its history and contents.'));
    }
    const on = el('input');
    on.type = 'checkbox';
    on.checked = form.maintenance;
    on.addEventListener('change', () => post({ type: 'settingsRun', action: 'maintenance', on: on.checked }));
    section(
      'Maintenance',
      actionRow(
        'Keep this repository maintained in the background',
        'Every hour git prefetches and writes the commit-graph; it’s set in your global git config and the system’s scheduler (git maintenance).',
        [on],
        form.canMaintain ? undefined : tooOld('2.31'),
      ),
    );
    return box;
  }

  function renderRow(r: SettingRow): HTMLElement {
    const disabled =
      'needs' in r && r.needs === 'updateRefs' && !facts.updateRefs
        ? `Needs git 2.38 or later. This repository’s git is ${facts.gitVersion ?? '?'}.`
        : undefined;
    switch (r.kind) {
      case 'defaultBranch':
        return defaultBranchRow();
      case 'exclude':
        return excludeRow();
      case 'fetching':
        return fetchingRows();
      case 'pull': {
        const method = pullMethod(config);
        const decidingKey = config.effective('pull.ff')?.value.toLowerCase() === 'only' ? 'pull.ff' : 'pull.rebase';
        const control = select(
          (Object.keys(PULL_LABELS) as PullMethod[]).map((m) => ({ value: m, label: PULL_LABELS[m] })),
          method,
          (m) => set(pullWrites(config, m as PullMethod)),
        );
        return row(r.question, undefined, [decidingKey, decidingKey === 'pull.ff' ? 'pull.rebase' : 'pull.ff'], control, (v) => v);
      }
      case 'switch': {
        const defaultOn = r.defaultOn === 'rerereCache' ? facts.rerereCache : r.defaultOn;
        const value = config.effective(r.key)?.value;
        const on = value === undefined ? defaultOn : normalizedBool(value) === 'true';
        const box = el('input');
        box.type = 'checkbox';
        box.checked = on;
        box.addEventListener('change', () => set([[r.key, box.checked ? 'true' : 'false']]));
        return row(r.question, r.help, [r.key], box, (v) => (normalizedBool(v) === 'true' ? 'On' : 'Off'), disabled);
      }
      case 'choice': {
        const raw = config.effective(r.key)?.value;
        const current = raw === undefined ? r.defaultValue : r.bool ? (normalizedBool(raw) ?? raw) : raw.toLowerCase();
        const label = (v: string) => r.options.find((o) => o.value === (r.bool ? (normalizedBool(v) ?? v) : v.toLowerCase()))?.label ?? v;
        return row(r.question, r.help, [r.key], select(r.options, current, (v) => set([[r.key, v]])), label, disabled);
      }
    }
  }

  function render(): void {
    renderNav();
    const parts: HTMLElement[] = [el('h2', 'settings-title', page.title)];
    if (error) parts.push(el('div', 'setting-warning', error));
    for (const section of page.sections) {
      const s = el('section', 'settings-section');
      // 題のない節は、行が自分で節を分ける（Fetching and Folders）
      if (section.title) s.append(el('h3', '', section.title));
      s.append(...section.rows.map(renderRow));
      parts.push(s);
    }
    content.replaceChildren(...parts);
  }

  render();
  return {
    root,
    onMessage(message: HostMessage) {
      if (message.type !== 'settings') return;
      config = new ConfigView(message.entries);
      facts = message.facts;
      error = message.error;
      render();
    },
    onShow() {
      post({ type: 'settingsLoad' });
    },
  };
}
