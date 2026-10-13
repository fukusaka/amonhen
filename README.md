# Amon Hen

VSCode 上で快適に Git を操作するための自分用の拡張。Marketplace には公開せず、このリポジトリの GitHub Releases で `.vsix` を配布する。設計は [docs/design.md](docs/design.md)、変更履歴は [CHANGELOG.md](CHANGELOG.md) を参照。

## 機能

アクティビティバーの Amon Hen アイコンからサイドバーを開く。

### サイドバー

| セクション | 内容 |
| --- | --- |
| Working Copy | Conflicts / Staged / Changes に分けて変更ファイルを表示 |
| Branches | `feature/x` のような名前はフォルダにまとめる。現在のブランチに ✓、upstream との差を ↑↓ で表示。右クリックで Checkout |
| Remotes | リモートごとのブランチ |
| Tags | 新しい順。展開したときに初めて取得する |
| Stashes | 展開したときに初めて取得する |
| Submodules | 初期化済みか、変更があるか |

複数のリポジトリを開いているときは、ビューのタイトルのボタンで切り替える。

### 差分と stage

- Working Copy のファイルをクリックすると、標準の diff エディタで開く。Changes は「index ↔ 作業中のファイル」（右側はそのまま編集できる）、Staged は「HEAD ↔ index」。
- ファイル単位の stage / unstage は、各行やグループの `+` / `−` ボタンで行う。複数選択もできる。
- diff エディタでは、選んだ行だけを stage / unstage できる。

| 操作 | キー | 対象 |
| --- | --- | --- |
| Stage Selected Lines | `ctrl+alt+s` | Changes の diff。行を選べばその行だけ、カーソルだけならその hunk |
| Unstage Selected Lines | `ctrl+alt+u` | Staged の diff。同上 |

削除した行を選ぶときは左側（変更前）のエディタで選ぶ。どちらの操作も diff エディタ右上のボタンからも実行できる。

### コミット

Working Copy の見出しかビューのタイトルにある ✓ ボタンから、stage 済みの変更をコミットする。

### 今の制限

- コミットメッセージは 1 行だけ。
- 行単位の stage は、autocrlf などの git のフィルターを考慮しない。
- 外部での変更がサイドバーに反映されるまで約 1 秒かかる（組み込みの Git 拡張の通知を待っているため）。

## インストール・更新

`gh` でこのリポジトリにアクセスできる必要がある（private リポジトリのため）。

```sh
scripts/install.sh            # 正式版（最新の Release）
scripts/install.sh snapshot   # main の最新
```

リポジトリを clone していない端末では:

```sh
gh api repos/fukusaka/vscode-amonhen/contents/scripts/install.sh --jq .content | base64 -d | sh -s -- snapshot
```

Remote-SSH の接続先に入れるときは、接続した VSCode の統合ターミナルで実行する。入れたあとはウィンドウを再読み込みする。

## リリース

| チャンネル | 作られるタイミング | バージョン |
| --- | --- | --- |
| stable | `v*` タグの push | `0.0.2` |
| snapshot | main への push のたび（`snapshot` という pre-release を置き換える） | `0.0.2-snapshot.<前回リリースからのコミット数>+<sha>` |

snapshot は次の patch の pre-release 扱いなので、正式版を入れると snapshot より新しいと判定される。

正式版を出す手順:

1. [CHANGELOG.md](CHANGELOG.md) の `Unreleased` を、出すバージョンと日付の見出しに改めてコミットする
2. `npm version patch && git push --follow-tags`

## 開発

```sh
npm install
npm run build      # dist/extension.js を生成
npm run typecheck
npm test
```

VSCode で F5 を押すと、拡張を読み込んだウィンドウが開く。Output の「Amon Hen」でログレベルを Trace にすると、git コマンドごとの所要時間が出る。

| ディレクトリ | 内容 |
| --- | --- |
| `src/git/` | git 実行層。プロセス起動、常駐 `cat-file`、出力のパース、`.git` の更新時刻による変化検知 |
| `src/diff/` | 行単位の差分と、選んだ変更だけを当てる処理 |
| `src/state/` | 状態管理層。リポジトリごとのキャッシュと更新 |
| `src/ui/` | サイドバー、diff の表示、コマンド |
| `test/` | `node --test` で動くテスト。git 実行層は一時ディレクトリの実リポジトリで確かめる |
| `scripts/` | インストールと snapshot のバージョン計算 |
