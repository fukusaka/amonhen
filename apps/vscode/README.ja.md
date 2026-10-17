# Amon Hen

[English](README.md)

VSCode 上で快適に Git を操作するための自分用の拡張。Marketplace には公開せず、このリポジトリの GitHub Releases で `.vsix` を配布する。設計は [docs/ja/design.md](docs/ja/design.md)、変更履歴は [CHANGELOG.md](CHANGELOG.md) を参照。

> [!WARNING]
> 開発の途中。機能・ファイルの形・コマンドは、予告なく変わることがある。無保証で、そのままの形で提供する
> （リポジトリのルートのライセンスを参照）。大事なリポジトリは、バックアップを取っておくこと。

## 機能

アクティビティバーの Amon Hen アイコンを押すと、サイドバーにナビが、エディタ領域にメインビューが開く（コマンドパレットの「Amon Hen: Open Amon Hen」でもメインビューを開ける）。ナビで選んだものをメインビューに出す。

### ナビ（サイドバー）

| 項目 | 内容 |
| --- | --- |
| タイトルのボタン | Fetch（すべてのリモートから取得）、Pull（現在のブランチに upstream を取り込む）、Push（upstream へ。無ければ origin へ送って upstream に設定）、Undo。`…` の中に New Branch、Stash Changes、Open Amon Hen、Refresh、Force Push |
| Working Copy | 変更の数。選ぶとメインビューに Working Copy を出す |
| History | 現在のブランチ。選ぶとメインビューに History を出す |
| merge / rebase の途中 | 「Rebase in progress (2/5)」のような行。Continue / Skip / Abort |
| Branches | `feature/x` はフォルダにまとめる。現在のブランチに ✓、upstream との差（↑↓）、upstream が消えていれば gone |
| Remotes / Tags / Stashes / Submodules | 開いたときに取得する |

- ブランチ・リモートブランチ・タグを選ぶと、メインビューの History にその履歴が出る。
- ブランチ・タグの右クリックで Checkout（リモートブランチなら追跡するローカルブランチを作って切り替える）、Merge into Current Branch、Rebase Current Branch onto This、Rename Branch、Delete Branch、New Branch（そこから作って切り替える）、New Tag（メッセージを入れれば annotated tag）。作成と削除は Undo で戻せる。
- Stashes の見出しの `+` で、作業中の変更を stash する（untracked のファイルを含めるかを選べる）。stash を選ぶと中身を複数ファイル diff で開き、右クリックで Apply / Pop / Drop（Undo で戻せる）。Apply と Pop は stage していた状態も戻す。
- Submodules は右クリックで Update（初期化して記録されているコミットに合わせる）と、別のウィンドウで開く。
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
- Changes のファイルは、一覧の ↺、Discard File、Discard Hunk、Discard N Lines で変更を破棄できる。untracked のファイルは消す。破棄した内容は Undo で戻せる。
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

どのコミットでも、右クリックで New Branch / New Tag（そのコミットから作る）、Revert（打ち消すコミットを作る）ができる。現在のブランチに無いコミットは Cherry-pick で現在のブランチに当てられる。どれも Undo で戻せる。

### 途中で止まった merge / rebase

コンフリクトや Edit・Split で止まると、ナビに「Rebase in progress (2/5)」のような行が出る。解決して stage したら Continue で続ける。rebase では Skip で今のコミットを飛ばせる。Abort で始める前の状態に戻る。

### Undo

ナビの Undo で、直前の操作を戻す。対象はコミット（amend を含む）、Merge、Rebase、fast-forward、Pull、ブランチ・タグの作成と削除、Cherry-pick、Revert、変更の破棄、stash の削除、History でのコミットの編集。

- コミットを戻すと、その変更は stage された状態に戻る。
- Merge や Rebase を戻すと、作業中の変更は残したまま、ブランチと checkout していたブランチを操作の前に戻す。
- 操作のあとでブランチが動いていたら、壊さないように Undo しない。変更の破棄も、破棄したあとでファイルを書き換えていたら Undo しない。
- Edit や Split で止めた操作は、Continue で終わった時点で Undo の対象になる。
- 記録は `.git/amonhen/undo.json` に残るので、VSCode を再起動しても戻せる。

### 今の制限

- History で編集できるのは、HEAD から最初の merge コミットまでの範囲だけ。
- Push は Undo できない。
- 認証の入力を求められる push・fetch はできない（SSH の鍵や credential helper で認証できる必要がある）。
- 行単位の stage は、autocrlf などの git のフィルターを考慮しない。
- 差分にシンタックスハイライトは付かない。
- 外部での変更が反映されるまで約 1 秒かかる（組み込みの Git 拡張の通知を待っているため）。

