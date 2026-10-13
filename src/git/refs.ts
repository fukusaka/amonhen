/** ref・remote・stash・submodule の取得コマンドと出力のパース */

/** レコードの先頭に RS（0x1e）、フィールドの区切りに NUL を置く */
const RS = '\x1e';

function records(output: string, fields: number): string[][] {
  const result: string[][] = [];
  for (const record of output.split(RS)) {
    if (record === '') {
      continue;
    }
    const parts = record.replace(/\n$/, '').split('\0');
    if (parts.length !== fields) {
      throw new Error(`expected ${fields} fields but got ${parts.length}: ${JSON.stringify(record)}`);
    }
    result.push(parts);
  }
  return result;
}

export interface Ref {
  /** `refs/heads/main` などの完全な名前 */
  name: string;
  oid: string;
  /** annotated tag が指すコミット */
  peeled?: string;
  upstream?: string;
  ahead?: number;
  behind?: number;
  /** upstream が設定されているがリモートに存在しない */
  gone?: boolean;
  isHead: boolean;
  symref?: string;
  /** committer date（tag は tagger date）。unix 秒 */
  date: number;
  subject: string;
}

const REF_FIELDS = [
  '%(refname)',
  '%(objectname)',
  '%(*objectname)',
  '%(upstream)',
  '%(upstream:track,nobracket)',
  '%(HEAD)',
  '%(symref)',
  '%(creatordate:unix)',
  '%(contents:subject)',
];

export function forEachRefArgs(patterns: readonly string[]): string[] {
  return ['for-each-ref', `--format=%1e${REF_FIELDS.join('%00')}`, ...patterns];
}

export function parseRefs(output: string): Ref[] {
  return records(output, REF_FIELDS.length).map(
    ([name, oid, peeled, upstream, track, head, symref, date, subject]) => {
      const ref: Ref = {
        name: name!,
        oid: oid!,
        isHead: head === '*',
        date: Number(date) || 0,
        subject: subject!,
      };
      if (peeled) ref.peeled = peeled;
      if (upstream) ref.upstream = upstream;
      if (symref) ref.symref = symref;
      if (track === 'gone') {
        ref.gone = true;
      } else if (track) {
        const ahead = /ahead (\d+)/.exec(track);
        const behind = /behind (\d+)/.exec(track);
        if (ahead) ref.ahead = Number(ahead[1]);
        if (behind) ref.behind = Number(behind[1]);
      }
      return ref;
    },
  );
}

export interface Remote {
  name: string;
  fetchUrl?: string;
  pushUrl?: string;
}

export const REMOTE_ARGS = ['remote', '-v'] as const;

/** `git remote -v` の出力（`name<TAB>url (fetch|push)`）をパースする */
export function parseRemotes(output: string): Remote[] {
  const remotes = new Map<string, Remote>();
  for (const line of output.split('\n')) {
    const m = /^([^\t]+)\t(.*) \((fetch|push)\)$/.exec(line);
    if (!m) {
      continue;
    }
    const [, name, url, kind] = m;
    let remote = remotes.get(name!);
    if (!remote) {
      remote = { name: name! };
      remotes.set(name!, remote);
    }
    if (kind === 'fetch') {
      remote.fetchUrl = url;
    } else {
      remote.pushUrl = url;
    }
  }
  return [...remotes.values()];
}

/** `refs/remotes/<remote>/<branch>` を分解する。remote 名は `/` を含みうるので最長一致で探す */
export function splitRemoteRef(
  name: string,
  remoteNames: readonly string[],
): { remote: string; branch: string } | undefined {
  const prefix = 'refs/remotes/';
  if (!name.startsWith(prefix)) {
    return undefined;
  }
  const rest = name.slice(prefix.length);
  let best: string | undefined;
  for (const remote of remoteNames) {
    if (rest.startsWith(`${remote}/`) && (!best || remote.length > best.length)) {
      best = remote;
    }
  }
  if (best) {
    return { remote: best, branch: rest.slice(best.length + 1) };
  }
  const slash = rest.indexOf('/');
  return slash < 0 ? undefined : { remote: rest.slice(0, slash), branch: rest.slice(slash + 1) };
}

export interface Stash {
  /** `stash@{0}` */
  name: string;
  oid: string;
  date: number;
  subject: string;
}

export const STASH_ARGS = ['stash', 'list', '--format=%x1e%gd%x00%H%x00%ct%x00%gs'] as const;

export function parseStashes(output: string): Stash[] {
  return records(output, 4).map(([name, oid, date, subject]) => ({
    name: name!,
    oid: oid!,
    date: Number(date) || 0,
    subject: subject!,
  }));
}

export interface SubmoduleConfig {
  name: string;
  path: string;
}

export const SUBMODULE_CONFIG_ARGS = [
  'config',
  '-z',
  '--file',
  '.gitmodules',
  '--get-regexp',
  '^submodule\\..*\\.path$',
] as const;

/** `git config -z --get-regexp` の出力（`key<LF>value<NUL>`）をパースする */
export function parseSubmoduleConfig(output: string): SubmoduleConfig[] {
  const result: SubmoduleConfig[] = [];
  for (const item of output.split('\0')) {
    const nl = item.indexOf('\n');
    if (nl < 0) {
      continue;
    }
    const key = item.slice(0, nl);
    result.push({ name: key.slice('submodule.'.length, -'.path'.length), path: item.slice(nl + 1) });
  }
  return result;
}
