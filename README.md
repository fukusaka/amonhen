# Amon Hen

VSCode 上で快適に Git を操作するための自分用の拡張。Marketplace には公開せず、このリポジトリの GitHub Releases で `.vsix` を配布する。設計は [docs/design.md](docs/design.md)、変更履歴は [CHANGELOG.md](CHANGELOG.md) を参照。

## 機能

アクティビティバーの Amon Hen アイコンを押すと、サイドバーにナビが、エディタ領域にメインビューが開く（コマンドパレットの「Amon Hen: Open Amon Hen」でもメインビューを開ける）。ナビで選んだものをメインビューに出す。

### ナビ（サイドバー）

| 項目 | 内容 |
| --- | --- |
| タイトルのボタン | Fetch（すべてのリモートから取得）、Pull（現在のブランチに upstream を取り込む）、Push（upstream へ。無ければ origin へ送って upstream に設定）、Undo。`…` の中に Open Amon Hen、Refresh、Force Push |
| Working Copy | 変更の数。選ぶとメインビューに Working Copy を出す |
| History | 現在のブランチ。選ぶとメインビューに History を出す |
| merge / rebase の途中 | 「Rebase in progress (2/5)」のような行。Continue / Skip / Abort |
| Branches | `feature/x` はフォルダにまとめる。現在のブランチに ✓、upstream との差（↑↓）、upstream が消えていれば gone |
| Remotes / Tags / Stashes / Submodules | 開いたときに取得する |

- ブランチ・リモートブランチ・タグを選ぶと、メインビューの History にその履歴が出る。
- 右クリックで Checkout（リモートブランチなら追跡するローカルブランチを作って切り替える）、Merge into Current Branch、Rebase Current Branch onto This、Delete Branch（Undo で戻せる）。
- 複数のリポジトリを開いているときは、タイトルのボタンで切り替える。

### ドラッグ＆ドロップ

ナビのブランチ・リモートブランチ・タグを、ナビのほかの項目にドロップすると操作の候補が出る。どの操作もドロップ先を書き換える。

| ドラッグするもの | ドロップ先 | 選べる操作 |
| --- | --- | --- |
| ブランチ・リモートブランチ・タグ | ローカルブランチ | Merge（ドラッグしたものをドロップ先に merge）、Rebase（ドロップ先をドラッグしたものの上に rebase） |
| ローカルブランチ | リモート、リモートブランチ | Push、Force Push（`--force-with-lease`） |

- ドロップ先が checkout されていないとき、fast-forward で済むなら checkout せずにブランチだけ進める。そうでなければ checkout してから実行する。
- upstream が無いブランチを push すると、push 先を upstream に設定する。

### Working Copy

左に Conflicts / Staged / Changes のファイル、下にコミット欄、右に選んだファイルの差分が出る。差分は hunk ごとに並び、変わった部分は単語単位で強調する。

- 差分の行をクリックで選び（Shift+クリックで範囲）、Stage N Lines で選んだ行だけを stage する。Staged のファイルでは同じ操作で unstage になる。
- hunk ごとの Stage Hunk、Stage File、一覧の `+` / `−`、Stage All でもできる。一覧では上下キーで選び、Space か Enter で stage / unstage する。
- コミット欄は Summary と Description（複数行）。Amend にすると直前のコミットのメッセージが入る。`⌘Enter` / `Ctrl+Enter` でもコミットできる。
- コンフリクトしたファイルでは、Open in Merge Editor（標準の 3-way マージエディタ）、Accept Current、Accept Incoming、Mark as Resolved を選べる。片側で削除されたファイルも Accept で解決できる。解決済みにするとき、マーカーが残っていれば確認する。

### History

左にコミットの一覧とグラフ、右に選んだコミットの詳細が出る。境目はドラッグで動かせて、幅は覚えておく。上部で Current Branch / All Branches / ナビで選んだ ref を切り替える。

- 一覧はクリックか、上下キーと PageUp / PageDown で選ぶ。履歴は 200 件ずつ、スクロールに合わせて読み込む。
- 詳細には、メッセージ全体、ブランチやタグ、作者と日時、コミット ID、親コミット（クリックでそのコミットへ移る）、変更ファイルと、ファイルごとの差分が並ぶ。差分は画面に入ったファイルから読み込む。merge コミットは最初の親との差分。
- 各ファイルの Open、Open All、一覧のダブルクリック・Enter で、標準の diff エディタでも開ける。

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

### 途中で止まった merge / rebase

コンフリクトや Edit・Split で止まると、ナビに「Rebase in progress (2/5)」のような行が出る。解決して stage したら Continue で続ける。rebase では Skip で今のコミットを飛ばせる。Abort で始める前の状態に戻る。

### Undo

ナビの Undo で、直前の操作を戻す。対象はコミット（amend を含む）、Merge、Rebase、fast-forward、Pull、ブランチの削除、History でのコミットの編集。

- コミットを戻すと、その変更は stage された状態に戻る。
- Merge や Rebase を戻すと、作業中の変更は残したまま、ブランチと checkout していたブランチを操作の前に戻す。
- 操作のあとでブランチが動いていたら、壊さないように Undo しない。
- Edit や Split で止めた操作は、Continue で終わった時点で Undo の対象になる。
- 記録は `.git/amonhen/undo.json` に残るので、VSCode を再起動しても戻せる。

### 今の制限

- History で編集できるのは、HEAD から最初の merge コミットまでの範囲だけ。
- Push は Undo できない。
- 認証の入力を求められる push・fetch はできない（SSH の鍵や credential helper で認証できる必要がある）。
- 行単位の stage は、autocrlf などの git のフィルターを考慮しない。
- 差分にシンタックスハイライトは付かない。
- 外部での変更が反映されるまで約 1 秒かかる（組み込みの Git 拡張の通知を待っているため）。

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
| `src/history/`, `src/workingCopy/`, `src/view/` | メインビューの各画面と拡張ホストの間のメッセージ、History のグラフと rebase の組み立て |
| `src/ui/` | サイドバーのナビ、メインビューのパネルと画面ごとのコントローラー、コマンド |
| `src/webview/` | メインビューの Webview（Working Copy、History、差分の描画） |
| `test/` | `node --test` で動くテスト。git 実行層は一時ディレクトリの実リポジトリで確かめる |
| `scripts/` | インストールと snapshot のバージョン計算 |
