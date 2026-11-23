# 使い方

[English](../usage.md)

アプリ（macOS）・SSH 先・共有の daemon・CLI の使い方。インストールは [install.md](install.md)、VSCode 拡張は
[apps/vscode](../../apps/vscode/README.ja.md) を参照。

## アプリ

- 起動するとリポジトリのページ（Window > Repositories、⇧⌘R。File > 開く…、⌘O でも出る）が出る。左に「すべて」「お気に入り」「場所」と自分のグループ、右にリポジトリが並び、列（名前・ブランチ・状態・最後のコミット・開いた日時）で並べ替えられる。「追加」（＋）か「場所」の見出しの右クリックに「手元のリポジトリを追加…」「SSH 接続を追加…」があり、「一覧から外す」で外す（フォルダには触れない）。File > Quick Open…（⇧⌘O）でリポジトリを名前で探して開く。最後のウィンドウを閉じてもアプリは窓のないまま残り、Dock のアイコンを押すと一覧が出る。
- サイドバー: Working Copy / History、ブランチ（フォルダ・上流との差 ↑↓・gone）、リモート、タグ、stash、サブモジュール。右クリックで checkout・merge・rebase・チェックアウトしていないブランチと上流を合わせる（後ろにだけあれば Fast-Forward、先にだけあれば Push、上流がなければ Publish）・作成・名前の変更・削除・stash の操作・サブモジュールの更新と別ウィンドウ。ブランチを別のブランチにドロップすると merge / rebase（fast-forward で済めば checkout しない）、リモートにドロップすると push。
- ツールバー: Fetch・Pull・Push・Stash・Undo。履歴の書き換え・変更の破棄・stash の削除などは Undo で戻せる（CLI・VSCode 拡張と記録を共有する）。
- Working Copy: ファイルは「リスト」（パス）・「ツリー」（フォルダ）・「ファイル名」で表示（一覧の上で切り替え）。差分の行をクリックで選び（hunk の見出しで hunk 全体、Shift で範囲、⌘ で追加）、Stage Lines / Discard Lines。差分の上の ␣ のボタンで、History と同じく空白を無視できる（無視している間はアクセントの色）。無視している間は、stage 済みと stage していない変更を分けて出し、行ごとの stage・破棄はできない。Space・Enter で stage / unstage。Summary と Description を書いて ⌘↩ でコミット。コンフリクトしたファイルは、一覧の下に 3 ペインのマージエディタが開く。コンフリクトごとに「Current を採用」「Incoming を採用」「両方を採用」（base があれば「Base を採用」も）、ファイル全体を片側にするなら「手元のファイルを使う」「取り込む側のファイルを使う」、終えたら「解決済みにする」。
- History: グラフ付きの一覧（件名とブランチ・タグのラベル、その下に作者・日時・ID）。HEAD の first-parent の並びのコミットは、右クリックで編集できる（ほかのコミットには項目を出さない）: 「メッセージを編集…」「前のコミットに Squash…」「Drop…」はその変更を付けて History の上で編集を始め、「ここから Interactive Rebase…」はそのコミットから始める（範囲の行の右端に pick・reword・squash・fixup・drop・edit の印を出す。印を押すか p・r・s・f・d・e のキーで選び、行をドラッグで並べ替え、範囲の下の線を引いて古いコミットを入れ、「変更を実行」で 1 回の rebase としてまとめて行う。merge はそのまま作り直す）。Split… はコミットで止めて、その変更を分けてコミットする。メニューは ID と名前のコピーから始まり、ほかに cherry-pick・revert、パッチとして保存（Save as Patch…。いくつか選べばひと続きに）、ブランチ・タグの作成。コミットをドラッグして、行の間に落とすと並べ替え、ほかのコミットに重ねるとそこへ squash、窓の外に落とすと drop になる。どれもその変更を付けて History の上で編集を始める（編集の間なら行の印を変えるだけ）。コミットは行のままの絵で運び、元の場所は空け、落とせない行は薄くする。merge はたためる（右クリックか、選んで ←）。取り込んだコミットを隠して「+N」の札を出し、札を押すか → で出す。表示のメニューではすべての merge をたたむ・出す。たたみはリポジトリごとに覚える。上部で All Branches / Current Branch / 選んだ ref を切り替える。⌘F でコミットを検索（Enter・⌘G で次へ、⇧Enter・⇧⌘G で前へ。一致したコミットだけを出すこともできる）。右の列は、上にコミットの詳細を固定し、変更ファイルは 1 つの差分のファイルの見出しが兼ねる（最初はすべて畳む）。見出しには種類（A・D・M・R）を色で、パス（名前の変更は `src/{old.rs → new.rs}`）と行数を出し、押すとその場で開く。上の帯で、変更とすべてのファイルを切り替え、ファイルの数と行数の合計を出し、リストとツリー（フォルダは畳める）を切り替え、フォルダ・差分をすべて開く・畳む（差分をすべて開くでは、lock ファイルや `linguist-generated` などの生成物と、とても大きな変更は畳んだまま）。␣ のボタンで、行を比べるときに無視する空白も選ぶ（行末・量の違い・すべて。`git diff --ignore-space-at-eol`・`-b`・`-w` と同じ。覚えない）。表示の設定には、詳細の見せ方・詳細を広げる・AI の項目がある。見出しの ⋯（か右クリック）に、ファイル全体を表示・この時点のファイルを開く・Blame・File History・戻す・パスのコピー。hunk の見出しの左の印で、前後を 20 行広げる。hunk の見出しには `git diff` と同じく囲む関数の名前を添える（ファイルの `diff=` 属性の `diff.<名前>.xfuncname`）。画像（と SVG）は、大きさを添えて並べるほか、スワイプ・オニオンスキン・違うピクセルで比べる。すべてのファイルでは、その時点のリポジトリ全体の木（変えたファイルに印、消したファイルは取り消し線）と、その時点のファイルの中身を出し、このコミットで足した行を緑、変えた行を青、消したところに印を付ける。画像はそこでも比べる欄でも拡大縮小できる（ボタン・ピンチ・⌘＋スクロール）。macOS が読める形式（GTK4 は gdk-pixbuf が読める形式）はどれも画像として出す。SVG は絵・テキスト・モバイルで見る。モバイルでは、Android VectorDrawable・iOS（CoreSVG）・flutter_svg・react-native-svg で崩れる機能を挙げ、部品ごとに元の絵とその部品での見え方を並べる。Android Studio と flutter_svg は本物の変換を移して入れてある（Java も Dart も使わない）、iOS と React Native は近似。Markdown は整形（プレビュー）かテキストで見る。整形はアプリが組み立て、GitHub の表・タスクリスト・脚注・囲み、構文の色つきのコード、Mermaid の flowchart（pele を移したもので描く。ほかの種類はコードのまま。色は Settings > テーマ >「図の配色」で構文の色・pele・Mermaid（外観で default と dark を切り替えるか、名前つきのテーマのどれか）・mermaid-cli の設定ファイルから読み込んだテーマから選ぶ。図の中の `%%{init}%%` や front matter の `theme`・`themeVariables` が先）、そのコミットの画像を出し、URL・#123・@user・コミットの ID をリンクにする。外からは何も読まない。そのほかのバイナリは Quick Look で開く（GTK4 は GNOME のプレビューか既定のアプリ）。変更ファイルが 200 を超えるコミットは、開いたファイルの差分をそのときに読む。
- Range Diff: ローカルブランチの右クリックの「前の版と比べる」（または Repository > 前の版と比べる）で、最後の rebase・コミットの編集・amend の前の版と比べる（`git range-diff` と同じ）: = 同じ、! 変わった（2 つのパッチの差分つき）、< 消えた、> 足された。History で選んだ 2 つのコミットや Reflog の行とも比べられる。
- パッチ: Repository > Apply Patch…、または .patch・.diff・.mbox・.eml のファイルをウィンドウにドロップすると、何を変えるかとそのまま当たるかを見せてから、メールの形のパッチはコミットとして（`git am`）、diff は作業ツリーに（`git apply`）当てる。am がパッチで止まったら、ツールバーの下の帯に理由と、3-way merge で当て直す・Continue・Skip・Abort が出る。どちらも Undo で戻せる。CLI には `amonhen format-patch`・`amonhen am`・`amonhen apply` がある。
- アーカイブ: Commit > Export Archive…、または History のコミットやサイドバーのブランチ・タグの右クリックの Export Archive… で、そのコミットのファイルを zip・tar.gz・tar のファイルに書き出す（`git archive`。何を外すかは `.gitattributes` に従う）。1 つのフォルダだけにもできる。CLI では `amonhen archive`。
- bundle: Repository > Save Repository as Bundle…、またはサイドバーのブランチ・タグの Save as Bundle… で、git が clone や fetch できる 1 つのファイルにコミットを保存する（`git bundle`）。履歴全体か、受け取る側が持っているコミットより後だけかを選べる。CLI では `amonhen bundle create`。bundle から clone するには、Clone のシートの URL にファイルを入れる（または bundle を選ぶ…）か、`amonhen clone` に渡す。Repository > Fetch from Bundle…（または `amonhen bundle fetch`）は、bundle のブランチを remote のブランチのようにそれ用の名前の下に取り込む。
- submodule: サイドバーは、取り出していない・記録と違うコミットにいる・中に変更がある submodule に印を付ける。メニューには Update（git と同じく記録したコミット）・Update…（中の submodule も、または追うブランチの最新のコミット）・URL の編集…・追うブランチ…・削除…（`.git/modules` のリポジトリのデータは、消すと選ばなければ残す）があり、Submodules の見出しと Repository のメニューに submodule を追加…・Update Submodules… がある。Clone のシートで submodule も取り出せる。CLI では `amonhen submodule` と `amonhen clone --recurse-submodules`。
- subtree（この機械の git に `git subtree` があるとき）: Repository のメニューの subtree を追加… で、別のリポジトリのブランチをフォルダに取り込む。サイドバーの Subtrees の見出しの下に、それぞれの Pull…・Push…・Split… があり、前に使ったリポジトリ・ブランチ・squash から始める。CLI では `amonhen subtree`。
- 差分: ドラッグで文字を選んで ⌘C、⌘F で検索。構文の色付けは表示の後に裏で付く。
- History の検索の構文: 語は件名・作者・ID から探す。空白で区切ると AND、`or` で OR、括弧でまとめる。`author:` `committer:` `subject:` `message:` `commit:` `path:` `contents:"..."`（例: `path:fs/ (author:linus or author:greg)`）。`author:`・`committer:` は名前とメールのどちらでも合い、mailmap で置き換えた名前でも合う。`message:` は本文やトレーラーも含むメッセージ全体から探す。
- 人はリポジトリの mailmap（`.mailmap`・`mailmap.file`）の名前とメールで出す（`git log` と同じ）。記録と違えば、詳細では記録された名前とメールを下に添える。切るのは git の `log.mailmap` で、リポジトリの設定の「作者と署名」（すべてのリポジトリなら Settings > Git）に mailmap のファイルと一緒にある。どちらを変えても、History を作り直さずに名前を出し直す。Blame は git と同じく、いつも mailmap で置き換える。
- ファイルの右クリック（Working Copy・History の変更ファイル）で Blame と File History（名前の変更も追う）。どちらもリポジトリの隣のタブで開き、同じツールバーが付く（ボタンを押すと、リポジトリのタブに戻ってそこで操作する）。タブを引き出せば別のウィンドウになり、Window > Move Tab to Window ▸ でリポジトリのタブに戻せる（前のタブを選んだ窓のタブに移す。リポジトリの窓が先頭に出る。タブをタブバーに落としてもよい。タブが 1 つのウィンドウでは View > Show Tab Bar、⇧⌘T でタブバーを出す）。同じものを開き直すと、そのタブを読み直したものに入れ替える。マージのコミットは、Show Merge Commits（一覧の右クリック、または Settings > 表示 > File History）を入れたときだけ出す。行やコミットをダブルクリックすると History で選ぶ。File History はすぐに開いて新しい方の 100 コミットを出し、残りは読めたら足す。名前を変えていれば、名前ごとにその時点の名前の見出しをコミットの上に出す。コミットの右クリックで History で見る・そのコミットでの Blame・ID やパスのコピー（Return で History に出し、⌘C で ID をコピーする）。
- ファイルを前の内容に戻すには、コミットの変更ファイル（History・Reflog）の右クリックの この版に戻す…・このコミットの前の版に戻す…、または File History のコミットの右クリックの この版に戻す…。作業ツリーのファイルを置き換える（`git restore --source`。その版にないファイルは消し、名前を変える前の版に戻すと元の名前に戻る）。シートで stage もでき、Undo で元のファイルに戻る。CLI では `amonhen restore --source <rev> [--staged] <path>…`。
- 大きなリポジトリ: clone のとき、「取得する範囲」で「すべて」「ファイルの中身は必要なときに取る」（partial clone、`--filter=blob:none`）「最近の履歴だけ」（1・100・1,000・10,000 か好きな数のコミットの shallow clone）を選び、「一部のフォルダだけ取り出す」で sparse checkout にできる。shallow clone の History の終わりには、古いコミットを取り足す帯が出る。Repository Settings の「取得とフォルダ」で、リポジトリが持っているものを見て変えられる: 履歴やファイルの中身をさらに取る、取り出すフォルダを選ぶ、git maintenance のオン・オフ。そうしたリポジトリでは、サイドバーの History と Working Copy に印が付く。
- View > One Line per Commit で History の行を 1 行にする。終了したときに開いていたリポジトリは、次の起動で開き直す。

