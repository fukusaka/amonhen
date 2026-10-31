# 使い方

[English](../usage.md)

アプリ（macOS）・SSH 先・共有の daemon・CLI の使い方。インストールは [install.md](install.md)、VSCode 拡張は
[apps/vscode](../../apps/vscode/README.ja.md) を参照。

## アプリ

- 起動するとリポジトリの一覧（File > Repositories…、⇧⌘O）が出る。開いたことのあるリポジトリが新しい順に並び、Add Repository… で加え、Remove で外す（フォルダには触れない）。File > Open…（⌘O）でほかのリポジトリを別のウィンドウで開く。最後のウィンドウを閉じると一覧に戻る。
- サイドバー: Working Copy / History、ブランチ（フォルダ・上流との差 ↑↓・gone）、リモート、タグ、stash、サブモジュール。右クリックで checkout・merge・rebase・チェックアウトしていないブランチと上流を合わせる（後ろにだけあれば Fast-Forward、先にだけあれば Push、上流がなければ Publish）・作成・名前の変更・削除・stash の操作・サブモジュールの更新と別ウィンドウ。ブランチを別のブランチにドロップすると merge / rebase（fast-forward で済めば checkout しない）、リモートにドロップすると push。
- ツールバー: Fetch・Pull・Push・Stash・Undo。履歴の書き換え・変更の破棄・stash の削除などは Undo で戻せる（CLI・VSCode 拡張と記録を共有する）。
- Working Copy: ファイルはパスの一覧かディレクトリの木で表示（一覧の上で切り替え）。差分の行をクリックで選び（hunk の見出しで hunk 全体、Shift で範囲、⌘ で追加）、Stage Lines / Discard Lines。Space・Enter で stage / unstage。Summary と Description を書いて ⌘↩ でコミット。コンフリクトはコンフリクトごとに Use Current / Use Incoming / Use Both、または Merge Editor…（3 ペインのマージエディタ）。
- History: グラフ付きの一覧（件名とブランチ・タグのラベル、その下に作者・日時・ID）。右クリックでコミットの編集（Edit Message・Squash・Fixup・Edit・Split・Drop）、cherry-pick・revert、パッチとして保存（Save as Patch…。いくつか選べばひと続きに）、ブランチ・タグの作成。ドラッグで並べ替え。上部で All Branches / Current Branch / 選んだ ref を切り替える。⌘F でコミットを検索（Enter・⌘G で次へ、⇧Enter・⇧⌘G で前へ。一致したコミットだけを出すこともできる）。
- Range Diff: ローカルブランチの右クリックの「前の版と比べる」（または Repository > 前の版と比べる）で、最後の rebase・コミットの編集・amend の前の版と比べる（`git range-diff` と同じ）: = 同じ、! 変わった（2 つのパッチの差分つき）、< 消えた、> 足された。History で選んだ 2 つのコミットや Reflog の行とも比べられる。
- パッチ: Repository > Apply Patch…、または .patch・.diff・.mbox・.eml のファイルをウィンドウにドロップすると、何を変えるかとそのまま当たるかを見せてから、メールの形のパッチはコミットとして（`git am`）、diff は作業ツリーに（`git apply`）当てる。am がパッチで止まったら、ツールバーの下の帯に理由と、3-way merge で当て直す・Continue・Skip・Abort が出る。どちらも Undo で戻せる。CLI には `amonhen format-patch`・`amonhen am`・`amonhen apply` がある。
- 差分: ドラッグで文字を選んで ⌘C、⌘F で検索。構文の色付けは表示の後に裏で付く。
- History の検索の構文: 語は件名・作者・ID から探す。空白で区切ると AND、`or` で OR、括弧でまとめる。`author:` `subject:` `commit:` `path:` `contents:"..."`（例: `path:fs/ (author:linus or author:greg)`）。作者は、mailmap で置き換えた名前とコミットに記録された名前のどちらでも合う。
- 人はリポジトリの mailmap（`.mailmap`・`mailmap.file`）の名前とメールで出す（`git log` と同じ）。記録と違えば、詳細では記録された名前とメールを下に添える。切るのは git の `log.mailmap` で、リポジトリの設定の「作者と署名」（すべてのリポジトリなら Settings > Git）に mailmap のファイルと一緒にある。どちらを変えても、History を作り直さずに名前を出し直す。Blame は git と同じく、いつも mailmap で置き換える。
- ファイルの右クリック（Working Copy・History の変更ファイル）で Blame と File History（名前の変更も追う）。どちらもリポジトリの隣のタブで開き、同じツールバーが付く（ボタンを押すと、リポジトリのタブに戻ってそこで操作する）。タブを引き出せば別のウィンドウになり、Window > Move Tab to Window ▸ でリポジトリのタブに戻せる（前のタブを選んだ窓のタブに移す。リポジトリの窓が先頭に出る。タブをタブバーに落としてもよい。タブが 1 つのウィンドウでは View > Show Tab Bar、⇧⌘T でタブバーを出す）。同じものを開き直すと、そのタブを読み直したものに入れ替える。マージのコミットは、Show Merge Commits（一覧の右クリック、または Settings > 表示 > File History）を入れたときだけ出す。行やコミットをダブルクリックすると History で選ぶ。File History はすぐに開いて新しい方の 100 コミットを出し、残りは読めたら足す。名前を変えていれば、名前ごとにその時点の名前の見出しをコミットの上に出す。コミットの右クリックで History で見る・そのコミットでの Blame・ID やパスのコピー（Return で History に出し、⌘C で ID をコピーする）。
- 大きなリポジトリ: clone のとき、「取得する範囲」で「すべて」「ファイルの中身は必要なときに取る」（partial clone、`--filter=blob:none`）「最近の履歴だけ」（1・100・1,000・10,000 か好きな数のコミットの shallow clone）を選び、「一部のフォルダだけ取り出す」で sparse checkout にできる。shallow clone の History の終わりには、古いコミットを取り足す帯が出る。Repository Settings の「取得とフォルダ」で、リポジトリが持っているものを見て変えられる: 履歴やファイルの中身をさらに取る、取り出すフォルダを選ぶ、git maintenance のオン・オフ。そうしたリポジトリでは、サイドバーの History と Working Copy に印が付く。
- View > One Line per Commit で History の行を 1 行にする。終了したときに開いていたリポジトリは、次の起動で開き直す。

