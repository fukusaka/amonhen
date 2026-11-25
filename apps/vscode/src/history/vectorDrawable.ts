/**
 * Android の VectorDrawable か（「すべてのファイル」で絵にするもの）。コアの `vectordrawable::is_vector_drawable` と
 * 同じ決まり: 拡張子が `.xml` で、根の要素が `<vector>` か `<animated-vector>`（頭の 4096 バイトだけを見る。宣言・
 * コメント・DOCTYPE は飛ばす）
 */
export function isVectorDrawable(path: string, data: Uint8Array): boolean {
  if (!/\.xml$/i.test(path)) return false;
  const root = rootElement(data);
  return root === 'vector' || root === 'animated-vector';
}

/** XML の根の要素の名前（接頭辞を除く）。読めなければ undefined */
export function rootElement(data: Uint8Array): string | undefined {
  let head = new TextDecoder().decode(data.subarray(0, 4096));
  if (head.startsWith('﻿')) head = head.slice(1);
  let i = 0;
  for (;;) {
    while (i < head.length && /\s/.test(head[i]!)) i++;
    const rest = head.slice(i);
    const skip = (end: string) => {
      const at = rest.indexOf(end);
      return at < 0 ? undefined : at + end.length;
    };
    let step: number | undefined;
    if (rest.startsWith('<?')) step = skip('?>');
    else if (rest.startsWith('<!--')) step = skip('-->');
    else if (rest.startsWith('<!')) step = skip('>');
    else if (rest.startsWith('<')) {
      const name = /^<([^\s>/]+)[\s>/]/.exec(rest)?.[1];
      const local = name?.split(':').pop();
      return local || undefined;
    } else return undefined;
    if (step === undefined) return undefined;
    i += step;
  }
}
