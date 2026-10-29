/** 試験で使う git の手伝い。サポートする最低の git（2.23）でも動くようにする */
import { execFileSync } from 'node:child_process';

/** `git init -b main`（`-b` は git 2.28 から）。`opts` は `--bare` など */
export function initMain(dir: string, ...opts: string[]): void {
  execFileSync('git', ['init', '-q', ...opts, dir]);
  execFileSync('git', ['symbolic-ref', 'HEAD', 'refs/heads/main'], { cwd: dir });
}
