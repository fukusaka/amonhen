# FE ごとの機能

[English](../frontends.md)

利用者に見える機能が、どの FE にあるか: macOS のアプリ（`apps/macos`）、GTK4 のアプリ（`apps/gtk4`）、VSCode 拡張（`apps/vscode`）、CLI（`crates/cli`）。行は機能を説明する [docs/ux.md](ux.md)（おもに macOS のアプリの形）の章の順で、History・サイドバー・Blame / File History・ステータスバー・確認・SSH を足した。設計ではなくコードで確かめた。

印:

- ✓ できている。
- △ 一部だけ、または違う形。どう違うかは Notes の列。
- — ない。
- n/a その FE には当てはまらない（CLI にサイドバーはない。VSCode は窓・設定・マージエディターが自前）。

CLI の △「`amonhen run` から」は、コアの命令はあるが、CLI からは JSON（`amonhen run '{"op":…}'`）でしか届かず、専用のサブコマンドがないこと。

メニューの操作そのものは 1 つずつ並べない: どの FE も共有の表 `crates/commands` から取り、GTK4 のアプリでまだないものは `apps/gtk4/src/appmenu.rs` の `PENDING` に並べる（今はない）。

この表は手で保つ。どの FE でも、機能を足す・外す・変えるときは、同じ変更でここも直す。

## 全体の配置

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Workspace の行でメインの表示を切り替える（Working Copy・History・Stashes・Reflog…） | ✓ | ✓ | △ | n/a | VSCode: Working Copy・History・Stashes だけで、Reflog・Conflicts はない。 |
| コンフリクトがあるあいだ Conflicts の行と数 | ✓ | ✓ | △ | n/a | VSCode: Working Copy の中の Conflicts のグループ。 |
| stash があるときだけ Stashes の行と数 | ✓ | ✓ | △ | n/a | VSCode: Stashes の節はいつも出る。 |
| Range Diff を開いているあいだの Range Diff の行 | ✓ | ✓ | — | n/a | GTK4: × ではなく Close Range Diff で閉じる。 |
| Branches・Remotes・Tags を絞り込む欄 | ✓ | ✓ | — | n/a |  |
| ブランチ・タグをいくつも選ぶ | ✓ | ✓ | — | n/a |  |
| Pull Requests・Worktrees は中身があるときだけ出す | ✓ | ✓ | — | n/a | VSCode: どちらのグループもない。 |

## サイドバー

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Branches の並び（今のもの・活発・止まった・マージ済み・アーカイブ・積み重ね） | ✓ | ✓ | △ | △ | VSCode: `/` で分けたフォルダーの木だけ。<br>CLI: `amonhen run`（`branchGroups`）から。 |
| お気に入りのブランチ | ✓ | ✓ | — | — |  |
| ブランチのアーカイブ | ✓ | ✓ | — | — |  |
| お気に入りをドラッグで並べ替える | ✓ | ✓ | — | n/a | GTK4: お気に入りの行の上 3 分の 1 か下 3 分の 1 に落とす。真ん中なら Merge・Rebase。 |
| ブランチのメニュー: checkout・名前の変更・削除（未マージは Delete Anyway） | ✓ | ✓ | ✓ | △ | macOS: Set Upstream は Repository メニューだけ。<br>GTK4: Set Upstream は今のブランチだけ。<br>CLI: `amonhen run` から。 |
| checkout していないブランチの同期（Fast-Forward・Push・Publish） | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| タグの一覧 | ✓ | ✓ | ✓ | ✓ |  |
| タグのリモートの印（ない・違う）とポップオーバー | ✓ | ✓ | — | △ | CLI: `amonhen run`（`tagRemotes`）から。 |
| タグの署名の印 | ✓ | ✓ | — | ✓ | CLI: `refs --verify`。 |
| タグのメニュー: Push to・Replace on・削除・リモートから削除 | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| リモートとそのブランチ | ✓ | ✓ | ✓ | ✓ |  |
| リモートのメニュー: 追加・名前の変更・URL の変更・削除・Prune | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| Pull Requests のグループ | ✓ | ✓ | — | — |  |
| Submodules のグループ（Update・新しい窓で開く） | ✓ | ✓ | ✓ | △ | CLI: `refs` が並べ、Update は `amonhen run` から。 |
| Working Copy の下の worktree と変更の数（✎ ⌂ ⑂） | ✓ | ✓ | — | △ | CLI: `worktree list --changes`。 |

