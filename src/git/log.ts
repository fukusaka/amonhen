/** `git log` の取得とパース。分割して取るので、1 回の量は少なく保つ */

export interface Commit {
  oid: string;
  parents: string[];
  author: string;
  email: string;
  /** author date。unix 秒 */
  date: number;
  subject: string;
}

const RS = '\x1e';
const LOG_FIELDS = ['%H', '%P', '%an', '%ae', '%at', '%s'];

/**
 * revs から辿れるコミットを、トポロジカル順で skip 件飛ばして max 件。
 * `--topo-order` は commit-graph があれば全体を辿らずに済む。
 */
export function logArgs(revs: readonly string[], skip: number, max: number): string[] {
  return [
    'log',
    '--topo-order',
    `--format=${RS}${LOG_FIELDS.join('%x00')}`,
    `--skip=${skip}`,
    `--max-count=${max}`,
    ...revs,
    '--',
  ];
}

export function parseLog(output: string): Commit[] {
  const commits: Commit[] = [];
  for (const record of output.split(RS)) {
    if (record === '') {
      continue;
    }
    const parts = record.replace(/\n$/, '').split('\0');
    if (parts.length !== LOG_FIELDS.length) {
      throw new Error(`malformed log record: ${JSON.stringify(record)}`);
    }
    const [oid, parents, author, email, date, subject] = parts as [string, string, string, string, string, string];
    commits.push({
      oid,
      parents: parents ? parents.split(' ') : [],
      author,
      email,
      date: Number(date) || 0,
      subject,
    });
  }
  return commits;
}

export interface FileChange {
  /** A / M / D / R / C / T */
  status: string;
  path: string;
  /** rename・copy の元のパス */
  origPath?: string;
}

/** 親（無ければ空の木）とコミットの間で変わったファイル */
export function diffTreeArgs(oid: string, parent: string | undefined): string[] {
  return parent
    ? ['diff-tree', '-r', '-z', '-M', '--name-status', '--no-commit-id', parent, oid]
    : ['diff-tree', '-r', '-z', '-M', '--name-status', '--no-commit-id', '--root', oid];
}

/** `--name-status -z` の出力（`status<NUL>path<NUL>`、rename は `R100<NUL>old<NUL>new<NUL>`）をパースする */
export function parseNameStatus(output: string): FileChange[] {
  const fields = output.split('\0');
  const changes: FileChange[] = [];
  for (let i = 0; i < fields.length; i++) {
    const status = fields[i];
    if (!status) {
      continue;
    }
    const kind = status[0]!;
    if (kind === 'R' || kind === 'C') {
      changes.push({ status: kind, origPath: fields[i + 1]!, path: fields[i + 2]! });
      i += 2;
    } else {
      changes.push({ status: kind, path: fields[i + 1]! });
      i += 1;
    }
  }
  return changes;
}
