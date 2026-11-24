/**
 * コミットのメッセージを詳細に並べるための切り分け（vscode・DOM に依存しない）。アプリの CommitHeader と同じ決まり
 */

/** 本文の最後の段落がトレーラー（`Key: value` の行だけ）なら、本文と分ける */
export function splitTrailers(body: string): { body: string; trailers: { key: string; value: string }[] } {
  const text = body.replace(/\s+$/, '');
  const split = text.lastIndexOf('\n\n');
  const last = split < 0 ? text : text.slice(split + 2);
  const lines = last.split('\n');
  const trailers: { key: string; value: string }[] = [];
  for (const line of lines) {
    const m = /^([A-Za-z0-9][A-Za-z0-9-]*):\s+(\S.*)$/.exec(line);
    if (!m) return { body: text, trailers: [] };
    trailers.push({ key: m[1]!, value: m[2]! });
  }
  if (trailers.length === 0) return { body: text, trailers: [] };
  return { body: split < 0 ? '' : text.slice(0, split).replace(/\n+$/, ''), trailers };
}

/** History の検索の問い合わせから、件名と本文で強調する語（`author:`・`path:` などは除く。`subject:`・`message:` は値） */
export function highlightTerms(query: string): string[] {
  const terms: string[] = [];
  let word = '';
  let quoted = false;
  const flush = () => {
    const lower = word.toLowerCase();
    if (word && lower !== 'or' && lower !== 'and') {
      const colon = word.indexOf(':');
      if (colon < 0) {
        terms.push(word);
      } else {
        const key = word.slice(0, colon).toLowerCase();
        const value = word.slice(colon + 1);
        if ((key === 'subject' || key === 'message') && value) terms.push(value);
        if (!['author', 'committer', 'commit', 'id', 'path', 'file', 'contents', 'content', 'subject', 'message', 'before', 'after'].includes(key)) {
          terms.push(word);
        }
      }
    }
    word = '';
  };
  for (const c of query) {
    if (c === '"') quoted = !quoted;
    else if (!quoted && (/\s/.test(c) || '()|'.includes(c))) flush();
    else word += c;
  }
  flush();
  return terms;
}

/** 文の中のリンクにする部分（URL と、7〜40 桁の 16 進のコミット ID） */
export type MessagePiece = { text: string } | { url: string } | { commit: string };

export function linkPieces(text: string): MessagePiece[] {
  const pieces: MessagePiece[] = [];
  const re = /(https?:\/\/[^\s<>()"']+[^\s<>()"'.,;:!?])|(?<![0-9A-Za-z/])([0-9a-f]{7,40})(?![0-9A-Za-z])/g;
  let at = 0;
  for (const m of text.matchAll(re)) {
    if (m.index! > at) pieces.push({ text: text.slice(at, m.index) });
    pieces.push(m[1] ? { url: m[1] } : { commit: m[2]! });
    at = m.index! + m[0].length;
  }
  if (at < text.length) pieces.push({ text: text.slice(at) });
  return pieces;
}