## 1. タブと窓

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 窓の中のリポジトリのタブ | ✓ | △ | n/a | n/a | GTK4: 窓の間の移動はメニュー（Move Tab to New Window・Merge All Windows）で、ドラッグではできない。 |
| 別の窓（New Window） | ✓ | ✓ | n/a | n/a |  |
| 起動時に窓とタブを戻す | ✓ | ✓ | n/a | n/a |  |
| SSH・daemon のリポジトリのつなぎ中のタブ（Cancel・Try Again） | ✓ | ✓ | n/a | n/a |  |
| 題に未コミットの ● と ahead/behind | ✓ | ✓ | n/a | n/a | GTK4: タブの名前に出る。 |
| 最後の窓を閉じてもアプリは終わらない | ✓ | — | n/a | n/a | GTK4: GNOME のアプリと同じく終わる。 |

## 2. ブックマーク（リポジトリの一覧）

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| All・Favorites・Places・グループのあるリポジトリのページ | ✓ | △ | n/a | n/a | GTK4: タブのページではなく別の Repositories の窓。 |
| 手元・SSH のリポジトリを足す | ✓ | ✓ | n/a | n/a |  |
| この機械の探すフォルダー（深さ・見張り） | △ | △ | n/a | n/a | macOS: 除くものは決まった一覧。<br>GTK4: 除くものは決まった一覧で、フォルダーを見張らない。 |
| SSH 先の探すフォルダー | ✓ | △ | n/a | △ | GTK4: Repositories で足す。SSH Hosts のページにはない。<br>CLI: `find-repositories` がアプリのために探す。 |
| 場所の下のリポジトリのフォルダーの木 | ✓ | — | n/a | n/a |  |
| 手で作るグループ・行を Favorites やグループへドラッグ | △ | △ | n/a | n/a | macOS: グループそのものはドラッグで並べ替えられない。<br>GTK4: ドラッグはない。 |
| 状態の列（ブランチ・変更・ahead/behind・最後のコミット・開いた時）、並べ替え | ✓ | ✓ | n/a | n/a |  |
| お気に入り・表示名・色のラベル | ✓ | ✓ | n/a | n/a |  |
| タブ・窓で開く、Finder（ファイル）・Terminal | △ | △ | n/a | n/a | macOS: Open in Editor はない。<br>GTK4: Open in Editor はない。 |
| 選んだリポジトリを Fetch | ✓ | ✓ | n/a | n/a |  |
| Quick Open（あいまい検索） | ✓ | ✓ | n/a | n/a | GTK4: 新しい窓で開く。 |
| ツールバーのリポジトリ名で切り替える | ✓ | — | △ | n/a | VSCode: Select Repository でワークスペースのリポジトリから選ぶ。 |
| Clone（自分の GitHub のリポジトリから選ぶ） | ✓ | ✓ | n/a | △ | VSCode: VSCode 自身の Git: Clone。<br>CLI: `clone <url>` だけ。 |
| 新しいリポジトリの Init | — | — | n/a | — | macOS: docs/ux.md の File メニューにあるが、作っていない。 |