## エンジンと設定

既定では、History・差分・操作を Amon Hen のエンジン（アプリ・CLI と同じもの）の共有の daemon で行う。`.vsix` に
macOS arm64・Linux x86_64 / aarch64・Windows x86_64 / aarch64 向けのエンジンを同梱していて、設定はいらない（初めて
使うときに `~/.local/share/amonhen`、Windows は `%LOCALAPPDATA%\amonhen` に置く）。アプリ・CLI・拡張で、同じ daemon・
History・検索の索引・Undo の記録を分け合う。Remote-SSH では、接続先でその OS 向けのエンジンを使う。

ホストの OS と CPU に合うエンジンがないとき、または `amonhen.daemon.enabled` を切ったときは、拡張の git 実行層で動く
（上の機能は同じように使えるが、アプリ・CLI とは分け合わない）。

| 設定 | 既定 | 内容 |
| --- | --- | --- |
| `amonhen.daemon.enabled` | `true` | エンジンの共有の daemon を使う。切ると git 実行層だけで動く |
| `amonhen.daemon.path` | `""` | 同梱のものの代わりに使うエンジン（`amonhen`）の場所 |

## インストール・更新

[Releases](https://github.com/fukusaka/amonhen/releases) から `amonhen-vscode-<版>.vsix` を取ってきて（正式版は
`vscode-v<版>`、main の最新は `snapshot`）入れ、ウィンドウを再読み込みする:

```sh
code --install-extension amonhen-vscode-<版>.vsix
```

Remote-SSH の接続先に入れるときは、つないだウィンドウの統合ターミナルで実行する。

ルートの `scripts/install-vscode.sh [stable|snapshot]` で、最新のものを探して取ってきて入れるまでを 1 回でできる（`curl` だけあれば
よい）。clone していない端末では:

```sh
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-vscode.sh | sh -s -- snapshot
```

## リリース

| チャンネル | 作られるタイミング | バージョン |
| --- | --- | --- |
| stable | `vscode-v*` タグの push（アプリ・エンジンの `v*` とは別） | `0.0.2` |
| snapshot | main への push のたび（アプリ・エンジンと同じ `snapshot` という pre-release に載せる） | `0.0.3-snapshot.<前回リリースからの apps/vscode のコミット数>+<sha>` |

snapshot は次の patch の pre-release 扱いなので、正式版を入れると snapshot より新しいと判定される。

正式版を出す手順:

1. [CHANGELOG.md](CHANGELOG.md) の `Unreleased` を、出すバージョンと日付の見出しに改めてコミットする
2. `npm version patch --no-git-tag-version` を、1. と同じコミットに入れる
3. `git tag vscode-v<版> && git push origin main vscode-v<版>`

## 開発

エンジンはリポジトリのルートで作る（`cargo build -p amonhen-cli`。テストはそれを使う）。`bin/` に同梱するエンジンは、
ルートで `scripts/build-engine.sh`、ここで `scripts/bundle-engine.sh` を実行して写す。

```sh
npm install
npm run build      # dist/extension.js を生成
npm run typecheck
npm test
```

VSCode で F5 を押すと、拡張を読み込んだウィンドウが開く。Output の「Amon Hen」でログレベルを Trace にすると、git コマンドごとの所要時間が出る。

| ディレクトリ | 内容 |
| --- | --- |
| `src/daemon/` | エンジン。探して置く、共有の daemon とのつながり、FE 向けの窓口のクライアント、それを通した History・差分・書き込み |
| `src/git/` | git 実行層。プロセス起動、常駐 `cat-file`、出力のパース、`.git` の更新時刻による変化検知 |
| `src/diff/` | 行単位の差分と、選んだ変更だけを当てる処理 |
| `src/state/` | 状態管理層。リポジトリごとのキャッシュと更新 |
| `src/history/`, `src/workingCopy/`, `src/view/` | メインビューの各画面と拡張ホストの間のメッセージ、History のグラフと rebase の組み立て |
| `src/ui/` | サイドバーのナビ、メインビューのパネルと画面ごとのコントローラー、コマンド |
| `src/webview/` | メインビューの Webview（Working Copy、History、差分の描画） |
| `test/` | `node --test` で動くテスト。git 実行層は一時ディレクトリの実リポジトリで確かめる |
| `scripts/` | インストールと snapshot のバージョン計算 |
