/**
 * git の失敗の見分け（コアの `crates/core/src/failure.rs` と同じ規則）。エラーの文から、要点の言い直しと次の一手を
 * 出せる種類を決める。見分けられなければ undefined（git の文をそのまま出す）。vscode に依存しない
 */

export type FailureKind =
  | 'worktreeInUse'
  | 'auth'
  | 'hostKey'
  | 'network'
  | 'rejected'
  | 'diverged'
  | 'localChanges'
  | 'locked'
  | 'noUpstream'
  | 'octopus';

export interface Failure {
  kind: FailureKind;
  /** 失敗に関わるパス（ほかの worktree の場所、ロックファイル） */
  path?: string;
  /** ほかの worktree で checkout しているブランチ（git の文から読めたとき） */
  branch?: string;
}

/** `marker` のすぐ前の `'…'` の中身 */
function quotedBefore(message: string, marker: string): string | undefined {
  const at = message.indexOf(`' ${marker}`);
  if (at < 0) return undefined;
  const start = message.lastIndexOf("'", at - 1);
  return start < 0 ? undefined : message.slice(start + 1, at);
}

/** `marker` のすぐ後の `'…'` の中身 */
function quotedAfter(message: string, marker: string): string | undefined {
  const at = message.indexOf(marker);
  if (at < 0) return undefined;
  const rest = message.slice(at + marker.length);
  if (!rest.startsWith("'")) return undefined;
  const end = rest.indexOf("'", 1);
  return end < 0 ? undefined : rest.slice(1, end);
}

export function classifyFailure(message: string): Failure | undefined {
  const has = (s: string) => message.includes(s);
  const worktree = quotedAfter(message, 'is already used by worktree at ') ?? quotedAfter(message, 'is already checked out at ');
  if (worktree) {
    const branch = quotedBefore(message, 'is already used by worktree at') ?? quotedBefore(message, 'is already checked out at');
    return { kind: 'worktreeInUse', path: worktree, ...(branch ? { branch } : {}) };
  }
  if (has("Unable to create '") && has(".lock': File exists")) {
    const path = quotedAfter(message, 'Unable to create ');
    return { kind: 'locked', ...(path ? { path } : {}) };
  }
  if (has('Host key verification failed') || has('REMOTE HOST IDENTIFICATION HAS CHANGED')) return { kind: 'hostKey' };
  if (
    has('could not read Username') ||
    has('could not read Password') ||
    has('Authentication failed') ||
    has('Invalid username or password') ||
    has('Permission denied (publickey') ||
    has('The requested URL returned error: 401') ||
    has('The requested URL returned error: 403') ||
    (has('remote: Permission to') && has('denied'))
  ) {
    return { kind: 'auth' };
  }
  if (
    has('Could not resolve host') ||
    has('Could not resolve hostname') ||
    has('Connection refused') ||
    has('Connection timed out') ||
    has('Operation timed out') ||
    has('Network is unreachable') ||
    has('Failed to connect to')
  ) {
    return { kind: 'network' };
  }
  if (has('[rejected]') || has('Updates were rejected')) return { kind: 'rejected' };
  if (has('Not possible to fast-forward') || has("Diverging branches can't be fast-forwarded")) return { kind: 'diverged' };
  if (
    has('Your local changes to the following files would be overwritten') ||
    has('untracked working tree files would be overwritten') ||
    has('not uptodate. Cannot merge')
  ) {
    return { kind: 'localChanges' };
  }
  if (has('Merge with strategy octopus failed')) return { kind: 'octopus' };
  if (has('There is no tracking information for the current branch') || has('has no upstream branch')) return { kind: 'noUpstream' };
  return undefined;
}

/**
 * 失敗した操作の見出し（アプリの帯の見出し。`apps/macos/Sources/AmonHen/Failures.swift` の title と同じ文）。op は
 * コアの命令の名前。見出しのない操作は「Couldn’t Finish」
 */
