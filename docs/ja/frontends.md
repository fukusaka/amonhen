# FE ごとの機能

[English](../frontends.md)

利用者に見える機能が、どの FE にあるか: macOS のアプリ（`apps/macos`）、GTK4 のアプリ（`apps/gtk4`）、Windows のアプリ（`apps/windows`、WinUI 3、まだ配っていない）、VSCode 拡張（`apps/vscode`）、CLI（`crates/cli`）、端末の FE（`amonhen tui`、`crates/tui`）。行は機能を説明する [docs/ux.md](ux.md)（おもに macOS のアプリの形）の章の順で、History・サイドバー・Blame / File History・ステータスバー・確認・SSH を足した。設計ではなくコードで確かめた。

印:

- ✓ できている。その FE の形のこともある（ポップオーバーの代わりにツールチップ、シートの代わりに入力欄）。どう違うかは Notes の列。
- △ 一部だけ。何が足りないかは Notes の列。 Windows のアプリは、macOS のアプリと形が違えば、Windows がその形を許さない場合を除いて △ にする。Windows の形は Notes の列。
- — ない。
- n/a その FE には当てはまらない（CLI にサイドバーはない）か、わざと土台に任せている（VSCode 本体の設定・キー割り当て・出力・マージエディター）。何が代わりかは Notes の列。

CLI の △「`amonhen run` から」は、コアの命令はあるが、CLI からは JSON（`amonhen run '{"op":…}'`）でしか届かず、専用のサブコマンドがないこと。

メニューの操作そのものは 1 つずつ並べない: どの FE も共有の表 `crates/commands` から取り、GTK4 のアプリでまだないものは `apps/gtk4/src/appmenu.rs` の `PENDING` に並べる（今はない）。

この表は手で保つ。どの FE でも、機能を足す・外す・変えるときは、同じ変更でここも直す。

## macOS と GTK4 のアプリのプラットフォームによる違い