## 3. Settings

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| General: 開いたリポジトリをブックマークする | ✓ | ✓ | n/a | n/a |  |
| General: 起動時に開き直す・既定はタブか窓か | — | — | n/a | n/a | macOS: いつも開き直し、切り替えはない。<br>GTK4: いつも開き直し、切り替えはない。 |
| General: 自動で fetch | ✓ | ✓ | — | n/a |  |
| General: 外のエディター | ✓ | ✓ | n/a | n/a | GTK4: コードエディターは desktop のエントリーで探す。Other… ではプログラムを選ぶ。 |
| General: 端末と diff のツール | — | — | n/a | n/a |  |
| Git: 機械全体のページ（Author and Signing・Commits・Operations・Working Copy・Keys and Values） | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| Git > Commits: 共有のメッセージのテンプレート | ✓ | ✓ | — | — |  |
| Git > Programs: git・gpg・ssh-keygen・gpgsm | ✓ | ✓ | — | △ | CLI: `amonhen run`（`programs`・`setMachineGit`）から。 |
| Git > この機械の Environment Variables（渡す・秘密を隠す） | ✓ | ✓ | — | n/a |  |
| Environment Variables: ログインシェルのものを使う（Read Again・SSH agent） | ✓ | — | — | n/a | GTK4: 外した。Linux のセッションはすでに持っている。 |
| SSH Hosts: + / − の付いたホスト、届かないものは薄く | ✓ | ✓ | n/a | n/a | VSCode: Remote-SSH が受け持つ。 |
| SSH Hosts > Connection: 状態・Check Connection・エンジン | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts > Connection: 署名の確かめ方 | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts > Connection: 探すフォルダー | ✓ | — | n/a | n/a |  |
| SSH Hosts: ホストのすべてのリポジトリの git のページ | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts: ホストの Environment Variables（ログインシェルの切り替え） | ✓ | ✓ | n/a | n/a |  |
| ホスト・この機械の列で選ぶパスのシート | ✓ | — | n/a | n/a | GTK4: ホストのパスは打ち込み、手元は GTK のファイルのダイアログ。 |
| Accounts: GitHub（CI の状態・アバター） | ✓ | ✓ | — | n/a |  |
| Appearance: History の順・1 行・行の項目・アバター | ✓ | ✓ | n/a | n/a |  |
| Appearance: テーマ・フォント・グラフの色 | — | — | n/a | n/a |  |
| Diff & Merge: 開いたときに迷わないコンフリクトを解く | ✓ | ✓ | n/a | n/a |  |
| Diff & Merge: unified/split・空白・前後の行・折り返し・タブの幅 | — | — | — | n/a |  |
| AI: 使う AI・CLI の場所と版・モデル・言語・リポジトリごとの許可 | ✓ | △ | — | n/a | GTK4: ログインの状態は出ない。 |
| 目的ごとの AI のプロンプト（編集・既定に戻す・試す） | ✓ | ✓ | — | n/a |  |
| Performance: commit-graph・共有の daemon | ✓ | ✓ | △ | n/a | VSCode: VSCode の設定の `amonhen.daemon.enabled`・`amonhen.daemon.path`。 |
| Performance: fsmonitor | ✓ | — | — | n/a |  |
| Performance: 永続キャッシュの場所・大きさ・消す | — | — | — | △ | CLI: `log --rebuild`・`daemon rebuild`。 |
| 設定の変更の Undo・Redo | △ | △ | n/a | n/a | macOS: Git のページとリポジトリの設定だけ。<br>GTK4: Git のページと Keys and Values だけ。 |

## 4. リポジトリの設定

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 設定のページ（ページの一覧・git の行・行ごとの出どころ・↩︎・ⓘ） | ✓ | ✓ | — | n/a |  |
| Author and Signing（署名の選択・集めた鍵・mailmap） | ✓ | ✓ | — | △ | CLI: `amonhen run`（`signingKeys`・`configSet`）から。 |
| Commits: メッセージのテンプレート（なし・共有・独自・ファイル） | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| Branches: 既定のブランチを選ぶ | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| Operations: pull・fetch.prune・autostash・updateRefs・コンフリクトの書き方・rerere | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| Working Copy: 改行・実行ビット・`.git/info/exclude` の編集 | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| Remotes: 表と、つながるか確かめるシート | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| Hooks: 表・Enabled・スクリプト・Create Hook… | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| LFS: 入っているか・パターン・Track Pattern…・ファイルの数 | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| Keys and Values（git config の表、Basic / Customized / All、Combined / Levels） | ✓ | ✓ | — | △ | CLI: `amonhen run`（`configList`・`configSet`）から。 |
| 要る git の版を添えて行を無効にする | ✓ | ✓ | — | n/a |  |

