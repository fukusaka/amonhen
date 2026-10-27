import { accessSync, constants, readFileSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { isAbsolute, posix, sep, win32 } from 'node:path';

/**
 * 使う git の決め方（エンジンの `amonhen_model::process::git_choice` と同じ順）: `AMONHEN_GIT` → この機械の
 * Amon Hen の設定（`settings.json` の `"git"`。アプリの Settings > Git > Programs で選ぶ）→ VSCode の git
 * （`git.path`、なければ vscode.git が見つけたもの）。daemon を使わないとき（拡張の git 実行層）に使う。daemon を
 * 使うときはエンジンが同じ順で決め、拡張は `git.path` を `AMONHEN_FALLBACK_GIT` で送る
 */
export interface GitChoice {
  path: string;
  source: 'environment' | 'settings' | 'vscode';
  /** 使えないときの理由（git を動かす操作はこれでエラーにする。VSCode の git に黙って戻さない） */
  problem?: string;
}

/** この機械の Amon Hen の設定のファイル（エンジンの `amonhen_model::settings::path` と同じ） */
export function settingsFile(env: NodeJS.ProcessEnv = process.env, platform = process.platform): string {
  if (platform === 'win32') {
    return win32.join(env['APPDATA'] ?? win32.join(homedir(), 'AppData', 'Roaming'), 'amonhen', 'settings.json');
  }
  const config = env['XDG_CONFIG_HOME'] || posix.join(env['HOME'] ?? homedir(), '.config');
  return posix.join(config, 'amonhen', 'settings.json');
}

let cached: { file: string; stamp: string; git: string | undefined; problem: string | undefined } | undefined;

/** 設定で選んだ git。ファイルが読めなければ、その理由。使うたびに更新時刻を確かめて読み直す */
export function settingsGit(file: string = settingsFile()): { git?: string; problem?: string } {
  let stamp = '';
  try {
    const s = statSync(file);
    stamp = `${s.mtimeMs}:${s.size}`;
  } catch {
    return {};
  }
  if (cached?.file === file && cached.stamp === stamp) {
    return { git: cached.git, problem: cached.problem };
  }
  let git: string | undefined;
  let problem: string | undefined;
  try {
    const text = readFileSync(file, 'utf8');
    const value: unknown = text.trim() === '' ? {} : JSON.parse(text);
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      throw new Error('not a JSON object');
    }
    const chosen = (value as Record<string, unknown>)['git'];
    git = typeof chosen === 'string' && chosen !== '' ? chosen : undefined;
  } catch (err) {
    problem = `Amon Hen’s settings (${shown(file)}) can’t be read: ${err instanceof Error ? err.message : String(err)}`;
  }
  cached = { file, stamp, git, problem };
  return { git, problem };
}

/** 人に見せるパス（ホームの下なら `~/…`。エンジンの文と同じ） */
function shown(path: string): string {
  const home = homedir();
  return path.startsWith(home + sep) ? `~${path.slice(home.length)}` : path;
}

/** 使う git。`vscodeGit` は VSCode の git（`git.path`、なければ vscode.git が見つけたもの） */
export function chooseGit(vscodeGit: string, env: NodeJS.ProcessEnv = process.env, file = settingsFile(env)): GitChoice {
  const override = env['AMONHEN_GIT'];
  if (override) {
    return { path: override, source: 'environment', problem: missing(override, `The git in AMONHEN_GIT (${override}) isn’t there`) };
  }
  const { git, problem } = settingsGit(file);
  if (git) {
    return {
      path: git,
      source: 'settings',
      problem: missing(
        git,
        `The git chosen for this machine (${git}) isn’t there. Choose another in Settings > Git > Programs (it’s kept in ${shown(file)})`,
      ),
    };
  }
  return { path: vscodeGit, source: 'vscode', problem };
}

/** パスで選んだ git が実行できなければ `message`（名前だけなら PATH で探すので確かめない） */
function missing(path: string, message: string): string | undefined {
  if (!isAbsolute(path)) {
    return undefined;
  }
  try {
    accessSync(path, process.platform === 'win32' ? constants.F_OK : constants.X_OK);
    return statSync(path).isFile() ? undefined : message;
  } catch {
    return message;
  }
}
