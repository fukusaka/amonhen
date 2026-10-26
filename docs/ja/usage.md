# 使い方

[English](../usage.md)

アプリ（macOS）・SSH 先・共有の daemon・CLI の使い方。インストールは [install.md](install.md)、VSCode 拡張は
[apps/vscode](../../apps/vscode/README.ja.md) を参照。

## アプリ

- 起動するとリポジトリの一覧（File > Repositories…、⇧⌘O）が出る。開いたことのあるリポジトリが新しい順に並び、Add Repository… で加え、Remove で外す（フォルダには触れない）。File > Open…（⌘O）でほかのリポジトリを別のウィンドウで開く。最後のウィンドウを閉じると一覧に戻る。
- サイドバー: Working Copy / History、ブランチ（フォルダ・上流との差 ↑↓・gone）、リモート、タグ、stash、サブモジュール。右クリックで checkout・merge・rebase・作成・名前の変更・削除・stash の操作・サブモジュールの更新と別ウィンドウ。ブランチを別のブランチにドロップすると merge / rebase（fast-forward で済めば checkout しない）、リモートにドロップすると push。
- ツールバー: Fetch・Pull・Push・Stash・Undo。履歴の書き換え・変更の破棄・stash の削除などは Undo で戻せる（CLI・VSCode 拡張と記録を共有する）。
- Working Copy: ファイルはパスの一覧かディレクトリの木で表示（一覧の上で切り替え）。差分の行をクリックで選び（hunk の見出しで hunk 全体、Shift で範囲、⌘ で追加）、Stage Lines / Discard Lines。Space・Enter で stage / unstage。Summary と Description を書いて ⌘↩ でコミット。コンフリクトはコンフリクトごとに Use Current / Use Incoming / Use Both、または Merge Editor…（3 ペインのマージエディタ）。
- History: グラフ付きの一覧（件名とブランチ・タグのラベル、その下に作者・日時・ID）。右クリックでコミットの編集（Edit Message・Squash・Fixup・Edit・Split・Drop）、cherry-pick・revert、ブランチ・タグの作成。ドラッグで並べ替え。上部で All Branches / Current Branch / 選んだ ref を切り替える。⌘F でコミットを検索（Enter・⌘G で次へ、⇧Enter・⇧⌘G で前へ。一致したコミットだけを出すこともできる）。
- 差分: ドラッグで文字を選んで ⌘C、⌘F で検索。構文の色付けは表示の後に裏で付く。
- History の検索の構文: 語は件名・作者・ID から探す。空白で区切ると AND、`or` で OR、括弧でまとめる。`author:` `subject:` `commit:` `path:` `contents:"..."`（例: `path:fs/ (author:linus or author:greg)`）。
- ファイルの右クリック（Working Copy・History の変更ファイル）で Blame と File History（名前の変更も追う）。どちらもリポジトリの隣のタブで開き、同じツールバーが付く（ボタンを押すと、リポジトリのタブに戻ってそこで操作する）。タブを引き出せば別のウィンドウになり、Window > Move Tab to Window ▸ でリポジトリのタブに戻せる（前のタブを選んだ窓のタブに移す。リポジトリの窓が先頭に出る。タブをタブバーに落としてもよい。タブが 1 つのウィンドウでは View > Show Tab Bar、⇧⌘T でタブバーを出す）。同じものを開き直すと、そのタブを読み直したものに入れ替える。マージのコミットは、Show Merge Commits（一覧の右クリック、または Settings > 表示 > File History）を入れたときだけ出す。行やコミットをダブルクリックすると History で選ぶ。File History はすぐに開いて新しい方の 100 コミットを出し、残りは読めたら足す。名前を変えていれば、名前ごとにその時点の名前の見出しをコミットの上に出す。コミットの右クリックで History で見る・そのコミットでの Blame・ID やパスのコピー（Return で History に出し、⌘C で ID をコピーする）。
- View > One Line per Commit で History の行を 1 行にする。終了したときに開いていたリポジトリは、次の起動で開き直す。

## SSH 先のリポジトリ

リポジトリの一覧の Add Remote Repository… で接続先とパスを入れるか、`AmonHen ssh://[user@]host[:port]/path/to/repo`（ホームからなら `/~/src/repo`）。鍵か ssh-agent でパスワードなしにつながること。SSH 先では git の処理だけを `amonhen --stdio` が行う。SSH 先の `PATH` に同じビルドの `amonhen` があるか、同じビルドのものが前に置かれていればそれを使い、なければ同梱のエンジン（Linux x86_64・aarch64、macOS arm64）を `~/.local/share/amonhen/engine/<中身のハッシュ>/` に送り込む（送ったら中身を確かめる。`AMONHEN_PUSH=0` で送らない、`AMONHEN_REMOTE_ENGINE` で場所を指定）。

## 共有の daemon

Settings > パフォーマンスの「リポジトリを共有の daemon 越しに開く」か、一覧の右クリックの Open via Daemon で、手元の共有の daemon 越しに開く（`daemon:/path`・`daemon:ssh://host/path`）。CLI・VSCode 拡張と History・検索の索引・SSH のつながりを共有する。様子は Window > Activity の Daemon のタブか `amonhen daemon status`。

## CLI

`amonhen --help` に全部の命令がある。主なもの:

```sh
amonhen .                      # アプリで開く
amonhen log -n 20 fix          # グラフ付きの履歴（検索の構文も使える）
amonhen status / diff / show   # Working Copy とコミットの中身（--json で FE が読む形）
amonhen stage --rows 3,5-9 f   # 差分の行ごとに stage（amonhen diff --combined --rows で番号を見る）
amonhen undo [--list]          # 操作を戻す（アプリ・VSCode 拡張と記録を共有）
amonhen log -C ssh://host/path # SSH 先（共有の daemon が SSH のつながりを持ち続ける）
amonhen daemon status          # 共有の daemon の様子
```
