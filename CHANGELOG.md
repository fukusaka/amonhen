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
- GitHub Releases から正式版または snapshot 版を入れるスクリプト（`scripts/install.sh`）。
