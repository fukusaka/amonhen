/** `git status --porcelain=v2 -z --branch --show-stash` の出力をパースする */

export type EntryKind = 'changed' | 'renamed' | 'unmerged' | 'untracked' | 'ignored';

export interface StatusEntry {
  kind: EntryKind;
  path: string;
  /** rename / copy の元のパス */
  origPath?: string;
  /** XY の X（index 側）。変更なしは '.'。untracked は '?' */
  index: string;
  /** XY の Y（worktree 側） */
  worktree: string;
  /** サブモジュールの状態（`N...` または `S<c><m><u>`） */
  submodule?: string;
}

export interface BranchStatus {
  /** `(initial)` のときは undefined */
  oid?: string;
  /** detached HEAD のときは undefined */
  head?: string;
  upstream?: string;
  ahead?: number;
  behind?: number;
}

export interface StatusResult {
  branch: BranchStatus;
  entries: StatusEntry[];
  stashCount: number;
}

export const STATUS_ARGS = [
  'status',
  '--porcelain=v2',
  '-z',
  '--branch',
  '--show-stash',
  '--untracked-files=all',
] as const;

/** 先頭から空白区切りで n 個のトークンを取り、残りをパスとして返す（パスは空白を含みうる） */
function split(record: string, n: number): { tokens: string[]; rest: string } {
  const tokens: string[] = [];
  let pos = 0;
  for (let i = 0; i < n; i++) {
    const sp = record.indexOf(' ', pos);
    if (sp < 0) {
      throw new Error(`malformed status record: ${JSON.stringify(record)}`);
    }
    tokens.push(record.slice(pos, sp));
    pos = sp + 1;
  }
  return { tokens, rest: record.slice(pos) };
}

export function parseStatus(output: string): StatusResult {
  const result: StatusResult = { branch: {}, entries: [], stashCount: 0 };
  const records = output.split('\0');
  for (let i = 0; i < records.length; i++) {
    const record = records[i]!;
    if (record === '') {
      continue;
    }
    switch (record[0]) {
      case '#': {
        const { tokens, rest } = split(record, 2);
        parseHeader(result, tokens[1]!, rest);
        break;
      }
      case '1': {
        const { tokens, rest } = split(record, 8);
        const xy = tokens[1]!;
        result.entries.push({
          kind: 'changed',
          path: rest,
          index: xy[0]!,
          worktree: xy[1]!,
          submodule: submoduleField(tokens[2]!),
        });
        break;
      }
      case '2': {
        const { tokens, rest } = split(record, 9);
        const xy = tokens[1]!;
        result.entries.push({
          kind: 'renamed',
          path: rest,
          origPath: records[++i],
          index: xy[0]!,
          worktree: xy[1]!,
          submodule: submoduleField(tokens[2]!),
        });
        break;
      }
      case 'u': {
        const { tokens, rest } = split(record, 10);
        const xy = tokens[1]!;
        result.entries.push({
          kind: 'unmerged',
          path: rest,
          index: xy[0]!,
          worktree: xy[1]!,
          submodule: submoduleField(tokens[2]!),
        });
        break;
      }
      case '?':
        result.entries.push({ kind: 'untracked', path: record.slice(2), index: '?', worktree: '?' });
        break;
      case '!':
        result.entries.push({ kind: 'ignored', path: record.slice(2), index: '!', worktree: '!' });
        break;
      default:
        throw new Error(`unknown status record: ${JSON.stringify(record)}`);
    }
  }
  return result;
}

function submoduleField(field: string): string | undefined {
  return field === 'N...' ? undefined : field;
}

function parseHeader(result: StatusResult, key: string, value: string): void {
  const branch = result.branch;
  switch (key) {
    case 'branch.oid':
      if (value !== '(initial)') {
        branch.oid = value;
      }
      break;
    case 'branch.head':
      if (value !== '(detached)') {
        branch.head = value;
      }
      break;
    case 'branch.upstream':
      branch.upstream = value;
      break;
    case 'branch.ab': {
      const m = /^\+(\d+) -(\d+)$/.exec(value);
      if (m) {
        branch.ahead = Number(m[1]);
        branch.behind = Number(m[2]);
      }
      break;
    }
    case 'stash':
      result.stashCount = Number(value);
      break;
  }
}
