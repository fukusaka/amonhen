/** Working Copy のコミット欄のメッセージの扱い。vscode に依存しない */

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
