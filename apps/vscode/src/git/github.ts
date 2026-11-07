/**
 * GitHub（gh）から読むもの。アプリと同じく、リモートの URL から GitHub のリポジトリを見分け、gh に `-R` で渡す。
 * vscode に依存しない
 */
import { execFile } from 'node:child_process';
import type { Remote } from './refs.ts';

/** マージされたプルリクエスト（コアの `branchCleanup` の `pullRequests` の形） */
export interface MergedPullRequest {
  number: number;
  branch: string;
  oid: string;
}

/**
 * リモートの URL（`https://github.com/o/r.git`・`git@github.com:o/r.git`・`ssh://git@github.com/o/r`）の、gh の
 * `-R` に渡す形（github.com なら `o/r`、ほかのホストなら `host/o/r`）。ホストの名前に github を含むものだけ
 * （GitHub Enterprise を含む）
 */
export function gitHubSlug(url: string): string | undefined {
  let rest = url.trim();
  let host: string;
  const scheme = rest.indexOf('://');
  if (scheme >= 0) {
    rest = rest.slice(scheme + 3);
    const at = rest.indexOf('@');
    if (at >= 0 && !rest.slice(0, at).includes('/')) rest = rest.slice(at + 1);
    const slash = rest.indexOf('/');
    if (slash < 0) return undefined;
    host = rest.slice(0, slash);
    rest = rest.slice(slash + 1);
  } else {
    // scp 形式: user@host:owner/repo
    const colon = rest.indexOf(':');
    if (colon < 0) return undefined;
    host = rest.slice(0, colon);
    host = host.slice(host.indexOf('@') + 1);
    rest = rest.slice(colon + 1);
  }
  host = host.split(':')[0]!;
  if (!host.toLowerCase().includes('github')) return undefined;
  const parts = rest.replace(/\.git$/, '').split('/').filter(Boolean);
  if (parts.length < 2) return undefined;
  const [owner, name] = parts.slice(-2) as [string, string];
  return host === 'github.com' ? `${owner}/${name}` : `${host}/${owner}/${name}`;
}

/** リポジトリの GitHub の slug（origin、なければ最初のリモート）。GitHub でなければ undefined */
export function repositorySlug(remotes: readonly Remote[]): string | undefined {
  const remote = remotes.find((r) => r.name === 'origin') ?? remotes[0];
  return remote?.fetchUrl ? gitHubSlug(remote.fetchUrl) : undefined;
}

/** gh が見つからない */
export class GhMissingError extends Error {}

function runGh(args: readonly string[], gh: string): Promise<string> {
  const env = { ...process.env, GH_PROMPT_DISABLED: '1', NO_COLOR: '1' };
  return new Promise((resolve, reject) => {
    execFile(gh, args, { windowsHide: true, env, timeout: 30_000, maxBuffer: 8 << 20 }, (err, stdout, stderr) => {
      if (err) {
        reject((err as NodeJS.ErrnoException).code === 'ENOENT' ? new GhMissingError('gh (GitHub CLI) wasn’t found') : new Error(stderr.trim() || err.message));
        return;
      }
      resolve(stdout);
    });
  });
}

/** プルリクエストを出す先。fork なら元のリポジトリで、`forkOwner` はその fork の持ち主 */
export interface PullRequestTarget {
  slug: string;
  forkOwner?: string;
}

/** `gh repo view --json parent` の答えから、`slug` の出す先（元は同じホスト） */
export function targetFromView(slug: string, view: string): PullRequestTarget {
  const parent = (JSON.parse(view) as { parent?: { name: string; owner: { login: string } } | null }).parent;
  if (!parent) return { slug };
  const parts = slug.split('/');
  const host = parts.length > 2 ? `${parts[0]}/` : '';
  return { slug: `${host}${parent.owner.login}/${parent.name}`, forkOwner: parts[parts.length - 2]! };
}

/** 読めた出す先（slug ごと）。読めなかったものは覚えない（次にまた読む） */
const targets = new Map<string, PullRequestTarget>();

/** `slug` のプルリクエストを出す先。読めなければ fork でないものとして扱う */
export async function pullRequestTarget(slug: string, gh = 'gh'): Promise<PullRequestTarget> {
  const known = targets.get(slug);
  if (known) return known;
  try {
    const target = targetFromView(slug, await runGh(['repo', 'view', slug, '--json', 'parent'], gh));
    targets.set(slug, target);
    return target;
  } catch {
    return { slug };
  }
}

/** `gh pr list` の答えの、この先に出したもの（fork なら、その fork から出したものだけ） */
export function parseMergedPullRequests(stdout: string, forkOwner?: string): MergedPullRequest[] {
  const list = JSON.parse(stdout) as { number: number; headRefName: string; headRefOid: string; headRepositoryOwner?: { login: string } | null }[];
  return list
    .filter((p) => !forkOwner || p.headRepositoryOwner?.login.toLowerCase() === forkOwner.toLowerCase())
    .map((p) => ({ number: p.number, branch: p.headRefName, oid: p.headRefOid }));
}

/**
 * マージされたプルリクエスト（新しい順、最大 200 件）。squash・rebase で取り込んだブランチを見分ける。fork なら、元の
 * リポジトリの新しいものから数えるとほかの人のもので埋まるので、作者で絞ってから数える
 */
export async function mergedPullRequests(target: PullRequestTarget, gh = 'gh'): Promise<MergedPullRequest[]> {
  const fork = target.forkOwner ? { field: ',headRepositoryOwner', args: ['--author', target.forkOwner] } : { field: '', args: [] };
  const args = ['pr', 'list', '-R', target.slug, '--state', 'merged', '--limit', '200', '--json', `number,headRefName,headRefOid${fork.field}`, ...fork.args];
  return parseMergedPullRequests(await runGh(args, gh), target.forkOwner);
}
