# vscode-amonhen

VSCode 上で快適に Git を操作するための、自分用の Git 拡張。Marketplace には公開せず、このリポジトリの GitHub Releases で `.vsix` を配布する。

設計の詳細は `docs/design.md` を参照すること。

## 前提

- 言語: TypeScript
- `package.json` の `name` は `amonhen`、表示名は `Amon Hen`
- コマンド ID・設定キー・View ID の接頭辞は `amonhen.`
- 独自 URI スキームは `amonhen:`
- `extensionKind: ["workspace"]`。Remote-SSH 接続時は拡張ホストが SSH 先で動き、git もそこで実行される
- git の操作とデータ取得は `git` を直接 spawn する。状態変化の検知だけ組み込みの `vscode.git` 拡張 API（`getAPI(1)` の `onDidChange` 系）を使う
- 差分表示とコンフリクト解決は、VSCode 標準の diff エディタと 3-way マージエディタを流用する
- 履歴を書き換える操作（rebase、amend、reset など）は、実行前に `HEAD` と関係する ref を記録し、Undo できるようにする

## 構成

- git 実行層: プロセス起動、常駐 `git cat-file --batch-command`、出力のパース
- 状態管理層: ref・status のキャッシュ、Undo 用の操作ログ
- UI 層: TreeDataProvider、TextDocumentContentProvider、Webview との通信

## 速度に関する方針

- 読み取り系の git コマンドには `--no-optional-locks` を付ける
- status は `git status --porcelain=v2 -z`、log は `%x00` 区切りで取得してパースする
- log は分割して取得し、Webview には最小限のデータだけ送る（Remote-SSH では postMessage が SSH を経由するため）

## リリース

- `v*` タグの push で GitHub Actions が `vsce package` を実行し、Release に `.vsix` を添付する
- リリース手順: `npm version patch && git push --follow-tags`

## 進捗

`docs/design.md` の「実装順序」を参照。完了したステップはそこのチェックボックスを更新すること。
