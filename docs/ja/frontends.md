# FE ごとの機能

[English](../frontends.md)

利用者に見える機能が、どの FE にあるか: macOS のアプリ（`apps/macos`）、GTK4 のアプリ（`apps/gtk4`）、VSCode 拡張（`apps/vscode`）、CLI（`crates/cli`）。行は機能を説明する [docs/ux.md](ux.md)（おもに macOS のアプリの形）の章の順で、History・サイドバー・Blame / File History・ステータスバー・確認・SSH を足した。設計ではなくコードで確かめた。

印:

- ✓ できている。その FE の形のこともある（ポップオーバーの代わりにツールチップ、シートの代わりに入力欄）。どう違うかは Notes の列。
- △ 一部だけ。何が足りないかは Notes の列。
- — ない。
- n/a その FE には当てはまらない（CLI にサイドバーはない）か、わざと土台に任せている（VSCode 本体の設定・キー割り当て・出力・マージエディター）。何が代わりかは Notes の列。

CLI の △「`amonhen run` から」は、コアの命令はあるが、CLI からは JSON（`amonhen run '{"op":…}'`）でしか届かず、専用のサブコマンドがないこと。

メニューの操作そのものは 1 つずつ並べない: どの FE も共有の表 `crates/commands` から取り、GTK4 のアプリでまだないものは `apps/gtk4/src/appmenu.rs` の `PENDING` に並べる（今はない）。

この表は手で保つ。どの FE でも、機能を足す・外す・変えるときは、同じ変更でここも直す。

## macOS と GTK4 のアプリのプラットフォームによる違い

