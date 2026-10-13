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

### ドラッグ＆ドロップ

Branches・Remotes・Tags の項目をドラッグして、ほかの項目にドロップする。ドロップすると操作の候補が出るので選ぶ。どの操作もドロップ先を書き換える。

| ドラッグするもの | ドロップ先 | 選べる操作 |
| --- | --- | --- |
| ブランチ・リモートブランチ・タグ | ローカルブランチ | Merge（ドラッグしたものをドロップ先に merge）、Rebase（ドロップ先をドラッグしたものの上に rebase） |
| ローカルブランチ | リモート、リモートブランチ | Push、Force Push（`--force-with-lease`） |

- ドロップ先が checkout されていないとき、fast-forward で済むなら checkout せずにブランチだけ進める。そうでなければ checkout してから実行する。
- upstream が無いブランチを push すると、push 先を upstream に設定する。
- コンフリクトで止まったときは、中止（Abort）するか、下の「コンフリクトの解決」の手順で解決して続ける。

### コンフリクトの解決

merge や rebase がコンフリクトで止まると、Working Copy の先頭に「Merge in progress」「Rebase in progress (2/5)」のような行が出て、その下の Conflicts にコンフリクトしたファイルが並ぶ。ファイルの横には「both modified」「deleted by them」などの種類が出る。

1. Conflicts のファイルをクリックすると、標準の 3-way マージエディタで開く（Current は HEAD 側、Incoming は取り込む側。rebase では HEAD が rebase 先で、Incoming が自分のコミット）。片側で削除されたファイルは普通に開く。
2. 解決したら `+` で解決済みにする。コンフリクトのマーカーが残っていれば確認される。
3. ファイルを丸ごとどちらかの内容にするなら、右クリックの Accept Current / Accept Incoming を使う。選んだ側で削除されていれば削除として解決する。
4. 全部解決したら、先頭の行の Continue で続ける。rebase では Skip で今のコミットを飛ばせる。Abort で始める前の状態に戻る。

### History

ビューのタイトルの History ボタンで、エディタ領域にコミットの履歴とグラフを開く。上部で「Current Branch」と「All Branches」を切り替える。

- ダブルクリックか Enter で、そのコミットの変更を標準の複数ファイル diff エディタで開く。
- 上下の矢印キーと PageUp / PageDown で選択を動かす。
- 履歴は 200 件ずつ、スクロールに合わせて読み込む。

HEAD から first-parent で辿れるコミット（最初の merge コミットの手前まで）は、右クリックで編集できる。どれも非対話の rebase で行い、Undo で戻せる。作業中の変更は自動で退避して戻す。

| メニュー | 内容 |
| --- | --- |
| Reword… | メッセージを書き換える。編集欄は `⌘Enter` / `Ctrl+Enter` で保存、`Esc` で取り消し |
| Squash into Parent… | 親コミットにまとめる。2 つのメッセージをつないだものを編集してから実行する |
| Fixup into Parent | 親コミットにまとめ、親のメッセージを残す |
| Edit (Stop Here) | そのコミットで止める。直してコミット（amend）してから Continue |
| Split… | そのコミットを取り消して変更を Working Copy に戻し、止める。分けてコミットしてから Continue |
| Drop… | 履歴から取り除く |
| Move Up / Move Down | 1 つ新しい側 / 古い側へ動かす。行をドラッグしても並べ替えられる |
| Copy Commit ID | コミット ID をコピーする |

### Undo

ビューのタイトルの Undo ボタンで、直前の操作を戻す。対象はコミット、Merge、Rebase、fast-forward、History でのコミットの編集。Edit や Split で止めた操作は、Continue で終わった時点で Undo の対象になる。

- コミットを戻すと、その変更は stage された状態に戻る。
- Merge や Rebase を戻すと、作業中の変更は残したまま、ブランチと checkout していたブランチを操作の前に戻す。
- 操作のあとでブランチが動いていたら、壊さないように Undo しない。
- 記録は `.git/amonhen/undo.json` に残るので、VSCode を再起動しても戻せる。

ビューのタイトルにはほかに、Fetch（すべてのリモートから取得し、消えたリモートブランチを片付ける）と Refresh がある。

### 今の制限

- コミットメッセージは 1 行だけ。
- History で編集できるのは、HEAD から最初の merge コミットまでの範囲だけ。
- Push は Undo できない。
- 認証の入力を求められる push・fetch はできない（SSH の鍵や credential helper で認証できる必要がある）。
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
