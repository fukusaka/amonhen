// snapshot 版のバージョンを出力する。
// package.json の version のタグ（vscode-v<version>）があれば、その次の patch の pre-release にする
// （例: vscode-v0.2.0 のあと apps/vscode に 5 コミット → 0.2.1-snapshot.5+abc1234）。まだなければ（版を上げてから
// タグを打つまで）、その版の pre-release にする（0.2.0-snapshot.5+abc1234）。どちらも正式版との新旧の順が正しくなる。
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const git = (...args) => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
const tryGit = (...args) => {
  try {
    return git(...args);
  } catch {
    return undefined;
  }
};

const { version } = JSON.parse(readFileSync('package.json', 'utf8'));
const [major, minor, patch] = version.split('.').map(Number);
const sha = git('rev-parse', '--short', 'HEAD');
if (tryGit('rev-parse', '-q', '--verify', `refs/tags/vscode-v${version}`) !== undefined) {
  const count = git('rev-list', '--count', `vscode-v${version}..HEAD`, '--', '.');
  console.log(`${major}.${minor}.${patch + 1}-snapshot.${count}+${sha}`);
} else {
  // 前の正式版からのコミット数（正式版がまだなければ全部）。
  const previous = tryGit('describe', '--tags', '--abbrev=0', '--match', 'vscode-v*');
  const count = git('rev-list', '--count', previous ? `${previous}..HEAD` : 'HEAD', '--', '.');
  console.log(`${version}-snapshot.${count}+${sha}`);
}
