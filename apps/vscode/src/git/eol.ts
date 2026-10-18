/**
 * 作業ツリーのファイルを git に入れる形にする改行の変換（git の convert.c と同じ規則）。vscode に依存しない。
 * 差分を index の blob と比べられるようにするため。`core.autocrlf=true`（Windows の既定）では、作業ツリーは CRLF、
 * index は LF になる。
 */

/** `git check-attr` の値。指定がなければ undefined、`text` は 'set'、`-text` は 'unset'、`text=auto` は 'auto' */
export type Attributes = Partial<Record<(typeof ATTRIBUTES)[number], string>>;

/** 改行の変換と、それ以外の変換（この拡張では再現しない）に関わる属性 */
export const ATTRIBUTES = ['text', 'eol', 'crlf', 'filter', 'working-tree-encoding', 'ident'] as const;

/** `git check-attr -z <ATTRIBUTES> -- <path>` の出力を読む */
export function parseCheckAttr(out: string): Attributes {
  const parts = out.split('\0');
  const attrs: Attributes = {};
  for (let i = 0; i + 2 < parts.length; i += 3) {
    const [name, value] = [parts[i + 1]!, parts[i + 2]!];
    if (value !== 'unspecified' && (ATTRIBUTES as readonly string[]).includes(name)) {
      attrs[name as keyof Attributes] = value;
    }
  }
  return attrs;
}

/**
 * git に入れる形。改行の変換がかからなければ data をそのまま返す。
 * autocrlf は `core.autocrlf` の値。index はその path の index の blob（自動の変換は、index に CR があればしない）。
 * 外部のフィルター・`working-tree-encoding`・`ident` は再現しないので、そのときも data をそのまま返す（今までどおり）
 */
export function toGitForm(data: Buffer, attrs: Attributes, autocrlf: string | undefined, index: Buffer | undefined): Buffer {
  if (attrs.filter !== undefined || attrs['working-tree-encoding'] !== undefined || attrs.ident === 'set') {
    return data;
  }
  const action = crlfAction(attrs, autocrlf);
  if (action === 'none' || !data.includes(0x0d)) {
    return data;
  }
  if (action === 'auto' && (looksBinary(data) || index?.includes(0x0d))) {
    return data;
  }
  return Buffer.from(data.toString('latin1').replace(/\r\n/g, '\n'), 'latin1');
}

/** 改行を変換するか。text は必ず、auto は中身がテキストなら、none は変換しない */
function crlfAction(attrs: Attributes, autocrlf: string | undefined): 'text' | 'auto' | 'none' {
  const text =
    attrs.text ?? (attrs.crlf === 'set' || attrs.crlf === 'input' ? 'set' : attrs.crlf === 'unset' ? 'unset' : undefined);
  if (text === 'unset') {
    return 'none';
  }
  if (text === 'auto') {
    return 'auto';
  }
  // eol を指定すれば text とみなす
  if (text === 'set' || attrs.eol === 'lf' || attrs.eol === 'crlf') {
    return 'text';
  }
  const auto = autocrlf?.toLowerCase();
  return auto === 'true' || auto === 'input' || auto === 'yes' || auto === 'on' || auto === '1' ? 'auto' : 'none';
}

/** git の自動の判定と同じく、単独の CR・NUL・制御文字の多いものはバイナリとみなす */
function looksBinary(data: Buffer): boolean {
  let printable = 0;
  let nonPrintable = 0;
  for (let i = 0; i < data.length; i++) {
    const c = data[i]!;
    if (c === 0x0d) {
      if (data[i + 1] !== 0x0a) return true;
      continue;
    }
    if (c === 0) return true;
    if (c === 0x7f || (c < 0x20 && c !== 0x08 && c !== 0x09 && c !== 0x0a && c !== 0x0c && c !== 0x1b)) {
      nonPrintable++;
    } else {
      printable++;
    }
  }
  return printable >> 7 < nonPrintable;
}