## SSH 先のリポジトリ

リポジトリの一覧の Add Remote Repository… で接続先とパスを入れるか、`AmonHen ssh://[user@]host[:port]/path/to/repo`（ホームからなら `/~/src/repo`）。鍵か ssh-agent でパスワードなしにつながること。SSH 先では git の処理だけを `amonhen --stdio` が行う。SSH 先の `PATH` に同じビルドの `amonhen` があるか、同じビルドのものが前に置かれていればそれを使い、なければ同梱のエンジン（Linux x86_64・aarch64、macOS arm64、Windows x86_64・arm64）を `~/.local/share/amonhen/engine/<中身のハッシュ>/` に送り込む（送ったら中身を確かめる。`AMONHEN_PUSH=0` で送らない、`AMONHEN_REMOTE_ENGINE` で場所を指定）。

Windows の SSH 先（その OpenSSH サーバ）にもつながり、既定のシェルは cmd.exe・PowerShell・pwsh のどれでもよい。ドライブの上のパスは `ssh://host/C:/src/repo`、ホームからなら `/~/src/repo`。エンジンは scp で `%LOCALAPPDATA%\amonhen\engine\<中身のハッシュ>\` に送り、既定のシェルが起動する。SSH 先のことは Windows PowerShell（Windows 10・11 にはいつもある）で読む。SSH 先の環境変数のページのログインシェルのスイッチは、Windows の SSH 先には当てはまらない。

Settings > SSH ホストに、SSH 先についてのすべてをまとめている。Amon Hen が知っている SSH 先（Places にあるものと、＋ で加えたもの）と、選んだ SSH 先の接続（状態、接続を確認、そこで動かすエンジンと合うものがなければ送り込むか、使っているエンジン、コミットの署名をこの Mac の鍵で検証するか SSH 先の鍵でか、探すフォルダ。Places と同じ一覧）、そしてその SSH 先のすべてのリポジトリに効く Settings > Git のページ（SSH 先の `--global` を、リポジトリなしで起動したエンジンを通して読み書きする）。プログラムのページで、そこで動かす git を選べる（SSH 先の Amon Hen の設定に置く。下を見ること）。環境変数のページは、エンジンの環境を見せ、エンジンを SSH 先のログインシェルの環境で起動するかを選べる。Places の SSH 先の「設定…」と、リポジトリの Settings の一覧の下の git の行からも開ける。Places の「この Mac」の「設定…」は Settings > Git を開く。

## 使う git

Amon Hen は、設定 > Git > プログラムで選んだ git を動かす。選んだものは機械の設定で、その機械の Amon Hen の設定 `~/.config/amonhen/settings.json`（`$XDG_CONFIG_HOME` があれば `$XDG_CONFIG_HOME/amonhen/`、Windows は `%APPDATA%\amonhen\`）に `{"git": "/opt/homebrew/bin/git"}` の形で置く。その機械の CLI・共有の daemon・VSCode 拡張も従い、SSH 先では、そこのエンジン・CLI・VSCode 拡張が SSH 先のファイルに従い、Settings > SSH ホストのその SSH 先のプログラムのページがそれを書く。選んでいなければ PATH で最初に見つかる `git`（VSCode 拡張は、その前に VSCode の `git.path`）。環境変数 `AMONHEN_GIT` は、そのプロセスだけファイルより優先する。選んだ git がなければ、git を動かす操作は、別の git で動かさずにそう書いたエラーで止まる。

## 共有の daemon

Settings > パフォーマンスの「リポジトリを共有の daemon 越しに開く」か、一覧の右クリックの Open via Daemon で、手元の共有の daemon 越しに開く（`daemon:/path`・`daemon:ssh://host/path`）。CLI・VSCode 拡張と History・検索の索引・SSH のつながりを共有する。様子は Window > Activity の Daemon のタブか `amonhen daemon status`。

## CLI

`amonhen --help` に全部の命令がある。主なもの:

```sh
amonhen .                      # アプリで開く
amonhen log -n 20 fix          # グラフ付きの履歴（検索の構文も使える。--json は mailmap で変えた行に recordedAuthor を足す）
amonhen status / diff / show   # Working Copy とコミットの中身（--json で FE が読む形）
amonhen stage --rows 3,5-9 f   # 差分の行ごとに stage（amonhen diff --combined --rows で番号を見る）
amonhen undo [--list]          # 操作を戻す（アプリ・VSCode 拡張と記録を共有）
amonhen range-diff --previous  # ブランチを、最後の rebase・amend の前の版と比べる（git range-diff）
amonhen log -C ssh://host/path # SSH 先（共有の daemon が SSH のつながりを持ち続ける）
amonhen daemon status          # 共有の daemon の様子
```
