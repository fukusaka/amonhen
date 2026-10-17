# Amon Hen

自分のために作っている Git の GUI。大きなリポジトリでも待たないことを、いちばん大事にしている。

- History: グラフ付きの一覧、コミットの検索、コミットの編集（メッセージ・squash・並べ替えなど）、cherry-pick・revert。
- Working Copy: 差分の行ごとの stage・破棄、3 ペインのマージエディタ。
- 履歴の書き換え・変更の破棄・stash の削除なども、Undo で戻せる。
- 同じエンジンで、CLI（`amonhen`）・VSCode 拡張からも使え、SSH 先のリポジトリも手元と同じように扱える。共有の daemon が
  History・検索の索引・SSH のつながりを持ち続け、アプリ・CLI・拡張で分け合う。

## 動作環境

- アプリ: macOS 14.4 以降、Apple シリコン（arm64）。
- エンジン（CLI と daemon）: macOS arm64、Linux x86_64・aarch64（musl の静的リンクなので、どの Linux でも動く）、
  Windows x86_64・aarch64。
- どれも `git` を使う（書き込みと一部の読み取りは `git` を起動する）。

## インストール

アプリ（[GitHub CLI](https://cli.github.com/) で Release から取ってきて `/Applications` に入れる。入っていれば置き換える）:

```sh
scripts/install.sh            # 最新のリリース（Amon Hen）
scripts/install.sh snapshot   # main の最新（Amon Hen Snapshot、正式版と並べて入れられる）
```

アプリを入れると、エンジン（CLI と daemon）も `~/.local/share/amonhen` に置かれ、`~/.local/bin/amonhen` から使える。
エンジンだけを入れるときは、Release の `amonhen-<版>-<OS>-<CPU>`（Linux・macOS は tar.gz、Windows は zip）を展開して
`amonhen install` を実行する（Windows は `%LOCALAPPDATA%\amonhen` に置き、`%LOCALAPPDATA%\amonhen\bin` を PATH に足す）。

VSCode 拡張は [apps/vscode](apps/vscode) の `scripts/install.sh [stable|snapshot]` で入れる。拡張はエンジンを同梱していて、
入れれば共有の daemon を使う。

取ってきたファイルが、このリポジトリの CI で作ったものかは `gh attestation verify <ファイル> --repo fukusaka/amonhen` で
確かめられる。

## 文書

- [使い方](docs/usage.md): アプリ・SSH 先・共有の daemon・CLI
- [VSCode 拡張](apps/vscode)
- [設計](docs/design.md)・[FE 向けの窓口の取り決め](docs/front.md)
- [開発](docs/development.md)・[リリース](docs/release.md)

## ライセンス

[MIT](LICENSE-MIT) か [Apache License 2.0](LICENSE-APACHE) の、好きなほうを選んで使える（`MIT OR Apache-2.0`）。
