/**
 * Push の確認（docs/ux.md の 5 の Push。アプリの SyncSheets.swift の pushWarnings と同じ）の決まり。送る前に見ておく
 * とよいこと: 既定のブランチへ直接、仕上がっていないコミット（fixup!・squash!・amend!・WIP）、秘密を持つかもしれない
 * ファイルの追加、バイナリの追加。vscode に依存しない
 */

export interface AddedFile {
  path: string;
  binary: boolean;
}

const DEFAULT_BRANCHES = ['main', 'master', 'trunk', 'develop'];

function unfinished(subject: string): boolean {
  const s = subject.toLowerCase();
  return (
    s.startsWith('fixup!') || s.startsWith('squash!') || s.startsWith('amend!') || s.startsWith('wip') || s.includes(' wip ') || s.endsWith(' wip')
  );
}

function secretLike(path: string): boolean {
  const name = (path.split('/').pop() ?? path).toLowerCase();
  return (
    name === '.env' ||
    name.startsWith('.env.') ||
    ['.pem', '.key', '.p12', '.pfx'].some((ext) => name.endsWith(ext)) ||
    name.startsWith('id_rsa') ||
    name.startsWith('id_ed25519') ||
    name.includes('credential') ||
    name.includes('secret')
  );
}

/** upstream（`origin/main`）へ subjects のコミットを送り、added のファイルが増えるときの注意 */
export function pushWarnings(upstream: string, subjects: readonly string[], added: readonly AddedFile[]): string[] {
  const out: string[] = [];
  const remoteBranch = upstream.slice(upstream.indexOf('/') + 1);
  if (DEFAULT_BRANCHES.includes(remoteBranch)) out.push(`This pushes straight to ${upstream}.`);
  const rough = subjects.filter(unfinished).length;
  if (rough > 0) out.push(`${rough} ${rough === 1 ? 'commit looks' : 'commits look'} unfinished (fixup!, squash! or WIP).`);
  const secrets = added.map((f) => f.path).filter(secretLike);
  if (secrets.length > 0) out.push(`Adds files that may hold secrets: ${secrets.slice(0, 3).join(', ')}`);
  const binaries = added.filter((f) => f.binary).length;
  if (binaries > 0) out.push(`Adds ${binaries} binary ${binaries === 1 ? 'file' : 'files'}.`);
  return out;
}
