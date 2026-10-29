/**
 * パッチのファイルを読む（当てる前の確認と、Undo のために触るファイルを知るため）。当てるのは git（`git am`・
 * `git apply`）で、ここは中身を見るだけ。コアの `crates/core/src/patch.rs` と同じ読み方。vscode に依存しない
 */

/** `mailbox`（`git am` でコミットとして当てる）か `diff`（`git apply` で作業ツリーに当てる） */
export type PatchKind = 'mailbox' | 'diff';

export interface PatchFileChange {
  /** 当てた後のパス（消すファイルなら消す前のパス） */
  path: string;
  /** 名前が変わるときの、当てる前のパス */
  oldPath?: string;
  added: number;
  removed: number;
  binary: boolean;
  status: 'modified' | 'added' | 'deleted' | 'renamed' | 'copied';
}

export interface Patch {
  /** `git am` がコミットの件名にするもの（`[PATCH 1/3]` や `Re:` を除いたもの） */
  subject: string;
  author?: string;
  authorEmail?: string;
  files: PatchFileChange[];
}

export interface PatchSummary {
  kind: PatchKind;
  /** メールの形なら 1 通ずつ。diff なら 1 つ（件名はなし） */
  patches: Patch[];
}

/** パッチが触るファイルのパス（当てる前と後の両方。重なりは除く） */
export function patchPaths(summary: PatchSummary): string[] {
  const out = new Set<string>();
  for (const file of summary.patches.flatMap((p) => p.files)) {
    out.add(file.path);
    if (file.oldPath) out.add(file.oldPath);
  }
  return [...out];
}

const isFromLine = (line: string): boolean => line.startsWith('From ') && line.includes(':');

/** パッチのファイルを読む。メールの形（先頭が `From ` の行か、メールのヘッダー）なら 1 通ずつに分ける */
export function parsePatch(text: string): PatchSummary {
  const lines = text.split('\n').map((l) => l.replace(/\r$/, ''));
  if (!isMailbox(lines)) {
    return { kind: 'diff', patches: [{ subject: '', files: parseDiff(lines) }] };
  }
  // `git mailsplit` と同じく、空行の次（か先頭）の `From ` の区切りで分ける
  const messages: string[][] = [];
  let start = 0;
  for (let i = 1; i < lines.length; i++) {
    if (isFromLine(lines[i]!) && lines[i - 1]!.trim() === '') {
      messages.push(lines.slice(start, i));
      start = i;
    }
  }
  messages.push(lines.slice(start));
  return {
    kind: 'mailbox',
    patches: messages.filter((m) => m.some((l) => l.trim() !== '')).map(parseMessage),
  };
}

function isMailbox(lines: readonly string[]): boolean {
  const first = lines.findIndex((l) => l.trim() !== '');
  if (first < 0) return false;
  if (isFromLine(lines[first]!)) return true;
  let mail = false;
  for (const line of lines.slice(first)) {
    if (line === '') break;
    if (/^[ \t]/.test(line)) continue;
    const name = /^([^\s:]+):/.exec(line)?.[1];
    if (!name) return false;
    mail ||= /^(from|subject)$/i.test(name);
  }
  return mail;
}

function parseMessage(message: string[]): Patch {
  let lines = message;
  if (lines[0] && isFromLine(lines[0])) lines = lines.slice(1);
  const end = lines.findIndex((l) => l === '');
  const headers = unfold(lines.slice(0, end < 0 ? lines.length : end));
  const body = end < 0 ? [] : lines.slice(end + 1);
  const patch: Patch = { subject: '', files: [] };
  const apply = ([name, value]: [string, string]): void => {
    if (/^subject$/i.test(name)) {
      patch.subject = cleanSubject(decodeWords(value));
    } else if (/^from$/i.test(name)) {
      const { name: author, email } = splitAddress(decodeWords(value));
      patch.author = author;
      patch.authorEmail = email;
    }
  };
  headers.forEach(apply);
  // 本文の先頭の From:・Date:・Subject:（作者が送り手と違うときに git format-patch が書く）
  let bodyStart = 0;
  const first = body.findIndex((l) => l.trim() !== '');
  if (first >= 0 && /^(from|date|subject):/i.test(body[first]!)) {
    const stop = body.slice(first).findIndex((l) => l.trim() === '');
    const count = stop < 0 ? body.length - first : stop;
    unfold(body.slice(first, first + count)).forEach(apply);
    bodyStart = first + count;
  }
  patch.files = parseDiff(body.slice(bodyStart));
  return patch;
}

