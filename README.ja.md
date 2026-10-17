<h1><img src="docs/images/banner.png" alt="Amon Hen"></h1>

[![CI](https://github.com/fukusaka/amonhen/actions/workflows/ci.yml/badge.svg)](https://github.com/fukusaka/amonhen/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fukusaka/amonhen?filter=!vscode-*)](https://github.com/fukusaka/amonhen/releases/latest)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#ライセンス)

[English](README.md)

自分が使いたい Git の GUI を、自分で作っている。いちばん大事なのは、大きなリポジトリでも待たされないこと。

- History: グラフ付きの一覧、コミットの検索、コミットの編集（メッセージ・squash・並べ替えなど）、cherry-pick・revert。
- Working Copy: 差分の行ごとの stage・破棄、3 ペインのマージエディタ。
- 履歴の書き換え・変更の破棄・stash の削除なども、Undo で戻せる。
- 同じエンジンで、CLI（`amonhen`）・VSCode 拡張からも使え、SSH 先のリポジトリも手元と同じように扱える。共有の daemon が
  History・検索の索引・SSH のつながりを持ち続け、アプリ・CLI・拡張で分け合う。

> [!WARNING]
> 開発の途中。機能・ファイルの形・コマンドは、予告なく変わることがある。無保証で、そのままの形で提供する
> （[ライセンス](#ライセンス)を参照）。大事なリポジトリは、バックアップを取っておくこと。

## 動作環境

- アプリ: macOS 14.4 以降、Apple シリコン（arm64）。
- エンジン（CLI と daemon）: macOS arm64、Linux x86_64・aarch64（musl の静的リンクなので、どの Linux でも動く）、
  Windows x86_64・aarch64。
- どれも `git` を使う（書き込みと一部の読み取りは `git` を起動する）。

## インストール

どれも [Releases](https://github.com/fukusaka/amonhen/releases) にある。次のどちらかを選ぶ。

- **正式版**: タグ `v<版>` のリリース（VSCode 拡張は別のタグ `vscode-v<版>`）。
- **snapshot**: pre-release の `snapshot`。main の CI が通るたびに作り直す。

> [!NOTE]
> 正式版は `v0.1.1` から署名・公証してある。snapshot は署名だけで公証していないので、下のスクリプトで入れるか、
> ブラウザで取ったときは「システム設定 > プライバシーとセキュリティ」で一度だけ許可する。VSCode 拡張は正式版が
> まだないので、snapshot を使う。

### アプリ（macOS）

1. `Amon-Hen-<版>.zip`（snapshot は `Amon-Hen-Snapshot-<版>.zip`）を取ってくる。
2. 展開して、`Amon Hen.app` を `/Applications` に移す。snapshot は `Amon Hen Snapshot.app` で、正式版と並べて置ける。
3. 開く。正式版は署名・公証してあるので、警告なしで開ける。

初めて起動すると、エンジン（CLI と daemon）も入り、`~/.local/bin/amonhen` から使える。

### エンジンだけ（CLI と daemon）

Linux・Windows、またはアプリを入れない Mac 向け。

macOS・Linux: `amonhen-<版>-<OS>-<CPU>.tar.gz` を取ってきて（下の `scripts/install-engine.sh` でもよい）、

```sh
tar xzf amonhen-<版>-linux-x86_64.tar.gz
./amonhen install    # ~/.local/share/amonhen に置き、~/.local/bin/amonhen を張る
```

Windows: `amonhen-<版>-windows-<CPU>.zip` を取ってきて展開し、

```powershell
.\amonhen.exe install    # %LOCALAPPDATA%\amonhen に置く
```

`%LOCALAPPDATA%\amonhen\bin` を `PATH` に足す。

### VSCode 拡張

1. `amonhen-vscode-<版>.vsix` を取ってくる。
2. 入れる:

   ```sh
   code --install-extension amonhen-vscode-<版>.vsix
   ```

3. ウィンドウを再読み込みする。Remote-SSH では、つないだウィンドウの統合ターミナルで実行すると SSH 先に入る。

拡張はエンジンを同梱しているので、ほかに入れるものはない。

### 取ってきたファイルを確かめる

どのファイルにも、ビルドの由来の証明が付いている:

```sh
gh attestation verify <ファイル> --repo fukusaka/amonhen
```

### 近道: インストールのスクリプト

最新のリリースを探して取ってきて、入れる（入れ替える）までを 1 回でする。`curl` だけあればよい。clone していなければ:

```sh
# アプリ（macOS）
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-app.sh | sh -s -- stable
# エンジンだけ（macOS・Linux）
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.sh | sh -s -- snapshot
# VSCode 拡張
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-vscode.sh | sh -s -- snapshot
```

引数は `stable` か `snapshot`。clone した手元では、`scripts/install-app.sh`・
`scripts/install-engine.sh`・`scripts/install-vscode.sh` をそのまま実行する。

## 文書

- [使い方](docs/ja/usage.md): アプリ・SSH 先・共有の daemon・CLI
- [VSCode 拡張](apps/vscode/README.ja.md)
- [設計](docs/ja/design.md)・[FE 向けの窓口の取り決め](docs/ja/front.md)
- [開発](docs/ja/development.md)・[リリース](docs/ja/release.md)
- [変更履歴](CHANGELOG.md)（英語）

## ライセンス

[MIT](LICENSE-MIT) か [Apache License 2.0](LICENSE-APACHE) の、好きなほうを選んで使える（`MIT OR Apache-2.0`）。
