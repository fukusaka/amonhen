/** git のフック（コアの HOOK_NAMES と同じ並び）と、それぞれが動くとき（アプリの言い方）。Webview でも使う */

export const HOOK_NAMES = [
  'applypatch-msg',
  'pre-applypatch',
  'post-applypatch',
  'pre-commit',
  'pre-merge-commit',
  'prepare-commit-msg',
  'commit-msg',
  'post-commit',
  'pre-rebase',
  'post-checkout',
  'post-merge',
  'pre-push',
  'pre-receive',
  'update',
  'proc-receive',
  'post-receive',
  'post-update',
  'reference-transaction',
  'push-to-checkout',
  'pre-auto-gc',
  'post-rewrite',
  'sendemail-validate',
  'fsmonitor-watchman',
  'p4-changelist',
  'p4-prepare-changelist',
  'p4-post-changelist',
  'p4-pre-submit',
  'post-index-change',
] as const;

/** フックが動くとき（git の説明の要約） */
export function hookWhen(name: string): string {
  switch (name) {
    case 'pre-commit':
      return 'Before a commit';
    case 'pre-merge-commit':
      return 'Before a merge commit';
    case 'prepare-commit-msg':
      return 'Before the message is edited';
    case 'commit-msg':
      return 'On the commit message';
    case 'post-commit':
      return 'After a commit';
    case 'pre-rebase':
      return 'Before a rebase';
    case 'post-checkout':
      return 'After a checkout';
    case 'post-merge':
      return 'After a merge';
    case 'pre-push':
      return 'Before a push';
    case 'pre-auto-gc':
      return 'Before an automatic gc';
    case 'post-rewrite':
      return 'After commits are rewritten';
    case 'applypatch-msg':
    case 'pre-applypatch':
    case 'post-applypatch':
      return 'Around git am';
    case 'post-index-change':
      return 'After the index changes';
    case 'fsmonitor-watchman':
      return 'For fsmonitor';
    case 'reference-transaction':
      return 'When refs change';
    case 'push-to-checkout':
    case 'pre-receive':
    case 'update':
    case 'post-receive':
    case 'post-update':
    case 'proc-receive':
    case 'sendemail-validate':
      return 'On the receiving side';
    default:
      return '';
  }
}
