/**
 * 手元のタグの、リモートでの様子（コアの `tagRemotes`。fetch のときに読んで覚えたもの）と、そこから決まる
 * タグのメニュー・印（アプリのサイドバーと同じ決まり）。vscode に依存しない
 */

export type TagRemoteState = 'same' | 'different' | 'missing';

/** コアの `tagRemotes` の答え */
export interface TagRemotes {
  /** すべてのリモートと、タグを読んだ時刻（読んだことがなければ null） */
  remotes: { name: string; checked: number | null }[];
  /** タグ → 読んだリモート → 様子 */
  tags: Record<string, Record<string, TagRemoteState>>;
}

/** タグを push・置き換え・削除できるリモート */
export interface TagTargets {
  /** まだないリモート（読んでいないリモートも） */
  push: string[];
  /** 同じ名前の別のタグがあるリモート */
  replace: string[];
  /** あるリモート（読んでいないリモートも） */
  onRemote: string[];
}

function checked(info: TagRemotes, remote: string): boolean {
  return info.remotes.some((r) => r.name === remote && r.checked !== null);
}

/**
 * まだないリモートにだけ Push、同じ名前の別のタグがあるリモートには Replace、あるリモートにだけ削除を出す。
 * まだ読んでいないリモートには Push と削除の両方を出す（あるかどうか分からない）
 */
export function tagTargets(info: TagRemotes, tag: string): TagTargets {
  const targets: TagTargets = { push: [], replace: [], onRemote: [] };
  for (const { name: remote } of info.remotes) {
    const state = info.tags[tag]?.[remote];
    if (!checked(info, remote)) {
      targets.push.push(remote);
      targets.onRemote.push(remote);
      continue;
    }
    if (state === 'missing' || state === undefined) targets.push.push(remote);
    if (state === 'different') targets.replace.push(remote);
    if (state === 'same' || state === 'different') targets.onRemote.push(remote);
  }
  return targets;
}

/**
 * タグの行の印: 同じ名前の別のタグがあるリモート（different）か、読んだどのリモートにもない（missing）。
 * どちらでもなければ（読んだリモートがない、どこかにある）undefined
 */
export function tagMark(info: TagRemotes, tag: string): { kind: 'different'; remotes: string[] } | { kind: 'missing' } | undefined {
  const read = info.remotes.filter((r) => r.checked !== null).map((r) => r.name);
  if (read.length === 0) return undefined;
  const different = read.filter((r) => info.tags[tag]?.[r] === 'different');
  if (different.length > 0) return { kind: 'different', remotes: different };
  if (read.every((r) => (info.tags[tag]?.[r] ?? 'missing') === 'missing')) return { kind: 'missing' };
  return undefined;
}

/** 印の短い言葉（行の説明に出す） */
export function tagMarkLabel(mark: NonNullable<ReturnType<typeof tagMark>>): string {
  return mark.kind === 'different' ? `different on ${mark.remotes.join(', ')}` : 'not on any remote';
}

/** タグの、リモート 1 つでの様子の一文 */
export function tagStateText(info: TagRemotes, tag: string, remote: string): string {
  if (!checked(info, remote)) return `${remote}: not checked yet (Fetch reads it)`;
  switch (info.tags[tag]?.[remote]) {
    case 'same':
      return `${remote}: has it`;
    case 'different':
      return `${remote}: has a different tag with this name`;
    default:
      return `${remote}: doesn’t have it`;
  }
}
