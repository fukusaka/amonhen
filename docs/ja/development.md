# 開発

[English](../development.md)

ビルド・テストと、確認用の起動オプション。設計は [design.md](design.md)、作業の決まりは [CLAUDE.md](../../CLAUDE.md) を参照。Windows の実機の準備と操作は [windows.md](windows.md)。

## ビルドとテスト

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
scripts/build-core.sh && (cd apps/macos && swift test)
scripts/build-engine.sh          # dist/engine にエンジン（macOS・Linux。Linux 用は brew install zig が要る）
scripts/build-app.sh snapshot   # dist/ に .app と zip を作る（compile-app.sh・Linux のエンジン・package-app.sh）
scripts/check-engine.sh dist/engine/amonhen-darwin-aarch64   # 作ったエンジンを一度動かす（版、このリポジトリを読めるか）
scripts/third-party-notices.sh   # dist/THIRD-PARTY-NOTICES.md（cargo install --locked cargo-about --features cli）
cargo deny check licenses        # 依存が deny.toml で許したライセンスだけか（cargo install --locked cargo-deny）
cargo build -p amonhen-cli && (cd apps/vscode && npm ci && npm test)   # VSCode 拡張
(cd apps/vscode && npm run test:e2e)   # 実際の VSCode での VSCode 拡張（上の行のあとで）
(cd apps/gtk4 && cargo clippy --all-targets -- -D warnings && cargo run --release -- <repo>)   # GTK4 のアプリ（GTK 4.14 以降が要る。libgtk-4-dev か brew install gtk4 pkgconf）
scripts/package-gtk4.sh          # GTK4 のアプリを dist/amonhen-gtk-<版>-linux-<arch>.tar.gz に（Linux で。dist/engine の Linux のエンジンがあれば入れる）
```

開発中のビルド（`apps/macos/.build` から起動したアプリ、`target/*/amonhen`）は、エンジンを自分のチェックアウトの
中だけで探す。SSH 先へ送るエンジンは `dist/engine`、共有の daemon は `target/release/amonhen`。worktree ではそこで
作る。別のビルドかもしれない本体のチェックアウトのものは拾わない。

## 確認用の起動オプション

`AmonHen <repo> --snapshot out.png [--select-first | --select-row N] [--working-copy] [--find TEXT] [--search TEXT] [--merge-editor PATH] [--app-settings [--app-settings-tab NAME] [--settings-page NAME]] [--snapshot-after SECONDS]`（`--app-settings` は Settings ウィンドウを英語の名前のタブで撮る。`--settings-page` は Git か SSH Hosts のページ、`--snapshot-after` はその秒数だけ待つ。SSH 先につながるのを待つときなど）、`--blame PATH`・`--file-history PATH`（そのウィンドウを開く）（画面を PNG に書き出して終了）、`--measure-scroll [--scroll-rows N]`（History を 600 フレーム自動でスクロールして速さと処理ごとの内訳を表示）、`--measure-select N`（コミットを N 個選び、詳細が出るまでの段階ごとの時間を表示）、`--log-scroll`（トラックパッドでのスクロールの減速を記録）、`--show-latency`（入力から表示までの時間をタイトルの下に出す）。`--select-first`・`--select-row N` は `--snapshot` なしでも効く。

README の画面は、`--snapshot`（ツールバーが空のまま写る）ではなく本物のウィンドウを撮る: デバッグ版をリポジトリと
`--select-row N` で起動し、`screencapture -o -l<ウィンドウの ID>` でウィンドウを撮る。アバターを出さないときは、先に
`defaults write AmonHen showAvatars -bool NO`（デバッグ版の設定のドメイン）を書き、撮ったら消す。ウィンドウの大きさは同じ
ドメインの `NSWindow Frame Repository <パス>` で決まる（README は 1200×720: `"1300 500 1200 720 0 0 <画面の幅> <画面の高さ> "`）。

GTK4 のアプリ（`apps/gtk4`）: `amonhen-gtk <repo> --snapshot out.png` は、見えている行を読んだら窓を PNG に書き出して終了する（macOS で真っ白になるときは `GSK_RENDERER=cairo`）。写すものは次のフラグで用意する。

- History: `--scroll-to N`、`--select-rows N,N,...`、`--select-ref NAME`、`--select-refs NAME,NAME,...`（その ref の History）、`--find-message TEXT`（詳細の中を探す）、`--search QUERY [--only-matching]`、`--menu-ref NAME` か `--menu-row N`（その ref・行の右クリックのメニュー。`--menu-item TITLE` で項目を選ぶ。サブメニューは ` > ` でつなぐ）、`--open-menu repository|primary|history-display|repository-name`（`repository-name` では `AMONHEN_CHOOSE_REPOSITORY=NAME` でその表示名の行を開く）、`--ci-row N`（CI の状態を読んだら、その行の CI の印を押す）、`--drop-commit FROM:TO`（そのコミットを TO の行の上へドラッグする）、`--find-diff TEXT`、`--toggle-viewed N`、`--diff-file-menu N`、`--signature-popover`、`--edit-commits ID [--edit-plan pick,squash,drop,...] [--edit-move FROM:ABOVE] [--edit-apply]`（History の編集モード）。
- サイドバー: `--sidebar-menu SPEC`（ref・リモート・見出し・worktree・`pr/<番号>`。`a,b` は複数選択。`--menu-item` も使える）、`--sidebar-mark SPEC`（その行の印の吹き出し）、`--filter-sidebar WORDS`、`--sidebar-mark signature:TAG`（タグの署名の吹き出し）、`--conflicts`（Conflicts の行）。
- ページ: `--working-copy [--select-path PATH] [--toggle-row N] [--layout list|tree|flat]`（`--toggle-row` はその差分の行の印を一度クリックしたことにする。`--keys list:space,diff:j,...` はそのキーを 1 つずつ押す: `list:space`・`list:delete`・`diff:space`・`diff:j`・`diff:k`・`diff:alt+j`・`diff:alt+k`・`commit`）、`--stashes [--stash-button apply|pop|drop|branch]`、`--reflog`、`--range-diff[=RANGES] [--select-pair N] [--menu-pair N]`、`--repository-settings PAGE`、`--edit-worktree PATH`（その worktree を書き込む先にする）。
- タブ: `--tab LOCATION`（繰り返せる）は最初の窓に裏のタブを足す。ほかのフラグは最初のタブに効く。`--connecting` は 1.5 秒後につなぎ中のページを撮る（遅い `AMONHEN_SSH` と使う）。
- 窓: `--blame PATH`、`--file-history PATH`、`--activity operations|history|performance|events|daemon`、`--settings general|git[/PAGE]|ssh-hosts[/HOST/PAGE]|accounts|appearance|diff-merge|ai|performance`、`--merge-editor PATH [--accept ours|theirs|both|base]`、`--apply-patch PATH,...`、`--repositories [--menu-row N [--menu-item TITLE]]`、`--quick-open QUERY`。
- パスを選ぶシート: `AMONHEN_PICKER_SNAPSHOT=PNG` はシートが出て 1.5 秒後に撮って取り消す（`AMONHEN_PICKER_CHOOSE` があれば決める）。`AMONHEN_SCAN_ADD` は SSH 先の探すフォルダーの ＋ を一度押す（`--settings ssh-hosts/HOST/connection` と使う）。
- 動かす: `--perform JSON`（コアの命令を 1 つメニューと同じ経路で実行）、`--action ID,...`（コマンドの表の操作。700 ms ずつ空けて順に）、`--print-file-menu PATH`（Working Copy のファイルのメニューを出力）、`--switch-query TEXT [--switch-enter]`（`--action switch-branch` と使う。ブランチの切り替えに打ち込み、選んだ行で切り替える）、`--busy`（操作が動いているものとして Activity を回す）、`--ai translate,explain,hunk,working-hunk,suggest,use,resolve,explain-conflict [--ai-allow]`（その AI のボタンを押せるようになったら押す。`--ai-allow` はリポジトリごとの確認を飛ばす。`gtk.ini` の `assistantPath.<cli>` で CLI の場所を指せる。試し用の偽の CLI など）。`AMONHEN_SETTINGS_SCRIPT` は設定のページ（`--settings git/PAGE`・`--repository-settings`・SSH 先のページ）で手順を動かす: `switch:NAME=on|off`、`git=PATH`、`revert:git` ほか（`reposettings.rs`）。

`--measure-scroll [--scroll-rows N]` は History を 1 フレームに 40 行（か N 行）ずつ 600 フレームスクロールし、1 フレームのメインスレッドの時間・History の snapshot の時間・落としたフレーム・フレームの始まりから画面に出るまでの時間を表示する。描画器は `GSK_RENDERER=cairo|gl|vulkan` で選ぶ。

- `--measure-select N` は History の行を N 個順に選び、差分が出るまでの段階ごとの p50・p95 を表示する。
- `--print-menus` は Repository メニューと主メニュー（≡）を 1 項目 1 行で出力し、窓を作らずに終える。
- `AMONHEN_HANG_LOG=DIR` はメインループが 0.15 秒答えないたびに、スタックを `DIR/hang-<時刻>.txt` に、1 行を `DIR/hangs.log` に書く（macOS は `sample`、ほかは `gdb`）。
- `AMONHEN_DAEMON=PATH` は同梱・置き場所のものではなく、そのエンジンを共有の daemon にする。

## VSCode 拡張

設計は [vscode.md](vscode.md)。

エンジンはリポジトリのルートで作る（`cargo build -p amonhen-cli`。テストはそれを使う）。`bin/` に同梱するエンジンは、
ルートで `scripts/build-engine.sh`、`apps/vscode` で `scripts/bundle-engine.sh` を実行して写す。

```sh
cd apps/vscode
npm install
npm run build      # dist/extension.js を生成
npm run typecheck
npm test
npm run test:e2e   # 実際の VSCode で（VSCode 1.120.0 を .vscode-test/ に落とす）
```

`npm run test:e2e`（`e2e/run.mjs`）は、拡張をビルドして一時ディレクトリに `.vsix` と同じ形で並べ（`bin/` にルートで
作ったエンジン）、そこに `core.autocrlf=true` でリポジトリを clone して（ファイルは CRLF になる）、拡張を読み込んだ
VSCode で開く（`@vscode/test-cli`、設定は `.vscode-test.mjs`）。エンジンの置き場所、共有の daemon のソケットとキャッシュ、
`HOME` は一時ディレクトリの中に向けるので、手元で動いている daemon やアプリには触らない。テスト（`e2e/*.test.ts`、
mocha）は拡張ホストで動く。メインビューの Webview が送るはずのメッセージを拡張ホストに渡して画面を動かし（`activate`
がパネルを返す）、送り返されるメッセージとリポジトリの様子を git で確かめる。同梱のエンジンでの起動、共有の daemon を
通した History、CRLF のファイルでの行単位の stage・行の破棄・Undo・commit（daemon で、と `amonhen.daemon.enabled` を
切って）、別のつながりと CLI との 1 つの daemon の共有、拡張が起動するプロセスがすべて `windowsHide` であること、リポジトリを閉じると git のプロセスが終わることを
確かめる。VSCode を落としたあとは、1 回 10 秒ほど。画面のない Linux では
`xvfb-run -a npm run test:e2e`（CI と同じ）。VSCode は PATH ではなく自分で git を探すので、別の git で流すとき
（CI の `git-minimum` のジョブはサポートする最低の版で流す）は `AMONHEN_E2E_GIT` に渡す。リポジトリをその git で作り、
VSCode の `git.path` をその git にする。

VSCode で F5 を押すと、拡張を読み込んだウィンドウが開く。Output の「Amon Hen」でログレベルを Trace にすると、git コマンドごとの所要時間が出る。

`apps/vscode` の下のディレクトリ:

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
| `e2e/` | 拡張を読み込んだ実際の VSCode で動くテスト（`npm run test:e2e`） |
| `scripts/` | エンジンを `bin/` に写す処理、snapshot のバージョン計算、ssh から Windows のデスクトップで `test:e2e` を走らせる処理 |
