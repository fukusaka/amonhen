# Changelog

リリースごとに、利用者から見た変更をまとめる。まだリリースしていない変更は `Unreleased` に書き、リリースするときに `## [x.y.z] - YYYY-MM-DD` の見出しに改める。

## [Unreleased]

### Added

- サイドバーに Working Copy / Branches / Remotes / Tags / Stashes / Submodules を表示する。
  - ブランチは `/` 区切りでフォルダにまとめ、upstream との差（↑↓）を表示する。
  - Tags / Stashes / Submodules は展開したときに初めて取得する。
  - 複数のリポジトリを開いているときは切り替えられる。
- ローカルブランチを右クリックから Checkout できる。
- Working Copy のファイルを標準の diff エディタで開く（Changes は index ↔ 作業中のファイル、Staged は HEAD ↔ index）。
- ファイル単位の stage / unstage。複数選択やグループ単位でもできる。
- diff エディタで選んだ行、またはカーソルのある hunk だけを stage / unstage する（`ctrl+alt+s` / `ctrl+alt+u`）。
- stage 済みの変更をコミットする（メッセージは 1 行）。
- ブランチのドラッグ＆ドロップで Merge / Rebase / Push / Force Push する。
  - ドロップ先が checkout されていなくても、fast-forward で済むなら checkout せずに進める。
  - コンフリクトで止まったら、Abort で中止できる。
- 直前の操作（コミット、Merge、Rebase、fast-forward）を Undo する。記録は VSCode を再起動しても残る。
- Fetch ボタン（すべてのリモートから取得し、消えたリモートブランチを片付ける）。
- History ビュー。コミットのグラフを表示し、200 件ずつ読み込む。ダブルクリックでコミットの変更を複数ファイル diff で開く。
- History の右クリックから、コミットを Reword / Squash / Fixup / Edit / Split / Drop / 並べ替えする（ドラッグでも並べ替えられる）。どれも Undo できる。
- merge や rebase が途中で止まっているとき、Working Copy の先頭に進み具合とともに表示し、Continue / Skip / Abort できる。
- コンフリクトしたファイルを標準の 3-way マージエディタで開く。種類（both modified など）を表示し、Accept Current / Accept Incoming でファイルごとに解決できる。解決済みにするときにマーカーが残っていれば確認する。
- GitHub Releases から正式版または snapshot 版を入れるスクリプト（`scripts/install.sh`）。