GTK4 のアプリは macOS のアプリと同じふるまいをめざす。わざと違えるのは次だけで、どれもプラットフォーム（おもに [GNOME HIG](https://developer.gnome.org/hig/)）に従う:

- 最後の窓を閉じるとアプリが終わる。
- ログインシェルの環境変数を読み込まない（Linux のセッションはもう持っている）。
- Linux と BSD では、Watchman が入っていて git が 2.26 以降なら fsmonitor に使う（git に fsmonitor--daemon がない）。
- Apple Intelligence はない。Claude Code と Codex だけ。
- Finder の代わりにデスクトップのファイルマネージャー。外部エディターはデスクトップ項目から探す。
- ヘッダーバーはアイコンだけ（ツールチップつき）。ツールバーのカスタマイズにアイコン・文字の選択はない。
- History の検索は GNOME の検索バー: ヘッダーバーの切り替えと Ctrl+F で、History の上にバーを出す。History で文字を打っても、打った文字でバーを開く（type-to-search。スペース・矢印・Ctrl や Alt との組み合わせでは開かない）。日本語入力のときは最初の一打が英字のまま入る（日本語入力は欄にフォーカスが移ってから働くため）。日本語で探すときは先に Ctrl+F を押す。
- 取り消せない操作の確認: キャンセルが先で Return と Esc はキャンセル、壊すボタンは赤。キャンセルは強調しない。
- GNOME が使うキーの近道は、ずらすか割り当てない（下の「キーボードの近道」）。
- 「この Mac」・Homebrew・launchd の代わりに「このコンピューター」・パッケージマネージャー・systemd や cron。

## 全体の配置

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Workspace の行でメインの表示を切り替える（Working Copy・History・Stashes・Reflog…） | ✓ | ✓ | △ | n/a | VSCode: Working Copy・History・Stashes と（エンジンがあれば）Reflog。Conflicts はない。 |
| コンフリクトがあるあいだ Conflicts の行と数 | ✓ | ✓ | ✓ | n/a | VSCode: Working Copy の中の Conflicts のグループ。 |
| stash があるときだけ Stashes の行と数 | ✓ | ✓ | ✓ | n/a | VSCode: Stashes の節。 |
| Range Diff を開いているあいだの Range Diff の行 | ✓ | ✓ | n/a | n/a | VSCode: Range Diff はエディターのタブで開く。 |
| Branches・Remotes・Tags を絞り込む欄 | ✓ | ✓ | ✓ | n/a | VSCode: サイドバーの上の絞り込みのボタンで言葉を入れ、合うものを平らに並べる。 |
| ブランチ・タグをいくつも選ぶ | ✓ | ✓ | ✓ | n/a | VSCode: Merge into Current Branch・Delete Branch・お気に入り・Archive が選んだものすべてに効く。 |
| Pull Requests・Worktrees は中身があるときだけ出す | ✓ | ✓ | △ | n/a | VSCode: Worktrees だけ（GitHub は入れない）。 |

## サイドバー

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Branches の並び（今のもの・活発・止まった・マージ済み・アーカイブ・積み重ね） | ✓ | ✓ | ✓ | △ | VSCode: エンジンがあるとき。なければ `/` で分けたフォルダーの木。<br>CLI: `amonhen run`（`branchGroups`）から。 |
| お気に入りのブランチ | ✓ | ✓ | ✓ | — | |
| ブランチのアーカイブ | ✓ | ✓ | ✓ | — | |
| ブランチを片付ける（取り込み済み・PR でマージ済み・上流が消えた。Undo 1 回） | ✓ | ✓ | — | △ | CLI: `amonhen run`（`branchCleanup`・`deleteBranches`）から。マージ済みの PR はアプリが gh で読む。 |
| お気に入りをドラッグで並べ替える | ✓ | ✓ | ✓ | n/a | VSCode: メニューの Move Up・Move Down in Favorites（ブランチに落とすと Merge・Rebase）。 |
| ブランチのメニュー: checkout・名前の変更・削除（未マージは Delete Anyway） | ✓ | ✓ | ✓ | △ | macOS: Set Upstream は Repository メニューだけ。<br>GTK4: Set Upstream は Repository メニューだけ。<br>CLI: `amonhen run` から。 |
| checkout していないブランチの同期（Fast-Forward・Push・Publish） | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| タグの一覧 | ✓ | ✓ | ✓ | ✓ |  |
| タグのリモートの印（ない・違う）とポップオーバー | ✓ | ✓ | ✓ | △ | VSCode: 行の説明に出し、リモートごとの様子はツールチップに（ポップオーバーはない）。<br>CLI: `amonhen run`（`tagRemotes`）から。 |
| タグの署名の印 | ✓ | ✓ | ✓ | ✓ | VSCode: 行のアイコンで、説明はツールチップに。<br>CLI: `refs --verify`。 |
| タグのメニュー: Push to・Replace on・削除・リモートから削除 | ✓ | ✓ | ✓ | △ | VSCode: リモートがいくつもあれば一覧で選ぶ。<br>CLI: `amonhen run` から。 |
| リモートとそのブランチ | ✓ | ✓ | ✓ | ✓ |  |
| リモートのメニュー: 追加・名前の変更・URL の変更・削除・Prune | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| Pull Requests のグループ | ✓ | ✓ | — | — |  |
| Submodules のグループ（Update・新しい窓で開く） | ✓ | ✓ | ✓ | ✓ | CLI: `submodule`・`submodule update`。 |
| submodule の印（取り出していない・違うコミット・中の変更・コンフリクト）とメニュー: Update…（中の submodule も・追うブランチの最新）・URL の編集・追うブランチ・外す（.git/modules も消せる）・追加・Sync URLs | ✓ | ✓ | ✓ | ✓ | VSCode: Remove は 2 つのボタンで、.git/modules を残すか一緒に消すかを聞く。<br>CLI: `submodule`（`add`・`remove`・`set-url`・`set-branch`・`sync`）。 |
| submodule: リポジトリのデータを .git/modules に移す・それぞれで命令を動かす | ✓ | ✓ | △ | ✓ | VSCode: Run in Each Submodule… は成否だけを知らせ、命令の出力は出ない。<br>CLI: `submodule absorb`・`submodule foreach`。 |
| Working Copy の下の worktree と変更の数（✎ ⌂ ⑂） | ✓ | ✓ | ✓ | △ | VSCode: 印は行のアイコン。ほかの worktree が checkout しているブランチには ⌂ か ⑂ が付く。<br>CLI: `worktree list --changes`。 |
| shallow clone・partial clone（History）と sparse checkout（Working Copy）の印 | ✓ | ✓ | △ | n/a | VSCode: History の上の Shallow・Partial（Partial は押すと中身をすべて取る）。sparse の印はない。 |

## 1. タブと窓

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 窓の中のリポジトリのタブ | ✓ | ✓ | n/a | n/a |  |
| 別の窓（New Window） | ✓ | ✓ | n/a | n/a |  |
| 起動時に窓とタブを戻す | ✓ | ✓ | n/a | n/a |  |
| SSH・daemon のリポジトリのつなぎ中のタブ（Cancel・Try Again） | ✓ | ✓ | n/a | n/a |  |
| 題に未コミットの ● と ahead/behind | ✓ | ✓ | n/a | n/a | GTK4: タブの名前に出る。 |
| 最後の窓を閉じてもアプリは終わらない | ✓ | — | n/a | n/a | GTK4: GNOME のアプリと同じく終わる。 |

## 2. ブックマーク（リポジトリの一覧）

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| All・Favorites・Places・グループのあるリポジトリのページ | ✓ | ✓ | n/a | n/a |  |
| 手元・SSH のリポジトリを足す | ✓ | ✓ | n/a | n/a |  |
| この機械の探すフォルダー（深さ・見張り） | △ | △ | n/a | n/a | macOS: 除くものは決まった一覧。<br>GTK4: 除くものは決まった一覧。 |
| SSH 先の探すフォルダー | ✓ | ✓ | n/a | △ | CLI: `find-repositories` がアプリのために探す。 |
| 場所の下のリポジトリのフォルダーの木 | ✓ | ✓ | n/a | n/a |  |
| 手で作るグループ・行を Favorites やグループへドラッグ | △ | △ | n/a | n/a | macOS: グループそのものはドラッグで並べ替えられない。<br>GTK4: グループそのものはドラッグで並べ替えられない。 |
| 状態の列（ブランチ・変更・ahead/behind・最後のコミット・開いた時）、並べ替え | ✓ | ✓ | n/a | n/a |  |
| お気に入り・表示名・色のラベル | ✓ | ✓ | n/a | n/a |  |
| タブ・窓で開く、Finder（ファイル）・Terminal | △ | △ | n/a | n/a | macOS: Open in Editor はない。<br>GTK4: Open in Editor はない。 |
| 選んだリポジトリを Fetch | ✓ | ✓ | n/a | n/a |  |
| Quick Open（あいまい検索） | ✓ | ✓ | n/a | n/a |  |
| ツールバーのリポジトリ名で切り替える | ✓ | ✓ | ✓ | n/a | VSCode: Select Repository でワークスペースのリポジトリから選ぶ。 |
| Clone（自分の GitHub のリポジトリから選ぶ） | ✓ | ✓ | n/a | △ | VSCode: VSCode 自身の Git: Clone。<br>CLI: `clone <url>` だけ。 |
| Clone: 取得する範囲（すべて・中身は必要なときに・最近の履歴）と、一部のフォルダだけ | ✓ | ✓ | n/a | ✓ | VSCode: VSCode の Git: Clone が代わり。<br>CLI: `clone --filter`・`--depth`・`--sparse`。 |
| Subtrees のグループ（git に git subtree があるとき）: 追加・Pull・Push・Split、相手と squash を覚える | ✓ | ✓ | ✓ | ✓ | VSCode: 値は入力欄、squash は一覧で聞く（シートではない）。<br>CLI: `subtree`。 |
| Clone: submodule も取り出す | ✓ | ✓ | n/a | ✓ | VSCode: VSCode の Git: Clone が代わり。<br>CLI: `clone --recurse-submodules`。 |
| 新しいリポジトリの Init | — | — | n/a | — | macOS: docs/ux.md の File メニューにあるが、作っていない。 |

## 3. Settings

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| General: 開いたリポジトリをブックマークする | ✓ | ✓ | n/a | n/a |  |
| General: 起動時に開き直す・既定はタブか窓か | — | — | n/a | n/a | macOS: いつも開き直し、切り替えはない。<br>GTK4: いつも開き直し、切り替えはない。 |
| General: 自動で fetch | ✓ | ✓ | n/a | n/a | VSCode: VSCode の `git.autofetch` が代わり。 |
| General: 外のエディター | ✓ | ✓ | n/a | n/a | GTK4: コードエディターは desktop のエントリーで macOS 版と同じ順に探す。なければテキストエディター（オフィスのアプリは選ばない）。Other… ではプログラムを選ぶ。 |
| General: 端末と diff のツール | — | — | n/a | n/a |  |
| Git: 機械全体のページ（Author and Signing・Commits・Operations・Working Copy・Keys and Values） | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| Git > Commits: 共有のメッセージのテンプレート | ✓ | ✓ | — | — |  |
| Git > Programs: git・gpg・ssh-keygen・gpgsm | ✓ | ✓ | n/a | △ | VSCode: git は VSCode の `git.path` で選ぶ（拡張はそれに従う）。gpg・ssh-keygen は git の設定で決まる。<br>CLI: `amonhen run`（`programs`・`setMachineGit`）から。 |
| Git > この機械の Environment Variables（渡す・秘密を隠す） | ✓ | ✓ | n/a | n/a | VSCode: VSCode の環境が代わり（拡張はそれを daemon に渡す）。 |
| Environment Variables: ログインシェルのものを使う（Read Again・SSH agent） | ✓ | — | n/a | n/a | GTK4: 外した。Linux のセッションはすでに持っている。<br>VSCode: VSCode の環境が代わり（拡張はそれを daemon に渡す）。 |
| SSH Hosts: + / − の付いたホスト、届かないものは薄く | ✓ | ✓ | n/a | n/a | VSCode: Remote-SSH が受け持つ。 |
| SSH Hosts > Connection: 状態・Check Connection・エンジン | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts > Connection: 署名の確かめ方 | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts > Connection: 探すフォルダー | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts: ホストのすべてのリポジトリの git のページ | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts: ホストの Environment Variables（ログインシェルの切り替え） | ✓ | ✓ | n/a | n/a |  |
| ホスト・この機械の列で選ぶパスのシート | ✓ | ✓ | n/a | n/a |  |
| Accounts: GitHub（CI の状態・アバター） | ✓ | ✓ | — | n/a |  |
| Appearance: History の順・1 行・行の項目・アバター | ✓ | ✓ | n/a | n/a |  |
| Appearance: テーマ・フォント・グラフの色 | — | — | n/a | n/a |  |
| Diff & Merge: 開いたときに迷わないコンフリクトを解く | ✓ | ✓ | n/a | n/a |  |
| Diff & Merge: unified/split・空白・前後の行・折り返し・タブの幅 | — | — | — | n/a |  |
| AI: 使う AI・CLI の場所と版・モデル・言語・リポジトリごとの許可 | ✓ | ✓ | — | n/a | どちらのアプリも CLI のログインの状態は読まない。CLI がなければ、入れてサインインするよう案内する。 |
| 目的ごとの AI のプロンプト（編集・既定に戻す・試す） | ✓ | ✓ | — | n/a |  |
| Performance: commit-graph・共有の daemon | ✓ | ✓ | △ | n/a | VSCode: VSCode の設定の `amonhen.daemon.enabled`・`amonhen.daemon.path`。 |
| Performance: fsmonitor | ✓ | ✓ | — | n/a | GTK4: Linux・BSD では、エンジンを git の fsmonitor の hook（`amonhen fsmonitor-watchman`）にして Watchman を使う。Watchman がないか、git が 2.26 より古い（hook の版 2 がない）と押せない。 |
| Performance: 永続キャッシュの場所・大きさ・消す | — | — | — | △ | CLI: `log --rebuild`・`daemon rebuild`。 |
| 設定の変更の Undo・Redo | △ | △ | n/a | n/a | macOS: Git のページとリポジトリの設定だけ。<br>GTK4: Git のページとリポジトリの設定だけ。 |

## 4. リポジトリの設定

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 設定のページ（ページの一覧・git の行・行ごとの出どころ・↩︎・ⓘ） | ✓ | ✓ | ✓ | n/a | VSCode: メインビューの Repository Settings（エンジンがあるとき）で、このリポジトリの設定だけ。 |
| Author and Signing（署名の選択・集めた鍵・mailmap） | ✓ | ✓ | ✓ | △ | VSCode: 鍵は今の形式のものだけを集める。<br>CLI: `amonhen run`（`signingKeys`・`configSet`）から。 |
| Commits: メッセージのテンプレート（なし・共有・独自・ファイル） | ✓ | ✓ | △ | △ | VSCode: なし・このリポジトリ専用（その場で書く）・ファイル。共有のテンプレートはない。<br>CLI: `amonhen run` から。 |
| Branches: 既定のブランチを選ぶ | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| Operations: pull・fetch.prune・autostash・updateRefs・コンフリクトの書き方・rerere | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| rerere: 1 つの解決を忘れる（マージエディタ）・すべて忘れる（リポジトリの設定） | ✓ | ✓ | ✓ | △ | VSCode: rerere が解決したコンフリクトの Working Copy の右クリックに Forget Resolution（マージエディタは VSCode のもの）、Repository Settings > Operations に Forget All…。<br>CLI: `amonhen run`（`forgetResolution`・`forgetAllResolutions`）から。 |
| Working Copy: 改行・実行ビット・`.git/info/exclude` の編集 | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| Remotes: 表と、つながるか確かめるシート | ✓ | ✓ | ✓ | △ | VSCode: 表と行ごとのボタン（Edit URL…・Push URL…・Check・Prune・Rename…・Remove…）と Add…。値はシートでなく入力欄で聞く。<br>CLI: `amonhen run` から。 |
| Hooks: 表・Enabled・スクリプト・Create Hook… | ✓ | ✓ | ✓ | △ | VSCode: Open で VSCode のエディターにスクリプトを開く。<br>CLI: `amonhen run` から。 |
| LFS: 入っているか・パターン・Track Pattern…・ファイルの数 | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| Keys and Values（git config の表、Basic / Customized / All、Combined / Levels） | ✓ | ✓ | △ | △ | VSCode: Basic / Customized / All と検索、効いている値と設定元。値は入力欄で変える。Levels の列はない。<br>CLI: `amonhen run`（`configList`・`configSet`）から。 |
| 要る git の版を添えて行を無効にする | ✓ | ✓ | ✓ | n/a | |
| git の行にリポジトリの形（shallow・partial・sparse・maintenance） | ✓ | ✓ | ✓ | △ | CLI: `status` が shallow・partial・sparse を添える。 |
| 取得とフォルダ: shallow clone の履歴を取り足す（数・日付まで・すべて） | ✓ | ✓ | ✓ | △ | CLI: `amonhen run`（`deepen`・`unshallow`）から。 |
| 取得とフォルダ: partial clone の中身をすべて取る（git 2.36） | ✓ | ✓ | ✓ | △ | CLI: `amonhen run`（`fetchAllContents`）から。 |
| 取得とフォルダ: sparse checkout のフォルダを HEAD の木から選ぶ（git 2.25） | ✓ | ✓ | ✓ | △ | VSCode: フォルダは HEAD の木を一覧でたどって選ぶ。<br>CLI: `amonhen run`（`sparseCheckout`・`treeFolders`）から。 |
| 取得とフォルダ: git maintenance の切り替え（git 2.31） | ✓ | ✓ | ✓ | △ | CLI: `amonhen run`（`maintenance`）から。 |

## 5. ツールバー

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 履歴のない新しいブランチ（空から・ファイルを残して） | ✓ | ✓ | ✓ | △ | VSCode: コマンドパレットと Branches のグループの右クリックから。<br>CLI: `amonhen run`（`createOrphanBranch`）から。 |
| ブランチの切り替え（あいまい検索・最近・追跡・新しいブランチ） | ✓ | ✓ | △ | △ | VSCode: サイドバーからの Checkout と New Branch… だけ。<br>CLI: `amonhen run` から。 |
| Fetch のシート（すべてか 1 つのリモート、prune を示す） | ✓ | ✓ | △ | △ | VSCode: シートなしで、すべてを prune 付きですぐ fetch。<br>CLI: `amonhen run` から。 |
| Pull のシート（コミット・merge / rebase / ff-only・なければ上流） | ✓ | ✓ | △ | △ | VSCode: シートも選択もなく、上流をマージする。<br>CLI: `amonhen run` から。 |
| Push のシート（注意・コミットと差分・lease 付きの Force Push） | ✓ | ✓ | △ | △ | VSCode: シートはなく、Force Push は別に尋ねる。<br>CLI: `amonhen run` から。 |
| Publish（リモートと名前を選ぶ） | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| Merge（no-ff・squash の選択） | △ | △ | △ | △ | macOS: no-ff・squash の選択はない。<br>GTK4: no-ff・squash の選択はない。<br>VSCode: メニューかドラッグで、選択はない。<br>CLI: `amonhen run` から。 |
| いくつもを 1 つにマージ（順番・コンフリクトの見込み） | ✓ | ✓ | ✓ | △ | CLI: `merge-check` で確かめ、マージは `amonhen run` から。 |
| 前と後を描く Rebase のシート | ✓ | ✓ | △ | △ | VSCode: 書き換えの見込みを文で示すダイアログ。<br>CLI: `rewrite-preview` が出し、rebase は `amonhen run` から。 |
| Stash（メッセージ・未追跡を含める） | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| Stash: stage した変更だけ・index を残す・選んだファイルだけ | ✓ | ✓ | ✓ | △ | VSCode: エンジンがあるとき、Stash の一覧に前の 2 つ、Working Copy のファイルの右クリックにそのファイルの Stash…。<br>CLI: `amonhen run`（`stashPush` の `staged`・`keepIndex`・`paths`）から。 |
| ignore されたファイルを消す（先に一覧） | ✓ | ✓ | ✓ | △ | VSCode: コマンドパレットと Working Copy の右クリックから。一覧は選ぶもので、はじめはすべて選んである。<br>CLI: `amonhen run`（`ignoredFiles`・`deleteIgnored`）から。 |
| Stash: ステージしたものだけ・選んだファイルだけ | — | — | — | — |  |
| 最新の stash を Pop（先に尋ねる） | ✓ | ✓ | ✓ | △ | VSCode: Stashes の見出しとコマンドパレットの Pop Latest Stash…（行の Pop は尋ねない）。<br>CLI: `amonhen run` から。 |
| Activity のくるくると数 | ✓ | ✓ | ✓ | n/a | VSCode: VSCode の進み具合の知らせ。 |
| Refresh と Rebuild History | ✓ | ✓ | △ | △ | VSCode: Refresh だけ。<br>CLI: `log --rebuild`・`daemon rebuild`。 |
| 直前の操作の Undo（尋ね、何を戻すか示す） | ✓ | ✓ | ✓ | ✓ | VSCode: 上の Undo とコマンドパレットは尋ねる。操作の直後に押す知らせの Undo は尋ねない。<br>CLI: `undo`（先に見るなら `--peek`）。 |
| 範囲付きの History の検索（件名・作者・ID・パス・中身） | ✓ | ✓ | — | ✓ | CLI: `log <query>`。 |
| ahead/behind のバッジ | ✓ | ✓ | ✓ | △ | VSCode: サイドバーのブランチの行に出る。<br>CLI: `status` に出る。 |
| ツールバーのカスタマイズ | ✓ | ✓ | n/a | n/a | GTK4: パレットは NSToolbar と同じ使い方で、シートと同じく見出しの下に中身に重ねて出し、窓の大きさは変えない。見出しはアイコンだけなので、表示（アイコンと文字）は選ばない。Repository のメニューと主メニューはメニューバーの代わりなので動かない。 |
| 120 秒黙った network の git を止める | ✓ | ✓ | — | ✓ | VSCode: 拡張自身の git 実行層にはない。 |

## 5a. Stashes

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Stashes の表示（一覧と中身） | ✓ | ✓ | ✓ | △ | VSCode: サイドバーの一覧で、Show Stash は VSCode の複数ファイルの差分。<br>CLI: `refs` が並べ、`show stash@{n}`。 |
| stash の行の詳細（ブランチ・HEAD に対する元・未追跡） | ✓ | ✓ | ✓ | △ | VSCode: 未追跡は行の説明に、ブランチと元はツールチップに（エンジンがあるとき）。<br>CLI: `amonhen run`（`stashBases`）から。 |
| Apply・Pop・Drop…（Undo できる）・Branch… | ✓ | ✓ | ✓ | △ | VSCode: Branch… はエンジンがあるとき。Apply・Pop はできれば index も戻す（`--index`）。<br>CLI: `amonhen run` から。 |

## 5b. Reflog

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 「どのブランチにもない」の付いた Reflog の表示 | ✓ | ✓ | ✓ | △ | VSCode: エンジンがあるときだけ。<br>CLI: `amonhen run` から。 |
| Reflog の操作（New Branch・Checkout・New Tag・Reset・Show in History・Copy） | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |

## 5c. Reset

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| コミットへ Reset（Soft / Mixed / Hard、Hard は先に stash、Undo できる） | ✓ | ✓ | ✓ | △ | VSCode: エンジンがあるときだけ。<br>CLI: `amonhen run` から。 |

## 5d. Range Diff

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Range Diff の表示（= ! < >・Changes Only・差分の差分） | ✓ | ✓ | △ | ✓ | VSCode: git の形の読むだけの文書。<br>CLI: `range-diff`。 |
| Compare with Previous Version | ✓ | ✓ | ✓ | ✓ | CLI: `range-diff --previous`。 |
| History の 2 つのコミット・Reflog の行を 2 つの版として比べる | ✓ | ✓ | △ | ✓ | VSCode: 代わりに Range Diff… で範囲を打ち込む。<br>CLI: どの範囲でも。 |
| 範囲を直して比べ直す | ✓ | ✓ | △ | ✓ | VSCode: 開く前だけ。 |

## 6. Working Copy

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 1 つのチェックボックスの一覧（☑ ▣ ☐、見出し・フォルダーのチェック） | ✓ | ✓ | ✓ | n/a | VSCode: Staged と Changes が別のグループ。 |
| List / Tree / Flat | ✓ | ✓ | — | n/a |  |
| パスで絞り込む | ✓ | ✓ | — | n/a |  |
| 行ごとの +/− の数と、ホバーで Discard | ✓ | ✓ | △ | △ | VSCode: 行の数はない。<br>CLI: `diff --stat`。 |
| Space で切り替え、⌫ で捨てる | ✓ | ✓ | △ | n/a | VSCode: Space で切り替えるが、⌫ はない。 |
| 上のコミットの欄、50/72 の数と本文の目安の線 | ✓ | ✓ | △ | n/a | VSCode: 数も目安の線もない。 |
| リポジトリごとの下書き、テンプレートから始める | ✓ | ✓ | — | n/a |  |
| 最近のメッセージ（⌥↑ / ⌥↓・一覧） | ✓ | ✓ | — | n/a |  |
| Conventional の種類・Co-authored-by・Signed-off-by を入れる | ✓ | ✓ | — | n/a |  |
| `#issue` の補完 | ✓ | ✓ | — | n/a |  |
| Commit and Push・Amend | ✓ | ✓ | △ | △ | VSCode: Amend だけ。<br>CLI: `commit -m … --amend`。 |
| Fixup / Squash Commit For | ✓ | ✓ | — | n/a |  |
| hooks の出力のシート | ✓ | ✓ | — | n/a |  |
| Commit が押せない理由を出す | — | — | △ | n/a | VSCode: コミットできないときに注意を出す。 |
| HEAD → 作業ツリーの 1 つの差分と、行・hunk のチェック | ✓ | ✓ | △ | ✓ | VSCode: 別々の未ステージ・ステージ済みの差分で行をステージ。<br>CLI: `diff --combined`・`stage --rows`。 |
| 行の範囲の選択、J/K・⌥J/⌥K | ✓ | ✓ | △ | n/a | VSCode: J/K のキーはない。 |
| 重ねられないファイルの Unstaged / Staged の切り替え | ✓ | ✓ | n/a | ✓ | VSCode: いつも別々に出す。<br>CLI: `diff --staged`。 |
| 差分の表示の選択（unified/split・空白・前後の行・折り返し） | — | — | — | n/a |  |
| 行・hunk・ファイルを捨てる（Undo できる） | ✓ | ✓ | ✓ | ✓ | CLI: `discard --rows`。 |
| Add to .gitignore | ✓ | ✓ | — | n/a |  |
| コンフリクトのファイルからマージエディターを開く | ✓ | ✓ | ✓ | n/a | VSCode: VSCode のマージエディターを開く。 |
| 一部のフォルダだけを取り出しているとき、一覧の上に知らせる | ✓ | ✓ | — | △ | CLI: `status` が sparse checkout を添える。 |

## History

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| ブランチ・タグのラベル付きのグラフ | ✓ | ✓ | ✓ | ✓ | CLI: `log`。 |
| 大きな履歴をかたまりで読み、永続キャッシュを使う | ✓ | ✓ | ✓ | ✓ |  |
| 一致の数・次 / 前・一致だけの検索 | ✓ | ✓ | — | △ | CLI: `log <query>` が一致を並べる。 |
| 表示: 順・1 行・行の項目・マージ済みを畳む・祖先を目立たせる | ✓ | ✓ | △ | △ | VSCode: 範囲（今・すべて・ref）だけ。<br>CLI: `--topo-order`・`--fold`。 |
| アバター | ✓ | ✓ | — | n/a |  |
| コミットの署名の印 | ✓ | ✓ | ✓ | △ | VSCode: エンジンがあるとき。<br>CLI: `show` に出る。 |
| コミットごとの CI の状態とポップオーバー | ✓ | ✓ | — | — |  |
| worktree の HEAD のラベル（✎ ⌂ ⑂・変更の数） | ✓ | ✓ | — | — |  |
| Go to HEAD・Parent・Child | ✓ | ✓ | ✓ | n/a | VSCode: History の上の HEAD のボタン、コミットのメニューの Go to Parent・Go to Child、詳細の親と子へのリンク（エンジンがあるとき）。 |
| Go to Ref | — | — | — | n/a |  |
| いくつものコミットを選ぶ（範囲・2 つ・飛び飛び） | ✓ | ✓ | — | △ | CLI: 2 つは `diff <rev> <rev>`。 |
| 選んだいくつもの ref の History | ✓ | ✓ | — | ✓ | CLI: `log --branch` を重ねる。 |
| コミットのメニュー: checkout・新しいブランチやタグ・cherry-pick・revert・ID のコピー | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| 編集モード（keep・reword・squash・fixup・drop・edit、ドラッグで並べ替え） | ✓ | ✓ | △ | △ | VSCode: 1 つずつで、いくつもの段の計画はない。<br>CLI: `amonhen run` から。 |
| 書き換えの見込み（積み重ね・push 済みのブランチ、コンフリクトの見込み） | ✓ | ✓ | △ | ✓ | VSCode: daemon 越しで、今のブランチだけ。<br>CLI: `rewrite-preview`。 |
| 近道: Edit Message…・Combine with Older…・Drop…・Combine N…・コミットのドラッグ | ✓ | ✓ | △ | △ | VSCode: Combine N Commits はない。<br>CLI: `amonhen run` から。 |
| コミットを分ける | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| 編集でマージを保つ（`--rebase-merges`） | ✓ | ✓ | — | △ | VSCode: 最初のマージで止まる。<br>CLI: `amonhen run` から。 |
| shallow clone: 終わりの帯で取り足す、境界のコミットの印 | ✓ | ✓ | ✓ | △ | VSCode: エンジンがあるとき。<br>CLI: `log` が履歴の止まる所を添える。取り足すのは `amonhen run` から。 |

## 7. History の右の列

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| メッセージ全体・ドラッグできる境・⤢ で広げる | ✓ | ✓ | △ | n/a | VSCode: ⤢ はない。 |
| 詳細の中を探す | ✓ | ✓ | — | n/a |  |
| すべてのファイルを積む（貼り付く見出し・畳む・Viewed） | ✓ | ✓ | △ | ✓ | VSCode: 積んでスクロールで読むが、畳むことも Viewed もない。<br>CLI: `show`。 |
| 差分の横の、変更したファイルの木 | ✓ | ✓ | △ | n/a | VSCode: 差分の上の平らな一覧。 |
| ファイルの見出しのメニュー（Blame・File History・パスのコピー・開く） | ✓ | ✓ | ✓ | n/a | VSCode: 右クリックの Blame・File History・Copy Path と、VSCode の差分のエディターで開く。 |
| 1 ファイルずつの差分に切り替える | ✓ | ✓ | — | n/a |  |
| 選んだものの差分（範囲をまとめる・A..B / A...B・飛び飛び） | ✓ | ✓ | — | △ | CLI: `diff <rev> <rev>`。 |
| 選んだものの操作（cherry-pick・revert・squash・ID のコピー） | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| 署名のポップオーバー | ✓ | ✓ | ✓ | n/a | VSCode: 詳細の Signature の行（様子・説明・署名した人と鍵）。ポップオーバーはない。 |
| 単語の強調と構文の色 | ✓ | ✓ | △ | n/a | VSCode: 単語だけ。 |

## Blame・File History

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Blame | ✓ | ✓ | △ | △ | VSCode: メインビューのページ。構文の色はない。<br>CLI: `amonhen run`（`blame`）から。 |
| ファイルをコミットの版に戻す（この版・その前の版。選べば stage も） | ✓ | ✓ | ✓ | ✓ | VSCode: エンジンがあるとき、History の詳細のファイルの右クリックから。Restore and Stage は 2 つめのボタン。<br>CLI: `restore --source <rev> [--staged]`。 |
| File History | ✓ | ✓ | ✓ | △ | VSCode: メインビューのページ。エクスプローラー・エディターのタブ・コマンドパレット・メインビューのファイルから開く。<br>CLI: `amonhen run`（`fileHistory`）から。 |
| リポジトリのタブの隣のタブに、リポジトリのツールバー付きで開く | ✓ | ✓ | n/a | n/a | GTK4: リポジトリのタブがほかの窓で前に出ているときは、ツールバーのない題だけの帯になる。 |
| 行の履歴 | — | — | — | — |  |

## 7a. パッチ

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Save as Patch（コミットごとか mbox・カバーレター・-v） | ✓ | ✓ | △ | ✓ | VSCode: `-v` はない。<br>CLI: `format-patch`。 |
| Apply Patch（am / apply・下見・3-way・--index） | ✓ | ✓ | ✓ | ✓ | CLI: `am`・`apply`（`--check`）。 |
| パッチのファイルを窓に落とす | ✓ | ✓ | ✓ | n/a | VSCode: エクスプローラーのメニューから。 |
| Export Archive（zip・tar.gz・tar、先頭のフォルダ、1 つのフォルダ） | ✓ | ✓ | — | ✓ | CLI: `archive`。 |
| Save as Bundle（すべて・一部の ref・基点より後） | ✓ | ✓ | — | ✓ | CLI: `bundle create`。 |
| bundle のファイルから clone（HEAD がなければブランチを選ぶ） | ✓ | ✓ | — | ✓ | CLI: `clone <file> [-b <branch>]`。 |
| Fetch from Bundle（名前の下に、前提のコミットを確かめて） | ✓ | ✓ | — | ✓ | CLI: `bundle fetch`。 |
| 止まった `am` と Retry with 3-Way・Skip・Abort・Continue | ✓ | ✓ | ✓ | ✓ | VSCode: サイドバーの行に出る。<br>CLI: `am --continue / --skip / --abort / --retry-3way`。 |

## 8. マージエディター

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 帯でつないだ 3 つの列（Current / Result / Incoming） | ✓ | ✓ | n/a | n/a | VSCode: VSCode 自身の 3-way のマージエディター。 |
| Show Base | ✓ | ✓ | n/a | n/a |  |
| Magic Merge（ファイルを開いたときの確かな規則も） | ✓ | ✓ | n/a | ✓ | VSCode: コンフリクトは VSCode 標準の 3-way マージエディターで開く。<br>CLI: `merge-file`（git の mergetool にも）。 |
| 直せる Result とコンフリクトごとの Undo | ✓ | ✓ | n/a | n/a |  |
| ファイルを Current / Incoming で解く | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| 外のマージツールで開く | — | — | n/a | n/a |  |
| エディターを別の窓に出す | ✓ | ✓ | n/a | n/a |  |
| Continue / Skip / Abort | ✓ | ✓ | ✓ | △ | VSCode: サイドバーの進行中の行に出る。<br>CLI: `amonhen run` から。 |

## 9. AI

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Claude Code と Codex | ✓ | ✓ | — | — |  |
| Apple Intelligence（端末の中） | ✓ | n/a | n/a | n/a |  |
| コミットのメッセージを翻訳 | ✓ | ✓ | — | — |  |
| コミット・選んだものを解説 | ✓ | ✓ | — | — |  |
| コミットのメッセージを提案 | ✓ | ✓ | — | — |  |
| 差分の hunk を解説 | ✓ | ✓ | — | — |  |
| AI でコンフリクトを解く | ✓ | ✓ | — | — |  |
| すべてのファイルを AI で解く | — | — | — | — | docs/ux.md（9 章）に設計はあるが、まだない。マージエディタはコンフリクトを 1 つずつ解く。 |
| コンフリクトを解説 | ✓ | ✓ | — | — |  |
| リポジトリごと・リポジトリのプロンプトの初回の確認 | ✓ | ✓ | — | — |  |
| 流れてくる結果とキャッシュ | ✓ | ✓ | — | — |  |

## 10. Activity

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Operations（git の命令・出力・進み具合・取り消し） | ✓ | ✓ | △ | △ | VSCode: Output のチャンネルの記録だけ。<br>CLI: `amonhen run`（`remoteActivity`・`cancelActivity`）から。 |
| History（操作ログ・ここまで戻す） | ✓ | ✓ | △ | △ | VSCode: Undo History… が Undo の記録を並べ、選んだところまで戻す。ほかの Activity はない。<br>CLI: `undo --list` が記録を出す。 |
| Performance | ✓ | ✓ | n/a | n/a | VSCode: VSCode の出力（Amon Hen のログ）が代わり。 |
| Events | ✓ | ✓ | n/a | n/a | VSCode: VSCode の出力（Amon Hen のログ）が代わり。 |
| JSON で書き出す | ✓ | ✓ | n/a | n/a | VSCode: VSCode の出力（Amon Hen のログ）が代わり。 |
| Daemon のタブ（起動・停止・つながり） | ✓ | ✓ | — | △ | CLI: `daemon status / start / stop`。 |

## 11. メニュー

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 共有の操作の表（`crates/commands`）から作るメニュー | ✓ | ✓ | n/a | n/a | VSCode: VSCode のコマンドパレットと右クリックのメニューが代わり。 |
| メニューバーなしで全部の操作に届く（≡） | ✓ | ✓ | ✓ | n/a | GTK4: ≡ のメニューに、メニューバーの File・Edit・View・Navigate・Commit・Window を macOS と同じ項目と並びで置き（Edit の取り消す・カット・コピーなどはフォーカスのある欄に効く）、その後に設定…・Amon Hen について・終了。<br>VSCode: コマンドパレットとコンテキストメニュー。 |
| キーボードの近道 | ✓ | ✓ | n/a | n/a | GTK4: Linux のデスクトップやほかの操作が使うキーは違う: サイドバーを表示は F9（GNOME のユーティリティの欄のキー）。ブランチを絞り込む・親へ・子へ・New Tag にはない（GNOME が Ctrl+Alt+矢印と Ctrl+Alt+T を使い、HIG では Alt+Up が別の意味）。マージエディターの前・次のコンフリクトは Alt+Up・Down。<br>VSCode: VSCode のキー割り当てが代わり。拡張は割り当てを持たず、どのコマンドにも割り当てられる。 |
| メニューと同じ名前のコンテキストメニュー | ✓ | ✓ | △ | n/a | VSCode: 名前がアプリと違う。 |
| キーボードの近道の窓 | — | ✓ | n/a | n/a | macOS: 代わりに macOS の App Shortcuts で変える。 |

## ステータスバー

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 進行中の操作と Continue / Skip / Abort | ✓ | ✓ | ✓ | n/a | VSCode: サイドバーの行。 |
| 失敗とその原因・Details | ✓ | ✓ | △ | n/a | VSCode: エラーの知らせだけ。 |
| 切れたつながりと Reconnect | ✓ | ✓ | n/a | n/a |  |
| 終わった操作が何をしたか、数秒 | ✓ | ✓ | ✓ | n/a | VSCode: ステータスバーの文か、Undo 付きの知らせ。 |

## 確認と Undo

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| リモート・HEAD を動かす・消す操作の前のシート | ✓ | ✓ | △ | n/a | GTK4: 名前を入れるものもすべての確認も同じシート。macOS と同じく、入力がどこにあっても（ドロップダウンやラジオでも）Return で既定のボタンを押す。<br>VSCode: 消す・書き換える・Reset のときだけで、Fetch・Pull・Push・Merge・Checkout にはない。 |
| git の操作の Undo（`undo.json`） | ✓ | ✓ | ✓ | ✓ |  |
| 押せない部品はツールチップで理由を言う | ✓ | ✓ | n/a | n/a | VSCode: 使えないコマンドは出さない（when 句）。 |

## 12. GitHub

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| `gh` を通す GitHub | ✓ | ✓ | — | — |  |
| Pull Requests（CI・draft・レビュー、checkout・開く・URL のコピー） | ✓ | ✓ | — | — |  |
| Create Pull Request（base・draft） | ✓ | ✓ | — | — | macOS: base は打ち込む（最初は既定のブランチ）。fork からは元のリポジトリへ。<br>GTK4: base は打ち込む（最初は既定のブランチ）。fork からは元のリポジトリへ。 |
| ブランチの状態のポップオーバー（CI・プルリクエスト） | ✓ | ✓ | — | — |  |
| Open on GitHub | ✓ | ✓ | — | — |  |
| メッセージの `#123` のリンク | ✓ | ✓ | — | — |  |
| GitHub のアバター | ✓ | ✓ | — | — |  |

## 13. 言語

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 英語の UI と日本語の訳 | ✓ | ✓ | — | ✓ | VSCode: 訳していない。日本語の文もまじる。 |

## 14. Worktree

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| worktree と変更の数 | ✓ | ✓ | ✓ | ✓ | CLI: `worktree list --changes`。 |
| 書き込む先の worktree（✎）と切り替え | ✓ | ✓ | ✓ | n/a | VSCode: 拡張が働くリポジトリを切り替える。ワークスペースの外のものはソース管理のビューには足さない。<br>CLI: 動かした worktree で働く。 |
| New Worktree…（ブランチ・コミットから） | ✓ | ✓ | ✓ | ✓ |  |
| Remove Worktree（尋ね、変更があれば `--force`） | ✓ | ✓ | ✓ | ✓ |  |
| ロックと解除・移動・移したフォルダを探す（repair） | ✓ | ✓ | ✓ | ✓ | VSCode: worktree の右クリックから。書き込む先の worktree は、ほかに切り替えてから移す。<br>CLI: `worktree lock`・`unlock`・`move`・`repair`。 |
| worktree の Prune | ✓ | ✓ | ✓ | ✓ |  |
| worktree のフォルダーを開くと、そのリポジトリでそれを書き込む先にする | ✓ | ✓ | ✓ | n/a | VSCode: フォルダーを開くとその worktree で働き、ほかの worktree も並ぶ。 |

## SSH 先・共有の daemon・askpass

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| SSH 先のリポジトリ（ssh 越しのエンジン） | ✓ | ✓ | n/a | ✓ | VSCode: Remote-SSH が拡張をホストで動かす。<br>CLI: `-C ssh://host/path`。 |
| 共有の daemon を通す | ✓ | ✓ | ✓ | ✓ | CLI: `-C daemon:<path>`。 |
| 合うものがなければホストにエンジンを送る | ✓ | ✓ | ✓ | ✓ | VSCode: 拡張が動くホストで、同梱したエンジンを使う。 |
| Windows の SSH 先 | ✓ | ✓ | ✓ | ✓ |  |
| パスワード・パスフレーズを尋ねる askpass のダイアログ | ✓ | ✓ | — | n/a | VSCode: 尋ねるのを切っている（`GIT_TERMINAL_PROMPT=0`）。<br>CLI: ダイアログはアプリのもの。 |
| 署名の確かめ（この機械か、ホストの鍵） | ✓ | ✓ | — | △ | CLI: `show`・`refs --verify` で確かめ、選択はない。 |