## SSH 先のリポジトリ

リポジトリのページの「SSH 接続を追加…」で接続先とパスを入れるか、`AmonHen ssh://[user@]host[:port]/path/to/repo`（ホームからなら `/~/src/repo`）。鍵か ssh-agent でパスワードなしにつながること。SSH 先では git の処理だけを `amonhen --stdio` が行う。SSH 先の `PATH` に同じビルドの `amonhen` があるか、同じビルドのものが前に置かれていればそれを使い、なければ同梱のエンジン（Linux x86_64・aarch64、macOS arm64、Windows x86_64・arm64）を `~/.local/share/amonhen/engine/<中身のハッシュ>/` に送り込む（送ったら中身を確かめる。`AMONHEN_PUSH=0` で送らない、`AMONHEN_REMOTE_ENGINE` で場所を指定）。

Windows の SSH 先（その OpenSSH サーバ）にもつながり、既定のシェルは cmd.exe・PowerShell・pwsh のどれでもよい。ドライブの上のパスは `ssh://host/C:/src/repo`、ホームからなら `/~/src/repo`。エンジンは scp で `%LOCALAPPDATA%\amonhen\engine\<中身のハッシュ>\` に送り、既定のシェルが起動する。SSH 先のことは Windows PowerShell（Windows 10・11 にはいつもある）で読む。SSH 先の環境変数のページのログインシェルのスイッチは、Windows の SSH 先には当てはまらない。

Settings > SSH ホストに、SSH 先についてのすべてをまとめている。Amon Hen が知っている SSH 先（Places にあるものと、＋ で加えたもの）と、選んだ SSH 先の接続（状態、接続を確認、そこで動かすエンジンと合うものがなければ送り込むか、使っているエンジン、コミットの署名をこの Mac の鍵で検証するか SSH 先の鍵でか、探すフォルダ。Places と同じ一覧）、そしてその SSH 先のすべてのリポジトリに効く Settings > Git のページ（SSH 先の `--global` を、リポジトリなしで起動したエンジンを通して読み書きする）。プログラムのページで、そこで動かす git を選べる（SSH 先の Amon Hen の設定に置く。下を見ること）。環境変数のページは、エンジンの環境を見せ、エンジンを SSH 先のログインシェルの環境で起動するかを選べる。Places の SSH 先の「設定…」と、リポジトリの Settings の一覧の下の git の行からも開ける。Places の「この Mac」の「設定…」は Settings > Git を開く。

## 使う git

Amon Hen は、設定 > Git > プログラムで選んだ git を動かす。選んだものは機械の設定で、その機械の Amon Hen の設定 `~/.config/amonhen/settings.json`（`$XDG_CONFIG_HOME` があれば `$XDG_CONFIG_HOME/amonhen/`、Windows は `%APPDATA%\amonhen\`）に `{"git": "/opt/homebrew/bin/git"}` の形で置く。その機械の CLI・共有の daemon・VSCode 拡張も従い、SSH 先では、そこのエンジン・CLI・VSCode 拡張が SSH 先のファイルに従い、Settings > SSH ホストのその SSH 先のプログラムのページがそれを書く。選んでいなければ PATH で最初に見つかる `git`（VSCode 拡張は、その前に VSCode の `git.path`）。環境変数 `AMONHEN_GIT` は、そのプロセスだけファイルより優先する。選んだ git がなければ、git を動かす操作は、別の git で動かさずにそう書いたエラーで止まる。

## 共有の daemon

Settings > パフォーマンスの「リポジトリを共有の daemon 越しに開く（SSH 先も）」か、リポジトリのページの右クリックの「共有の daemon 越しに開く」で、手元の共有の daemon 越しに開く（`daemon:/path`・`daemon:ssh://host/path`）。CLI・VSCode 拡張と History・検索の索引・SSH のつながりを共有する。様子は Window > Activity の Daemon のタブか `amonhen daemon status`。

