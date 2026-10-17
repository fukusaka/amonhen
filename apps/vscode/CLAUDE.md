# vscode-amonhen（apps/vscode）

fukusaka/amonhen のリポジトリの `apps/vscode`。エンジン（ルートの Rust）と同じリポジトリで作り、エンジンを同梱する。

VSCode 上で快適に Git を操作するための、自分用の Git 拡張。Marketplace には公開せず、このリポジトリの GitHub Releases で `.vsix` を配布する。

設計の詳細は `docs/design.md` を参照すること。

## 前提

- 言語: TypeScript
- `package.json` の `name` は `amonhen`、表示名は `Amon Hen`
- コマンド ID・設定キー・View ID の接頭辞は `amonhen.`
- 独自 URI スキームは `amonhen:`
- `extensionKind: ["workspace"]`。Remote-SSH 接続時は拡張ホストが SSH 先で動き、git もそこで実行される
- 既定では、Amon Hen のエンジン（ルートの Rust の `amonhen`。ルートの `scripts/build-engine.sh` で作り、`scripts/bundle-engine.sh` で `bin/` に写して同梱する）の共有の daemon で History・差分・操作を行う（`src/daemon/`。取り決めはルートの `docs/front.md`）。エンジンを使わないとき（`amonhen.daemon.enabled` を切る、合うエンジンがない）は、以下の git 実行層で動く
- git の操作とデータ取得は `git` を直接 spawn する。状態変化の検知だけ組み込みの `vscode.git` 拡張 API（`getAPI(1)` の `onDidChange` 系）を使う
- ナビはサイドバーの TreeView、中身（Working Copy・History と差分）はエディタ領域の 1 つの Webview（メインビュー）に出す。コンフリクト解決は VSCode 標準の 3-way マージエディタを流用する
- 履歴を書き換える操作（rebase、amend、reset など）は、実行前に `HEAD` と関係する ref を記録し、Undo できるようにする

## 構成

- git 実行層: プロセス起動、常駐 `git cat-file --batch-command`、出力のパース
- 状態管理層: ref・status のキャッシュ、Undo 用の操作ログ
- UI 層: サイドバーのナビ（TreeDataProvider）、メインビュー（Webview）との通信、TextDocumentContentProvider

## 速度に関する方針

- 読み取り系の git コマンドには `--no-optional-locks` を付ける
- status は `git status --porcelain=v2 -z`、log は `%x00` 区切りで取得してパースする
- log は分割して取得し、Webview には最小限のデータだけ送る（Remote-SSH では postMessage が SSH を経由するため）

## リリース

- `vscode-v*` タグの push で GitHub Actions（ルートの `.github/workflows/vscode-release.yml`）がエンジンを作って同梱し、Release に `.vsix` を添付する（アプリ・エンジンの `v*` とは別）
- 利用者から見える変更を加えたら、`CHANGELOG.md` の `Unreleased` に追記する（コミット単位ではなく、リリース単位でまとめる）
- リリース手順: `CHANGELOG.md` の `Unreleased` をバージョンと日付の見出しに改め、`npm version patch --no-git-tag-version` と一緒にコミット → `git tag vscode-v<版> && git push origin main vscode-v<版>`
- main への push のたびに、ルートの snapshot（`snapshot` という pre-release。アプリ・エンジンと一緒）に `.vsix` も載せる（バージョンは `<次の patch>-snapshot.<apps/vscode のコミット数>+<sha>`）
- インストール: `scripts/install.sh [stable|snapshot]`

## 進捗

`docs/design.md` の「Implementation order」を参照。完了したステップはそこのチェックボックスを更新すること。文書は英語が正で、日本語版（`README.ja.md`・`docs/ja/`）も同じように直す。
