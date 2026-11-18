/**
 * 結合テスト（e2e）の一時ディレクトリの中の決まった場所と、VSCode に渡す環境変数。
 * `run.mjs` と `.vscode-test.mjs` の両方が使う
 */
import { join } from 'node:path';

/** 一時ディレクトリの中の場所 */
export function layout(dir) {
  return {
    // 拡張（.vsix と同じ形に並べたもの。bin にこのリポジトリのエンジン、e2e にテスト）
    ext: join(dir, 'ext'),
    // VSCode で開くリポジトリ（core.autocrlf=true で clone したもの）
    repo: join(dir, 'repo'),
    origin: join(dir, 'origin'),
    user: join(dir, 'user'),
    home: join(dir, 'home'),
  };
}

/**
 * VSCode（とそこから起動する daemon・git）の環境変数。エンジンの置き場所・daemon のソケットとキャッシュ・Amon Hen の
 * 設定（`settings.json`）を一時ディレクトリに置き、手元の daemon・アプリ・`~/.local/bin/amonhen`・設定に触らない
 */
export function testEnv(dir, engine) {
  const { home } = layout(dir);
  const env = {
    HOME: home,
    AMONHEN_DAEMON_DIR: join(dir, 's'),
    // テストから `amonhen daemon status` を聞くためのエンジン
    AMONHEN_E2E_ENGINE: engine,
  };
  if (process.platform === 'win32') {
    env.LOCALAPPDATA = join(dir, 'local');
    env.APPDATA = join(dir, 'roaming');
  } else {
    env.XDG_DATA_HOME = join(home, '.local', 'share');
    env.XDG_CONFIG_HOME = join(home, '.config');
    // 動かした git をすべて記録する（trace2。拡張・組み込みの git 拡張・daemon・テスト自身のどれも）。index の鍵と
    // ぶつかったときに、その瞬間に動いていた git を言うため。Windows では開いたままのファイルが片付けの邪魔になるので取らない
    env.GIT_TRACE2_EVENT = join(dir, 'trace2.jsonl');
    env.GIT_TRACE2_ENV_VARS = 'GIT_OPTIONAL_LOCKS';
  }
  return env;
}
