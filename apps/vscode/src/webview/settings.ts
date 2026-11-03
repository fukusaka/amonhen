/**
 * メインビューの Repository Settings 画面（アプリの Repository Settings と同じ形）。左にページの並びと、このリポジトリを
 * 扱う git・リポジトリの形、右に選んだページの行。行は、質問とその下の説明、効いている値の欄、このリポジトリが
 * 上書きしている間だけ ↩︎、設定元、ⓘ（キーと値とファイル、上書きしなければ効く値）
 */
import { ConfigView, normalizedBool, pullMethod, pullWrites, remoteKind, scopeLabel, type ConfigEntry, type PullMethod } from '../settings/model.ts';
import { PAGES, pageKeys, type SettingRow } from '../settings/pages.ts';
import { hookWhen } from '../settings/hooks.ts';
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
  let facts: SettingsFacts = {
    updateRefs: true,
    rerereCache: false,
    form: [],
    branches: [],
    exclude: '',
    remotes: [],
    knownKeys: [],
    signingKeys: [],
  };
  let entries: ConfigEntry[] = [];
  /** Keys and Values の出し方と絞り込み */
  let configMode: 'basic' | 'customized' | 'all' = 'customized';
  let configFilter = '';
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

  /** Remotes の表（アプリの表と同じ列: Name・URL・Push URL・Points to）。行ごとにボタン */
  function remotesTable(): HTMLElement {
    const box = el('div');
    const table = el('table', 'settings-table');
    const head = el('tr');
    for (const h of ['Name', 'URL', 'Push URL', 'Points to', '']) head.append(el('th', '', h));
    table.append(head);
    for (const r of facts.remotes) {
      const tr = el('tr');
      const same = !r.pushUrl || r.pushUrl === r.fetchUrl;
      const push = el('td', same ? 'muted' : '', same ? '(same)' : r.pushUrl);
      const actions = el('td', 'settings-actions');
      for (const [op, label, title] of [
        ['editUrl', 'Edit URL…', 'Change the URL fetch and push use'],
        ['editPushUrl', 'Push URL…', 'Push somewhere other than the fetch URL'],
        ['check', 'Check', 'Connect and count the remote’s branches and tags (git ls-remote)'],
        ['prune', 'Prune', 'Remove remote-tracking branches that no longer exist on the remote'],
        ['rename', 'Rename…', ''],
        ['remove', 'Remove…', ''],
      ] as const) {
        const b = button(label, { action: 'remote', op, name: r.name }, title || undefined);
        actions.append(b);
      }
      tr.append(el('td', '', r.name), el('td', 'mono', r.fetchUrl), push, el('td', 'muted', remoteKind(r.fetchUrl)), actions);
      table.append(tr);
    }
    if (facts.remotes.length === 0) {
      const tr = el('tr');
      const td = el('td', 'muted', 'No remotes');
      td.colSpan = 5;
      tr.append(td);
      table.append(tr);
    }
    box.append(table, button('Add…', { action: 'remote', op: 'add' }, 'Add a remote'));
    return box;
  }

  /**
   * Keys and Values（アプリの git の設定の表）。Basic はページの行が聞くキー（設定してあってもなくても）、
   * Customized はこのリポジトリ（と worktree）で決めているキー、All は git が読むキーすべて
   */
  function configTable(): HTMLElement {
    const box = el('div');
    const bar = el('div', 'settings-config-bar');
    const mode = select(
      [
        { value: 'basic', label: 'Basic' },
        { value: 'customized', label: 'Customized' },
        { value: 'all', label: 'All' },
      ],
      configMode,
      (v) => {
        configMode = v as typeof configMode;
        render();
      },
    );
    mode.title = 'Basic: the settings the pages here ask about, set or not. Customized: the keys set in this repository or this worktree. All: every key git reads.';
    const search = el('input');
    search.type = 'search';
    search.placeholder = 'Search by key or value';
    search.value = configFilter;
    search.addEventListener('input', () => {
      configFilter = search.value;
      fill();
      search.focus();
    });
    bar.append(mode, search, button('Add…', { action: 'configEdit' }, 'Add a key to this repository'));
    const table = el('table', 'settings-table');
    const fill = () => {
      const lower = (k: string) => k.toLowerCase();
      const named = new Map<string, string>();
      for (const e of entries) named.set(lower(e.key), e.key);
      let keys: string[];
      if (configMode === 'basic') keys = pageKeys();
      else if (configMode === 'customized') keys = [...new Set(entries.filter((e) => e.scope === 'local' || e.scope === 'worktree').map((e) => e.key))];
      else keys = [...new Set([...facts.knownKeys, ...named.values()].map((k) => named.get(lower(k)) ?? k))];
      const words = configFilter.toLowerCase().split(/\s+/).filter(Boolean);
      const rows = keys
        .map((key) => ({ key, e: config.effective(key) }))
        .filter(({ key, e }) => words.every((w) => key.toLowerCase().includes(w) || (e?.value.toLowerCase().includes(w) ?? false)))
        .sort((a, b) => a.key.localeCompare(b.key));
      const head = el('tr');
      for (const h of ['Key', 'Value in effect', 'Source', '']) head.append(el('th', '', h));
      table.replaceChildren(head);
      for (const { key, e } of rows) {
        const tr = el('tr');
        const value = el('td', e ? 'mono' : 'mono muted', e ? e.value : '—');
        const source = el('td', 'muted', e ? scopeLabel(e.scope) : 'git’s default');
        if (e) source.title = e.origin.replace(/^file:/, '');
        const actions = el('td', 'settings-actions');
        actions.append(button('Edit…', { action: 'configEdit', key }, 'Edit in this repository'));
        if (config.local(key) !== undefined) {
          const remove = el('button', 'secondary', 'Remove');
          remove.title = 'Remove from this repository';
          remove.addEventListener('click', () => set([[key, null]]));
          actions.append(remove);
        }
        tr.append(el('td', 'mono', key), value, source, actions);
        table.append(tr);
      }
      if (rows.length === 0) {
        const tr = el('tr');
        const td = el('td', 'muted', configMode === 'customized' && !configFilter ? 'Nothing is set in this repository' : 'No keys');
        td.colSpan = 4;
        tr.append(td);
        table.append(tr);
      }
    };
    fill();
    box.append(bar, table);
    return box;
  }

  /** Hooks and LFS（アプリと同じ）: フックの表（名前・いつ動くか・有効）と Git LFS（入っているか・パターン・数） */
  function hooksRows(): HTMLElement {
    const box = el('div');
    const info = facts.hooksAndLfs;
    if (!info) return box;
    const hooks = el('section', 'settings-section');
    hooks.append(el('h3', '', 'Hooks'));
    if (info.hooks.length === 0) {
      hooks.append(el('div', 'setting-help', 'No hooks. .sample files aren’t counted.'));
    } else {
      const table = el('table', 'settings-table');
      const head = el('tr');
      for (const h of ['Name', 'Runs', 'Enabled', '']) head.append(el('th', '', h));
      table.append(head);
      for (const h of info.hooks) {
        const tr = el('tr');
        const box = el('input');
        box.type = 'checkbox';
        box.checked = h.enabled;
        box.title = 'The executable bit. git runs only executable hooks.';
        box.addEventListener('change', () => post({ type: 'settingsRun', action: 'hookEnabled', name: h.name, enabled: box.checked }));
        const enabled = el('td');
        enabled.append(box);
        const actions = el('td', 'settings-actions');
        actions.append(button('Open', { action: 'hookOpen', name: h.name }, 'Open the script in the editor'));
        tr.append(el('td', 'mono', h.name), el('td', 'muted', hookWhen(h.name)), enabled, actions);
        table.append(tr);
      }
      hooks.append(table);
    }
    hooks.append(
      button('Create Hook…', { action: 'hookCreate' }, 'Puts a #!/bin/sh skeleton in place and makes it executable.'),
      el('div', 'setting-help', `Location: ${info.hooksPath ? `core.hooksPath = ${info.hooksPath}` : '.git/hooks'}`),
    );
    const lfs = el('section', 'settings-section');
    lfs.append(el('h3', '', 'Git LFS'));
    lfs.append(
      el(
        'div',
        'setting-help',
        info.lfs
          ? info.lfsFiles !== null
            ? `Installed. ${info.lfsFiles} files are in LFS.`
            : 'Installed (filter.lfs is set)'
          : 'Not installed. Install git-lfs and run git lfs install.',
      ),
    );
    if (info.lfsPatterns.length === 0) {
      lfs.append(el('div', 'setting-help', 'No LFS patterns in .gitattributes.'));
    }
    for (const pattern of info.lfsPatterns) {
      const item = el('div', 'setting-folder');
      item.append(el('code', '', pattern), button('Remove', { action: 'lfsTrack', pattern, track: false }, 'Stop storing these in LFS (git lfs untrack)'));
      lfs.append(item);
    }
    const track = button('Track Pattern…', { action: 'lfsTrack', track: true }, 'Store files matching a pattern in LFS (git lfs track)');
    track.disabled = !info.lfs;
    lfs.append(track);
    box.append(hooks, lfs);
    return box;
  }

  /** コミットのメッセージのテンプレート（アプリと同じく、なし・このリポジトリ専用・ファイル）と、専用のものの中身 */
  let templateDraft: string | undefined;
  function templateRows(): HTMLElement {
    const box = el('div');
    const key = 'commit.template';
    const value = config.effective(key)?.value ?? '';
    const own = facts.commitTemplate?.path;
    const isOwn = (p: string) => p === own || /[\\/]amonhen[\\/]commit-template$/.test(p);
    const current = !value ? '' : isOwn(value) ? 'own' : 'file';
    const options = [
      { value: '', label: 'None' },
      { value: 'own', label: 'This repository’s own' },
      ...(current === 'file' ? [{ value: 'file', label: `File: ${value}` }] : []),
      { value: 'pick', label: 'A file…' },
    ];
    const control = select(options, current, (v) => {
      if (v === '') set([[key, null]]);
      else if (v === 'own') post({ type: 'settingsRun', action: 'templateSave', text: facts.commitTemplate?.text ?? '' });
      else if (v === 'pick') post({ type: 'settingsRun', action: 'templateFile' });
    });
    box.append(row('Message template', 'What the commit message starts with (commit.template).', [key], control, (v) => (isOwn(v) ? 'This repository’s own' : v)));
    if (current === 'own') {
      const div = el('div', 'setting-row');
      const text = el('textarea', 'setting-text');
      const saved = facts.commitTemplate?.text ?? '';
      text.value = templateDraft ?? saved;
      text.rows = 8;
      text.spellcheck = false;
      const buttons = el('div', 'setting-buttons');
      const revert = el('button', 'secondary', 'Revert');
      const save = el('button', '', 'Save');
      const update = () => {
        revert.disabled = text.value === saved;
        save.disabled = text.value === saved;
      };
      text.addEventListener('input', () => {
        templateDraft = text.value;
        update();
      });
      revert.addEventListener('click', () => {
        templateDraft = undefined;
        text.value = saved;
        update();
      });
      save.addEventListener('click', () => {
        templateDraft = undefined;
        post({ type: 'settingsRun', action: 'templateSave', text: text.value });
      });
      update();
      buttons.append(revert, save);
      div.append(el('div', 'setting-help', 'This repository’s template, kept in .git/amonhen/commit-template.'), text, buttons);
      box.append(div);
    }
    return box;
  }

  const FORMATS: Record<string, string> = { openpgp: 'OpenPGP', ssh: 'SSH', x509: 'X.509' };
  const signingFormat = () => {
    const f = config.effective('gpg.format')?.value.toLowerCase() ?? 'openpgp';
    return f in FORMATS ? f : 'openpgp';
  };

  /** 文字の設定。欄から離れたときか Enter で書き、空にすると上書きを消す */
  function textRow(r: Extract<SettingRow, { kind: 'text' }>): HTMLElement {
    const input = el('input', 'setting-input');
    input.value = config.local(r.key) ?? '';
    input.placeholder = config.effective(r.key)?.value ?? r.placeholder ?? '';
    // Enter では keydown と change の両方が来る。書き終わって読み直すまでに 2 回書くと、git の config のロックで
    // 片方が失敗するので、最後に書いた値と同じなら書かない
    let written = config.local(r.key) ?? '';
    const commit = () => {
      const value = input.value.trim();
      if (value === written) return;
      written = value;
      set([[r.key, value || null]]);
    };
    input.addEventListener('change', commit);
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') commit();
    });
    return row(r.question, r.help, [r.key], input, (v) => v);
  }

  /** 署名（アプリと同じく、しない・OpenPGP・SSH・X.509。形式を選ぶと gpg.format も書く） */
  function signingRow(): HTMLElement {
    const on = normalizedBool(config.effective('commit.gpgsign')?.value ?? 'false') === 'true';
    const format = signingFormat();
    const control = select(
      [{ value: '', label: 'No signing' }, ...Object.entries(FORMATS).map(([value, label]) => ({ value, label }))],
      on ? format : '',
      (v) => {
        if (!v) set([['commit.gpgsign', 'false']]);
        else set([['commit.gpgsign', 'true'], ...(format !== v || config.local('gpg.format') !== undefined ? [['gpg.format', v] as [string, string]] : [])]);
      },
    );
    return row('Signing', undefined, ['commit.gpgsign', 'gpg.format'], control, (v) =>
      normalizedBool(v) === 'true' ? 'On' : normalizedBool(v) === 'false' ? 'Off' : (FORMATS[v.toLowerCase()] ?? v),
    );
  }

  /** 鍵: コアが集めた今の形式の鍵（user.email の鍵が先）から選ぶ。一覧にないものは Other… で */
  function signingKeyRow(): HTMLElement {
    const key = 'user.signingkey';
    const format = signingFormat();
    const current = config.effective(key)?.value ?? '';
    const options = [
      { value: '', label: format === 'openpgp' ? 'Key for your email' : 'None' },
      ...facts.signingKeys.map((k) => ({ value: k.id, label: k.label })),
      { value: '\u0001other', label: 'Other…' },
    ];
    const control = select(options, current, (v) => {
      if (v === '\u0001other') post({ type: 'settingsRun', action: 'signingKeyOther' });
      else set([[key, v || null]]);
    });
    return row('Key', `${FORMATS[format]} keys found on the machine that runs git.`, [key], control, (v) => facts.signingKeys.find((k) => k.id === v)?.label ?? v);
  }

  /** mailmap のファイル（mailmap.file。リポジトリの .mailmap の後に読む） */
  function mailmapFileRow(): HTMLElement {
    const key = 'mailmap.file';
    const current = config.effective(key)?.value ?? '';
    const options = [
      { value: '', label: 'None' },
      ...(current ? [{ value: current, label: `File: ${current}` }] : []),
      { value: '\u0001pick', label: 'A file…' },
    ];
    const control = select(options, current, (v) => {
      if (v === '\u0001pick') post({ type: 'settingsRun', action: 'mailmapFile' });
      else if (!v) set([[key, null]]);
    });
    return row('Mailmap file', 'Read after the repository’s .mailmap.', [key], control, (v) => (v ? `File: ${v}` : 'None'));
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
      case 'remotes':
        return remotesTable();
      case 'config':
        return configTable();
      case 'hooks':
        return hooksRows();
      case 'template':
        return templateRows();
      case 'text':
        return textRow(r);
      case 'signing':
        return signingRow();
      case 'signingKey':
        return signingKeyRow();
      case 'mailmapFile':
        return mailmapFileRow();
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
      entries = message.entries;
      facts = message.facts;
      error = message.error;
      render();
    },
    onShow() {
      post({ type: 'settingsLoad' });
    },
  };
}