## 5. ツールバー

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| ブランチの切り替え（あいまい検索・最近・追跡・新しいブランチ） | ✓ | ✓ | △ | △ | VSCode: サイドバーからの Checkout と New Branch… だけ。<br>CLI: `amonhen run` から。 |
| Fetch のシート（すべてか 1 つのリモート、prune を示す） | ✓ | ✓ | △ | △ | VSCode: シートなしで、すべてを prune 付きですぐ fetch。<br>CLI: `amonhen run` から。 |
| Pull のシート（コミット・merge / rebase / ff-only・なければ上流） | ✓ | ✓ | △ | △ | VSCode: シートも選択もなく、上流をマージする。<br>CLI: `amonhen run` から。 |
| Push のシート（注意・コミットと差分・lease 付きの Force Push） | ✓ | ✓ | △ | △ | VSCode: シートはなく、Force Push は別に尋ねる。<br>CLI: `amonhen run` から。 |
| Publish（リモートと名前を選ぶ） | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| Merge（no-ff・squash の選択） | △ | △ | △ | △ | macOS: no-ff・squash の選択はない。<br>GTK4: no-ff・squash の選択はない。<br>VSCode: メニューかドラッグで、選択はない。<br>CLI: `amonhen run` から。 |
| いくつもを 1 つにマージ（順番・コンフリクトの見込み） | ✓ | ✓ | ✓ | △ | CLI: `merge-check` で確かめ、マージは `amonhen run` から。 |
| 前と後を描く Rebase のシート | ✓ | ✓ | △ | △ | VSCode: 書き換えの見込みを文で示すダイアログ。<br>CLI: `rewrite-preview` が出し、rebase は `amonhen run` から。 |
| Stash（メッセージ・未追跡を含める） | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| Stash: ステージしたものだけ・選んだファイルだけ | — | — | — | — |  |
| 最新の stash を Pop（先に尋ねる） | ✓ | ✓ | — | △ | VSCode: stash の行ごとに Pop し、尋ねない。<br>CLI: `amonhen run` から。 |
| Activity のくるくると数 | ✓ | ✓ | △ | n/a | VSCode: VSCode の進み具合の知らせ。 |
| Refresh と Rebuild History | ✓ | ✓ | △ | △ | VSCode: Refresh だけ。<br>CLI: `log --rebuild`・`daemon rebuild`。 |
| 直前の操作の Undo（尋ね、何を戻すか示す） | ✓ | ✓ | △ | ✓ | VSCode: 尋ねずに戻す。<br>CLI: `undo`（先に見るなら `--peek`）。 |
| 範囲付きの History の検索（件名・作者・ID・パス・中身） | ✓ | ✓ | — | ✓ | GTK4: 範囲は検索の書き方で決める。<br>CLI: `log <query>`。 |
| ahead/behind のバッジ | ✓ | ✓ | △ | △ | VSCode: サイドバーのブランチの行に出る。<br>CLI: `status` に出る。 |
| ツールバーのカスタマイズ | ✓ | — | n/a | n/a |  |
| 120 秒黙った network の git を止める | ✓ | ✓ | — | ✓ | VSCode: 拡張自身の git 実行層にはない。 |

## 5a. Stashes

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Stashes の表示（一覧と中身） | ✓ | ✓ | △ | △ | VSCode: サイドバーの一覧で、Show Stash は VSCode の複数ファイルの差分。<br>CLI: `refs` が並べ、`show stash@{n}`。 |
| stash の行の詳細（ブランチ・HEAD に対する元・未追跡） | ✓ | ✓ | △ | △ | VSCode: 名前と件名だけ。<br>CLI: `amonhen run`（`stashBases`）から。 |
| Apply・Pop・Drop…（Undo できる）・Branch… | ✓ | ✓ | △ | △ | VSCode: Branch… も `--index` もない。<br>CLI: `amonhen run` から。 |

## 5b. Reflog

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 「どのブランチにもない」の付いた Reflog の表示 | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| Reflog の操作（New Branch・Checkout・New Tag・Reset・Show in History・Copy） | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |

## 5c. Reset

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| コミットへ Reset（Soft / Mixed / Hard、Hard は先に stash、Undo できる） | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |

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
| 1 つのチェックボックスの一覧（☑ ▣ ☐、見出し・フォルダーのチェック） | ✓ | ✓ | △ | n/a | VSCode: Staged と Changes が別のグループ。 |
| List / Tree / Flat | ✓ | ✓ | — | n/a |  |
| パスで絞り込む | ✓ | ✓ | — | n/a |  |
| 行ごとの +/− の数と、ホバーで Discard | ✓ | ✓ | △ | △ | VSCode: 行の数はない。<br>CLI: `diff --stat`。 |
| Space で切り替え、⌫ で捨てる | ✓ | ✓ | △ | n/a | VSCode: Space で切り替えるが、⌫ はない。 |
| 上のコミットの欄、50/72 の数と本文の目安の線 | ✓ | ✓ | △ | n/a | VSCode: 数も目安の線もない。 |
| リポジトリごとの下書き、テンプレートから始める | ✓ | ✓ | — | n/a |  |
| 最近のメッセージ（⌥↑ / ⌥↓・一覧） | ✓ | ✓ | — | n/a |  |
| Conventional の種類・Co-authored-by・Signed-off-by を入れる | ✓ | ✓ | — | n/a |  |
| `#issue` の補完 | — | — | — | n/a |  |
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
| コンフリクトのファイルからマージエディターを開く | ✓ | △ | △ | n/a | GTK4: 別の窓で開く。<br>VSCode: VSCode のマージエディターを開く。 |

