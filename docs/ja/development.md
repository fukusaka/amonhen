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

GTK4 のアプリ（`apps/gtk4`）: `amonhen-gtk <repo> --snapshot out.png [--scroll-to N | --select-rows N,N,... | --select-ref NAME] [--menu-ref NAME | --menu-row N] [--search QUERY [--only-matching]] [--blame PATH | --file-history PATH | --activity operations|history|performance|events|daemon | --settings git|appearance|performance|advanced | --merge-editor PATH [--accept ours|theirs|both|base]] [--open-menu repository|primary] [--perform JSON] [--working-copy [--select-path PATH] [--toggle-row N] [--layout list|tree|flat]]`（`--menu-ref` はその ref の右クリックのメニューを開く、`--perform` はコアの命令を 1 つメニューと同じ経路で実行する、`--toggle-row` はその差分の行の印を一度クリックしたことにする）（見えている行を読んだら窓を PNG に書き出して終了）、`--measure-scroll [--scroll-rows N]`（History を 1 フレームに 40 行（か N 行）ずつ 600 フレームスクロールし、1 フレームのメインスレッドの時間・History の snapshot の時間・落としたフレーム・フレームの始まりから画面に出るまでの時間を表示。描画器は `GSK_RENDERER=cairo|gl|vulkan` で選ぶ）。

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
`xvfb-run -a npm run test:e2e`（CI と同じ）。

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