GTK4 のアプリは macOS のアプリと同じふるまいをめざす。わざと違えるのは次だけで、どれもプラットフォーム（おもに [GNOME HIG](https://developer.gnome.org/hig/)）に従う:

- 最後の窓を閉じるとアプリが終わる。
- ログインシェルの環境変数を読み込まない（Linux のセッションはもう持っている）。
- Linux と BSD では、Watchman が入っていて git が 2.26 以降なら fsmonitor に使う（BSD の git には fsmonitor--daemon がない。Linux では git 2.55 からあるが、Amon Hen はまだ使わない）。
- Apple Intelligence はない。Claude Code と Codex だけ。
- Finder の代わりにデスクトップのファイルマネージャー。外部エディターはデスクトップ項目から探す。
- ヘッダーバーはアイコンだけ（ツールチップつき）。ツールバーのカスタマイズにアイコン・文字の選択はない。
- History の検索は GNOME の検索バー: ヘッダーバーの切り替えと Ctrl+F で、History の上にバーを出す。History で文字を打っても、打った文字でバーを開く（type-to-search。スペース・矢印・Ctrl や Alt との組み合わせでは開かない）。日本語入力のときは最初の一打が英字のまま入る（日本語入力は欄にフォーカスが移ってから働くため）。日本語で探すときは先に Ctrl+F を押す。
- 開いている吹き出し（CI の吹き出し・メニュー）の外を押すと、閉じるだけ（GTK の吹き出しはどれもそう）。macOS はその押しも押した先に届くので、別の吹き出しを開くのが macOS は 1 回、GTK4 は 2 回。
- 取り消せない操作の確認: キャンセルが先で Return と Esc はキャンセル、壊すボタンは赤。キャンセルは強調しない。
- GNOME が使うキーの近道は、ずらすか割り当てない（下の「キーボードの近道」）。
- キーの近道を一覧する窓がある（GNOME のアプリの流儀。macOS はメニューに出る）。
- 「この Mac」・Homebrew・launchd の代わりに「このコンピューター」・パッケージマネージャー・systemd や cron。

## 全体の配置

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Workspace の行でメインの表示を切り替える（Working Copy・History・Stashes・Reflog…） | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Working Copy・History。エンジンがあれば Reflog・Repository Settings、開いている間は Range Diff。Conflicts はコンフリクトがあるときだけで、押すと最初のファイルをマージエディタで開く。Stashes はサイドバーの欄。<br>TUI: Changes・History・Reflog（`1` `2` `3` でも）。stash はサイドバーに並べる。 |
| コンフリクトがあるあいだ Conflicts の行と数 | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Working Copy の中の Conflicts のグループ。<br>TUI: サイドバーの Workspace の下。選ぶと Working Copy。 |
| stash があるときだけ Stashes の行と数 | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Stashes の節。<br>TUI: stash があるあいだサイドバーに Stashes の節。見出しに数。 |
| Range Diff を開いているあいだの Range Diff の行 | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: ほかのファイルの Blame・File History に替わるまで。 |
| Branches・Remotes・Tags を絞り込む欄 | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: サイドバーの上の絞り込みのボタンで言葉を入れ、合うブランチ・リモートのブランチ・タグを名前のまま平らに並べる。<br>TUI: サイドバーで `/`。 |
| ブランチ・タグをいくつも選ぶ | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Copy Names・Merge into Current Branch・Delete Branch・お気に入り・Archive が選んだものすべてに効き、タグだけなら Push・Replace・削除・リモートから削除も。<br>TUI: サイドバーの `v` で ref に印をつけ、`m` で 1 つのコミットで取り込み、`D` で印をつけたブランチを消し、`*` `z` でお気に入りにする・アーカイブする。 |
| Pull Requests・Worktrees は中身があるときだけ出す | ✓ | ✓ | ✓ | △ | n/a | △ | VSCode: Worktrees だけ（GitHub は入れない）。<br>Windows: Worktrees は Working Copy の下に出す。<br>TUI: Worktrees だけ（2 つ以上あるとき）。 |

## サイドバー

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Branches の並び（今のもの・活発・止まった・マージ済み・アーカイブ・積み重ね） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: エンジンがあるとき。なければ `/` で分けたフォルダーの木。<br>CLI: `amonhen run`（`branchGroups`）から。<br>TUI: お気に入り（★）、今のブランチと既定のブランチ、動いているもの（新しい順）、積んだものは土台の下（↳）、取り込み済みに印、Inactive・Archived は畳む（Enter で開く）。`/` のフォルダーはない。 |
| お気に入りのブランチ | ✓ | ✓ | ✓ | ✓ | — | ✓ | TUI: サイドバーのブランチで `*`。 |
| ブランチのアーカイブ | ✓ | ✓ | ✓ | ✓ | — | ✓ | TUI: サイドバーのブランチで `z`。 |
| ブランチを片付ける（取り込み済み・PR でマージ済み・上流が消えた。Undo 1 回） | ✓ | ✓ | ✓ | ✓ | △ | △ | VSCode: エンジンがあるとき、理由ごとの見出しを付けた複数選択の一覧（コマンドパレット、Branches の見出しの右クリック）。CLI: `amonhen run`（`branchCleanup`・`deleteBranches`）から。マージ済みの PR はアプリと VSCode が gh で読む。<br>TUI: 命令の一覧の Clean Up Branches。`a` で上流の消えたブランチも含める。PR でマージされたブランチは探さない（GitHub はない）。 |
| PR でマージ済みのブランチの印（GitHub で squash・rebase） | ✓ | ✓ | ✓ | ✓ | △ | — | VSCode: エンジンがあるとき。番号は行のツールチップに出る。CLI: `amonhen run`（`pullRequests` を渡した `branchGroups`）から。マージ済みの PR はアプリと VSCode が gh で読む。 |
| お気に入りをドラッグで並べ替える | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: メニューの Move Up・Move Down in Favorites（ブランチに落とすと Merge・Rebase）。<br>TUI: お気に入りで `<` `>`。 |
| ブランチのメニュー: checkout・名前の変更・削除（未マージは Delete Anyway） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | macOS: Set Upstream は Repository メニューだけ。<br>GTK4: Set Upstream は Repository メニューだけ。<br>VSCode: コマンドパレットの Rename Branch… で今のブランチの名前を変え、Undo で前の名前に戻る。いくつも消すときは、それぞれのほかのどこにもないコミットの数を添えて 1 度だけ尋ね、Delete Anyway で消す。<br>CLI: `amonhen run` から。<br>TUI: `c` で checkout、`n` で名前の変更、`D` で削除。ほかのどこにもないコミットがあれば、その数と絵を出して Delete Anyway。 |
| checkout していないブランチの同期（Fast-Forward・Push・Publish） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Fast-Forward は先に尋ねる。メインビューが開いていれば前と後の絵のシート、なければ modal。<br>CLI: `amonhen run` から。<br>TUI: サイドバーのそのブランチで `p`・`P`。 |
| タグの一覧 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| タグのリモートの印（ない・違う）とポップオーバー | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: 行の説明に出し、リモートごとの様子はツールチップに（ポップオーバーはない）。<br>CLI: `amonhen run`（`tagRemotes`）から。<br>Windows: 名前の後ろに ⚠（リモートに別のタグ）か ∅（どのリモートにもない）。押すとポップオーバー。<br>TUI: 行の右端に ⚠ か ∅。ポップオーバーの代わりに `i` で一番下の行にリモートごとの様子を出し、`t` で push・置き換え。 |
| タグの署名の印 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: 行のアイコンで、説明はツールチップに。<br>CLI: `refs --verify`。<br>TUI: 行の右端に ✓（正しい署名。鍵を信用していなければ黄色）・!（期限切れ・失効）・✗（正しくない）・?（確かめられない）。`i` で署名の詳細を枠に出す。 |
| タグのメニュー: Push to・Replace on・削除・リモートから削除 | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: リモートがいくつもあれば一覧で選ぶ。Push はタグを並べて先に聞く。<br>CLI: `amonhen run` から。<br>TUI: `D` で手元のタグを消し、`t` でリモートへ push か置き換え、`X` でリモートから消す。リモートは数字で選ぶ。 |
| リモートとそのブランチ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| リモートのメニュー: 追加・名前の変更・URL の変更・削除・Prune | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: アプリの 1 つの Edit… のシートの代わりに Rename…・Edit URL…・Edit Push URL…・Check Connection。<br>CLI: `amonhen run` から。<br>TUI: 命令の一覧の Add Remote。サイドバーのリモートのブランチの上で `e` で名前と URL を直し、`X` で Prune、`x` で消す。 |
| Pull Requests のグループ | ✓ | ✓ | ✓ | — | — | — |  |
| Submodules のグループ（Update・新しい窓で開く） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `submodule`・`submodule update`。<br>TUI: サイドバーの Submodules の節。Enter でその submodule で TUI を開き直す。命令の一覧に Update Submodules。 |
| submodule の印（取り出していない・違うコミット・中の変更・コンフリクト）とメニュー: Update…（中の submodule も・追うブランチの最新）・URL の編集・追うブランチ・外す（.git/modules も消せる）・追加・Sync URLs | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: Remove は 2 つのボタンで、.git/modules を残すか一緒に消すかを聞く。<br>CLI: `submodule`（`add`・`remove`・`set-url`・`set-branch`・`sync`）。<br>TUI: 行の右端に印。コマンドの一覧に Update Submodules・Add Submodule、submodule の行で `e` URL、`B` 追うブランチ、`x` 外す（`d` で .git/modules も消す）、`y` Sync URLs。 |
| submodule: リポジトリのデータを .git/modules に移す・それぞれで命令を動かす | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: Run in Each Submodule… の出力は出力パネルに出る（アプリの Activity にあたる）。<br>CLI: `submodule absorb`・`submodule foreach`。<br>TUI: submodule の行で `a`・`F`。命令の出力は `!` で出す。 |
| Working Copy の下の worktree と変更の数（✎ ⌂ ⑂） | ✓ | ✓ | ✓ | ✓ | △ | △ | VSCode: 印は行のアイコン。ほかの worktree が checkout しているブランチは ⌂ か ⑂ だけ（worktree の名前はツールチップ）で、Checkout の代わりに Open Worktree。<br>CLI: `worktree list --changes`。<br>TUI: 2 つ以上あればサイドバーに（フォルダ・ブランチ・変更の数）。Working Copy の下ではない。 |
| shallow clone・partial clone（History）と sparse checkout（Working Copy）の印 | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: History の上の Shallow・Partial（Partial は押すと中身をすべて取る）。sparse checkout なら Working Copy の行に「· some folders」。<br>TUI: 一番上の行のブランチの後ろに [shallow]・[partial]。sparse checkout は Working Copy の一覧の上に知らせ。 |

## 1. タブと窓

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| 窓の中のリポジトリのタブ | ✓ | ✓ | ✓ | n/a | n/a | n/a | Windows: 後ろのタブもリポジトリを開いたまま。戻ると、離れたときの画面を描き直す。<br>TUI: タブも窓もない。ほかの worktree で Enter、Quick Open で、TUI をそこで開き直す。 |
| 別の窓（New Window） | ✓ | ✓ | ✓ | n/a | n/a | n/a | Windows: New Window は今のリポジトリの窓を開き、Repositories を出す。タブは引いてほかの窓へ移すか、外へ引き出して新しい窓にする。Window のメニューに窓が並ぶ。 |
| 起動時に窓とタブを戻す | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| SSH・daemon のリポジトリのつなぎ中のタブ（Cancel・Try Again） | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| 題に未コミットの ● と ahead/behind | ✓ | ✓ | ✓ | n/a | n/a | n/a | GTK4: タブの名前に出る。<br>Windows: 窓のタイトルとそれぞれのタブの名前に。<br>TUI: 一番上の行にブランチと ahead/behind。 |
| 最後の窓を閉じてもアプリは終わらない | ✓ | — | — | n/a | n/a | n/a | GTK4: GTK のアプリの既定と同じく終わる。<br>Windows: Windows のアプリと同じく終わる。 |

## 2. ブックマーク（リポジトリの一覧）

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| All・Favorites・Places・グループのあるリポジトリのページ | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: Ctrl-o かコマンドの一覧の Repositories で、画面に重ねる。Quick Open にも並ぶ。 |
| 手元・SSH のリポジトリを足す | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: リポジトリのページの `a`。この機械のパスか `ssh://host/path` を打つ。 |
| この機械の探すフォルダー（深さ・見張り） | △ | △ | △ | n/a | n/a | △ | macOS: 除くものは決まった一覧。<br>GTK4: 除くものは決まった一覧。<br>Windows: 除外は決まったもの。フォルダは Repositories を出している間だけ見張る。<br>TUI: 除外は決まった並び。リポジトリのページの `a` で足し、🔍 の上で `d` 深さ、`r` 探し直し、`x` やめる。ページを開いている間は見張る。 |
| SSH 先の探すフォルダー | ✓ | ✓ | ✓ | n/a | △ | ✓ | CLI: `find-repositories` がアプリのために探す。<br>TUI: SSH 先のエンジンが探す（要れば、送ってよいか聞いてから）。 |
| 場所の下のリポジトリのフォルダーの木 | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: リポジトリのページの、場所ごとの下。 |
| 手で作るグループ・行を Favorites やグループへドラッグ | △ | △ | ✓ | n/a | n/a | △ | macOS: グループそのものはドラッグで並べ替えられない。<br>GTK4: グループそのものはドラッグで並べ替えられない。<br>TUI: `g` でグループを打つ。ドラッグはない。 |
| 状態の列（ブランチ・変更・ahead/behind・最後のコミット・開いた時）、並べ替え、列の順の入れ替え | ✓ | ✓ | ✓ | n/a | n/a | △ | TUI: `s` で並べる列を替える。列は並べ替えられない。 |
| お気に入り・表示名・色のラベル | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: リポジトリのページの `*`・`n`・`c`。 |
| タブ・窓で開く、Finder（ファイル）・Terminal・エディタ | ✓ | ✓ | ✓ | n/a | n/a | ✓ | GTK4: 知っているコードエディタと「その他…」で選んだプログラム以外は、inode/directory を宣言しているアプリだけがフォルダを開く。<br>Windows: エクスプローラー・Windows Terminal（なければ cmd）・Settings で選んだエディタ。<br>TUI: Enter でそこで開き直す。`t` でそのフォルダのシェル（端末を渡す）、`e` で git のエディタ、`o` でファイルの窓。 |
| 選んだリポジトリを Fetch | ✓ | ✓ | ✓ | n/a | n/a | ✓ | Windows: Ctrl・Shift で行を選び、選んだ行のメニューの Fetch で、選んだものすべてを fetch する。<br>TUI: `v` で行に印をつけ、`f` で裏で Fetch。SSH 先のリポジトリは共有の daemon 越しに（要れば、エンジンを送ってよいか聞いてから）。 |
| Quick Open（あいまい検索） | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: 命令の一覧の Quick Open が、アプリと同じものを並べる: 共有のブックマーク（`bookmarks.json`。お気に入りを先に）、最近開いたもの（`recent.json`）、手元の探すフォルダの下で見つけたもの（裏で探す。SSH 先のフォルダは探さない）。Enter で TUI をそこで開き直す。 |
| ツールバーのリポジトリ名で切り替える | ✓ | ✓ | ✓ | ✓ | n/a | n/a | VSCode: Select Repository でワークスペースのリポジトリから選ぶ。<br>TUI: Quick Open が代わり。 |
| Clone（自分の GitHub のリポジトリから選ぶ） | ✓ | ✓ | ✓ | n/a | △ | △ | VSCode: VSCode 自身の Git: Clone。<br>CLI: `clone <url>` だけ。<br>Windows: clone したものは新しいタブで開く。<br>TUI: Quick Open に URL を打つ。GitHub はない。 |
| Clone: 取得する範囲（すべて・中身は必要なときに・最近の履歴）と、一部のフォルダだけ | ✓ | ✓ | ✓ | n/a | ✓ | ✓ | VSCode: VSCode の Git: Clone が代わり。<br>CLI: `clone --filter`・`--depth`・`--sparse`。<br>TUI: 場所の次に聞く。 |
| Subtrees のグループ（git に git subtree があるとき）: 追加・Pull・Push・Split、相手と squash を覚える | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: 値は入力欄、squash は一覧で聞く（シートではない）。<br>CLI: `subtree`。<br>TUI: コマンドの一覧の Add Subtree、subtree の行の `p` `P` `t`。相手は一番下の行に打ち、squash もそこで選ぶ。 |
| Clone: submodule も取り出す | ✓ | ✓ | ✓ | n/a | ✓ | ✓ | VSCode: VSCode の Git: Clone が代わり。<br>CLI: `clone --recurse-submodules`。<br>TUI: clone の前に聞く。 |
| 新しいリポジトリの Init | — | — | — | n/a | — | — | macOS: docs/ux.md の File メニューにあるが、作っていない。 |

## 3. Settings

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| General: 開いたリポジトリをブックマークする | ✓ | ✓ | ✓ | n/a | n/a | n/a | TUI: 設定のページはない。設定はアプリと `amonhen run` が変え、TUI は読む。マウスの既定は `settings.json` の `tui.mouse`。 |
| General: 起動時に開き直す・既定はタブか窓か | — | — | — | n/a | n/a | n/a | macOS: いつも開き直し、切り替えはない。<br>GTK4: いつも開き直し、切り替えはない。 |
| General: 自動で fetch | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: VSCode の `git.autofetch` が代わり。<br>Windows: Windows: 失敗はステータスバーに添える。 |
| General: 外のエディター | ✓ | ✓ | ✓ | n/a | n/a | n/a | GTK4: コードエディターは desktop のエントリーで macOS 版と同じ順に探す。なければテキストエディター（オフィスのアプリは選ばない）。Other… ではプログラムを選ぶ。<br>Windows: 知っているコードエディタはインストール先か PATH の起動用のスクリプトから、macOS 版の順に探す。なければメモ帳（フォルダは開けない）。その他… で実行ファイルを選べる。<br>TUI: git のエディタ。 |
| General: 端末と diff のツール | — | — | — | n/a | n/a | n/a |  |
| Git: 機械全体のページ（Author and Signing・Commits・Operations・Working Copy・Keys and Values） | ✓ | ✓ | ✓ | — | △ | n/a | CLI: `amonhen run` から。 |
| Git > Commits: 共有のメッセージのテンプレート | ✓ | ✓ | ✓ | — | — | n/a |  |
| Git > Programs: git・gpg・ssh-keygen・gpgsm | ✓ | ✓ | ✓ | n/a | △ | n/a | VSCode: 拡張は、ここで機械に選んだ git を動かし、選んでいなければ VSCode の `git.path`。gpg・ssh-keygen は git の設定で決まる。<br>CLI: `amonhen run`（`programs`・`setMachineGit`）から。<br>Windows: Settings の Git に。 |
| Git > この機械の Environment Variables（渡す・秘密を隠す） | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: VSCode の環境が代わり（拡張はそれを daemon に渡す）。 |
| Environment Variables: ログインシェルのものを使う（Read Again・SSH agent） | ✓ | — | — | n/a | n/a | n/a | GTK4: 外した。Linux のセッションはすでに持っている。<br>Windows: 外した。Windows はログインシェルを経ずに利用者の環境をアプリに渡す。<br>VSCode: VSCode の環境が代わり（拡張はそれを daemon に渡す）。 |
| SSH Hosts: + / − の付いたホスト、届かないものは薄く | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: Remote-SSH が受け持つ。 |
| SSH Hosts > Connection: 状態・Check Connection・エンジン | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| SSH Hosts > Connection: 署名の確かめ方 | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| SSH Hosts > Connection: 探すフォルダー | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| SSH Hosts: ホストのすべてのリポジトリの git のページ | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| SSH Hosts: ホストの Environment Variables（ログインシェルの切り替え） | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| ホスト・この機械の列で選ぶパスのシート | ✓ | ✓ | ✓ | n/a | n/a | n/a | Windows: Repositories の「リポジトリを追加」「スキャンするフォルダを追加」と SSH 先のフォルダ。ファイル > 開く… と Clone は Windows 標準のダイアログ。 |
| Accounts: GitHub（CI の状態・アバター） | ✓ | ✓ | ✓ | — | n/a | — |  |
| Appearance: History の順・1 行・行の項目・アバター | ✓ | ✓ | ✓ | n/a | n/a | n/a | TUI: `o`・`z` で、その回だけ順と畳み方を変える。 |
| Appearance: フォント・グラフの色 | — | — | — | n/a | n/a | n/a |  |
| Theme: 窓の背景（透過とぼかし・背景画像・ツールバーの色・リポジトリごと） | ✓ | ✓ | ✓ | n/a | n/a | n/a | GTK4: ぼかしはない（GNOME で窓の後ろをぼかせるのは GNOME 51 と GTK 4.24 から。`ext-background-effect-v1` を使うが、アプリはまだ使っていない）。透かすにはコンポジタが要る。<br>Windows: ぼかしの強さは Acrylic で決まっている。 |
| Theme: 構文と差分の色（内蔵のテーマ・VSCode のテーマ・差分は標準の色） | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: VSCode 自身のテーマに従う。<br>Windows: Windows のアプリ モード（明るい・暗い）に合わせ、外観ごとにテーマを選ぶ。<br>TUI: 端末の色に従う。 |
| Diff & Merge: 開いたときに迷わないコンフリクトを解く | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| Diff & Merge: unified/split・空白・前後の行・折り返し・タブの幅 | — | — | — | — | n/a | n/a |  |
| AI: 使う AI・CLI の場所と版・モデル・言語・リポジトリごとの許可 | ✓ | ✓ | — | — | n/a | — | どちらのアプリも CLI のログインの状態は読まない。CLI がなければ、入れてサインインするよう案内する。 |
| 目的ごとの AI のプロンプト（編集・既定に戻す・試す） | ✓ | ✓ | — | — | n/a | — |  |
| Performance: commit-graph・共有の daemon | ✓ | ✓ | ✓ | ✓ | n/a | n/a | VSCode: VSCode の設定の `amonhen.writeCommitGraph`・`amonhen.daemon.enabled`・`amonhen.daemon.path`。 |
| Performance: fsmonitor | ✓ | ✓ | ✓ | ✓ | n/a | n/a | GTK4: Linux・BSD では、エンジンを git の fsmonitor の hook（`amonhen fsmonitor-watchman`）にして Watchman を使う。Watchman がないか、git が 2.26 より古い（hook の版 2 がない）と押せない。<br>VSCode: VSCode の設定の `amonhen.fsmonitor`（既定はオン）。 |
| Performance: 永続キャッシュの場所・大きさ・消す | ✓ | ✓ | ✓ | — | △ | n/a | History と AI の結果を 1 行ずつ、大きさと「消す」。CLI: `log --rebuild`・`daemon rebuild`。<br>Windows: History だけ（AI はない）。 |
| 設定の変更の Undo・Redo | △ | △ | △ | n/a | n/a | n/a | macOS: Git のページとリポジトリの設定だけ。<br>GTK4: Git のページとリポジトリの設定だけ。<br>Windows: Git のページとリポジトリの設定だけ（文字の欄の外で Ctrl+Z・Ctrl+Shift+Z）。 |

## 4. リポジトリの設定

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| 設定のページ（ページの一覧・git の行・行ごとの出どころ・↩︎・ⓘ） | ✓ | ✓ | ✓ | ✓ | n/a | n/a | VSCode: メインビューの Repository Settings（エンジンがあるとき）で、このリポジトリの設定だけ。<br>Windows: ページを左に並べる（一般・Git・表示・差分とマージ・パフォーマンス）。設定の行には設定元・↩︎・ⓘ（段ごとの値をツールチップで）。<br>TUI: 設定のページはない。アプリと `amonhen run` が変える。 |
| Author and Signing（署名の選択・集めた鍵・mailmap） | ✓ | ✓ | ✓ | ✓ | △ | n/a | VSCode: 鍵は今の形式のものだけを集める。<br>CLI: `amonhen run`（`signingKeys`・`configSet`）から。 |
| Commits: メッセージのテンプレート（なし・共有・独自・ファイル） | ✓ | ✓ | ✓ | △ | △ | n/a | VSCode: なし・このリポジトリ専用（その場で書く）・ファイル（行の下に最初の数行を出す）。共有のテンプレートはない。<br>CLI: `amonhen run` から。 |
| Branches: 既定のブランチを選ぶ | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: `amonhen run` から。 |
| Operations: pull・fetch.prune・autostash・updateRefs・コンフリクトの書き方・rerere | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: `amonhen run` から。 |
| rerere: 1 つの解決を忘れる（マージエディタ）・すべて忘れる（リポジトリの設定） | ✓ | ✓ | ✓ | ✓ | △ | n/a | VSCode: rerere が解決したコンフリクトの Working Copy の右クリックに Forget Resolution（マージエディタは VSCode のもの）、Repository Settings > Operations に Forget All…。<br>CLI: `amonhen run`（`forgetResolution`・`forgetAllResolutions`）から。 |
| Working Copy: 改行・実行ビット・`.git/info/exclude` の編集 | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: `amonhen run` から。 |
| Remotes: 表と、つながるか確かめるシート | ✓ | ✓ | ✓ | ✓ | △ | n/a | VSCode: 表と行ごとのボタン（Edit URL…・Push URL…・Check・Prune・Rename…・Remove…）と Add…。値はシートでなく入力欄で聞く。<br>CLI: `amonhen run` から。 |
| Hooks: 表・Enabled・スクリプト・Create Hook… | ✓ | ✓ | ✓ | ✓ | △ | n/a | VSCode: Open で VSCode のエディターにスクリプトを開く。<br>CLI: `amonhen run` から。<br>Windows: Windows ではフックを止められない（git が実行の許可にかかわらず動かす）。Edit With はない |
| LFS: 入っているか・パターン・Track Pattern…・ファイルの数 | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: `amonhen run` から。 |
| Keys and Values（git config の表、Basic / Customized / All、Combined / Levels） | ✓ | ✓ | ✓ | ✓ | △ | n/a | VSCode: Basic / Customized / All、Combined / Levels と検索。section ごとの組、列での並べ替え、いくつもある値はすべて出す。値は入力欄で変える。<br>CLI: `amonhen run`（`configList`・`configSet`）から。 |
| 要る git の版を添えて行を無効にする | ✓ | ✓ | ✓ | ✓ | n/a | n/a | Windows: 取得とフォルダの行（partial clone・sparse checkout・maintenance） |
| git の行にリポジトリの形（shallow・partial・sparse・maintenance） | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: `status` が shallow・partial・sparse を添える。 |
| 取得とフォルダ: shallow clone の履歴を取り足す（数・日付まで・すべて） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: `amonhen run`（`deepen`・`unshallow`）から。<br>TUI: History の `+`。 |
| 取得とフォルダ: partial clone の中身をすべて取る（git 2.36） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: `amonhen run`（`fetchAllContents`）から。<br>TUI: History の `+` から `c`。 |
| 取得とフォルダ: sparse checkout のフォルダを HEAD の木から選ぶ（git 2.25） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: フォルダは HEAD の木を一覧でたどって選ぶ。<br>CLI: `amonhen run`（`sparseCheckout`・`treeFolders`）から。<br>TUI: Working Copy の `W`。フォルダを打ち、Tab で HEAD の木から補う。空ならすべて取り出す。 |
| 取得とフォルダ: git maintenance の切り替え（git 2.31） | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: `amonhen run`（`maintenance`）から。 |

## 5. ツールバー

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| 履歴のない新しいブランチ（空から・ファイルを残して） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: コマンドパレットと Branches のグループの右クリックから。<br>CLI: `amonhen run`（`createOrphanBranch`）から。<br>TUI: 命令の一覧から。 |
| ブランチの切り替え（あいまい検索・最近・追跡・新しいブランチ） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Switch Branch…（QuickPick）。サイドバーの ⋯・Branches の見出し・コマンドパレットから。⌘⇧B は VSCode のビルドのタスクなので近道はない。<br>CLI: `amonhen run` から。<br>TUI: `Ctrl-b`（HEAD の reflog から最近のものを先に、打った語で絞る。リモートのものは追跡するブランチを作り、合うものがなければ新しいブランチ）か、サイドバーのブランチで `c`。 |
| Fetch のシート（すべてか 1 つのリモート、prune を示す） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: リモートが 1 つならモーダル、いくつもならすべてか 1 つを QuickPick で。<br>CLI: `amonhen run` から。<br>TUI: `f` で一番下の行で尋ねる。`a` ですべてのリモート、数字で 1 つ。prune の設定を示す。 |
| Pull のシート（コミット・merge / rebase / ff-only・なければ上流） | ✓ | ✓ | ✓ | ✓ | △ | △ | VSCode: メインビューが開いていればシート（取り込み方ごとの前と後の絵、コミット）。なければ QuickPick。コンフリクトの見込みはない。<br>CLI: `amonhen run` から。<br>TUI: `p`。分かれているときだけ、コンフリクトしそうなファイルを言ってから、git の設定のほかに merge か rebase を選ばせる。そうでなく上流が進んでいれば、数と 3 つまでの件名を添えて尋ねる。上流がなければ、取り込むリモートのブランチを選ばせて上流にする。上流だけが進んでいれば fast-forward only も選べる。手元だけのコミットは数だけで、一覧はない。 |
| Push のシート（注意・コミットと差分・lease 付きの Force Push） | ✓ | ✓ | ✓ | ✓ | △ | △ | VSCode: メインビューが開いていればシート（push・force push の後の絵、注意、送るコミット、上流から消えるコミット、Pull… か lease 付きの Force Push）。なければ同じ中身の QuickPick。送る変更すべてとそれぞれのコミットは、選ぶと差分を複数ファイルの diff エディタで開く（シートは閉じない）。上流のないブランチの Force Push は、Push と同じく Publish になる。<br>CLI: `amonhen run` から。<br>Windows: 窓の 85% の ContentDialog（窓の大きさを変えると合わせ直す）。左に送るコミット、右に選んだものの差分。ContentDialog なので、窓の外に出したり、動かしたり、大きさを変えたりはできない。<br>TUI: `P` はコミットの数と 3 つまでの件名を添えて尋ね、上流が進んでいれば lease 付きの Force Push とその絵も出す。差分はない。 |
| Publish（リモートと名前を選ぶ） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: `amonhen run` から。<br>TUI: 上流のないブランチで `P`。 |
| Merge（fast-forward だけ・no-ff・squash の選択） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: メインビューが開いていれば、取り込み方ごとの絵の付いたシート。なければ一覧。選ぶことが確認を兼ねる。<br>CLI: `amonhen run` から（`mode`）。<br>TUI: サイドバーで `m`。 |
| いくつもを 1 つにマージ（順番・コンフリクトの見込み） | ✓ | ✓ | ✓ | ✓ | △ | △ | VSCode: 選んだ順。Change the Order… で merge する順に 1 つずつ選び直す（QuickPick ではドラッグできない）。最後より前のものがぶつかりそうなら、それを最後に回す項目も出す。<br>CLI: `merge-check` で確かめ、マージは `amonhen run` から。<br>Windows: History の選択から。順（ドラッグ）、ぶつかる見込みと最後に回す、取り込み方、1 つずつ、メッセージ、前と後の絵、git が断ったら 1 つずつ Merge。<br>TUI: サイドバーの `v` で印をつけた ref で `m`。印をつけた順に、それぞれが持ち込むコミットの数とコンフリクトの見込みを添える。`l` でコンフリクトしそうな最初のものを最後に回す。ほかの並べ替えはない。 |
| 前と後を描く Rebase のシート | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: メインビューが開いていれば絵の付いたシート。なければダイアログに、今と rebase の後のコミットを文で並べる。<br>CLI: `rewrite-preview` が出し、rebase は `amonhen run` から。<br>TUI: サイドバーで `r`。絵は画面に重ねる。 |
| Stash（メッセージ・未追跡を含める） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: `amonhen run` から。<br>TUI: `s`。 |
| Stash: stage した変更だけ・index を残す・選んだファイルだけ | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: アプリのシートと同じく、Stash はすべての変更か stage した変更だけかを聞き、続けて stage した変更を残す・untracked を含めるを組み合わせて選ぶ（前の 2 つはエンジンがあるとき）。Working Copy のファイルの右クリックに、そのファイルか選んだファイルの Stash…（選び方は同じ）。<br>CLI: `amonhen run`（`stashPush` の `staged`・`keepIndex`・`paths`）から。<br>TUI: `s` で `k` index を残す、`s` stage したものだけ、Working Copy では `f` 選んでいるファイルだけ。 |
| ignore されたファイルを消す（先に一覧） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: コマンドパレットと、サイドバーの Working Copy の行の右クリックから。一覧は選ぶもので、はじめはすべて選んである。<br>CLI: `amonhen run`（`ignoredFiles`・`deleteIgnored`）から。<br>TUI: 命令の一覧から。 |
| 最新の stash を Pop（先に尋ねる） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Stashes の見出しとコマンドパレットの Pop Latest Stash…。行の Pop も尋ねる。<br>CLI: `amonhen run` から。<br>TUI: `S`。stash の名前とメッセージを出して尋ねる。サイドバーの stash の `a`・`o` も同じ。 |
| Activity のくるくると数 | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: VSCode の進み具合の知らせ。<br>TUI: 一番上の行に動いている操作の名前。 |
| Refresh と Rebuild History | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Rebuild History はサイドバーの ⋯ とコマンドパレットから。エンジンがあれば。<br>CLI: `log --rebuild`・`daemon rebuild`。<br>TUI: `Ctrl-r` で Refresh、命令の一覧に Rebuild History。`Ctrl-l` は読み直さずに画面を描き直す。 |
| 直前の操作の Undo（尋ね、何を戻すか示す） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: 上の Undo とコマンドパレットは尋ねる。操作の直後に押す知らせの Undo は尋ねない。<br>CLI: `undo`（先に見るなら `--peek`）。<br>TUI: `u`。 |
| 範囲付きの History の検索（件名・作者・ID・パス・中身） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: エンジンがあるとき。欄の前の ▾ に、アプリと同じ 9 つの探し方。<br>CLI: `log <query>`。<br>TUI: History で `/`。 |
| ahead/behind のバッジ | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: サイドバーのブランチの行に出る。<br>CLI: `status` に出る。<br>TUI: サイドバーのブランチと一番上の行に出る。 |
| ツールバーのカスタマイズ | ✓ | ✓ | ✓ | n/a | n/a | n/a | GTK4: パレットは NSToolbar と同じ使い方で、シートと同じく見出しの下に中身に重ねて出し、窓の大きさは変えない。見出しはアイコンだけなので、表示（アイコンと文字）は選ばない。Repository のメニューと主メニューはメニューバーの代わりなので動かない。<br>Windows: パレットは NSToolbar のシートと同じく中身の上に出す。窓の見出しはドラッグを受けないので、ツールバーはシートの一番上の並びで編集する（見出しのツールバーは薄くなり、変えるたびに映る）。項目・スペース・フレキシブルスペース・既定のセットをその並びに引いて足し、並びの中で引いて並べ替え、パレットへ引くかほかの所に落として外す。表示でアイコンとテキスト・アイコンのみ・テキストのみを選ぶ。<br>TUI: ツールバーはない。操作はキー。 |
| 120 秒黙った network の git を止める | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: エンジンのものと、エンジンを使わないときの拡張自身の fetch・push。<br>Windows: エンジンのもの。<br>TUI: エンジンのもの。 |

## 5a. Stashes

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Stashes の表示（一覧と中身） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: サイドバーの一覧で、Show Stash は VSCode の複数ファイルの差分。<br>CLI: `refs` が並べ、`show stash@{n}`。<br>TUI: サイドバーに並べ、Enter で stash の差分を詳細に出す。 |
| stash の行の詳細（ブランチ・HEAD に対する元・未追跡） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: アプリの行と同じく、行の説明にブランチ・HEAD に対する元・いつ・未追跡・元の短い ID（元と未追跡はエンジンがあるとき）。行の右クリックに Copy Name。<br>CLI: `amonhen run`（`stashBases`）から。<br>TUI: Enter で stash を出したとき、一番下の行に。 |
| Apply・Pop・Drop…（Undo できる）・Branch… | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Branch… はエンジンがあるとき。Apply・Pop はできれば index も戻す（`--index`）。<br>CLI: `amonhen run` から。<br>TUI: サイドバーで `a`・`o`・`x`・`b`。 |

## 5b. Reflog

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| 「どのブランチにもない」の付いた Reflog の表示 | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: エンジンがあるときだけ。<br>CLI: `amonhen run` から。<br>TUI: `3`。コミットが History にない行は薄くする。ブランチで `L` を押すとそのブランチの reflog。 |
| Reflog の操作（New Branch・Checkout・New Tag・Reset・Show in History・Copy） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: `amonhen run` から。<br>TUI: `b` で新しいブランチ、`c` で checkout、`T` で新しいタグ、`R` で reset、`H` で Show in History、`Y` でコピー。 |

## 5c. Reset

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| コミットへ Reset（Soft / Mixed / Hard、Hard は先に stash、Undo できる） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: エンジンがあるときだけ。<br>CLI: `amonhen run` から。<br>TUI: History か Reflog で `R`。 |

## 5d. Range Diff

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Range Diff の表示（= ! < >・Changes Only・差分の差分） | ✓ | ✓ | ✓ | ✓ | ✓ | △ | CLI: `range-diff`。<br>TUI: 独立したページではなく、Blame・File History と同じ画面に出す。Changes Only は `c`。 |
| Compare with Previous Version | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `range-diff --previous`。<br>TUI: 命令の一覧から。 |
| History の 2 つのコミット・Reflog の行を 2 つの版として比べる | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: どの範囲でも。<br>TUI: History で 2 つ選んで `=`、Reflog の行で `=`（今の HEAD と）。 |
| 範囲を直して比べ直す | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | TUI: range diff で `r`。git range-diff と同じ書き方で打つ。 |

## 6. Working Copy

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| 1 つのチェックボックスの一覧（☑ ▣ ☐、見出し・フォルダーのチェック） | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Staged と Changes が別のグループ。<br>TUI: ファイルごとに `[x]` `[-]` `[ ]`。`+` ですべて stage（フォルダはない）。 |
| List / Tree / File Name | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: 一覧の `v` で順に切り替える。フォルダの行で中をまとめて stage し、Enter で畳む。 |
| パスで絞り込む | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: 一覧の `/`。 |
| 行ごとの +/− の数と、ホバーで Discard | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: 行数はエンジンがあるときだけ。<br>CLI: `diff --stat`。<br>Windows: ↶ はマウスを最後に載せた行に残る。<br>TUI: 数は各行の右の端に。ホバーの ↶ か `d` で捨てる。 |
| Space で切り替え、⌫ で捨てる | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: 選んだファイルすべてに効く（⌘・Ctrl クリック、Shift クリック、Shift+↑↓ でいくつも選べ、行のボタンと Stash… も選んだものに効く）。<br>TUI: Space と `d`。 |
| 上のコミットの欄、50/72 の数と本文の目安の線 | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: 列が 72 文字より狭いので、目安の線の代わりに、72 を超える本文の行の長さを出す。<br>TUI: 見出しに件名の行の文字数（50 を超えると黄色）。件名の 50 文字・本文の行の 72 文字を超えた分は黄色。 |
| リポジトリごとの下書き、テンプレートから始める | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: `commit.template` だけで、共有のテンプレートは使わない。<br>TUI: 下書きは `<git dir>/amonhen/commit-draft` に置く。テンプレートは `commit.template`（リポジトリ専用のものは SSH 先でも）で、共有のテンプレートは使わない。 |
| 最近のメッセージ（⌥↑ / ⌥↓・一覧） | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: メッセージの欄の Alt-↑ / Alt-↓。一覧は Working Copy の `R`。 |
| Conventional の種類・Co-authored-by・Signed-off-by を入れる | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: Working Copy の `I`。 |
| `#issue` の補完 | ✓ | ✓ | ✓ | — | n/a | — |  |
| Commit and Push・Amend | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: `commit -m … --amend`。<br>TUI: `Ctrl-p` でコミットしてから push を尋ねる。`a` で amend。 |
| Fixup / Squash Commit For | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: Working Copy の `F`。Tab で squash! に切り替える。 |
| hooks の出力のシート | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: 1 行目を一番下の行に出し、`!` で全部を枠に出す。 |
| Commit が押せない理由を出す | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | ボタンのツールチップに。<br>TUI: 一番下の行に。 |
| HEAD → 作業ツリーの 1 つの差分と、行・hunk のチェック | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: エンジンがあれば。なければ別々の未ステージ・ステージ済みの差分で行をステージ。<br>CLI: `diff --combined`・`stage --rows`。 |
| 行の範囲の選択、J/K・⌥J/⌥K | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: `v` で範囲を始め、`[` `]` で hunk の見出しへ。 |
| 重ねられないファイルの Unstaged / Staged の切り替え | ✓ | ✓ | ✓ | n/a | ✓ | ✓ | VSCode: いつも別々に出す。<br>CLI: `diff --staged`。<br>TUI: 初めは stage していない側。差分の `s` で替える。 |
| 差分の表示の選択（unified/split・前後の行・折り返し） | △ | △ | △ | △ | n/a | △ | macOS・GTK4: 長い行の折り返しだけ（「長い行を折り返す」、すべての差分・ファイルの中身・マージエディタで）。左右に並べる表示はない。<br>Windows: 同じ。<br>VSCode: 長い行の折り返しだけ（`amonhen.wrapLongLines`、Alt+Z か差分の上の ↩︎。マージエディタは VSCode のもの）。<br>TUI: 長い行の折り返しだけ（Alt-z かコマンドの一覧の Wrap Long Lines。その回だけ）。左右に並べる表示はない。 |
| 空白を無視する（その間は行ごとの stage・破棄はしない） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: エンジンがあれば。その間は選んだ側（Changes か Staged）の差分。<br>CLI: `diff -w`・`-b`・`--ignore-space-at-eol`（`--combined` とは使えない）。<br>TUI: 差分の `w` で順に替える。その間は片方の側だけ（`s` で替える）。 |
| 行・hunk・ファイルを捨てる（Undo できる） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `discard --rows`。<br>Windows: Working Copy の差分で行を押して選び（Shift で範囲、Ctrl で足す・外す、hunk の見出しでその hunk）、右クリックの Discard Lines。選んでいない行・hunk の見出しを右クリックすれば、そこだけ。 |
| Add to .gitignore | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: エンジンがあれば。<br>TUI: untracked のファイルで `i`、続けて `p` でそのパス、`e` で拡張子。 |
| コンフリクトのファイルからマージエディターを開く | ✓ | ✓ | ✓ | ✓ | n/a | n/a | macOS, GTK4: UTF-8 でないファイルは開かない。片側を選ぶ。<br>VSCode: VSCode のマージエディターを開く。<br>TUI: ファイルの差分にマーカーを出し、`o` `t` `b` `a` で選んでいる行のコンフリクトを解く。 |
| 一部のフォルダだけを取り出しているとき、一覧の上に知らせる | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: `status` が sparse checkout を添える。 |

## History

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| ブランチ・タグのラベル付きのグラフ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: 札は 3 つまで、残りは +N、長い名前は真ん中を切り、押すとすべての名前を出す。一覧の横にコミットの頻度とブランチの動きの帯（エンジンがあれば。アプリと同じ）。<br>CLI: `log`。<br>TUI: 1 コミット 1 行。 |
| 大きな履歴をかたまりで読み、永続キャッシュを使う | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| 一致の数・次 / 前・一致だけの検索 | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: エンジンがあるとき。<br>CLI: `log <query>` が一致を並べる。<br>TUI: `/`・`n` `N`、一致だけは `&`。 |
| 表示: 順・1 行・行の項目・マージ済みを畳む・祖先を目立たせる | ✓ | ✓ | ✓ | ✓ | △ | △ | macOS・GTK4: merge を 1 つずつたたむ（+N の札・右クリック・←/→）。リポジトリごとに覚える。<br>VSCode: 順・畳む・目立たせるはエンジンで。merge を 1 つずつたたむ（右クリック・←/→）。リポジトリごとに覚え、アプリと共有。<br>CLI: `--topo-order`・`--fold`。<br>Windows: View に。<br>TUI: `o` で順（その回だけ）、`z` ですべて畳み `Z` で 1 つずつ（リポジトリごとに覚え、アプリと共有）。`I` で行の項目（その回だけ）。いつも 1 行。 |
| アバター | ✓ | ✓ | ✓ | — | n/a | — |  |
| コミットの署名の印 | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: エンジンがあるとき。<br>CLI: `show` に出る。<br>TUI: 件名の前の印。吹き出しはない。 |
| コミットごとの CI の状態とポップオーバー | ✓ | ✓ | ✓ | — | — | — |  |
| worktree の HEAD のラベル（✎ ⌂ ⑂・変更の数） | ✓ | ✓ | ✓ | ✓ | — | ✓ | TUI: worktree が 2 つ以上あるとき。 |
| Go to HEAD・Parent・Child | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: History の上の HEAD のボタン、コミットのメニューの Go to Parent・Go to Child、詳細の親と子へのリンク（エンジンがあるとき）。<br>TUI: `@`・`]`・`[`。 |
| Go to Ref | — | — | — | — | n/a | — |  |
| いくつものコミットを選ぶ（範囲・2 つ・飛び飛び） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: エンジンがあるとき。<br>CLI: 2 つは `diff <rev> <rev>`。<br>TUI: `m`・Shift-↑↓・Ctrl か Alt のクリック。 |
| 選んだいくつもの ref の History | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `log --branch` を重ねる。<br>Windows: サイドバーで ref を選ぶとその History、Ctrl+クリックで ref を足す・外す。View に All Branches と Current Branch。<br>TUI: サイドバーの `v` で ref に印をつけると、History はそれらから辿れるものだけ。Esc で印を外す。ref を開くとそのコミットを選ぶ。 |
| コミットのメニュー: checkout・新しいブランチやタグ・cherry-pick・revert・ID のコピー | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: コミットに付いた ref の Copy Branch or Tag Name…。Cherry-pick はどのコミットにも、Split は古い編集できるコミットがあるときだけ。Go to Parent / Child は ⌘⌥↓・⌘⌥↑（ほかは Ctrl+Alt）でも。New Tag… はメッセージも尋ね、注釈付きのタグを作る（空なら軽量のタグ。アプリが作るのはこちらだけ）。<br>CLI: `amonhen run` から。<br>TUI: `c`・`b`・`T`・`y`・`v`・`Y`（OSC 52 で端末のクリップボードへ）。 |
| 編集モード（pick・reword・squash・fixup・drop・edit、ドラッグで並べ替え） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: エンジンがあるとき。ないときは 1 つずつ。p・r・s・f・d・e で行のすることを決め、行の下の線を引いて古いコミットを取り込む・手を付けていないものを外す。編集モードは History の代わりに出す一覧。<br>CLI: `amonhen run` から。<br>TUI: `E` で画面に重ねる。`J` `K` で行を動かす。 |
| 書き換えの見込み（積み重ね・push 済みのブランチ、コンフリクトの見込み） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: daemon 越し。ほかのブランチの rebase では、アプリと同じく代わりにコンフリクトの見込みを出す。<br>CLI: `rewrite-preview`。 |
| 近道: Edit Message…・Squash into Previous…・Drop…・Combine N…・コミットのドラッグ | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: エンジンがあれば編集モードを開く。ないときはすぐ書き換え、Combine N Commits はない。コミットを行の間に落とすと動かし、行に重ねて落とすとそのコミットにまとめる（エンジンがあれば。History と編集モードで）。Webview は外への落としを知れないので、外へ出して drop はない。<br>CLI: `amonhen run` から。<br>TUI: `e` でメッセージ、`F` で親にまとめる、`X` で Drop、選んだコミットの `E` で Combine N。merge と rebase はサイドバーの `m` `r`。コミットを別のコミットへドラッグすると、動かすかまとめるかを聞く。 |
| コミットを分ける | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: `amonhen run` から。<br>TUI: History の `\|`（先に確かめる）。Working Copy で止まる。 |
| 編集でマージを保つ（`--rebase-merges`） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: エンジンがあるとき。ないときは最初のマージで止まる。<br>CLI: `amonhen run` から。 |
| shallow clone: 終わりの帯で取り足す、境界のコミットの印 | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: エンジンがあるとき。<br>CLI: `log` が履歴の止まる所を添える。取り足すのは `amonhen run` から。<br>TUI: 最後の行の下に 1 行、境界のコミットの件名の前に ┄。History の `+` で取り足す（数・日付まで・すべて）。 |

## 7. History の右の列

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| メッセージ全体・ドラッグできる境・⤢ で広げる | ✓ | ✓ | ✓ | ✓ | n/a | △ | VSCode: メッセージの URL とコミット ID はリンク。トレーラーは分けて並べ、git notes は後に（エンジンがあれば）、History の検索の語に印。見出しの右クリックで ID・短い ID・件名・メッセージ・git show のコマンドをコピーし、編集できるコミットなら Edit Message…。作者とコミッターの名前を押すと History をその人で探す。帯の ⚙︎（表示の設定）に、表示項目（簡潔・標準・詳細。詳細は子・含むブランチとタグ・push 済みか・取り込んだコミット・関係するコミットも）、高さを自動で決める、コミット情報だけを表示（コミットを替えても保つ）、長い行を折り返す。<br>TUI: 境はマウスでドラッグでき、幅はリポジトリの `amonhen/tui.json` に覚える。⤢ はなく、狭い端末では詳細は History の代わりに開く。 |
| 詳細の中を探す | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: ⌘F でメインビュー全体を探す。まだ読んでいない差分は探さない。<br>TUI: 詳細で `/`、`n` `N` で次と前。 |
| 変更ファイルを差分のファイルの見出しで兼ねる（最初は畳む・貼り付く・操作は帯 1 本） | ✓ | ✓ | ✓ | ✓ | ✓ | △ | CLI: `show`。<br>VSCode: 見出しの ⋯ で右クリックのメニューを開く。Open は VSCode の差分エディタで開く。<br>TUI: 見ているファイルの見出しが一番上に貼り付き、`o` か見出しのクリックでファイルを畳む。`[` `]` でファイルを移る。生成物と大きなファイルは最初から畳む。変更ファイルは差分の上の一覧にもある。 |
| 変更ファイルをリストか、フォルダを畳めるツリーで | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: 差分の上に。`t` で木に替え、クリックでフォルダを畳む（`z` ですべて）かファイルの差分へ。 |
| hunk の前後を広げる・ファイル全体を出す | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: エンジンがあれば。hunk の見出しの ⇕ でそのファイルを 20 行広げ、ファイル全体は ⋯ の Show Whole File。<br>TUI: `+` で今見ているファイルの前後を 20 行広げ、`F` で全体を出す。 |
| 空白を無視する（行末・量の違い・すべて） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: エンジンがあれば。<br>CLI: `show -w`・`-b`・`--ignore-space-at-eol`。<br>TUI: 詳細の `w` で順に替える。 |
| 画像を比べる（並べる・スワイプ・オニオンスキン・違うピクセル） | ✓ | ✓ | ✓ | ✓ | n/a | — | VSCode: 20 MB を超える画像は比べない。拡大縮小（−・倍率・+・合わせる・⌘ スクロール）と違うピクセルの割合はアプリと同じ。SVG は絵で出し、Show as Text で文字に。 |
| 透けた絵の下の市松模様を明るく・暗く（既定は絵の色から） | ✓ | ✓ | ✓ | ✓ | n/a | — | |
| すべてのファイル: その時点の木と、コミットの行に印を付けたファイルの中身 | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: エンジンがあれば。リストか木（帯の List・Tree）、変えたフォルダは最初から開く、消したファイルは前の版を出す、パスの後に @ とコミット、Show This Change でその差分へ、Markdown の相対リンクでそのファイルを開く。<br>TUI: History の `a`。画面に重ねる。プレビューはない（中身は文字で出す）。 |
| すべてのファイルのプレビュー: 拡大縮小できる画像・SVG の絵とモバイル向けの確認・整形した Markdown・バイナリの Quick Look | ✓ | ✓ | ✓ | △ | n/a | — | VSCode: どのファイルも Quick Look の代わりに VSCode のエディタで開ける。モバイル向けの確認は部品を切り替え（覚える）、SVG・Markdown・VectorDrawable の見方も覚える。Mermaid・DOT の図と数式は、エンジンと一緒に同梱した絵の手伝い（`amonhen-render`。エンジンそのものは描かない）が描く。図はテーマの構文の色で描く（図の中で指定したテーマが先。図の配色の設定はない）。gantt の日付はシステムの時間帯で数える。<br>GTK4: Quick Look の代わりに GNOME のプレビュー（か既定のアプリ）。SVG の絵は librsvg で描く。<br>Windows: 拡大縮小できる画像・SVG の絵とモバイル向けの確認・整形した Markdown とその中の Mermaid・DOT の図（Direct2D の SVG は文字を描かないので resvg で描く）。部品の切り替えはパスの帯にある。数式はほかのアプリと同じく描く（文中の式は基線にそろえる）。バイナリはその時点の中身を既定のアプリで開く（Windows には Quick Look がない）。コミットの中の画像・`<picture>`・`align`・画像の大きさ（幅の割合は、窓の幅に合わせて変わる本文の欄の幅に対して決める）・`<details>` を描き、DOT の画像ノードも絵を出す。文の途中の画像は行の中に置き、`<kbd>` はキーの札、`<sub>`・`<sup>` は下付き・上付き。 |
| 整形した Markdown の中を探す（キーの札・表のマスも） | ✓ | ✓ | ✓ | ✓ | n/a | — | 絵と閉じた `<details>` の中は探さない。<br>VSCode: Markdown を押して入ってから ⌘F（Ctrl+F）。ビューのほかの所の ⌘F は VSCode の検索。 |
| すべてのファイル: Android の VectorDrawable の絵（色はコミットの `res` から、animated-vector は動く前の絵） | ✓ | ✓ | ✓ | ✓ | n/a | — | |
| ファイルの見出しのメニュー（Blame・File History・パスのコピー・開く） | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: 先頭に Expand・Collapse、Open File at This Commit、Show Whole File の後は Show Changes Only。いくつも選んだときの見出しにも File History・Copy Path。 右クリックと ⋯ の Show Whole File・Blame・File History・この版／前の版に戻す・Copy Path（Copy Path 以外はエンジンがあれば）と、VSCode の差分のエディターで開く。この時点のファイルを開くはない。<br>TUI: 見ているファイルの `B`・`L`・`Y`（パスのコピー）・`e`（git のエディタで開く）。 |
| 選んだものの差分（範囲をまとめる・A..B / A...B・飛び飛び） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: エンジンがあるとき。<br>CLI: `diff <rev> <rev>`。<br>TUI: `m` と Shift-↑↓（か Ctrl・Alt を押しながらクリック）で選ぶ。`x` で向きを入れ替え、`.` で merge-base から。 |
| 選んだものの操作（cherry-pick・revert・squash・ID のコピー） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Copy Short IDs も。merge を選んでいれば Save Patches は出さず、Merge は HEAD 以外が 2 つ以上のとき。 エンジンがあるとき。<br>CLI: `amonhen run` から。<br>TUI: 印をつけたコミットに `y` `v`、`E` でまとめた形の編集モード、`Y` で ID のコピー、Save as Patch で保存。 |
| 署名のポップオーバー | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: 詳細の Signature の行（様子・説明・署名した人・形式・鍵。指紋はツールチップ）と、タガー・件名・署名を添えた Tag・Merged Tag の行（エンジンがあれば）。ポップオーバーはない。<br>TUI: 確かめた結果を詳細に出す。ポップオーバーはない。 |
| 単語の強調と構文の色 | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: 構文の色はエンジンがあれば、差分を出した後に付く。 |

## Blame・File History

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Blame | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: メインビューのページ。<br>CLI: `amonhen run`（`blame`）から。<br>Windows: 行のメニューからそのコミットを History に出す。<br>TUI: Working Copy のファイルかコミットの詳細で `B`。`H` で行のコミットを History で選ぶ。 |
| ファイルをコミットの版に戻す（この版・その前の版。選べば stage も） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: エンジンがあるとき、詳細（History・Reflog・File History）のファイルの右クリックと、File History の今の名前の行の Restore This Version… から。Restore and Stage は 2 つめのボタン。<br>CLI: `restore --source <rev> [--staged]`。<br>TUI: File History の `V`。`y` この版、`b` その前の版、`s`・`B` で stage も。 |
| File History | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: メインビューのページ。エクスプローラー・エディターのタブ・コマンドパレット・メインビューのファイルから開く。行の右クリックにアプリと同じ項目（Copy Short ID も）。Merge Commits はメニューではなく帯のチェックボックス。<br>CLI: `amonhen run`（`fileHistory`）から。<br>Windows: Working Copy のファイルから。<br>TUI: 同じく `L`。変更のないファイルは `O` でパスを聞く。 |
| リポジトリのタブの隣のタブに、リポジトリのツールバー付きで開く | ✓ | ✓ | ✓ | n/a | n/a | n/a | GTK4: リポジトリのタブがほかの窓で前に出ているときは、ツールバーのない題だけの帯になる。<br>TUI: メインの画面の代わりに開き、Esc で戻る。 |
| 行の履歴 | — | — | — | — | — | — |  |

## 7a. パッチ

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Save as Patch（コミットごとか mbox・カバーレター・-v） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: 形とカバーレターを組み合わせられ、前に選んだものが先頭。<br>CLI: `format-patch`。<br>TUI: 命令の一覧から、選んでいるコミットか、印をつけたコミットを古い順に。`.mbox` で終わる名前なら 1 つのファイル、`c` でカバーレター、`v` で版を聞く。 |
| Apply Patch（am / apply・下見・3-way・--index） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `am`・`apply`（`--check`）。<br>TUI: 命令の一覧から。下見は確かめる文の件名かファイル、当たらなければ 3-way を勧め、`i` で index にも。 |
| パッチのファイルを窓に落とす | ✓ | ✓ | ✓ | ✓ | n/a | n/a | VSCode: エクスプローラーのメニューから。<br>TUI: Apply Patch がファイルのパスを聞く。 |
| Export Archive（zip・tar.gz・tar、先頭のフォルダ、1 つのフォルダ） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: エンジンがあれば。ブランチ・タグの右クリック、History のコミットの右クリック、HEAD はコマンドパレットから。<br>CLI: `archive`。<br>TUI: 命令の一覧から、選んでいるコミット。形は拡張子で決まり、`y` で先頭のフォルダ、`t` でなし、`o` で 1 つのフォルダ（パスを書く）。 |
| Save as Bundle（すべて・一部の ref・基点より後） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: エンジンがあれば。選んだブランチ・タグの右クリック、リポジトリ全体はサイドバーの ⋯ から。<br>CLI: `bundle create`。<br>TUI: 命令の一覧から。すべて、`b` で今のブランチだけ、`a` で今のブランチの基点より後。 |
| bundle のファイルから clone（HEAD がなければブランチを選ぶ） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: サイドバーの ⋯ と最初の画面から。clone したものを開く・新しい窓で開く・ワークスペースに足す。<br>CLI: `clone <file> [-b <branch>]`。<br>TUI: Quick Open に `.bundle` のパスを打つ。ブランチは `1`〜`9` で選ぶ。 |
| Fetch from Bundle（名前の下に、前提のコミットを確かめて） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: エンジンがあれば。サイドバーの ⋯ から。<br>CLI: `bundle fetch`。<br>TUI: 命令の一覧から。 |
| 止まった `am` と Retry with 3-Way・Skip・Abort・Continue | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: サイドバーの行に出る。<br>CLI: `am --continue / --skip / --abort / --retry-3way`。<br>Windows: Windows: Working Copy の上の帯に。<br>TUI: `C` `K` `A`、Retry with 3-Way は `R`。 |

## 8. マージエディター

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| 帯でつないだ 3 つの列（Current / Result / Incoming） | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: VSCode 自身の 3-way のマージエディター。<br>Windows: 3 つの列。左右の欄はコンフリクトごとに色を付け（Current は青、Incoming は緑、Base は灰色、選んでいるものは濃く、解いたものは薄く）、≫・≪ でその側を取る。選んだコンフリクトへ 3 つの列ともスクロールする。<br>TUI: マーカーつきのファイルの差分で、`o` `t` `b` `a` でコンフリクトごとに解く。 |
| Show Base | ✓ | ✓ | ✓ | n/a | n/a | n/a | TUI: `merge.conflictStyle` が `diff3` か `zdiff3` なら、マーカーの中に共通の祖先が出る。 |
| Magic Merge（ファイルを開いたときの確かな規則も） | ✓ | ✓ | ✓ | n/a | ✓ | △ | VSCode: コンフリクトは VSCode 標準の 3-way マージエディターで開く。<br>CLI: `merge-file`（git の mergetool にも）。<br>TUI: コンフリクトのファイルで `m`。開いたときには当てない（決めたこと: TUI には後で保存するマージエディタがなく、開いただけでファイルを書き換えることになるため）。 |
| 直せる Result とコンフリクトごとの Undo | ✓ | ✓ | ✓ | n/a | n/a | n/a | TUI: `e` でファイルを git のエディタで開く。 |
| ファイルを Current / Incoming で解く | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: `amonhen run` から。<br>TUI: 一覧で `o` `t`。 |
| 外のマージツールで開く | — | — | — | n/a | n/a | — |  |
| エディターを別の窓に出す | ✓ | ✓ | ✓ | n/a | n/a | n/a | Windows: 新しい窓で同じファイルを開く。保存していない編集も持って行く。 |
| Continue / Skip / Abort | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: サイドバーの進行中の行に出る。Skip と Abort は、アプリと同じく先に尋ねない。<br>CLI: `amonhen run` から。<br>TUI: `C` `K` `A`。 |

## 9. AI

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Claude Code と Codex | ✓ | ✓ | — | — | — | — |  |
| Apple Intelligence（端末の中） | ✓ | n/a | — | n/a | n/a | — |  |
| コミットのメッセージを翻訳 | ✓ | ✓ | — | — | — | — |  |
| コミット・選んだものを解説 | ✓ | ✓ | — | — | — | — |  |
| コミットのメッセージを提案 | ✓ | ✓ | — | — | — | — |  |
| 差分の hunk を解説 | ✓ | ✓ | — | — | — | — |  |
| AI でコンフリクトを解く | ✓ | ✓ | — | — | — | — |  |
| すべてのファイルを AI で解く | — | — | — | — | — | — | docs/ux.md（9 章）に設計はあるが、まだない。マージエディタはコンフリクトを 1 つずつ解く。 |
| コンフリクトを解説 | ✓ | ✓ | — | — | — | — |  |
| リポジトリごと・リポジトリのプロンプトの初回の確認 | ✓ | ✓ | — | — | — | — |  |
| 流れてくる結果とキャッシュ | ✓ | ✓ | — | — | — | — |  |

## 10. Activity

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Operations（git の命令・出力・進み具合・取り消し） | ✓ | ✓ | ✓ | △ | △ | ✓ | VSCode: Output のチャンネルの記録だけ。<br>CLI: `amonhen run`（`remoteActivity`・`cancelActivity`）から。<br>TUI: Activity の操作ログで `Tab`。git を動かす側（daemon 越しならその側）の git の命令とコアの命令、下に選んだものの出力か失敗の理由。`x` で動いているものを取り消し、`r` で読み直す。 |
| History（操作ログ・ここまで戻す） | ✓ | ✓ | ✓ | △ | △ | ✓ | VSCode: Undo History… が Undo の記録を並べ、選んだところまで戻す。ほかの Activity はない。<br>CLI: `undo --list` が記録を出す。<br>TUI: 命令の一覧の Activity。Enter で、選んだところまで戻す。 |
| Performance | ✓ | ✓ | ✓ | n/a | n/a | ✓ | VSCode: VSCode の出力（Amon Hen のログ）が代わり。<br>Windows: 差分の読み込みと、History・status の読み直し。<br>TUI: History・status の読み直しと差分の読み込み（回数・p50・p95・最大・最後）。Activity の History Cache の次。 |
| History のキャッシュの書き込み（リポジトリごとの大きさ・回数・量・最後の書き込み） | ✓ | ✓ | ✓ | — | △ | ✓ | CLI: `amonhen run`（`historyCache`）。<br>TUI: 開いているリポジトリのもの。Activity の Operations の次（`Tab`）。 |
| Events | ✓ | ✓ | ✓ | n/a | n/a | ✓ | VSCode: VSCode の出力（Amon Hen のログ）が代わり。<br>TUI: Activity の History Cache の次（`Tab`）。変化の知らせと、それで読み直したもの。 |
| JSON で書き出す | ✓ | ✓ | ✓ | n/a | n/a | ✓ | VSCode: VSCode の出力（Amon Hen のログ）が代わり。<br>TUI: Activity の Operations で `s`。この機械のファイルに。 |
| Daemon のタブ（起動・停止・つながり） | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: メインビューのページ（サイドバーの ⋯ とコマンドパレットの Show Daemon）。窓のつながりは手元の daemon だけ。<br>CLI: `daemon status / start / stop`。<br>TUI: Activity の History Cache の次（`Tab`）。`S` で起動、`X` で止める。 |

## 11. メニュー

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| 共有の操作の表（`crates/commands`）から作るメニュー | ✓ | ✓ | ✓ | n/a | n/a | ✓ | VSCode: VSCode のコマンドパレットと右クリックのメニューが代わり。<br>TUI: 命令の一覧（`:`）の名前をここから取る。 |
| メニューバーなしで全部の操作に届く（≡） | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | GTK4: ≡ のメニューに、メニューバーの File・Edit・View・Navigate・Commit・Window を macOS と同じ項目と並びで置き（Edit の取り消す・カット・コピーなどはフォーカスのある欄に効く）、その後に設定…・Amon Hen について・終了。<br>Windows: タイトルバーの右の ≡ に、メニューバーのメニューをすべて同じ項目と並びで入れ子で置く。設定…（Ctrl+,）は Windows のアプリの習いどおり File の終わりの方に置く。窓にメニューバーの段はない。<br>VSCode: コマンドパレットとコンテキストメニュー。メニューの Commit・Commit and Push・Stage All・Rename Branch…・Set Upstream…・Rebase…・Go to HEAD・Find in History… もコマンドにある。Merge Branches… と Rebase… では History で選んでいるコミットも選べる。<br>TUI: `:` が TUI にあるすべての操作をキーと一緒に並べる。 |
| キーボードの近道 | ✓ | ✓ | ✓ | n/a | n/a | ✓ | GTK4: Linux のデスクトップやほかの操作が使うキーは違う: サイドバーを表示は F9（GNOME のユーティリティの欄のキー）。ブランチを絞り込む・親へ・子へ・New Tag にはない（GNOME が Ctrl+Alt+矢印を、Ubuntu が Ctrl+Alt+T を使い、HIG では Alt+Up が別の意味）。マージエディターの前・次のコンフリクトは Alt+Up・Down。<br>VSCode: VSCode のキー割り当てが代わり。拡張が持つのは Alt+Z（長い行を折り返す）だけで、拡張のコマンドならどれにも割り当てられる。<br>TUI: 独自の 1 文字のキー（表の近道は端末では打てない）。 |
| メニューと同じ名前のコンテキストメニュー | ✓ | ✓ | ✓ | ✓ | n/a | n/a | VSCode: VSCode のメニューの項目は決まった文なので、アプリが入れる名前を除いた同じ言葉（Checkout main… は Checkout…、Delete main… は Delete…）。<br>Windows: History とその変更ファイル・サイドバー・Working Copy・Stashes・File History・Blame。<br>TUI: 操作は選んでいる行へのキー。 |
| キーボードの近道の窓 | — | ✓ | — | n/a | n/a | ✓ | macOS: 代わりに macOS の App Shortcuts で変える。<br>TUI: `?` でフォーカスのある所のキーを並べる。 |

## ステータスバー

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| 進行中の操作と Continue / Skip / Abort | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: サイドバーの行。エンジンがあれば、アプリの帯と同じく何をどこへしている途中か（何手目か）と、なぜ止まったか（コミット・コンフリクト・exec・編集で止まった）と次にすることを言う。revert には Skip を出さない（アプリと同じ）。<br>TUI: 一番下の行に出し、`C` `K` `A`。 |
| 失敗とその原因・Details | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: 見分けられる失敗（認証・ホスト鍵・つながらない・断られた・分かれた・作業中の変更・ロック・上流なし・worktree）は知らせを分かる言葉にし、アプリと同じ次の一手（Pull…・Open Working Copy・Set Upstream…・Open Worktree・Open in Terminal・Try Again）を添える。そうでなければ git の fatal:・error: の行。Details で助言と全文と git の命令を出力パネルに出す。<br>TUI: 原因は一番下の行に。`!` で git のコマンドと答えの全部を出す。 |
| 切れたつながりと Reconnect | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: 一番下の行に出し、`R` で同じ場所を開き直す。 |
| 終わった操作が何をしたか、数秒 | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: ステータスバーの文か、Undo 付きの知らせ。<br>TUI: 一番下の行に。 |

## 確認と Undo

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| リモート・HEAD を動かす・消す操作の前のシート | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | GTK4: 名前を入れるものもすべての確認も同じシート。macOS と同じく、入力がどこにあっても（ドロップダウンやラジオでも）Return で既定のボタンを押す。<br>VSCode: modal と、Fetch・Pull・Push は QuickPick。Switch Branch… はアプリの切り替えと同じくすぐ切り替える。<br>TUI: 一番下の行で尋ねる。前と後の絵は画面に重ねる。 |
| git の操作の Undo（`undo.json`） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| 押せない部品はツールチップで理由を言う | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: 使えないコマンドは出さない（when 句）。サイドバーの見出しの Pull・Push はツールチップ付きで押せなくできないので押せるまま（上流のないブランチでは Push が Publish になる）、することがなければ押したときにメッセージでわけを言う。<br>Windows: WinUI は押せない部品にツールチップを出さないので、押せないボタンや欄は包む部品に付ける。包めないツールバーの Push は押せるまま薄く描く（押すとベルが鳴る）。Commit はわけを言う。<br>TUI: 動けないキーは一番下の行で理由を言う。 |

## 12. GitHub

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| `gh` を通す GitHub | ✓ | ✓ | ✓ | — | — | — |  |
| Pull Requests（CI・draft・レビュー、checkout・開く・URL のコピー） | ✓ | ✓ | ✓ | — | — | — |  |
| Create Pull Request（base・draft） | ✓ | ✓ | ✓ | — | — | — | macOS: base は打ち込む（最初は既定のブランチ）。fork からは元のリポジトリへ。<br>GTK4: base は打ち込む（最初は既定のブランチ）。fork からは元のリポジトリへ。<br>Windows: base は打ち込む（最初は既定のブランチ）。fork からは元のリポジトリへ。 |
| ブランチの状態のポップオーバー（CI・プルリクエスト） | ✓ | ✓ | ✓ | — | — | — | Windows: 行の右端の印（取り込み済み・gone・上流との差）と、右クリックのメニューの先頭から開く。キーボードで開いたメニューからはダイアログ。 |
| Open on GitHub | ✓ | ✓ | ✓ | — | — | — |  |
| メッセージの `#123` のリンク | ✓ | ✓ | ✓ | — | — | — |  |
| GitHub のアバター | ✓ | ✓ | ✓ | — | — | — |  |

## 13. 言語

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| 英語の UI と日本語の訳 | ✓ | ✓ | ✓ | — | ✓ | ✓ | VSCode: 拡張そのものの文は英語だけで、訳していない。エンジンのメッセージは VSCode の表示言語に従う。 |

## 14. Worktree

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| worktree と変更の数 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `worktree list --changes`。<br>TUI: 2 つ以上あればサイドバーに。 |
| 書き込む先の worktree（✎）と切り替え | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: 拡張が働くリポジトリを切り替える。ワークスペースの外のものはソース管理のビューには足さない。<br>CLI: 動かした worktree で働く。<br>TUI: ほかの worktree で Enter を押すと、TUI をそこで開き直す。 |
| New Worktree…（ブランチ・コミットから） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | TUI: 手元のブランチで `w`。History の `w` で、選んでいるコミットに新しいブランチを作って。 |
| Remove Worktree（尋ね、変更があれば `--force`） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | TUI: `x` で尋ねてから消す。変更があればその数を書き、`--force` で消す。 |
| ロックと解除・移動・移したフォルダを探す（repair） | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: worktree の右クリックから。書き込む先の worktree は、ほかに切り替えてから移す。<br>CLI: `worktree lock`・`unlock`・`move`・`repair`。<br>TUI: サイドバーの worktree で `L`・`n`。 |
| worktree の Prune | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | TUI: 命令の一覧から。 |
| worktree のフォルダーを開くと、そのリポジトリでそれを書き込む先にする | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: フォルダーを開くとその worktree で働き、ほかの worktree も並ぶ。<br>Windows: フォルダはタブで開き、その worktree が ✎、ほかも並ぶ。 |

## SSH 先・共有の daemon・askpass

| 機能 | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| SSH 先のリポジトリ（ssh 越しのエンジン） | ✓ | ✓ | ✓ | n/a | ✓ | ✓ | VSCode: Remote-SSH が拡張をホストで動かす。<br>CLI: `-C ssh://host/path`。<br>TUI: `amonhen tui ssh://host/path`。 |
| 共有の daemon を通す | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `-C daemon:<path>`。<br>TUI: `amonhen tui daemon:<path>`。 |
| 合うものがなければホストにエンジンを送る | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: 拡張が動くホストで、同梱したエンジンを使う。<br>Windows: 他のアプリと同じく、先に聞く。 |
| Windows の SSH 先 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| パスワード・パスフレーズを尋ねる askpass のダイアログ | ✓ | ✓ | ✓ | ✓ | n/a | △ | VSCode: VSCode の入力欄。拡張が git・ssh・共有の daemon に渡すスクリプトから尋ねる。<br>CLI: ダイアログはアプリのもの。<br>TUI: 一番下の行で。Unix だけ。Windows では git の credential helper・ssh-agent・pinentry に任せ、止まったらそう言う。 |
| 署名の確かめ（この機械か、ホストの鍵） | ✓ | ✓ | ✓ | n/a | △ | ✓ | VSCode: 拡張は `ssh://` のリポジトリを開かない。Remote-SSH では拡張がホストで動くので、開くほかのリポジトリと同じく、git がホストの鍵で確かめる。<br>CLI: `show`・`refs --verify` で確かめ、選択はない。<br>TUI: 詳細の `S` で、リポジトリごとに（`amonhen/tui.json` に覚える）。署名の行に、どこで確かめたかを添える。 |

## 端末の FE（TUI）

`amonhen tui`（`crates/tui`）の列は上の表にある。詳しいことと残りは [tui.md](tui.md)。端末でアプリと同じことができないところは、TUI の形にしている: 操作はキーで、メニューと右クリックのメニューの代わりに、命令の一覧（`:`）がメニューと同じ名前で並べる。確認・名前・選択は、シートの代わりに一番下の行で聞き、前と後の絵は画面に重ねる。タブと窓の代わりに、ほかの worktree で Enter、Quick Open で、TUI を閉じてそこで開き直す。