## CLI

`amonhen --help` に全部の命令がある。主なもの:

```sh
amonhen .                      # アプリで開く
amonhen log -n 20 fix          # グラフ付きの履歴（検索の構文も使える。--json は mailmap で変えた行に recordedAuthor を足す）
amonhen status / diff / show   # Working Copy とコミットの中身（--json で FE が読む形）
amonhen stage --rows 3,5-9 f   # 差分の行ごとに stage（amonhen diff --combined --rows で番号を見る）
amonhen diff -w HEAD           # git diff と同じく空白を無視（-w・-b・--ignore-space-at-eol。diff と show）
amonhen undo [--list]          # 操作を戻す（アプリ・VSCode 拡張と記録を共有）
amonhen range-diff --previous  # ブランチを、最後の rebase・amend の前の版と比べる（git range-diff）
amonhen log -C ssh://host/path # SSH 先（共有の daemon が SSH のつながりを持ち続ける）
                               # SSH 先にエンジンを送る前に聞く（--send-engine なら聞かずに送る）
amonhen daemon status          # 共有の daemon の様子
```

## 端末（TUI）

`amonhen tui [<path> | ssh://host/path | daemon:<path>]` は、端末の中でリポジトリを見せる。SSH のセッションや、エディタの横の端末のためのもの。まだ最初の段階で、作ったものと作っていないものは [tui.md](tui.md) にある。`?` で、フォーカスのある枠のキーの一覧、`:` で名前から選べる命令の一覧。その機械の `settings.json` に `{"tui": {"mouse": false}}` とあればマウスを取り込まずに始め、動いている間は `M` で切り替える。

