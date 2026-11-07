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