## History

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| ブランチ・タグのラベル付きのグラフ | ✓ | ✓ | ✓ | ✓ | CLI: `log`。 |
| 大きな履歴をかたまりで読み、永続キャッシュを使う | ✓ | ✓ | ✓ | ✓ |  |
| 一致の数・次 / 前・一致だけの検索 | ✓ | ✓ | — | △ | CLI: `log <query>` が一致を並べる。 |
| 表示: 順・1 行・行の項目・マージ済みを畳む・祖先を目立たせる | ✓ | ✓ | △ | △ | VSCode: 範囲（今・すべて・ref）だけ。<br>CLI: `--topo-order`・`--fold`。 |
| アバター | ✓ | △ | — | n/a | GTK4: 詳細にだけ出し、メモリーに持つ。 |
| コミットの署名の印 | ✓ | ✓ | — | △ | CLI: `show` に出る。 |
| コミットごとの CI の状態とポップオーバー | ✓ | ✓ | — | — |  |
| worktree の HEAD のラベル（✎ ⌂ ⑂・変更の数） | ✓ | ✓ | — | — |  |
| Go to HEAD・Parent・Child | ✓ | ✓ | △ | n/a | VSCode: 詳細の親へのリンクだけ。 |
| Go to Ref | — | — | — | n/a |  |
| いくつものコミットを選ぶ（範囲・2 つ・飛び飛び） | ✓ | ✓ | — | △ | CLI: 2 つは `diff <rev> <rev>`。 |
| 選んだいくつもの ref の History | ✓ | ✓ | — | ✓ | CLI: `log --branch` を重ねる。 |
| コミットのメニュー: checkout・新しいブランチやタグ・cherry-pick・revert・ID のコピー | ✓ | ✓ | △ | △ | VSCode: コミットの checkout はない。<br>CLI: `amonhen run` から。 |
| 編集モード（keep・reword・squash・fixup・drop・edit、ドラッグで並べ替え） | ✓ | ✓ | △ | △ | VSCode: 1 つずつで、いくつもの段の計画はない。<br>CLI: `amonhen run` から。 |
| 書き換えの見込み（積み重ね・push 済みのブランチ、コンフリクトの見込み） | ✓ | ✓ | △ | ✓ | VSCode: daemon 越しで、今のブランチだけ。<br>CLI: `rewrite-preview`。 |
| 近道: Edit Message…・Combine with Older…・Drop…・Combine N…・コミットのドラッグ | ✓ | ✓ | △ | △ | VSCode: Combine N Commits はない。<br>CLI: `amonhen run` から。 |
| コミットを分ける | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| 編集でマージを保つ（`--rebase-merges`） | ✓ | ✓ | — | △ | VSCode: 最初のマージで止まる。<br>CLI: `amonhen run` から。 |

## 7. History の右の列

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| メッセージ全体・ドラッグできる境・⤢ で広げる | ✓ | ✓ | △ | n/a | VSCode: ⤢ はない。 |
| 詳細の中を探す | ✓ | ✓ | — | n/a |  |
| すべてのファイルを積む（貼り付く見出し・畳む・Viewed） | ✓ | ✓ | △ | ✓ | VSCode: 積んでスクロールで読むが、畳むことも Viewed もない。<br>CLI: `show`。 |
| 差分の横の、変更したファイルの木 | ✓ | ✓ | △ | n/a | VSCode: 差分の上の平らな一覧。 |
| ファイルの見出しのメニュー（Blame・File History・パスのコピー・開く） | ✓ | ✓ | △ | n/a | VSCode: VSCode の差分のエディターで開くだけ。 |
| 1 ファイルずつの差分に切り替える | ✓ | ✓ | — | n/a |  |
| 選んだものの差分（範囲をまとめる・A..B / A...B・飛び飛び） | ✓ | ✓ | — | △ | CLI: `diff <rev> <rev>`。 |
| 選んだものの操作（cherry-pick・revert・squash・ID のコピー） | ✓ | ✓ | — | △ | CLI: `amonhen run` から。 |
| 署名のポップオーバー | ✓ | ✓ | — | n/a |  |
| 単語の強調と構文の色 | ✓ | ✓ | △ | n/a | VSCode: 単語だけ。 |

