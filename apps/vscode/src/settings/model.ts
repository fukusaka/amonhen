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

  /** ひとつの設定元（`system`・`global`・`local`・`worktree`）での値。その中で最後に書かれたもの */
  at(key: string, scope: string): ConfigEntry | undefined {
    const k = key.toLowerCase();
    return findLast(this.entries, (e) => e.key.toLowerCase() === k && e.scope === scope);
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

/** リモートの URL がどこを指すか（アプリの Remotes の表の「Points to」と同じ言い方） */
export function remoteKind(url: string): string {
  const lower = url.toLowerCase();
  for (const [host, name] of [
    ['github.com', 'GitHub'],
    ['gitlab', 'GitLab'],
    ['bitbucket', 'Bitbucket'],
    ['codeberg', 'Codeberg'],
  ] as const) {
    if (lower.includes(host)) return name;
  }
  if (/^(\/|\.\/|\.\.\/|~|file:\/\/)/.test(lower) || /^[a-z]:[\\/]/.test(lower)) return 'A path';
  const ssh = /^(?:git\+)?ssh:\/\/(?:[^@/]+@)?([^/:]+)/.exec(url);
  if (ssh) return `SSH host ${ssh[1]}`;
  // scp の形（user@host:path）
  const scp = /^(?:[^@/]+@)?([^/:]+):/.exec(url);
  if (!lower.includes('://') && scp) return `SSH host ${scp[1]}`;
  const host = /^[a-z][a-z0-9+.-]*:\/\/(?:[^@/]+@)?([^/:]+)/i.exec(url);
  if (host) return host[1]!;
  return url ? 'Other' : '';
}

/**
 * Keys and Values の Basic のキー: ページ群が尋ねるもの（アプリの `basicKeys` と同じ並び）。git が出す形（小文字）で
 */
export const BASIC_KEYS = [
  'user.name',
  'user.email',
  'commit.gpgsign',
  'gpg.format',
  'user.signingkey',
  'gpg.ssh.allowedsignersfile',
  'commit.template',
  'amonhen.defaultbranch',
  'pull.rebase',
  'pull.ff',
  'fetch.prune',
  'rebase.autostash',
  'rebase.updaterefs',
  'merge.conflictstyle',
  'rerere.enabled',
  'rerere.autoupdate',
  'core.autocrlf',
  'core.filemode',
] as const;

/**
 * Keys and Values の表の 1 行: 1 つのキーと、設定元（段）ごとの値。同じ段に同じキーがいくつもあれば
 * （`remote.origin.fetch` など）、改行で並べる（アプリの `ConfigRow`）
 */
export interface ConfigRow {
  key: string;
  /** 今効いている段の値（その段の値すべて）と、その段。どの段にもなければ空 */
  effective: string;
  effectiveScope: string;
  /** 段ごとの値と、その段のファイル */
  values: Record<string, string>;
  files: Record<string, string>;
}

/** どこにも設定のない行（Basic の未設定のキー、All の git が知っているだけのキー） */
export function blankConfigRow(key: string): ConfigRow {
  return { key, effective: '', effectiveScope: '', values: {}, files: {} };
}

/** git が知っている変数の綴り（`core.fileMode`）を、小文字のキーから引く */
export function canonicalKeys(knownKeys: readonly string[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const k of knownKeys) if (!map.has(k.toLowerCase())) map.set(k.toLowerCase(), k);
  return map;
}

/**
 * 設定の並びをキーごとに 1 行にまとめる（順は git が出した順）。キーは git が知っている綴りにそろえる。
 * 効いている段は最後に書かれたものの段で、その値はその段の値すべて
 */
export function configRows(entries: readonly ConfigEntry[], knownKeys: readonly string[]): ConfigRow[] {
  const canonical = canonicalKeys(knownKeys);
  const byKey = new Map<string, ConfigEntry[]>();
  const names = new Map<string, string>();
  for (const e of entries) {
    const lower = e.key.toLowerCase();
    if (!names.has(lower)) names.set(lower, canonical.get(lower) ?? e.key);
    const list = byKey.get(lower);
    if (list) list.push(e);
    else byKey.set(lower, [e]);
  }
  return [...byKey].map(([lower, es]) => {
    const values: Record<string, string> = {};
    const files: Record<string, string> = {};
    for (const e of es) {
      values[e.scope] = e.scope in values ? `${values[e.scope]}\n${e.value}` : e.value;
      files[e.scope] = e.origin.replace(/^file:/, '');
    }
    const scope = es[es.length - 1]!.scope;
    return { key: names.get(lower)!, effective: values[scope] ?? '', effectiveScope: scope, values, files };
  });
}

/**
 * キーの 3 つの部分。`section.name` か `section.subsection.name` で、subsection にはドットが入りうる
 * （`url.https://….insteadOf`）ので、最初と最後のドットで切る
 */
export function keyParts(key: string): { section: string; subsection?: string; name: string } {
  const first = key.indexOf('.');
  const last = key.lastIndexOf('.');
  if (first < 0) return { section: key, name: '' };
  return {
    section: key.slice(0, first),
    ...(first === last ? {} : { subsection: key.slice(first + 1, last) }),
    name: key.slice(last + 1),
  };
}

/** 表の並べ替えの列（アプリの表の見出しと同じ。`source` は Combined、段の名前は Levels） */
export type ConfigSortColumn = 'key' | 'effective' | 'source' | 'system' | 'global' | 'local' | 'worktree';
export interface ConfigSort {
  column: ConfigSortColumn;
  reverse: boolean;
}

/** 組（section か subsection）: 見出しのキー（`remote`・`remote.origin`）と名前、その中の行と組 */
export interface ConfigGroup {
  key: string;
  label: string;
  rows: ConfigRow[];
  groups: ConfigGroup[];
}

function sortValue(row: ConfigRow, column: ConfigSortColumn): string {
  switch (column) {
    case 'key':
      return row.key;
    case 'effective':
      return row.effective;
    case 'source':
      return row.effectiveScope;
    default:
      return row.values[column] ?? '';
  }
}

/** Finder と同じ並べ方（大文字小文字を区別せず、数は数として） */
const collator = new Intl.Collator('en', { numeric: true, sensitivity: 'base' });

/**
 * 行を並べ替えて、キーのドットで組にする（section の下に subsection。アプリの表と同じ）。組の順はキーの並びと同じ
 * 向き（キーの逆順で並べていれば逆）、中の行の順は並べ替えのまま。section の中は、行が先で subsection が後
 */
export function groupConfigRows(rows: readonly ConfigRow[], sort: ConfigSort): ConfigGroup[] {
  const sign = sort.reverse ? -1 : 1;
  const sorted = [...rows].sort((a, b) => sign * collator.compare(sortValue(a, sort.column), sortValue(b, sort.column)));
  const sections = new Map<string, ConfigGroup>();
  for (const row of sorted) {
    const p = keyParts(row.key);
    let section = sections.get(p.section);
    if (!section) {
      section = { key: p.section, label: p.section, rows: [], groups: [] };
      sections.set(p.section, section);
    }
    if (p.subsection === undefined) {
      section.rows.push(row);
      continue;
    }
    const key = `${p.section}.${p.subsection}`;
    let sub = section.groups.find((g) => g.key === key);
    if (!sub) {
      sub = { key, label: p.subsection, rows: [], groups: [] };
      section.groups.push(sub);
    }
    sub.rows.push(row);
  }
  const keyReversed = sort.column === 'key' && sort.reverse;
  const order = (a: ConfigGroup, b: ConfigGroup) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0) * (keyReversed ? -1 : 1);
  const result = [...sections.values()].sort(order);
  for (const s of result) s.groups.sort(order);
  return result;
}