- 枠: サイドバー（100 桁から。狭ければ `0`）、メインの画面（`1` Changes、`2` History、`3` Reflog）、その横か Enter で開く、コミットの詳細かファイルの差分。Tab で順に回り、Esc で戻り、`q` で終わる。
- History: `j` `k` などで動く。`/` で History の検索の書き方で検索し、`n` `N` で次と前の合った行へ。選んでいるコミットを `y` で cherry-pick、`v` で revert、`c` でチェックアウト。`e` でメッセージを書き換え、`F` で親にまとめ、`X` で消し、`|` で分ける（変更を作業ツリーに戻し、分けてコミットしてから Continue）。
- Working Copy: Space でファイルを stage / unstage。差分では行（hunk の見出しではその hunk）。`d` は確かめてから捨てる。`B` と `L` で、そのファイルの Blame と File History を開く。`v` で行の範囲を選び始める。`c` でメッセージを書き（そこで `Ctrl-e` を押すと git のエディタで開く）、`Ctrl-s` でコミット、`Ctrl-p` でコミットしてから push。`a` で amend。コンフリクトのあるファイルは `o` `t` で Current か Incoming の側を取り、その差分では `o` `t` `b` でコンフリクトを 1 つずつ解決する。
- 操作: `f` fetch、`p` pull、`P` push、`b` 新しいブランチ、`s` stash、`S` pop、`u` undo、`R` 選んでいるコミットへ reset。`M` でマウスを切ると端末で文字を選べる。`Ctrl-z` で一時停止。サイドバーの ref の上では、Enter でそのコミットを選ぶ、`c` でチェックアウト、`m` で merge、`r` でその上に rebase、`n` でブランチの名前を変える、`D` でブランチかタグを消す。`T` は、どこからでも、選んでいるコミットか ref にタグを作る。stash の上では `a` `o` `x` で戻す・pop・消す。merge や rebase が止まっている間は、`C` で続ける、`K` で飛ばす、`A` でやめる。
- git や ssh が尋ねるパスワードとパスフレーズは、一番下の行で聞く（Unix）。
