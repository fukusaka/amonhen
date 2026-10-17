<h1><img src="docs/images/banner.png" alt="Amon Hen"></h1>

[![CI](https://github.com/fukusaka/amonhen/actions/workflows/ci.yml/badge.svg)](https://github.com/fukusaka/amonhen/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fukusaka/amonhen?filter=!vscode-*)](https://github.com/fukusaka/amonhen/releases/latest)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#ライセンス)

[English](README.md)

自分が使いたい Git の GUI を、自分で作っています。いちばん大事にしているのは、大きなリポジトリでも待たされないこと
です。

## できること

- **History**: コミットの履歴をブランチのグラフ付きで見たり、検索したり、編集したり（メッセージの書き換え、squash、
  並べ替えなど）できます。cherry-pick や revert もできます。
- **Working Copy**: コミットに入れる変更を行ごとに選べます。いらない変更を捨てたり、コンフリクトを 3 つの欄に
  分かれたマージエディタで解消したりできます。
- **Undo**: 履歴の書き換えや stash の削除も含め、たいていの操作は元に戻せます。
- **アプリの外でも**: 同じことを、コマンド（`amonhen`）や VSCode からもできます。SSH でつないだ別のマシンの
  リポジトリも、手元と同じように扱えます。

Git の処理は、**エンジン**という 1 つのプログラムが受け持ちます。macOS のアプリ、コマンド、VSCode 拡張は、どれも同じ
エンジンを使います。読み込んだ結果を分け合うので、どれから使っても速いままです。

> [!WARNING]
> 開発の途中です。機能・ファイルの形式・コマンドは、予告なく変わることがあります。保証はありません
> （[ライセンス](#ライセンス)を参照）。大事なリポジトリは、バックアップを取っておいてください。

## 動作環境

- **アプリ**: macOS 14.4 以降、Apple シリコンの Mac。
- **エンジン**（コマンドと、VSCode 拡張が使うもの）: Apple シリコンの macOS、Linux（x86_64・arm64、ディストリビュー
  ションは問いません）、Windows（x86_64・arm64）。
- `git` が入っている必要があります。

## インストール

[Releases](https://github.com/fukusaka/amonhen/releases) のページから取ってきます。次の 2 種類があります。

- **正式版**: タグ `v<版>` のリリースです（VSCode 拡張は、別のタグ `vscode-v<版>` です）。
- **snapshot**: 開発中の最新版です。テストが通るたびに作り直されます。

> [!NOTE]
> 正式版は、macOS で警告なしに開けます。snapshot は警告が出るので、下のスクリプトで入れるか、取ってきたあと
> 「システム設定 > プライバシーとセキュリティ」で一度だけ許可してください。

### アプリ（macOS）

1. `Amon-Hen-<版>.zip`（snapshot は `Amon-Hen-Snapshot-<版>.zip`）を取ってきます。
2. 展開して、`Amon Hen.app` を `/Applications` に移します。snapshot は `Amon Hen Snapshot.app` という名前なので、
   正式版と両方置いておけます。
3. 開きます。

初めて開いたときに、コマンドの `amonhen` も使えるようになります（`~/.local/bin` に入ります）。

### コマンドだけ

Linux・Windows、またはアプリを入れない Mac で使うときです。

macOS・Linux では、`amonhen-<版>-<OS>-<CPU>.tar.gz` を取ってきて、次を実行します。

```sh
tar xzf amonhen-<版>-linux-x86_64.tar.gz
./amonhen install    # ~/.local/share/amonhen に置き、~/.local/bin/amonhen を作ります
```

Windows では、`amonhen-<版>-windows-<CPU>.zip` を取ってきて展開し、次を実行します。

```powershell
.\amonhen.exe install    # %LOCALAPPDATA%\amonhen に置きます
```

そのあと、`%LOCALAPPDATA%\amonhen\bin` を `PATH` に足してください。

### VSCode 拡張

1. `amonhen-vscode-<版>.vsix` を取ってきます。
2. 入れます。

   ```sh
   code --install-extension amonhen-vscode-<版>.vsix
   ```

3. VSCode のウィンドウを再読み込みします。

拡張にはエンジンが入っているので、ほかに入れるものはありません。Remote-SSH で別のマシンにつないで使うときは、その
ウィンドウのターミナルで実行すると、つないだ先のマシンに入ります。

### 1 行で入れる・新しくする

次のスクリプトは、最新のリリースを探して取ってきて、入れる（入っていれば入れ替える）ところまでを 1 回で行います。
`curl` があれば使えます。

```sh
# アプリ（macOS）
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-app.sh | sh -s -- stable
# コマンドだけ（macOS・Linux）
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.sh | sh -s -- stable
# VSCode 拡張
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-vscode.sh | sh -s -- stable
```

開発中の最新版を入れるときは、`stable` を `snapshot` にします。同じ行をもう一度実行すると、新しい版に入れ替わります。
アプリを開いたまま入れ替えたときはスクリプトがそう知らせるので、アプリを開き直すと新しい版になります。

取ってきたファイルが、本当にこのリポジトリから作られたものかを確かめる方法は、
[リリース](docs/ja/release.md#ビルドの由来の証明)にあります。

## 文書

- [使い方](docs/ja/usage.md): アプリ、SSH でつないだ先のリポジトリ、コマンド
- [VSCode 拡張](apps/vscode/README.ja.md)と、[その作り](docs/ja/vscode.md)
- [設計](docs/ja/design.md)と、[エンジンとアプリの間のやりとりの決まり](docs/ja/front.md)
- [開発](docs/ja/development.md)と、[リリースの出し方](docs/ja/release.md)
- [変更履歴](CHANGELOG.md)（英語）

## ライセンス

[MIT License](LICENSE-MIT) と [Apache License 2.0](LICENSE-APACHE) の、どちらか好きなほうで使えます
（`MIT OR Apache-2.0`）。
