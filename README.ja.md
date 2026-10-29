<h1><img src="docs/images/banner.png" alt="Amon Hen"></h1>

[![CI](https://github.com/fukusaka/amonhen/actions/workflows/ci.yml/badge.svg)](https://github.com/fukusaka/amonhen/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fukusaka/amonhen?filter=!vscode-*)](https://github.com/fukusaka/amonhen/releases/latest)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#ライセンス)

[English](README.md)

軽快に応える Git の GUI。欲しいものを自分の手で。

![Amon Hen で開いたこのリポジトリ。History と、v0.2.0 のリリースのコミットの詳細・差分](docs/images/history.png)

## できること

- **History** — ブランチのグラフ付きで、コミットの履歴を見ます。探します。メッセージの書き換え、squash、並べ替え、
  cherry-pick、revert もできます。パッチに書き出したり、メールや diff のパッチを当てたりもできます。
- **行ごとのコミット** — 変更の中から、コミットに入れる行だけを選びます。いらない変更は捨てられます。
- **Undo** — 履歴の書き換えや stash の削除も含め、たいていの操作は元に戻せます。
- **マージエディタ** — 両側と結果を見比べながら、コンフリクトを解消します。
- **コマンドと VSCode** — 同じことを、`amonhen` コマンドと VSCode 拡張からもできます。
- **SSH 越し** — 別のマシンにあるリポジトリも、手元と同じように使えます。

> [!WARNING]
> 開発の途中です。機能・ファイルの形式・コマンドは、予告なく変わることがあります。保証はありません。大事な
> リポジトリは、バックアップを取っておいてください。

## インストール

Mac（macOS 14.4 以降、Apple シリコン）の場合:

1. 最新の [`Amon-Hen-<版>.zip`](https://github.com/fukusaka/amonhen/releases/latest) を取ってきます。
2. 展開して、`Amon Hen.app` を `/Applications` に移します。
3. 開きます。

Linux（作り始めの GTK4 のアプリとコマンド）・Windows（コマンド）、VSCode 拡張、開発中の版、インストールのスクリプトについては、
[インストール](docs/ja/install.md)を見てください。

## 文書

- [インストール](docs/ja/install.md)
- [使い方](docs/ja/usage.md): アプリ、SSH 越しのリポジトリ、コマンド
- [VSCode 拡張](apps/vscode/README.ja.md)
- [設計](docs/ja/design.md)と、[VSCode 拡張](docs/ja/vscode.md)・[エンジンのやりとりの決まり](docs/ja/front.md)の作り
- [開発](docs/ja/development.md)、[Windows の実機](docs/ja/windows.md)と[リリースの出し方](docs/ja/release.md)
- [変更履歴](CHANGELOG.md)（英語）

## ライセンス

[MIT](LICENSE-MIT) か [Apache License 2.0](LICENSE-APACHE) の、好きなほうで使えます。
