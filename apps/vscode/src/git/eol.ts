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

/** `git config --get-regexp '^core\.(autocrlf|eol)$'` の出力を読む（キーは小文字で出る） */
export function parseEolConfig(out: string): EolConfig {
  const config: EolConfig = {};
  for (const line of out.split('\n')) {
    const [key, value] = [line.slice(0, line.indexOf(' ')), line.slice(line.indexOf(' ') + 1).trim()];
    if (key === 'core.autocrlf') config.autocrlf = value;
    else if (key === 'core.eol') config.eol = value;
  }
  return config;
}

/** ファイルの改行の種類（`git ls-files --eol` の `i/`・`w/` と同じ見方） */
export type Eol = 'none' | 'lf' | 'crlf' | 'mixed';

/**
 * Working Copy の 1 ファイルの改行。差分は git に入れる形どうしで比べるので、改行の変換や混ざった改行は差分に
 * 出ない。それを見せるためのもの（コアの `LineEndings` と同じ）
 */
export interface LineEndings {
  /** git（index）に入っている形 */
  index: Eol;
  worktree: Eol;
  /** index の内容を checkout したときの形 */
  checkout: Eol;
  /** かかっている変換（`text=auto eol=crlf`・`core.autocrlf=true` など。なければ空） */
  rule: string;
  /** 見せる値打ちがある（改行を変換している、または warning） */
  notable: boolean;
  /** 改行が混ざっている、または作業ツリーが CRLF なのに checkout すると LF になる */
  warning: boolean;
}

/** data の改行の種類。NUL を含む（バイナリ）なら none */
export function eolOf(data: Buffer): Eol {
  if (data.subarray(0, 8000).includes(0)) {
    return 'none';
  }
  let lf = false;
  let crlf = false;
  for (let i = data.indexOf(0x0a); i >= 0; i = data.indexOf(0x0a, i + 1)) {
    if (i > 0 && data[i - 1] === 0x0d) crlf = true;
    else lf = true;
    if (lf && crlf) return 'mixed';
  }
  return lf ? 'lf' : crlf ? 'crlf' : 'none';
}

/** 改行の設定（`core.autocrlf`・`core.eol`） */
export interface EolConfig {
  autocrlf?: string;
  eol?: string;
}

/**
 * ファイルの改行。raw は作業中のファイルの生のバイト、index は index の blob（なければ空）。
 * checkout したときの形は、git の checkout と同じ規則で求める（`working-tree-encoding` などは見ない）
 */
export function lineEndingsOf(raw: Buffer, index: Buffer, attrs: Attributes, config: EolConfig): LineEndings {
  const indexEol = eolOf(index);
  const worktree = eolOf(raw);
  const checkout = index.length > 0 && toCrlfOnCheckout(index, attrs, config) ? 'crlf' : indexEol;
  const differs = (a: Eol, b: Eol) => a !== b && a !== 'none' && b !== 'none';
  // 作業ツリーが LF で checkout すると CRLF になるだけ（core.autocrlf=true でツールが LF で書いたファイル）は、
  // git に入る形が変わらないので気にしない
  const warning = worktree === 'mixed' || indexEol === 'mixed' || (worktree === 'crlf' && checkout === 'lf');
  const notable = differs(indexEol, worktree) || warning;
  return { index: indexEol, worktree, checkout, rule: notable ? lineEndingRule(attrs, config) : '', notable, warning };
}

/** checkout で LF を CRLF にするか */
function toCrlfOnCheckout(index: Buffer, attrs: Attributes, config: EolConfig): boolean {
  if (eolOf(index) === 'none' || attrs['working-tree-encoding'] !== undefined) {
    return false;
  }
  const action = crlfAction(attrs, config.autocrlf);
  if (action === 'none') {
    return false;
  }
  // 自動の判定では、CR を含むかバイナリに見える blob は変換しない
  if (action === 'auto' && (index.includes(0x0d) || looksBinary(index))) {
    return false;
  }
  if (attrs.eol === 'crlf' || attrs.eol === 'lf') {
    return attrs.eol === 'crlf';
  }
  if (attrs.crlf === 'input') {
    return false;
  }
  const auto = config.autocrlf?.toLowerCase();
  if (auto === 'input') return false;
  if (auto === 'true' || auto === 'yes' || auto === 'on' || auto === '1') return true;
  const eol = config.eol?.toLowerCase();
  return eol === 'crlf' || ((eol === undefined || eol === 'native') && process.platform === 'win32');
}

/** かかっている変換を `git check-attr` と同じ書き方で（コアの `line_ending_rule` と同じ） */
export function lineEndingRule(attrs: Attributes, config: EolConfig): string {
  const parts: string[] = [];
  for (const name of ['text', 'eol', 'crlf', 'working-tree-encoding'] as const) {
    const value = attrs[name];
    if (value === 'set') parts.push(name);
    else if (value === 'unset') parts.push(`-${name}`);
    else if (value !== undefined) parts.push(`${name}=${value}`);
  }
  // 設定は効くときだけ足す。テキストでないとしたファイルと、eol の属性があるファイルには効かない
  const binary = attrs.text === 'unset' || attrs.crlf === 'unset';
  const text = attrs.text === 'set' || attrs.text === 'auto' || attrs.crlf === 'set';
  const auto = config.autocrlf?.toLowerCase();
  const eol = config.eol?.toLowerCase();
  if (!binary && attrs.eol === undefined) {
    if (auto === 'true' || auto === 'yes' || auto === 'on' || auto === '1') parts.push('core.autocrlf=true');
    else if (auto === 'input') parts.push('core.autocrlf=input');
    else if (text && (eol === 'lf' || eol === 'crlf')) parts.push(`core.eol=${eol}`);
  }
  return parts.join(' ');
}
