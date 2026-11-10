/**
 * status で git の fsmonitor を使うか（設定 `amonhen.fsmonitor`。アプリの Settings > Performance と同じ）。macOS と
 * Windows は `git fsmonitor--daemon`（リポジトリごとに常駐する）、ほかの Unix（Linux・BSD）は Watchman を、エンジンを
 * hook にして使う（Watchman があり、git が 2.26 以降のときだけ。それより古い git は hook の版 1 しか話さない）。
 * リポジトリの設定には書かず、status の起動にだけ渡す（コアと同じ）
 */
import { accessSync, constants } from 'node:fs';
import { delimiter, join } from 'node:path';

export interface FsmonitorSetting {
  enabled: boolean;
  /** Linux・BSD で hook にするエンジン（`amonhen`）。なければ使わない */
  engine?: string;
}

let setting: FsmonitorSetting = { enabled: false };

/** Linux・BSD で読んだ git の版（status のたびに読まない。設定を変えたら読み直す） */
let gitVersion: Promise<readonly [number, number] | undefined> | undefined;

export function setFsmonitor(next: FsmonitorSetting): void {
  setting = next;
  gitVersion = undefined;
}

/** git の `fsmonitor--daemon` がある OS か（macOS と Windows）。ほかでは Watchman を使う */
export function hasFsmonitorDaemon(platform: NodeJS.Platform = process.platform): boolean {
  return platform === 'darwin' || platform === 'win32';
}

/** シェルの 1 語にする（`'...'`。中の `'` は `'\''`） */
function quote(text: string): string {
  return `'${text.replace(/'/g, `'\\''`)}'`;
}

/**
 * status に足す `-c` の設定（使わなければ空）。`watchman` は PATH に Watchman があるか、`git` は git の版
 * （major・minor。読めなければ undefined）
 */
export function fsmonitorConfig(
  s: FsmonitorSetting,
  platform: NodeJS.Platform,
  watchman: boolean,
  git: readonly [number, number] | undefined,
): string[] {
  if (!s.enabled) return [];
  if (hasFsmonitorDaemon(platform)) return ['-c', 'core.fsmonitor=true'];
  const hookV2 = git !== undefined && (git[0] > 2 || (git[0] === 2 && git[1] >= 26));
  if (!watchman || !hookV2 || !s.engine) return [];
  return ['-c', `core.fsmonitor=${quote(s.engine)} fsmonitor-watchman`, '-c', 'core.fsmonitorHookVersion=2'];
}

let watchmanFound: boolean | undefined;

/** PATH に Watchman があるか（一度だけ探す） */
function watchmanOnPath(): boolean {
  watchmanFound ??= (process.env.PATH ?? '').split(delimiter).some((dir) => {
    try {
      accessSync(join(dir, 'watchman'), constants.X_OK);
      return true;
    } catch {
      return false;
    }
  });
  return watchmanFound;
}

/** 今の設定で status に足す `-c` の設定。git の版は Linux・BSD で要るときだけ `version` で読む */
export async function fsmonitorArgs(version: () => Promise<string>): Promise<string[]> {
  if (!setting.enabled) return [];
  if (hasFsmonitorDaemon()) return fsmonitorConfig(setting, process.platform, false, undefined);
  if (!setting.engine || !watchmanOnPath()) return [];
  gitVersion ??= version()
    .catch(() => '')
    .then((text) => {
      const m = /(\d+)\.(\d+)/.exec(text);
      return m ? ([Number(m[1]), Number(m[2])] as const) : undefined;
    });
  return fsmonitorConfig(setting, process.platform, true, await gitVersion);
}
