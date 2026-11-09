/**
 * 失敗の知らせ（アプリのウィンドウの帯の「原因・Details」）。git の出力の `fatal:`・`error:` の行を原因として見せ、
 * 全文は Details で見せる。vscode に依存しない
 */

/** 知らせに出す原因の行（`fatal:`・`error:` の行。なければ最初の空でない行） */
export function failureCause(message: string): string {
  const lines = message
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean);
  const marked = lines.filter((l) => /^(fatal|error):/i.test(l));
  return (marked.length > 0 ? marked : lines.slice(0, 1)).join(' ') || message.trim();
}

/** 原因の行だけでは全文を言い尽くさないか（Details を添えるか） */
export function hasMoreThan(cause: string, message: string): boolean {
  return message.trim().replace(/\s+/g, ' ') !== cause.replace(/\s+/g, ' ');
}