function unfold(lines: readonly string[]): [string, string][] {
  const out: [string, string][] = [];
  for (const line of lines) {
    const last = out[out.length - 1];
    if (/^[ \t]/.test(line)) {
      if (last) last[1] += ` ${line.trim()}`;
      continue;
    }
    const colon = line.indexOf(':');
    if (colon > 0) out.push([line.slice(0, colon).trim(), line.slice(colon + 1).trim()]);
  }
  return out;
}

/** `git mailinfo` と同じく、件名の先頭の `Re:` と `[...]` を除く */
export function cleanSubject(subject: string): string {
  let s = subject.trim();
  for (;;) {
    if (/^re:/i.test(s)) {
      s = s.slice(3).trimStart();
    } else if (s.startsWith('[') && s.includes(']')) {
      s = s.slice(s.indexOf(']') + 1).trimStart();
    } else {
      return s;
    }
  }
}

function splitAddress(value: string): { name?: string; email?: string } {
  const m = /^(.*)<([^>]*)>\s*$/.exec(value.trim());
  if (m) {
    const name = m[1]!.trim().replace(/^"|"$/g, '').trim();
    const email = m[2]!.trim();
    return { name: name || undefined, email: email || undefined };
  }
  return value.includes('@') ? { email: value.trim() } : { name: value.trim() || undefined };
}

/** RFC 2047 の encoded-word（`=?UTF-8?q?...?=`・`=?UTF-8?b?...?=`）を戻す。並んだ encoded-word の間の空白は除く */
export function decodeWords(value: string): string {
  return value
    .replace(/(\?=)\s+(=\?)/g, '$1$2')
    .replace(/=\?([^?]+)\?([qQbB])\?([^?]*)\?=/g, (whole, charset: string, encoding: string, data: string) => {
      let bytes: Buffer;
      if (encoding.toLowerCase() === 'b') {
        bytes = Buffer.from(data, 'base64');
      } else {
        const out: number[] = [];
        for (let i = 0; i < data.length; i++) {
          const c = data[i]!;
          if (c === '_') out.push(0x20);
          else if (c === '=' && /^[0-9a-fA-F]{2}$/.test(data.slice(i + 1, i + 3))) {
            out.push(parseInt(data.slice(i + 1, i + 3), 16));
            i += 2;
          } else out.push(c.charCodeAt(0));
        }
        bytes = Buffer.from(out);
      }
      const name = charset.split('*')[0]!.toLowerCase();
      return name === 'iso-8859-1' || name === 'latin1' ? bytes.toString('latin1') : bytes.toString('utf8') || whole;
    });
}

