# 開発

[English](../development.md)

VSCode 拡張の作り方・試し方・リリース。使い方は [README](../../README.ja.md) を参照。

## 作る・試す

エンジンはリポジトリのルートで作る（`cargo build -p amonhen-cli`。テストはそれを使う）。`bin/` に同梱するエンジンは、
ルートで `scripts/build-engine.sh`、ここで `scripts/bundle-engine.sh` を実行して写す。

```sh
npm install
npm run build      # dist/extension.js を生成
npm run typecheck
npm test
```

VSCode で F5 を押すと、拡張を読み込んだウィンドウが開く。Output の「Amon Hen」でログレベルを Trace にすると、git コマンドごとの所要時間が出る。

| ディレクトリ | 内容 |
| --- | --- |
| `src/daemon/` | エンジン。探して置く、共有の daemon とのつながり、FE 向けの窓口のクライアント、それを通した History・差分・書き込み |
| `src/git/` | git 実行層。プロセス起動、常駐 `cat-file`、出力のパース、`.git` の更新時刻による変化検知 |
| `src/diff/` | 行単位の差分と、選んだ変更だけを当てる処理 |
| `src/state/` | 状態管理層。リポジトリごとのキャッシュと更新 |
| `src/history/`, `src/workingCopy/`, `src/view/` | メインビューの各画面と拡張ホストの間のメッセージ、History のグラフと rebase の組み立て |
| `src/ui/` | サイドバーのナビ、メインビューのパネルと画面ごとのコントローラー、コマンド |
| `src/webview/` | メインビューの Webview（Working Copy、History、差分の描画） |
| `test/` | `node --test` で動くテスト。git 実行層は一時ディレクトリの実リポジトリで確かめる |
| `scripts/` | エンジンを `bin/` に写す処理と、snapshot のバージョン計算 |

## リリース

| チャンネル | 作られるタイミング | バージョン |
| --- | --- | --- |
| stable | `vscode-v*` タグの push（アプリ・エンジンの `v*` とは別） | `0.0.2` |
| snapshot | main の CI が通るたび（アプリ・エンジンと同じ `snapshot` という pre-release に載せる） | `0.0.3-snapshot.<前回リリースからの apps/vscode のコミット数>+<sha>` |

snapshot は次の patch の pre-release 扱いなので、正式版を入れると snapshot より新しいと判定される。

正式版を出す手順:

1. ルートの [CHANGELOG.md](../../../../CHANGELOG.md) の「VSCode extension」の節の `Unreleased` を、出すバージョンと日付の見出しに改めてコミットする
2. `npm version patch --no-git-tag-version` を、1. と同じコミットに入れる
3. `git tag vscode-v<版> && git push origin main vscode-v<版>`
