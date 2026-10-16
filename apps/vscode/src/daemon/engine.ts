/**
 * 拡張に同梱したエンジン（bin/amonhen-<os>-<arch>）を、エンジンの置き場所（~/.local/share/amonhen）に置き、共有の
 * daemon に使うエンジンを決める。どちらもエンジン自身に任せる（`amonhen install`・`amonhen engine`）ので、アプリ・
 * CLI と同じ決まりで、同じ daemon を共有する。vscode に依存しない
 */
import { execFile } from 'node:child_process';
import { access, chmod } from 'node:fs/promises';
import { join } from 'node:path';

/** この拡張のホストの OS と CPU（エンジンの名前と同じ形。合うエンジンを作っていなければ undefined） */
export function platformName(platform = process.platform, arch = process.arch): string | undefined {
  const os = platform === 'darwin' || platform === 'linux' ? platform : platform === 'win32' ? 'windows' : undefined;
  const cpu = arch === 'arm64' ? 'aarch64' : arch === 'x64' ? 'x86_64' : undefined;
  return os && cpu ? `${os}-${cpu}` : undefined;
}

function run(program: string, args: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(program, args, (err, stdout, stderr) => (err ? reject(new Error(stderr.trim() || err.message)) : resolve(stdout)));
  });
}

/**
 * `binDir`（拡張の bin）にこのホスト向けのエンジンがあれば、置き場所に置き（今のものより古くなければ current にする）、
 * 共有の daemon に使うエンジンの場所を返す。なければ undefined（拡張は git 実行層だけで動く）
 */
export async function prepareEngine(binDir: string): Promise<string | undefined> {
  const name = platformName();
  if (!name) {
    return undefined;
  }
  const bundled = join(binDir, `amonhen-${name}${process.platform === 'win32' ? '.exe' : ''}`);
  try {
    await access(bundled);
  } catch {
    return undefined;
  }
  // .vsix を展開すると実行の権限が落ちることがある
  await chmod(bundled, 0o755);
  await run(bundled, ['install', '--quiet']);
  return (await run(bundled, ['engine'])).trim() || bundled;
}
