/** Working Copy のコミット欄のメッセージの扱い。vscode に依存しない */

/** Conventional Commits の型 */
export const COMMIT_TYPES = ['feat', 'fix', 'docs', 'style', 'refactor', 'perf', 'test', 'build', 'ci', 'chore', 'revert'];

export interface MessageParts {
  summary: string;
  body: string;
}

/** 文字数（サロゲートペアも 1 文字） */
export function messageLength(text: string): number {
  return [...text].length;
}

/**
 * Commit を押せないわけ（押せるときは undefined）。アプリと同じく、コミットしている間はそのこと、そうでなければ
 * チェックしたファイルがない（Amend でないとき）・メッセージがないを改行でつなぐ
 */
export function commitDisabledReason(state: { staged: number; amend: boolean; summary: string; committing: boolean }): string | undefined {
  if (state.committing) return 'Committing…';
  const reasons = [
    ...(state.staged === 0 && !state.amend ? ['No files are checked'] : []),
    ...(!state.summary.trim() ? ['Write a commit message'] : []),
  ];
  return reasons.length > 0 ? reasons.join('\n') : undefined;
}

/** 長さの目安: 50 を超えると長め、72 を超えると長すぎ（git の慣習） */
export function lengthLevel(length: number): 'ok' | 'long' | 'over' {
  return length > 72 ? 'over' : length > 50 ? 'long' : 'ok';
}

/** 本文でいちばん長い行の文字数 */
export function longestLine(body: string): number {
  return Math.max(0, ...body.split('\n').map(messageLength));
}

/** メッセージを件名と本文に分ける */
export function splitMessage(text: string): MessageParts {
  const trimmed = text.replace(/^\s+|\s+$/g, '');
  const newline = trimmed.indexOf('\n');
  if (newline < 0) return { summary: trimmed, body: '' };
  return { summary: trimmed.slice(0, newline).trimEnd(), body: trimmed.slice(newline + 1).replace(/^\n+/, '') };
}

/** テンプレートのファイルの中身から、`#` の行を除いたメッセージを作る */
export function templateMessage(raw: string): MessageParts {
  return splitMessage(
    raw
      .split(/\r?\n/)
      .filter((line) => !line.startsWith('#'))
      .join('\n'),
  );
}

/** 件名の先頭を `type: ` にする（もう型があれば置き換える） */
export function withType(summary: string, type: string): string {
  return `${type}: ${summary.replace(/^[a-z]+(\([^)]*\))?!?:\s*/, '')}`;
}

/** 本文の末尾にトレーラーを足す（もうあれば足さない）。本文とは空行で分け、前の行がトレーラーなら続ける */
export function addTrailer(body: string, line: string): string {
  if (body.includes(line)) return body;
  const text = body.trim();
  if (!text) return line;
  const last = text.split('\n').pop()!;
  return `${text}${/^[A-Za-z-]+: /.test(last) ? '\n' : '\n\n'}${line}`;
}

/** `fixup!`・`squash!` を付けられる件名（新しい順に重ねず 15 個。`rebase --autosquash` は元の件名で探す） */
export function fixupSubjects(messages: readonly string[]): string[] {
  const subjects: string[] = [];
  for (const message of messages) {
    const subject = message.split('\n')[0]!;
    if (!subject || /^(fixup|squash)! /.test(subject) || subjects.includes(subject)) continue;
    subjects.push(subject);
    if (subjects.length === 15) break;
  }
  return subjects;
}
