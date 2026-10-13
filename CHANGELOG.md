# Changelog

リリースごとに、利用者から見た変更をまとめる。まだリリースしていない変更は `Unreleased` に書き、リリースするときに `## [x.y.z] - YYYY-MM-DD` の見出しに改める。

## [Unreleased]

### Added

- メインビュー。エディタ領域の 1 つの Webview に、左のナビ（Working Copy / History / ブランチ類）と右の画面をまとめる。アクティビティバーの Amon Hen アイコンで開く。
- Working Copy: 変更ファイルの一覧、hunk ごとの差分（単語単位の強調付き）、行・hunk・ファイル単位の stage / unstage、複数行のコミット欄と amend。
- History: コミットのグラフと、選んだコミットの詳細（メッセージ、作者、親、変更ファイルとその差分）。履歴は 200 件ずつ、差分は画面に入ったファイルから読み込む。
- History の右クリックから、コミットを Reword / Squash / Fixup / Edit / Split / Drop / 並べ替えする（ドラッグでも並べ替えられる）。
- ナビの Branches / Remotes / Tags / Stashes / Submodules。ブランチを選ぶとその履歴を History に出し、ダブルクリックで切り替える（リモートブランチなら追跡するブランチを作る）。右クリックで Checkout / Merge / Rebase / Delete。
- ブランチのドラッグ＆ドロップで Merge / Rebase / Push / Force Push する。ドロップ先が checkout されていなくても、fast-forward で済むなら checkout せずに進める。
- ナビのツールバーで Fetch / Pull / Push / Undo。
- 直前の操作（コミット、Merge、Rebase、fast-forward、Pull、ブランチの削除、コミットの編集）を Undo する。記録は VSCode を再起動しても残る。
- merge や rebase が途中で止まっているとき、進み具合を表示し、Continue / Skip / Abort できる。
- コンフリクトしたファイルを標準の 3-way マージエディタで開く。Accept Current / Accept Incoming でファイルごとに解決でき、解決済みにするときにマーカーが残っていれば確認する。
- サイドバーの Changes ビュー。変更ファイルを標準の diff エディタで開き、選んだ行を stage / unstage できる（`ctrl+alt+s` / `ctrl+alt+u`）。
- GitHub Releases から正式版または snapshot 版を入れるスクリプト（`scripts/install.sh`）。