export function failureTitle(op: string): string {
  switch (op) {
    case 'checkout':
    case 'track':
    case 'detach':
      return 'Couldn’t Checkout';
    case 'pull':
      return 'Couldn’t Pull';
    case 'push':
    case 'pushTags':
      return 'Couldn’t Push';
    case 'fetch':
    case 'deepen':
    case 'unshallow':
    case 'fetchAllContents':
      return 'Couldn’t Fetch';
    case 'sparseCheckout':
    case 'disableSparseCheckout':
      return 'Couldn’t Change the Folders';
    case 'maintenance':
      return 'Couldn’t Change git maintenance';
    case 'merge':
    case 'mergeInto':
      return 'Couldn’t Merge';
    case 'rebase':
      return 'Couldn’t Rebase';
    case 'cherryPick':
      return 'Couldn’t Cherry-pick';
    case 'revert':
      return 'Couldn’t Revert';
    case 'formatPatch':
      return 'Couldn’t Save the Patches';
    case 'am':
    case 'amThreeWay':
    case 'applyPatch':
      return 'Couldn’t Apply the Patch';
    case 'commit':
      return 'Couldn’t Commit';
    case 'continue':
      return 'Couldn’t Continue';
    case 'deleteBranch':
      return 'Couldn’t Delete the Branch';
    case 'stashPush':
      return 'Couldn’t Stash';
    case 'stashApply':
      return 'Couldn’t Apply the Stash';
    case 'discard':
    case 'deleteUntracked':
      return 'Couldn’t Discard';
    case 'undo':
      return 'Couldn’t Undo';
    default:
      return 'Couldn’t Finish';
  }
}

/**
 * 要点（1 行。アプリの帯と同じ言い直し）。言い直せない種類は undefined。branch は操作したブランチ（ほかの worktree
 * で checkout しているとき、その名前を言う）
 */
export function failureSummary(f: Failure, branch?: string): string | undefined {
  switch (f.kind) {
    case 'worktreeInUse': {
      const place = f.path?.split(/[\\/]/).filter(Boolean).pop() ?? '';
      return `${branch ?? f.branch ?? 'The branch'} is checked out in the worktree ${place}. A branch can be checked out in only one worktree.`;
    }
    case 'auth':
      return 'The remote didn’t accept the sign-in (password, token or SSH key).';
    case 'hostKey':
      return 'The SSH host key of the remote isn’t known yet, or it has changed.';
    case 'network':
      return 'Can’t reach the remote. Check the network and the remote’s address.';
    case 'rejected':
      return 'The remote has commits you don’t have yet. Pull them first, then Push.';
    case 'diverged':
      return 'Both your branch and its upstream have new commits, so it can’t just move forward.';
    case 'localChanges':
      return 'Uncommitted changes would be overwritten. Commit or stash them first.';
    case 'locked':
      // ロックファイルの場所も添える（どれが残っているか分かるように）
      return `Another git command is running in this repository, or one stopped and left a lock file${f.path ? ` (${f.path})` : ''}.`;
    case 'noUpstream':
      return 'This branch has no upstream to pull from or push to.';
    default:
      return undefined;
  }
}

/** 次にどうすればよいか（Details に出す。分からなければ undefined） */
export function failureAdvice(f: Failure): string | undefined {
  switch (f.kind) {
    case 'auth':
      return (
        'The remote refused the password, token or SSH key. If you were asked for a password or passphrase, check what you typed ' +
        '(GitHub needs a token, not the account password). To stop being asked, store the credentials with a credential helper ' +
        '(for example `gh auth setup-git`), or add the SSH key to the agent (`ssh-add`).'
      );
    case 'hostKey':
      return 'Connect once in a terminal (for example `ssh -T git@github.com`) and confirm the host key, then try again.';
    case 'locked':
      return f.path ? `If no other git command is running, delete the lock file and try again:\n${f.path}` : undefined;
    case 'diverged':
      return 'Pull with Merge or Rebase instead of Fast-Forward Only.';
    default:
      return undefined;
  }
}

/** 次の一手（ボタン）。retry は「もう一度」が使えるとき */
export type Remedy = 'openWorktree' | 'openTerminal' | 'retry' | 'pull' | 'workingCopy' | 'setUpstream';

export function failureRemedies(f: Failure, canRetry: boolean): Remedy[] {
  const retry: Remedy[] = canRetry ? ['retry'] : [];
  switch (f.kind) {
    case 'worktreeInUse':
      return f.path ? ['openWorktree'] : [];
    case 'auth':
    case 'hostKey':
      return ['openTerminal', ...retry];
    case 'network':
    case 'locked':
      return retry;
    case 'rejected':
    case 'diverged':
      return ['pull'];
    case 'localChanges':
      return ['workingCopy'];
    case 'noUpstream':
      return ['setUpstream'];
    default:
      return [];
  }
}

/** 失敗した操作（コアの命令の名前）を添えたエラー。showError がアプリの帯と同じ見出し（failureTitle）を付ける */
export class OperationError extends Error {
  readonly op: string;

  constructor(message: string, op: unknown) {
    super(message);
    this.op = typeof op === 'string' ? op : '';
  }
}

/** エラーに添えた操作の名前（なければ undefined） */
export function operationOf(err: unknown): string | undefined {
  return err instanceof OperationError && err.op ? err.op : undefined;
}
