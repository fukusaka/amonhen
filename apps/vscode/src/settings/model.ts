/**
 * リポジトリの設定の画面のもと（アプリの Repository Settings と同じ決まり）。コアの `configList` の並び（後のものほど
 * 優先）から、キーの効いている値・このリポジトリの上書き・上書きしなければ効く値と、その設定元を決める。
 * vscode に依存しない（Webview と拡張ホストの両方で使う）
 */

/** コアの `configList` の 1 件 */
export interface ConfigEntry {
  key: string;
  value: string;
  /** `local`・`global`・`system`・`worktree`・`command` など */
  scope: string;
  /** `file:<path>` など */
  origin: string;
}

/** 後ろから探す（Array.prototype.findLast は ES2023） */
function findLast<T>(list: readonly T[], match: (item: T) => boolean): T | undefined {
  for (let i = list.length - 1; i >= 0; i--) if (match(list[i]!)) return list[i];
  return undefined;
}

/** 設定の並びから引く */
export class ConfigView {
  private readonly entries: readonly ConfigEntry[];

  constructor(entries: readonly ConfigEntry[]) {
    this.entries = entries;
  }

  /** 効いている値（最後に書かれたもの）。どこにもなければ undefined（git の既定） */
  effective(key: string): ConfigEntry | undefined {
    const k = key.toLowerCase();
    return findLast(this.entries, (e) => e.key.toLowerCase() === k);
  }

  /** このリポジトリの上書き（`.git/config`） */
  local(key: string): string | undefined {
    const k = key.toLowerCase();
    return findLast(this.entries, (e) => e.key.toLowerCase() === k && e.scope === 'local')?.value;
  }

  /** 上書きしなければ効く値（このリポジトリより上で決まっているもの） */
  inherited(key: string): ConfigEntry | undefined {
    const k = key.toLowerCase();
    return findLast(this.entries, (e) => e.key.toLowerCase() === k && e.scope !== 'local' && e.scope !== 'worktree');
  }

  /** 設定元の言い方とファイル（ツールチップ用）。なければ git の既定 */
  origin(key: string): { label: string; file?: string } {
    const e = this.effective(key);
    if (!e) return { label: 'git’s default' };
    return { label: scopeLabel(e.scope), file: e.origin.replace(/^file:/, '') };
  }
}

/** 設定元（`git config` の scope）の言い方。行の右と同じ言葉 */
export function scopeLabel(scope: string): string {
  switch (scope) {
    case 'local':
      return 'this repository';
    case 'global':
      return 'all repositories';
    case 'worktree':
      return 'this worktree';
    case 'system':
      return 'system-wide';
    default:
      return scope;
  }
}

/** git の真偽値の書き方を true・false にそろえる（それ以外は undefined） */
export function normalizedBool(value: string): 'true' | 'false' | undefined {
  switch (value.toLowerCase()) {
    case 'true':
    case 'yes':
    case 'on':
    case '1':
    case '':
      return 'true';
    case 'false':
    case 'no':
    case 'off':
    case '0':
      return 'false';
    default:
      return undefined;
  }
}

export type PullMethod = 'merge' | 'rebase' | 'ffOnly';

/** Pull の取り込み方（`pull.ff=only` が先、それから `pull.rebase`） */
export function pullMethod(config: ConfigView): PullMethod {
  if (config.effective('pull.ff')?.value.toLowerCase() === 'only') return 'ffOnly';
  const r = config.effective('pull.rebase')?.value.toLowerCase();
  if (r && ['true', 'yes', 'on', '1', 'merges', 'interactive'].includes(r)) return 'rebase';
  return 'merge';
}

/**
 * 取り込み方を選んだときに書く値（`null` は消す）。fast-forward だけなら pull.ff=only（pull.rebase=true が残って
 * いると、どちらが勝つかが git の版で変わるので false も書く）。それ以外は pull.ff の only を消してから pull.rebase
 */
export function pullWrites(config: ConfigView, method: PullMethod): [key: string, value: string | null][] {
  const writes: [string, string | null][] = [];
  if (config.local('pull.ff')?.toLowerCase() === 'only') writes.push(['pull.ff', null]);
  switch (method) {
    case 'merge':
      writes.push(['pull.rebase', 'false']);
      break;
    case 'rebase':
      writes.push(['pull.rebase', 'true']);
      break;
    case 'ffOnly':
      writes.push(['pull.rebase', 'false'], ['pull.ff', 'only']);
      break;
  }
  return writes;
}