## Blame・File History

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Blame | ✓ | ✓ | — | △ | CLI: `amonhen run`（`blame`）から。 |
| File History | ✓ | ✓ | — | △ | CLI: `amonhen run`（`fileHistory`）から。 |
| 行の履歴 | — | — | — | — |  |

## 7a. パッチ

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Save as Patch（コミットごとか mbox・カバーレター・-v） | ✓ | ✓ | △ | ✓ | VSCode: `-v` はない。<br>CLI: `format-patch`。 |
| Apply Patch（am / apply・下見・3-way・--index） | ✓ | ✓ | ✓ | ✓ | CLI: `am`・`apply`（`--check`）。 |
| パッチのファイルを窓に落とす | ✓ | ✓ | △ | n/a | VSCode: エクスプローラーのメニューから。 |
| 止まった `am` と Retry with 3-Way・Skip・Abort・Continue | ✓ | ✓ | ✓ | ✓ | VSCode: サイドバーの行に出る。<br>CLI: `am --continue / --skip / --abort / --retry-3way`。 |

## 8. マージエディター

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 帯でつないだ 3 つの列（Current / Result / Incoming） | ✓ | △ | n/a | n/a | GTK4: 埋め込みではなく別の窓。<br>VSCode: VSCode 自身の 3-way のマージエディター。 |
| Show Base | ✓ | ✓ | n/a | n/a |  |
| Magic Merge（ファイルを開いたときの確かな規則も） | ✓ | ✓ | — | ✓ | CLI: `merge-file`（git の mergetool にも）。 |
| 直せる Result とコンフリクトごとの Undo | ✓ | ✓ | n/a | n/a |  |
| ファイルを Current / Incoming で解く | ✓ | ✓ | ✓ | △ | CLI: `amonhen run` から。 |
| 外のマージツールで開く | — | — | n/a | n/a |  |
| エディターを別の窓に出す | ✓ | n/a | n/a | n/a | GTK4: いつも別の窓。 |
| Continue / Skip / Abort | ✓ | ✓ | ✓ | △ | VSCode: サイドバーの進行中の行に出る。<br>CLI: `amonhen run` から。 |

## 9. AI

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Claude Code と Codex | ✓ | ✓ | — | — |  |
| Apple Intelligence（端末の中） | ✓ | n/a | n/a | n/a |  |
| コミットのメッセージを翻訳 | ✓ | ✓ | — | — | GTK4: メッセージの下にそのまま出す。 |
| コミット・選んだものを解説 | ✓ | ✓ | — | — | GTK4: そのまま出す。 |
| コミットのメッセージを提案 | ✓ | ✓ | — | — |  |
| 差分の hunk を解説 | ✓ | ✓ | — | — |  |
| AI でコンフリクトを解く | ✓ | △ | — | — | GTK4: 1 つずつで、帯として出す。 |
| すべてのファイルを AI で解く | ✓ | — | — | — |  |
| コンフリクトを解説 | ✓ | ✓ | — | — |  |
| リポジトリごと・リポジトリのプロンプトの初回の確認 | ✓ | ✓ | — | — |  |
| 流れてくる結果とキャッシュ | ✓ | ✓ | — | — |  |

## 10. Activity

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Operations（git の命令・出力・進み具合・取り消し） | ✓ | ✓ | △ | △ | VSCode: Output のチャンネルの記録だけ。<br>CLI: `amonhen run`（`remoteActivity`・`cancelActivity`）から。 |
| History（操作ログ・ここまで戻す） | ✓ | ✓ | — | △ | CLI: `undo --list` が記録を出す。 |
| Performance | ✓ | ✓ | — | n/a |  |
| Events | ✓ | ✓ | — | n/a |  |
| JSON で書き出す | ✓ | ✓ | — | n/a |  |
| Daemon のタブ（起動・停止・つながり） | ✓ | ✓ | — | △ | CLI: `daemon status / start / stop`。 |

