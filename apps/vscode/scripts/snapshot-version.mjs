// snapshot 版のバージョンを出力する。
// package.json の version を最後のリリース（タグ vscode-v<version>）とみなし、その次の patch の pre-release にする。
// 例: 0.0.1 のあと 5 コミット → 0.0.2-snapshot.5+abc1234
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const git = (...args) => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();

const { version } = JSON.parse(readFileSync('package.json', 'utf8'));
const [major, minor, patch] = version.split('.').map(Number);
let count;
try {
  count = git('rev-list', '--count', `vscode-v${version}..HEAD`, '--', '.');
} catch {
  // まだタグが無い
  count = git('rev-list', '--count', 'HEAD', '--', '.');
}
const sha = git('rev-parse', '--short', 'HEAD');
console.log(`${major}.${minor}.${patch + 1}-snapshot.${count}+${sha}`);
