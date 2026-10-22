# インストール

[English](../install.md)

どれも [Releases](https://github.com/fukusaka/amonhen/releases) のページにあります。リリースには 2 種類あります。

- **正式版**: タグ `v<版>` のリリースです。VSCode 拡張は、別のタグ `vscode-v<版>` です。
- **snapshot**: 開発中の最新版です。テストが通るたびに作り直されます。

> [!NOTE]
> 正式版は、macOS で警告なしに開けます。snapshot は警告が出るので、下のスクリプトで入れるか、取ってきたあと
> 「システム設定 > プライバシーとセキュリティ」で一度だけ許可してください。

## アプリ（macOS）

1. `Amon-Hen-<版>.zip`（snapshot は `Amon-Hen-Snapshot-<版>.zip`）を取ってきます。
2. 展開して、`Amon Hen.app` を `/Applications` に移します。snapshot は `Amon Hen Snapshot.app` という名前なので、
   正式版と両方置いておけます。
3. 開きます。

初めて開いたときに、コマンドの `amonhen` も使えるようになります（`~/.local/bin` に入ります）。

## GTK4 のアプリ（Linux、作り始め）

Linux のネイティブのアプリです。まだ作り始めで、History・Working Copy・マージエディタ・Blame・File History が
使えます（共有の daemon 越しや SSH 先のリポジトリも）。`amonhen-gtk-<版>-linux-<CPU>.tar.gz` を取ってきて、
次を実行します。

```sh
tar xzf amonhen-gtk-<版>-linux-x86_64.tar.gz
amonhen-gtk-linux-x86_64/install.sh
```

`~/.local` に置き、デスクトップのアプリの一覧に出し、コマンドも使えるようにします。GTK は入っていないので、
なければ入れてください（Ubuntu なら `sudo apt install libgtk-4-1`）。

## コマンドだけ

Linux・Windows、またはアプリを入れない Mac で使うときです。コマンドは、アプリと VSCode 拡張が使っているのと同じ
プログラム（エンジン）です。

macOS・Linux では、`amonhen-<版>-<OS>-<CPU>.tar.gz` を取ってきて、次を実行します。

```sh
tar xzf amonhen-<版>-linux-x86_64.tar.gz
./amonhen install    # ~/.local/share/amonhen に置き、~/.local/bin/amonhen を作ります
```

Windows では、`amonhen-<版>-windows-<CPU>.zip` を取ってきて展開し、次を実行します。

```powershell
.\amonhen.exe install    # %LOCALAPPDATA%\amonhen に置きます
```

そのあと、`%LOCALAPPDATA%\amonhen\bin` を `PATH` に足してください。下の PowerShell のスクリプトを使えば、`PATH` の
ほかはまとめて行えます。

## VSCode 拡張

1. `amonhen-vscode-<版>.vsix` を取ってきます。
2. 入れます。

   ```sh
   code --install-extension amonhen-vscode-<版>.vsix
   ```

3. VSCode のウィンドウを再読み込みします。

拡張にはエンジンが入っているので、ほかに入れるものはありません。Remote-SSH で別のマシンにつないで使うときは、その
ウィンドウのターミナルで実行すると、つないだ先のマシンに入ります。

## 1 行で入れる・新しくする

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

Windows では PowerShell で次を実行します。

```powershell
# コマンドだけ（Windows）
irm https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.ps1 | iex
# snapshot や、決まったリリース（v0.2.4 など）
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.ps1))) snapshot
```

マシンの CPU に合う zip を選び、[GitHub CLI](https://cli.github.com/) が入っていてログインしていればビルドの由来の
証明を確かめ（必ず確かめるなら `-Verify require`、確かめないなら `-Verify skip`）、`amonhen.exe install` を実行します。
こうして取ってきたファイルには（ブラウザーが付ける）Mark of the Web が付かないので、SmartScreen に止められません。

開発中の最新版を入れるときは、`stable` を `snapshot` にします。同じ行をもう一度実行すると、新しい版に入れ替わります。
アプリを開いたまま入れ替えたときはスクリプトがそう知らせるので、アプリを開き直すと新しい版になります。

アプリは `/Applications` に入ります。`--user` を付けると（`sh -s -- --user stable`）ユーザーのアプリフォルダ
（`~/Applications`）に、`--system` を付けると `/Applications` に、`--dir <フォルダ>` を付けるとそのフォルダ（なければ
作ります）に入れます。どれも付けなければ、今入っているところで
入れ替えます。

リポジトリを clone した手元では、`scripts/install-app.sh`・`scripts/install-engine.sh`・`scripts/install-vscode.sh`・
`scripts\install-engine.ps1` をそのまま実行します。

## 取ってきたファイルを確かめる

Release のどのファイルにも、ビルドの由来の証明が付いています。このリポジトリの CI が、どのコミットから作ったかの
証明です。[GitHub CLI](https://cli.github.com/) で確かめられます。

```sh
gh attestation verify <ファイル> --repo fukusaka/amonhen
```

## 動作環境

- **アプリ**: macOS 14.4 以降、Apple シリコンの Mac。
- **GTK4 のアプリ**: Linux（x86_64・arm64）で GTK 4.14 以降（Ubuntu 24.04・Debian 13 以降）。
- **コマンドと VSCode 拡張**: Apple シリコンの macOS、Linux（x86_64・arm64、ディストリビューションは問いません）、
  Windows（x86_64・arm64）。
- `git` が入っている必要があります。
