/** 文字の中を探す（整形した Markdown の検索。アプリと同じく大文字・小文字を区別しない）。DOM に触れない（試験から使う） */

/** 大文字・小文字をそろえる（長さを変えない: 小文字にすると長さの変わる文字はそのまま） */
function fold(text: string): string {
  const lower = text.toLowerCase();
  if (lower.length === text.length) return lower;
  let out = '';
  for (const c of text) {
    const l = c.toLowerCase();
    out += l.length === c.length ? l : c;
  }
  return out;
}

/** `text` の中の `query` の場所（[始め, 終わり)、文書の順、重ならない）。空の `query` は当たらない */
export function findAll(text: string, query: string): [number, number][] {
  if (!query) return [];
  const haystack = fold(text);
  const needle = fold(query);
  const found: [number, number][] = [];
  for (let at = haystack.indexOf(needle); at >= 0; at = haystack.indexOf(needle, at + Math.max(needle.length, 1))) {
    found.push([at, at + needle.length]);
  }
  return found;
}

/** 数の欄の文字（アプリと同じ: 空なら出さない、一致がなければ「Not found」、あれば「n / m」） */
export function findCountText(query: string, current: number | undefined, count: number): string {
  if (!query) return '';
  return current === undefined ? 'Not found' : `${current + 1} / ${count}`;
}
