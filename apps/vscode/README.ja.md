# Amon Hen

[English](https://github.com/fukusaka/amonhen/blob/main/apps/vscode/README.md)

VSCode 上で快適に Git を操作するための自分用の拡張。Marketplace には公開せず、このリポジトリの GitHub Releases で `.vsix` を配布する。変更履歴はリポジトリの [CHANGELOG.md](https://github.com/fukusaka/amonhen/blob/main/CHANGELOG.md) の「VSCode extension」の節、手を入れるときは [開発](https://github.com/fukusaka/amonhen/blob/main/docs/ja/development.md#vscode-拡張) と [設計](https://github.com/fukusaka/amonhen/blob/main/docs/ja/vscode.md) を参照。

> [!WARNING]
> 開発の途中。機能・ファイルの形・コマンドは、予告なく変わることがある。無保証で、そのままの形で提供する
> （リポジトリのルートのライセンスを参照）。大事なリポジトリは、バックアップを取っておくこと。

## 機能

アクティビティバーの Amon Hen アイコンを押すと、サイドバーにナビが、エディタ領域にメインビューが開く（コマンドパレットの「Amon Hen: Open Amon Hen」でもメインビューを開ける）。ナビで選んだものをメインビューに出す。

### ナビ（サイドバー）

| 項目 | 内容 |
| --- | --- |
| タイトルのボタン | Fetch（すべてのリモートから取得）、Pull（現在のブランチに upstream を取り込む）、Push（upstream へ。現在のブランチに upstream が無ければ、ボタンは Publish Branch になり、リモートとそこでの名前を選んで送り、upstream にする）、Undo…（何を戻すかを見せて確かめる）。`…` の中に New Branch、Stash Changes、Open Amon Hen、Refresh、Force Push、Undo History…（Undo できる操作を新しい順に並べ、選んだものとその後のものを戻す） |
| Working Copy | 変更の数。選ぶとメインビューに Working Copy を出す |
| History | 現在のブランチ。選ぶとメインビューに History を出す |
| merge / rebase の途中 | 「Rebase in progress (2/5)」のような行。Continue / Skip / Abort |
| Branches | `feature/x` はフォルダにまとめる。現在のブランチに ✓、upstream との差（↑↓）、upstream が消えていれば gone |
| Remotes / Tags / Stashes / Submodules | 開いたときに取得する |
| Reflog | エンジンがあるとき。選ぶと、HEAD が指してきたところをメインビューに出す |

- ブランチ・リモートブランチ・タグを選ぶと、メインビューの History にその履歴が出る。
- ブランチ・タグの右クリックで Checkout（リモートブランチなら追跡するローカルブランチを作って切り替える）、チェックアウトしていないローカルブランチなら Fast-Forward to Upstream（upstream より後ろにだけあるとき。最後の Fetch の時点。ほかの worktree でチェックアウト中のものは除く）か Push to Upstream（先にだけあるとき）、upstream の無いローカルブランチなら Publish Branch、Merge into Current Branch、Rebase Current Branch onto This、Rename Branch、Delete Branch、New Branch（そこから作って切り替える）、New Tag（メッセージを入れれば annotated tag）。作成と削除は Undo で戻せる。
- ローカルブランチの右クリックの Compare with Previous Version で、最後の rebase・amend の前の版（reflog から）と比べ、`git range-diff` と同じ形の読み取り専用の文書に出す。パレットの「Amon Hen: Range Diff…」では、好きな 2 つの範囲を比べる。
- エンジンがあるとき、Repository Settings（サイドバーの下）にこのリポジトリの git の設定がアプリと同じページで出る。行には効いている値、このリポジトリが上書きしている間の ↩︎、設定元、ⓘ（キー・ファイル・上書きしなければ効く値）があり、git の行にリポジトリを動かす git と形（shallow・partial・sparse・maintenance）が出る。Author and Signing に名前・メール・署名（OpenPGP・SSH・X.509。見つかった鍵から選ぶ）・mailmap、Commits にメッセージのテンプレート（なし・このリポジトリ専用（その場で書く）・ファイル）、Operations に Pull の取り込み方・fetch.prune・rebase.autoStash・rebase.updateRefs（git 2.38）・コンフリクトの書き方・rerere、Branches にほかのブランチを測る既定のブランチ、Working Copy に改行・実行ビット・`.git/info/exclude`、Fetching and Folders に shallow clone の取り足し・partial clone の全取得、一部のフォルダだけを取り出す（HEAD の木から選ぶ。sparse checkout、git 2.25）・すべてに戻す、git maintenance の切り替え（git 2.31）、Remotes にリモートの一覧（URL と指す先）と、URL の変更・fetch と別の push の URL・prune・つながるかの確かめ・名前の変更・削除・追加、Hooks and LFS にフックの一覧（いつ動くか・有効か。エディターで開く・雛形から作る）と Git LFS のパターン・ファイルの数、Keys and Values に git の設定の表（ページの行が聞くキー・このリポジトリで決めているキー・git が読むキーすべて。検索できる）と、効いている値・設定元、このリポジトリでのキーの変更・削除・追加がある。
- エンジンがあるとき、shallow clone の History には Shallow の札が付き、履歴が止まるコミットに ⋯ の印、最後に Fetch More…（100・1,000・10,000 コミット、ほかの数、日付まで）・Fetch All の帯が出る。partial clone には Partial の札が付き、押すと確かめてから取らずにおいた中身をすべて取る（git 2.36 から）。同じものはコマンドパレットにもある。
- エンジンがあるとき、History の署名したコミットに署名の印が付き（アプリと同じく、Verified は緑、鍵の信用度が決まっていない・署名者がコミッターと違うときは黄、不正な署名・失効した鍵は赤）、詳細に署名（説明・署名した人・鍵）と子のコミットが出る。History の上の HEAD のボタンと、コミットのメニューの Go to Parent・Go to Child で、そのコミットに移る。
- エンジンがあるとき、Branches はアプリと同じ並びになる。お気に入り（★。Move Up・Move Down in Favorites で決めた順）、今のブランチ（その積み重ねの根元ごと）と既定のブランチ、ほかは新しい順（`/` の名前はフォルダー、既定のブランチに取り込み済みなら「merged」）、畳んだ Inactive（90 日コミットがない）と Archived。ほかのブランチの上に積んだブランチは、その下に入れ子で出る。Add to Favorites・Archive はブランチのメニューにあり、git の設定に書くのでアプリと共有する。サイドバーの上の絞り込みのボタンで言葉を入れると、名前にどれも含むブランチ・リモートブランチ・タグだけを出す（Clear Filter で戻る）。ブランチ・タグをいくつも選ぶ（⇧・⌘ を押してクリック）と、今のブランチに 1 つのコミットで merge する、まとめて消す、まとめてお気に入り・アーカイブにする、ができる。
- エンジンがあるとき、File History（エクスプローラー・エディターのタブ・メインビューのファイルの右クリック、コマンドパレットなら今のエディターのファイル）は、ファイルを変えたコミットを名前の変更も追って並べ（前の名前ごとに見出しを挟む）、選んだコミットの詳細をそのファイルの位置で開く。Merge Commits でマージも出す。ダブルクリックか Enter でそのコミットを History で選び、右クリックで Show in History・Copy Commit ID・Copy Path。
- エンジンがあるとき、Blame（同じメニューから）は各行を最後に変えたコミットを出し、同じコミットの行をまとめて ID・作者・日時・件名を付ける。まだコミットしていない行はそう出す。コミットのファイル（History の詳細・File History）からならそのコミットの内容を、ほかは作業ツリーの内容を出し、書き換えると読み直す。行をダブルクリックすると、そのコミットを History で選ぶ。
- エンジンがあり worktree が 2 つ以上あるとき、Working Copy の下に変更の数と並ぶ。書き込む先（拡張が働くリポジトリ）は鉛筆、メインは家のアイコンで、変更のないものは No changes にまとまる。選ぶとそれを書き込む先にする。右クリックで Open in New Window・Reveal in File Manager・Open in Terminal・Copy Path・Remove Worktree…（先に尋ね、変更が消えるならそう言う）。New Worktree…（Working Copy・ブランチ・タグ、History のコミットから）はブランチと、メインの隣のフォルダーを作って書き込む先にする。Prune Worktrees はフォルダーの消えたものを片付ける。ほかの worktree が checkout しているブランチには ⌂ か ⑂ が付く。
- エンジンがあるとき、タグにリモートでの様子（最後の Fetch の時点で、どのリモートにもない「not on any remote」、同じ名前の別のタグがある「different on」）と署名の印（アイコン。説明はツールチップ）が付く。タグの右クリックで Push Tag（まだないリモートへ）・Replace Tag on Remote…・Delete Tag…（Undo で戻せる）・Delete Tag from Remote…、リモートの右クリックで Rename…・Edit URL…・Prune・Remove…、Remotes の見出しの `+` でリモートを足せる。
- Stashes の節は stash があるときだけ出る。その見出しの `+` で、作業中の変更を stash する（untracked のファイルを含めるかを選べる）。stash を選ぶと中身を複数ファイル diff で開き、右クリックで Apply / Pop / Drop（Undo で戻せる）。Apply と Pop は stage していた状態も戻す。エンジンがあるとき、stash の行には untracked のファイルを含むかが付き、ツールチップに作ったときのブランチと、土台から HEAD がどれだけ動いたかが出る。Branch… はその土台にブランチを作って stash を当て、見出しの Pop Latest Stash… は確かめてから最新の stash を pop する。
- Reflog は HEAD の reflog を新しい順に並べ、右に選んだ行のコミットを出す。どのブランチからも辿れなくなったコミット（reset・rebase・amend で外れたもの）には「not on any branch」と付く。一覧の上の New Branch…・Checkout と、右クリック（New Tag・Reset Current Branch to Here…・Show in History・Copy Commit ID・Copy Name）で取り戻せる。
- History のコミットの右クリックにも Checkout (Detached) と、エンジンがあれば Reset Current Branch to Here… がある。`git reset` と同じに Mixed・Soft・Hard を選ぶ。Hard は先に作業中の変更を stash に残し、Undo でブランチを戻せる。
- Submodules は右クリックで Update（初期化して記録されているコミットに合わせる）と、別のウィンドウで開く。エンジンがあれば、行にアプリと同じ印（取り出していない・違うコミット・中の変更・コンフリクト）と URL・追うブランチが出て、メニューに Update…（中の submodule も・追うブランチの最新）・Edit URL…・Branch to Follow…・Remove… も並ぶ。グループには Update All・Update Submodules…・Sync URLs・Add Submodule…。
- 複数のリポジトリを開いているときは、タイトルのボタンで切り替える。

### ドラッグ＆ドロップ

ナビのブランチ・リモートブランチ・タグを、ナビのほかの項目にドロップすると操作の候補が出る。どの操作もドロップ先を書き換える。

| ドラッグするもの | ドロップ先 | 選べる操作 |
| --- | --- | --- |
| ブランチ・リモートブランチ・タグ | ローカルブランチ | Merge（ドラッグしたものをドロップ先に merge）、Rebase（ドロップ先をドラッグしたものの上に rebase） |
| ローカルブランチ | リモート、リモートブランチ | Push、Force Push（`--force-with-lease`） |

- ドロップ先が checkout されていないとき、fast-forward で済むなら checkout せずにブランチだけ進める。そうでなければ checkout してから実行する。
- コマンドパレットの Merge Branches… で、いくつものブランチ・タグを `git merge a b c` と同じく 1 つのコミットで今のブランチに merge する。先にそれぞれのコンフリクトの見込みを出し、git が 1 つのコミットにまとめられないときは 1 つずつ merge することを勧める。Undo 1 回で戻せる。
- upstream が無いブランチをドロップで push すると、push 先を upstream に設定する。

### Working Copy

左に Conflicts / Staged / Changes のファイル、下にコミット欄、右に選んだファイルの差分が出る。差分は hunk ごとに並び、変わった部分は単語単位で強調する。

- 差分の行をクリックで選び（Shift+クリックで範囲）、Stage N Lines で選んだ行だけを stage する。Staged のファイルでは同じ操作で unstage になる。
- hunk ごとの Stage Hunk、Stage File、一覧の `+` / `−`、Stage All でもできる。一覧では上下キーで選び、Space か Enter で stage / unstage する。
- Changes のファイルは、一覧の ↺、Discard File、Discard Hunk、Discard N Lines で変更を破棄できる。untracked のファイルは消す。破棄した内容は Undo で戻せる。
- コミット欄は Summary と Description（複数行）。Amend にすると直前のコミットのメッセージが入る。`⌘Enter` / `Ctrl+Enter` でもコミットできる。
- コンフリクトしたファイルでは、Open in Merge Editor（標準の 3-way マージエディタ）、Accept Current、Accept Incoming、Mark as Resolved を選べる。片側で削除されたファイルも Accept で解決できる。解決済みにするとき、マーカーが残っていれば確認する。マーカーの残っていないファイル（git の rerere が前の解決を当てた、ほかのエディタで直した）は ✓ と「マーカーなし」か「rerere」を出して Mark as Resolved を先に勧め、Conflicts の見出しからまとめて解決済みにできる。そうしたファイルで Accept を選ぶと、解決した内容を置き換える前に確認する。

### History

左にコミットの一覧とグラフ、右に選んだコミットの詳細が出る。境目はドラッグで動かせて、幅は覚えておく。上部で Current Branch / All Branches / ナビで選んだ ref を切り替える。

- 一覧はクリックか、上下キーと PageUp / PageDown で選ぶ。履歴は 200 件ずつ、スクロールに合わせて読み込む。
- エンジンを使っていれば、検索の欄でアプリと同じ書き方でコミットを探せる（語、`author:`・`subject:`・`commit:`・`path:`・`contents:`・`after:`・`before:`、AND・OR・括弧）。一致には色が付き、Enter・Shift+Enter（∧ ∨、一覧では F3）で一致の間を移る。Matching Only で一致したコミットだけを出す。`contents:` の検索は、分かった一致と進み具合を途中から出す。partial clone では、先に取る中身の数を出して確かめ、多すぎれば `path:`・`after:`・`before:` で絞るよう頼む。
- 詳細には、メッセージ全体、ブランチやタグ、作者と日時、コミット ID、親コミット（クリックでそのコミットへ移る）、変更ファイルと、ファイルごとの差分が並ぶ。差分は画面に入ったファイルから読み込む。merge コミットは最初の親との差分。
- 各ファイルの Open、Open All、一覧のダブルクリック・Enter で、標準の diff エディタでも開ける。

HEAD から first-parent で辿れるコミット（最初の merge コミットの手前まで）は、右クリックで編集できる。どれも非対話の rebase で行い、Undo で戻せる。作業中の変更は自動で退避して戻す。編集は rebase と同じく先に確かめる。エンジンを使っていれば、積み重ねたブランチ（書き換えるコミットに先頭があるブランチ）のうち一緒に動くもの・残るもの、あとの push に Force Push が要る push 済みのブランチ、コンフリクトしそうなコミットを確認に出し、2 つ目のボタンでその回だけ積み重ねたブランチを動かす・動かさないを選べる。

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

### パッチ

History のコミットを右クリックして Save as Patch…（そのコミット）か Save Patches from Here to HEAD…（HEAD までの
ひと続き）で、1 コミット 1 ファイル（`git format-patch`）か 1 つの mbox に書き出せる。コマンドパレットの
Apply Patch…、またはエクスプローラーで .patch・.diff・.mbox・.eml のファイルから、パッチが何を変えるかとそのまま
当たるかを見てから、メールの形のパッチはコミットとして（`git am`）、diff は作業ツリーに（`git apply`）当てる。当たら
ないパッチがあると、ナビに「Applying patches (1/3)」と、Retry with 3-Way Merge・Continue・Skip・Abort が出る。
どちらも Undo で戻せる。

### 途中で止まった merge / rebase

コンフリクトや Edit・Split で止まると、ナビに「Rebase in progress (2/5)」のような行が出る。解決して stage したら Continue で続ける。rebase では Skip で今のコミットを飛ばせる。Abort で始める前の状態に戻る。

### Undo

ナビの Undo で、直前の操作を戻す。対象はコミット（amend を含む）、Merge、Rebase、fast-forward、Pull、ブランチ・タグの作成と削除、Cherry-pick、Revert、パッチを当てる操作、変更の破棄、stash の削除、History でのコミットの編集。

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

どちらでも、拡張はその機械の Amon Hen の設定（`~/.config/amonhen/settings.json`。アプリの設定 > Git > プログラムで
選ぶ）の git を動かし、選んでいないときだけ VSCode の `git.path` を使う。選んだ git がなければ、git を動かすコマンドは
エラーで止まる。

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
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-vscode.sh | sh -s -- stable
```
