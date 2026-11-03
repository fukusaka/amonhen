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
}

export type SettingsHostMessage =
  /** 読んだ設定（`error` は読めなかったわけ） */
  { type: 'settings'; entries: ConfigEntry[]; facts: SettingsFacts; error?: string };

export type SettingsWebviewMessage =
  /** Repository Settings を出した（読み直してもらう） */
  | { type: 'settingsLoad' }
  /** このリポジトリの設定を書く（`value` が null なら消す。上書きをやめて上の値に戻る） */
  | { type: 'settingsSet'; writes: [key: string, value: string | null][] };
