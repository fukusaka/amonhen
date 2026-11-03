/**
 * リポジトリの設定のページと行（アプリの Repository Settings と同じ質問・説明・選択肢）。Webview はこれを描き、
 * 値は拡張ホストが `configSet` で書く。vscode に依存しない
 */

/** 行。`switch` ははい／いいえ、`choice` は選ぶもの、`pull` は Pull の取り込み方（2 つのキーで決まる） */
export type SettingRow =
  | { kind: 'switch'; question: string; help?: string; key: string; defaultOn: boolean | 'rerereCache'; needs?: Needs }
  | {
      kind: 'choice';
      question: string;
      help?: string;
      key: string;
      options: { value: string; label: string }[];
      defaultValue: string;
      bool?: boolean;
      needs?: Needs;
    }
  | { kind: 'pull'; question: string }
  /** 既定のブランチ（Amon Hen の `amonhen.defaultBranch`。なければ判定したもの） */
  | { kind: 'defaultBranch' }
  /** `.git/info/exclude` の中身を書く欄 */
  | { kind: 'exclude' }
  /** リポジトリの形（shallow・partial・sparse・maintenance）に合わせた、取得とフォルダの行（Fetching and Folders） */
  | { kind: 'fetching' }
  /** リモートの表（Add・Edit URL・Push URL・Check・Prune・Rename・Remove） */
  | { kind: 'remotes' }
  /** git の設定の表（Keys and Values） */
  | { kind: 'config' }
  /** フックの表と Git LFS（Hooks and LFS） */
  | { kind: 'hooks' };

/** 使えるのに要る git（無ければ欄を灰色にし、理由を出す） */
export type Needs = 'updateRefs';

export interface SettingSection {
  title: string;
  rows: SettingRow[];
}

export interface SettingPage {
  id: string;
  title: string;
  sections: SettingSection[];
}

/** ページの並び（アプリと同じ順。まだ作っていないページは入れない） */
export const PAGES: SettingPage[] = [
  {
    id: 'operations',
    title: 'Operations',
    sections: [
      { title: 'Pull', rows: [{ kind: 'pull', question: 'When pulling, how are the changes brought in?' }] },
      {
        title: 'Fetch',
        rows: [{ kind: 'switch', question: 'Remove remote-tracking branches deleted on the remote', key: 'fetch.prune', defaultOn: false }],
      },
      {
        title: 'Rebase',
        rows: [
          {
            kind: 'switch',
            question: 'Stash changes before rebasing',
            help: 'History edits always stash, whatever this says.',
            key: 'rebase.autoStash',
            defaultOn: false,
          },
          {
            kind: 'choice',
            question: 'Move stacked branches along when rebasing or editing History',
            help: 'Branches whose tips are among the rewritten commits. Undo puts them back.',
            key: 'rebase.updateRefs',
            options: [
              { value: 'false', label: 'Leave them' },
              { value: 'true', label: 'Move them along' },
            ],
            defaultValue: 'false',
            bool: true,
            needs: 'updateRefs',
          },
        ],
      },
      {
        title: 'Merge',
        rows: [
          {
            kind: 'choice',
            question: 'How conflicts are written in files',
            help: 'With the base (diff3, zdiff3), the merge editor resolves one-sided conflicts by itself.',
            key: 'merge.conflictStyle',
            options: [
              { value: 'merge', label: 'Both sides' },
              { value: 'diff3', label: 'Both sides and the common base' },
              { value: 'zdiff3', label: 'Both sides and the base, shortened' },
            ],
            defaultValue: 'merge',
          },
          {
            kind: 'switch',
            question: 'Reuse how conflicts were resolved before',
            help: 'When the same conflict comes back, git puts the earlier resolution in the file (rerere).',
            key: 'rerere.enabled',
            defaultOn: 'rerereCache',
          },
          {
            kind: 'switch',
            question: 'Stage files resolved that way',
            help: 'Only while the switch above is on.',
            key: 'rerere.autoUpdate',
            defaultOn: false,
          },
        ],
      },
    ],
  },
  {
    id: 'branches',
    title: 'Branches',
    sections: [{ title: 'Default branch', rows: [{ kind: 'defaultBranch' }] }],
  },
  {
    id: 'workingCopy',
    title: 'Working Copy',
    sections: [
      {
        title: 'Files',
        rows: [
          {
            kind: 'choice',
            question: 'Line endings',
            key: 'core.autocrlf',
            options: [
              { value: 'false', label: 'Keep as committed' },
              { value: 'true', label: 'CRLF in the working tree, LF in commits' },
              { value: 'input', label: 'LF in commits, no change on checkout' },
            ],
            defaultValue: 'false',
          },
          { kind: 'switch', question: 'Track changes to the executable bit', key: 'core.fileMode', defaultOn: true },
        ],
      },
      { title: 'Ignore in this repository only', rows: [{ kind: 'exclude' }] },
    ],
  },
  {
    id: 'fetching',
    title: 'Fetching and Folders',
    sections: [{ title: '', rows: [{ kind: 'fetching' }] }],
  },
  {
    id: 'remotes',
    title: 'Remotes',
    sections: [{ title: '', rows: [{ kind: 'remotes' }] }],
  },
  {
    id: 'hooks',
    title: 'Hooks and LFS',
    sections: [{ title: '', rows: [{ kind: 'hooks' }] }],
  },
  {
    id: 'config',
    title: 'Keys and Values',
    sections: [{ title: '', rows: [{ kind: 'config' }] }],
  },
];

/** ページの行が聞くキー（Keys and Values の Basic に出す） */
export function pageKeys(): string[] {
  const keys = new Set<string>(['pull.rebase', 'pull.ff', 'amonhen.defaultBranch']);
  for (const page of PAGES) {
    for (const section of page.sections) {
      for (const row of section.rows) if ('key' in row) keys.add(row.key);
    }
  }
  return [...keys].sort();
}