## 11. メニュー

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 共有の操作の表（`crates/commands`）から作るメニュー | ✓ | ✓ | — | n/a | VSCode: package.json に書く。 |
| メニューバーなしで全部の操作に届く（≡） | ✓ | ✓ | △ | n/a | VSCode: コマンドパレットとコンテキストメニュー。 |
| キーボードの近道 | ✓ | ✓ | — | n/a | VSCode: キーの割り当てを出していない。 |
| メニューと同じ名前のコンテキストメニュー | ✓ | ✓ | △ | n/a | VSCode: 名前がアプリと違う。 |
| キーボードの近道の窓 | — | ✓ | n/a | n/a | macOS: 代わりに macOS の App Shortcuts で変える。 |

## ステータスバー

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 進行中の操作と Continue / Skip / Abort | ✓ | ✓ | △ | n/a | VSCode: サイドバーの行。 |
| 失敗とその原因・Details | ✓ | ✓ | △ | n/a | VSCode: エラーの知らせだけ。 |
| 切れたつながりと Reconnect | ✓ | ✓ | n/a | n/a |  |
| 終わった操作が何をしたか、数秒 | ✓ | ✓ | △ | n/a | VSCode: ステータスバーの文か、Undo 付きの知らせ。 |

## 確認と Undo

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| リモート・HEAD を動かす・消す操作の前のシート | ✓ | ✓ | △ | n/a | VSCode: 消す・書き換えるときだけで、Fetch・Pull・Push・Merge・Checkout にはない。 |
| git の操作の Undo（`undo.json`） | ✓ | ✓ | ✓ | ✓ |  |
| 押せない部品はツールチップで理由を言う | ✓ | △ | — | n/a | GTK4: ツールバーの Push と Undo、新しい git の要る設定の行だけ。ほかはまだ。 |

## 12. GitHub

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| `gh` を通す GitHub | ✓ | ✓ | — | — |  |
| Pull Requests（CI・draft・レビュー、checkout・開く・URL のコピー） | ✓ | ✓ | — | — |  |
| Create Pull Request（base・draft） | ✓ | ✓ | — | — | macOS: base は打ち込む（最初は既定のブランチ）。<br>GTK4: base は打ち込む（最初は既定のブランチ）。 |
| ブランチの状態のポップオーバー（CI・プルリクエスト） | ✓ | ✓ | — | — |  |
| Open on GitHub | ✓ | ✓ | — | — |  |
| メッセージの `#123` のリンク | ✓ | ✓ | — | — |  |
| GitHub のアバター | ✓ | △ | — | — | GTK4: メモリーにだけ持つ。 |

## 13. 言語

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| 英語の UI と日本語の訳 | ✓ | ✓ | — | ✓ | VSCode: 訳していない。日本語の文もまじる。 |

## 14. Worktree

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| worktree と変更の数 | ✓ | ✓ | — | ✓ | CLI: `worktree list --changes`。 |
| 書き込む先の worktree（✎）と切り替え | ✓ | ✓ | — | n/a | CLI: 動かした worktree で働く。 |
| New Worktree…（ブランチ・コミットから） | ✓ | ✓ | — | ✓ |  |
| Remove Worktree（尋ね、変更があれば `--force`） | ✓ | △ | — | ✓ | GTK4: main と開いたものでは出さない。 |
| worktree の Prune | ✓ | ✓ | — | ✓ |  |
| worktree のフォルダーを開くと、そのリポジトリでそれを書き込む先にする | ✓ | ✓ | — | n/a |  |

## SSH 先・共有の daemon・askpass

| 機能 | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| SSH 先のリポジトリ（ssh 越しのエンジン） | ✓ | ✓ | n/a | ✓ | VSCode: Remote-SSH が拡張をホストで動かす。<br>CLI: `-C ssh://host/path`。 |
| 共有の daemon を通す | ✓ | ✓ | ✓ | ✓ | CLI: `-C daemon:<path>`。 |
| 合うものがなければホストにエンジンを送る | ✓ | ✓ | △ | ✓ | VSCode: 拡張が動くホストで、同梱したエンジンを使う。 |
| Windows の SSH 先 | ✓ | ✓ | ✓ | ✓ |  |
| パスワード・パスフレーズを尋ねる askpass のダイアログ | ✓ | ✓ | — | n/a | VSCode: 尋ねるのを切っている（`GIT_TERMINAL_PROMPT=0`）。<br>CLI: ダイアログはアプリのもの。 |
| 署名の確かめ（この機械か、ホストの鍵） | ✓ | ✓ | — | △ | CLI: `show`・`refs --verify` で確かめ、選択はない。 |
