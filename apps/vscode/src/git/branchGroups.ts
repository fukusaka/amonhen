/**
 * ローカルブランチの並べ方（アプリのサイドバーの Branches と同じ決まり）。コアの `branchGroups` の答えから、上から
 * お気に入り・今のブランチ（その積み重ねの根元ごと）と既定のブランチ・動いているブランチ（新しい順）・Inactive・
 * Archived に分け、ほかのブランチの上に積んだブランチはその土台の下に入れ子にする。vscode に依存しない
 */

/** コアの `branchGroups` のブランチ 1 つ */
export interface BranchFacts {
  name: string;
  /** 先端のコミットの日時（unix 秒） */
  time: number | null;
  /** 既定のブランチに取り込み済み */
  merged: boolean;
  /** 既定のブランチの先端と同じ */
  atBase: boolean;
  /** 取り込んでいない自分のコミットがある */
  mine: boolean;
  /** その上に積んであるブランチ */
  parent: string | null;
  /** `branch.<name>.amonhenArchived` */
  archived: boolean;
  /** お気に入りの順位（`branch.<name>.amonhenPinned`。小さいほど上） */
  pinned: number | null;
  /** 取り込んでいないが、この番号のプルリクエストが今の先端を head にしてマージされた（squash・rebase） */
  pullRequest?: number | null;
}

export interface BranchGroupsInfo {
  /** 既定のブランチ（`origin/HEAD` の先、なければ main・master） */
  base: string | null;
  branches: BranchFacts[];
}

/** 並べたブランチ。`stack` はその上に積んだブランチ（入れ子に出す）、`stackedOn` は土台（お気に入りの行の右に書く） */
export interface Arranged {
  name: string;
  stack: Arranged[];
  stackedOn?: string;
}

export interface Arrangement {
  pinned: Arranged[];
  top: Arranged[];
  /** 新しい順（`/` のフォルダはサイドバーが作る） */
  active: Arranged[];
  inactive: Arranged[];
  archived: Arranged[];
}

/** Inactive とみなす日数（最後のコミットからこれだけ経ったブランチ） */
export const STALE_DAYS = 90;

/**
 * `locals` はローカルブランチの名前、`head` は今のブランチ、`now` は unix 秒、`elsewhere` はほかの worktree が
 * checkout しているか（それは Inactive に入れない）
 */
export function arrangeBranches(
  locals: readonly string[],
  head: string | undefined,
  info: BranchGroupsInfo,
  now: number,
  elsewhere: (name: string) => boolean = () => false,
): Arrangement {
  const facts = new Map(info.branches.map((b) => [b.name, b]));
  const exists = new Set(locals);
  const base = info.base && (exists.has(info.base) ? info.base : info.base.split('/').slice(1).join('/'));
  const time = (name: string) => facts.get(name)?.time ?? 0;
  const newest = (a: string, b: string) => time(b) - time(a);
  const isArchived = (name: string) => facts.get(name)?.archived === true;
  const isPinned = (name: string) => facts.get(name)?.pinned != null;
  // 今のブランチがほかのブランチの上に積んであれば、その積み重ねの根元ごと一番上に出す（積み重ねを切らない）
  let stackRoot = head;
  const seen = new Set<string>();
  for (;;) {
    const parent = stackRoot ? facts.get(stackRoot)?.parent : null;
    if (!stackRoot || !parent || seen.has(parent) || !exists.has(parent) || isArchived(parent)) break;
    seen.add(stackRoot);
    stackRoot = parent;
  }
  // 根元がお気に入りなら、そのお気に入りの下に入れ子で出す（お気に入りはもう一番上にある）
  const isTop = (name: string) => (name === stackRoot && !isPinned(name)) || name === base;
  // 土台のブランチ → その上に積んだブランチ（土台がアーカイブ済みなら、積んだほうは自分の場所に出す。
  // お気に入りは入れ子にしない）
  const stacked = new Map<string, string[]>();
  const nested = new Set<string>();
  for (const name of locals) {
    if (isTop(name) || isArchived(name) || isPinned(name)) continue;
    const parent = facts.get(name)?.parent;
    if (!parent || !exists.has(parent) || isArchived(parent)) continue;
    stacked.set(parent, [...(stacked.get(parent) ?? []), name]);
    nested.add(name);
  }
  const visiting = new Set<string>();
  const tree = (name: string, stackedOn?: string): Arranged => {
    // 親をたどって輪になっていても止まる
    if (visiting.has(name)) return { name, stack: [], ...(stackedOn ? { stackedOn } : {}) };
    visiting.add(name);
    const stack = [...(stacked.get(name) ?? [])].sort(newest).map((child) => tree(child, name));
    visiting.delete(name);
    return { name, stack, ...(stackedOn ? { stackedOn } : {}) };
  };
  const result: Arrangement = { pinned: [], top: [], active: [], inactive: [], archived: [] };
  const pinned: string[] = [];
  const top: string[] = [];
  const active: string[] = [];
  const inactive: string[] = [];
  const archived: string[] = [];
  for (const name of locals) {
    if (nested.has(name)) continue;
    const f = facts.get(name);
    if (isPinned(name)) pinned.push(name);
    else if (isTop(name)) top.push(name);
    else if (isArchived(name)) archived.push(name);
    else if (f && !elsewhere(name) && f.time !== null && now - f.time > STALE_DAYS * 86_400) inactive.push(name);
    else active.push(name);
  }
  // お気に入りは決めた順（順位が同じなら名前順）。積んだブランチでも平らに出し、土台は行の右に書く
  pinned.sort((a, b) => (facts.get(a)!.pinned! - facts.get(b)!.pinned!) || a.localeCompare(b));
  result.pinned = pinned.map((name) => {
    const parent = facts.get(name)?.parent;
    const node = tree(name);
    return parent && exists.has(parent) ? { ...node, stackedOn: parent } : node;
  });
  // 積み重ねの根元を先に、既定のブランチを後に
  result.top = top.sort((a, b) => Number(b === stackRoot) - Number(a === stackRoot)).map((name) => tree(name));
  result.active = active.sort(newest).map((name) => tree(name));
  result.inactive = inactive.sort(newest).map((name) => tree(name));
  result.archived = archived.sort((a, b) => a.localeCompare(b)).map((name) => tree(name));
  return result;
}

/** お気に入りに足した・外した後の順（上から）。`names` を足す（すでにあれば何もしない）か外す */
export function pinnedOrder(current: readonly string[], names: readonly string[], add: boolean): string[] {
  return add ? [...current, ...names.filter((n) => !current.includes(n))] : current.filter((n) => !names.includes(n));
}
