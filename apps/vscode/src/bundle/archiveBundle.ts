/**
 * アーカイブ（`git archive`）と bundle（`git bundle`）の決まり（docs/ux.md の 7d・7e。アプリと同じ）。vscode に依存しない
 */

export type ArchiveFormat = 'zip' | 'tar.gz' | 'tar';

export const ARCHIVE_FORMATS: readonly ArchiveFormat[] = ['zip', 'tar.gz', 'tar'];

/** 既定の形: 前に選んだもの、なければ macOS と Windows は zip、ほかは tar.gz */
export function defaultArchiveFormat(platform: string, last: ArchiveFormat | undefined): ArchiveFormat {
  return last ?? (platform === 'linux' || platform.endsWith('bsd') ? 'tar.gz' : 'zip');
}

/** アーカイブの中の最上位のフォルダ: `<リポジトリ>-<タグ・ブランチ・短い ID>`（ファイル名に使えない文字は - に） */
export function archiveTopFolder(repository: string, label: string): string {
  return `${repository}-${label}`.replace(/[\\/:*?"<>|\s]+/g, '-');
}

/** bundle のファイルの見出し（git を動かさずに読む）。HEAD は refs に入れない */
export interface BundleHeader {
  refs: string[];
  head: boolean;
  /** 受け取る側が持っているはずのコミットの数（あれば clone できない） */
  prerequisites: number;
}

/**
 * `data`（ファイルの先頭）が bundle なら、その見出し。形は `# v2 git bundle`（v3 なら続けて `@capability` の行）、
 * 前提のコミットの `-<id> <comment>` の行、ref の `<id> <name>` の行、空の行、その後に pack
 */
export function readBundleHeader(data: Buffer): BundleHeader | undefined {
  const end = data.indexOf('\n\n');
  if (end < 0) return undefined;
  const lines = data.subarray(0, end).toString('utf8').split('\n');
  if (lines[0] !== '# v2 git bundle' && lines[0] !== '# v3 git bundle') return undefined;
  const header: BundleHeader = { refs: [], head: false, prerequisites: 0 };
  for (const line of lines.slice(1)) {
    if (line.startsWith('@')) continue;
    if (line.startsWith('-')) {
      header.prerequisites++;
      continue;
    }
    const space = line.indexOf(' ');
    if (space < 0) return undefined;
    const name = line.slice(space + 1);
    if (name === 'HEAD') header.head = true;
    else header.refs.push(name);
  }
  return header;
}

/** HEAD のない bundle から clone するときに取り出すブランチ: main、master、なければ最初のブランチ */
export function cloneBranch(header: BundleHeader): string | undefined {
  const branches = header.refs.filter((r) => r.startsWith('refs/heads/')).map((r) => r.slice('refs/heads/'.length));
  return branches.find((b) => b === 'main') ?? branches.find((b) => b === 'master') ?? branches[0];
}

/** bundle のファイル名から、clone するフォルダの名前（`.bundle` を除く） */
export function folderForBundle(file: string): string {
  const name = file.replace(/\\/g, '/').split('/').pop() ?? 'bundle';
  return name.replace(/\.bundle$/i, '') || 'bundle';
}
