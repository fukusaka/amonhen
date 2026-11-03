/** メインビューの Repository Settings と拡張ホストの間のメッセージ */
import type { ConfigEntry } from './model.ts';

/** このリポジトリを扱う git と、リポジトリの形（git の行に出す） */
export interface SettingsFacts {
  gitVersion?: string;
  /** rebase.updateRefs が使える（git 2.38 から） */
  updateRefs: boolean;
  /** rr-cache がある（rerere.enabled が書かれていなければ、git は rerere を使う） */
  rerereCache: boolean;
  /** リポジトリの形の一言（shallow clone・partial clone・sparse checkout・maintenance） */
  form: { title: string; help: string }[];
  /** ローカルブランチ（既定のブランチを選ぶ） */
  branches: string[];
  /** 判定した既定のブランチ（origin/HEAD の先、なければ main・master）。選んでいればそれ */
  base?: string;
  /** `.git/info/exclude` の中身 */
  exclude: string;
  /** コアの `repositoryForm`（Fetching and Folders）。読めなければ undefined */
  repositoryForm?: RepositoryForm;
  /** リモート（Remotes） */
  remotes: { name: string; fetchUrl: string; pushUrl: string }[];
  /** git が知っている設定の名前（Keys and Values の All） */
  knownKeys: string[];
  /** 今の署名の形式（gpg.format）で使える鍵（コアの `signingKeys`。`user.email` の鍵が先） */
  signingKeys: { id: string; label: string; emails: string[] }[];
  /** このリポジトリ専用のテンプレート（`text` はなければ null）と、`commit.template` に書くそのパス */
  commitTemplate?: { text: string | null; path: string };
  /** コアの `hooksAndLfs`（Hooks and LFS） */
  hooksAndLfs?: {
    hooks: { name: string; enabled: boolean }[];
    hooksPath: string | null;
    lfs: boolean;
    lfsFiles: number | null;
    lfsPatterns: string[];
  };
}

export interface RepositoryForm {
  shallow: boolean;
  partial: { remote: string; filter: string | null } | null;
  /** cone mode なら取り出しているフォルダ、そうでなければパターン */
  sparse: { cone: boolean; folders: string[] } | null;
  maintenance: boolean;
  workingTree: boolean;
  canFetchAllContents: boolean;
  canSparseCheckout: boolean;
  canMaintain: boolean;
}

/** Fetching and Folders のボタン */
export type FetchingAction =
  | { action: 'fetchMore' | 'fetchAll' | 'fetchContents' | 'addFolder' | 'checkOutAll' }
  | { action: 'maintenance'; on: boolean }
  | { action: 'removeFolder'; folder: string }
  /** Remotes の行のボタン（add は名前なし） */
  | { action: 'remote'; op: 'add' | 'rename' | 'editUrl' | 'editPushUrl' | 'check' | 'prune' | 'remove'; name?: string }
  /** Keys and Values: キーの値を聞いて書く（`key` がなければ、キーから聞いて足す） */
  | { action: 'configEdit'; key?: string }
  /** Hooks and LFS: フックの実行の許可・エディタで開く・作る、LFS で追跡するパターンを足す・外す */
  | { action: 'hookEnabled'; name: string; enabled: boolean }
  | { action: 'hookOpen'; name: string }
  | { action: 'hookCreate' }
  | { action: 'lfsTrack'; pattern?: string; track: boolean }
  /** Commits: 専用のテンプレートを書く（commit.template をそれに向ける）・ファイルを選ぶ */
  | { action: 'templateSave'; text: string }
  | { action: 'templateFile' }
  /** Author and Signing: 一覧にない鍵を入れる・mailmap のファイルを選ぶ */
  | { action: 'signingKeyOther' }
  | { action: 'mailmapFile' };

export type SettingsHostMessage =
  /** 読んだ設定（`error` は読めなかったわけ） */
  { type: 'settings'; entries: ConfigEntry[]; facts: SettingsFacts; error?: string };

export type SettingsWebviewMessage =
  /** Repository Settings を出した（読み直してもらう） */
  | { type: 'settingsLoad' }
  /** このリポジトリの設定を書く（`value` が null なら消す。上書きをやめて上の値に戻る） */
  | { type: 'settingsSet'; writes: [key: string, value: string | null][] }
  /** `.git/info/exclude` を書く（`.gitignore` はそのまま） */
  | { type: 'settingsSaveExclude'; exclude: string }
  /** Fetching and Folders のボタン（取得・sparse checkout・maintenance） */
  | ({ type: 'settingsRun' } & FetchingAction);