/** diff のファイルごとの変更。行の数は hunk のヘッダーの数だけ数える（本文の `-- ` の署名を取り違えないように） */
function parseDiff(lines: readonly string[]): PatchFileChange[] {
  const files: PatchFileChange[] = [];
  let left: [number, number] = [0, 0];
  // `diff --git` の行から、最初の hunk までの間
  let inHeader = false;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!;
    const file = files[files.length - 1];
    if (left[0] > 0 || left[1] > 0) {
      if (line.startsWith('+')) {
        left[1]--;
        if (file) file.added++;
      } else if (line.startsWith('-')) {
        left[0]--;
        if (file) file.removed++;
      } else if (!line.startsWith('\\')) {
        left = [left[0] - 1, left[1] - 1];
      }
      continue;
    }
    if (line.startsWith('diff --git ')) {
      const [a, b] = splitGitPaths(line.slice('diff --git '.length));
      files.push({ path: b ?? '', oldPath: a, added: 0, removed: 0, binary: false, status: 'modified' });
      inHeader = true;
    } else if (line.startsWith('@@ ')) {
      const m = /^@@ -\d+(?:,(\d+))? \+\d+(?:,(\d+))? @@/.exec(line);
      left = m ? [Number(m[1] ?? 1), Number(m[2] ?? 1)] : [0, 0];
      inHeader = false;
    } else if (line.startsWith('--- ') && lines[i + 1]?.startsWith('+++ ')) {
      const oldPath = diffPath(line.slice(4));
      const newPath = diffPath(lines[i + 1]!.slice(4));
      i++;
      if (!inHeader) files.push({ path: '', added: 0, removed: 0, binary: false, status: 'modified' });
      inHeader = false;
      const f = files[files.length - 1]!;
      if (oldPath === undefined && newPath !== undefined) {
        Object.assign(f, { status: 'added', path: newPath, oldPath: undefined });
      } else if (newPath === undefined && oldPath !== undefined) {
        Object.assign(f, { status: 'deleted', path: oldPath, oldPath: undefined });
      } else if (oldPath !== undefined && newPath !== undefined) {
        f.oldPath = oldPath;
        f.path = newPath;
      }
    } else if (file) {
      if (line.startsWith('new file mode')) {
        file.status = 'added';
        file.oldPath = undefined;
      } else if (line.startsWith('deleted file mode')) {
        file.status = 'deleted';
        if (file.oldPath) file.path = file.oldPath;
        file.oldPath = undefined;
      } else if (line.startsWith('rename from ')) {
        file.status = 'renamed';
        file.oldPath = unquote(line.slice('rename from '.length));
      } else if (line.startsWith('rename to ')) {
        file.path = unquote(line.slice('rename to '.length));
      } else if (line.startsWith('copy from ')) {
        file.status = 'copied';
        file.oldPath = unquote(line.slice('copy from '.length));
      } else if (line.startsWith('copy to ')) {
        file.path = unquote(line.slice('copy to '.length));
      } else if (line === 'GIT binary patch' || line.startsWith('Binary files ')) {
        file.binary = true;
      }
    }
  }
  for (const f of files) {
    if (f.oldPath === f.path) f.oldPath = undefined;
    if (f.status === 'modified' && f.oldPath) f.status = 'renamed';
  }
  return files;
}

const stripPrefix = (s: string): string => (s.includes('/') ? s.slice(s.indexOf('/') + 1) : s);

function diffPath(s: string): string | undefined {
  const path = s.split('\t')[0]!.trimEnd();
  return path === '/dev/null' ? undefined : stripPrefix(unquote(path));
}

function splitGitPaths(rest: string): [string | undefined, string | undefined] {
  if (rest.startsWith('"')) {
    const [a, tail] = takeQuoted(rest);
    const t = tail.trimStart();
    return [stripPrefix(a), stripPrefix(t.startsWith('"') ? takeQuoted(t)[0] : t)];
  }
  const quoted = rest.indexOf(' "');
  if (quoted >= 0) return [stripPrefix(rest.slice(0, quoted)), stripPrefix(takeQuoted(rest.slice(quoted + 1))[0])];
  // 同じ名前の 2 つに分けられれば、それ（名前を変えないとき）
  if (rest.length % 2 === 1) {
    const mid = (rest.length - 1) / 2;
    if (rest[mid] === ' ' && stripPrefix(rest.slice(0, mid)) === stripPrefix(rest.slice(mid + 1))) {
      const path = stripPrefix(rest.slice(0, mid));
      return [path, path];
    }
  }
  const b = rest.indexOf(' b/');
  if (b >= 0) return [stripPrefix(rest.slice(0, b)), rest.slice(b + 3)];
  const space = rest.indexOf(' ');
  return space < 0 ? [undefined, undefined] : [stripPrefix(rest.slice(0, space)), stripPrefix(rest.slice(space + 1))];
}

/** 引用符で囲んだ文字列（C の書き方の `\303\251` などを戻す）と、その後ろ */
function takeQuoted(s: string): [string, string] {
  const out: number[] = [];
  let i = 1;
  while (i < s.length) {
    const c = s[i]!;
    if (c === '"') return [Buffer.from(out).toString('utf8'), s.slice(i + 1)];
    if (c === '\\' && i + 1 < s.length) {
      const n = s[i + 1]!;
      if (/[0-7]/.test(n)) {
        out.push(parseInt(s.slice(i + 1, i + 4), 8));
        i += 4;
        continue;
      }
      out.push(n === 'n' ? 10 : n === 't' ? 9 : n.charCodeAt(0));
      i += 2;
      continue;
    }
    out.push(...Buffer.from(c, 'utf8'));
    i++;
  }
  return [Buffer.from(out).toString('utf8'), ''];
}

const unquote = (s: string): string => (s.startsWith('"') ? takeQuoted(s)[0] : s);
