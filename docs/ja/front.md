# FE 向けの窓口（`amonhen-front`）

[English](../front.md)

コアのすべての働きを、言語を問わず使える形で出す取り決め。VSCode 拡張（TypeScript）と CLI が使う。コアは手元の共有の daemon（`amonhen --serve`）で動き、FE はコアを持たない。SSH 先のリポジトリも daemon が開く（FE からは手元と同じに見える）。CLI は手元のリポジトリを同じプロセスのコア（`amonhen_front::InProcess`）でも開き、同じ命令を通す。

macOS のアプリは、同じプロセスのコアを C ABI（`crates/ffi-c`）で使う（共有メモリと mmap でコピーしない、いちばん速い道）。

## つなぐ

1. Unix ドメインソケット（Windows は名前付きパイプ `\\.\pipe\amonhen-<利用者の SID のハッシュ>-<ビルドの指紋>`。Node は `net.connect` でそのままつなげる）につなぐ。場所は `amonhen --socket` が出す（macOS は `$TMPDIR/amonhend-<ビルドの指紋の頭 8 文字>.sock`、ほかは `$XDG_RUNTIME_DIR` か `/tmp/amonhen-<uid>`。`AMONHEN_DAEMON_DIR` で変えられる）。つながらなければ `amonhen --serve <socket>` を、親から切り離して起動し、つながるまで待つ。
2. 最初に、開く場所を送る: 長さ（u32、LE）と UTF-8 の文字列 `"\u0001" + 場所`。続けて `"\u0000"` と、FE の環境（`NAME=value` を `"\u0000"` で区切ったもの）を送ってもよい。場所は手元のリポジトリの絶対パスか `ssh://host/path`。daemon は、このつながりで動かす git・ssh（`ssh://` を開くときも）を、自分の環境ではなくこの環境で動かす。受け取るのは、git と ssh がいつも使うもの（`PATH`・`SSH_AUTH_SOCK`・`SSH_AGENT_PID`・`XDG_CONFIG_HOME`・`XDG_DATA_HOME`・`GIT_*` と、`AMONHEN_ASKPASS`・`AMONHEN_GIT`・`AMONHEN_FALLBACK_GIT`・`AMONHEN_PASS_ENV`）と、`AMONHEN_PASS_ENV`（`NAME1,NAME2`）に並べた名前で、リポジトリを指すもの（`GIT_DIR` など）・`GIT_ASKPASS`・ほかの `AMONHEN_*` は受け取らない。ほかは捨てる（`amonhen_model::process::ClientEnv`）。FE の環境が変わったら、同じつながりで送り直せ、次の要求から効く（アプリは `Request::Environment`、この窓口の FE は `environment`。VSCode 拡張は `git.path` を変えたときに送る）。
3. その後はフレームでやりとりする。最初の要求は `{"op":"hello","language":"ja"}` にする（開けなければ、その答えが理由つきの失敗になる）。`language` は FE の表示の言語（`en`・`ja`・`ja-JP` など）で、daemon はこの接続のエラーの文をその言語で作る（訳がないとき・渡さないときは英語）。

## フレーム

| バイト | 中身 |
|---|---|
| 0..4 | 長さ（u32、LE）: 種類から中身の終わりまで（9 + 中身の長さ） |
| 4 | 種類: 1 要求、2 成功、3 失敗、4 知らせ、5 取り消し（最上位ビットは圧縮の印。この窓口では使わない） |
| 5..13 | 番号（u64、LE）。答えは要求と同じ番号。知らせは 0 |
| 13.. | 中身 |

- 要求の中身: 命令の JSON（UTF-8）。
- 成功の中身: JSON の長さ（u32、LE）、JSON、8 バイト境界までの詰め物、レコード（下）。
- 失敗の中身: 理由の文（UTF-8）。
- 知らせの中身: `{"changes": <ビット>}`。1 refs、2 index、4 作業ツリー、8 config。
- 取り消し: 走っている要求（検索など）の番号を送る。その要求は失敗で終わる。

要求は並べて送ってよい。答えは終わった順に返る。

## レコード

成功の JSON の `data` に、区画ごとの `[始め, 長さ]`（バイト。レコードの先頭から）がある。区画は 8 バイト境界から始まる。数値はすべて LE。TS では、中身から区画を `ArrayBuffer` に切り出し（`slice` で 0 始まりにする）、型付き配列で読む。

| 型 | 大きさ | 並び |
|---|---|---|
| 点 `Node` | 12 | row u32・lane u16・color u16・flags u16（1 merge、2 HEAD、4 stash、8 未コミット）・空き u16 |
| 線 `Edge` | 24 | rowStart u32・rowEnd u32・laneStart u16・laneEnd u16・color u16・kind u16（1 first-parent、2 合流の始まり）・child u32・parent u32 |
| ID | 20 | SHA-1 のバイト列 |
| 差分の行 | 16 | kind u8（0 文脈・1 追加・2 削除・3 hunk・4 ours・5 theirs・6 base・7 マーカー・8 ファイルの見出し）・flags u8（1 stage 済み）・空き u16・old u32・new u32・`text` の中の始め u32 |

## 命令

History と差分は番号（`history`・`diff`）で持つ。FE がスクロールしている間に History が新しくなっても、同じ番号なら同じ行が返る。各 FE で新しいものから 16 個まで覚え、古いものは「もうありません」になる（読み直す）。

| 命令 | 引数 | 答え（JSON） | レコード |
|---|---|---|---|
| `hello` | language | version・workdir・gitDir | |
| `activity` | | records（git を動かしている側の Activity の操作: seq・kind・repo・label・started（ミリ秒）・duration・ok・error・progress・cancellable・output）・remote（SSH 先のものなら true。取り消しは `cancelActivity` の remote で） | |
| `environment` | env（FE の環境。開くときと同じく `NAME=value` を `"\u0000"` で区切ったもの） | | |
| `history` | rebuild?・fold?（merge をすべてたたむ）・expanded?（ID の配列。fold のときに開いておく merge）・collapsed?（ID の配列。fold でなければ、これらの merge だけたたむ）・tips?（ID の配列。そこから辿れるものだけ） | history・generation・rows・head・headRow・folded・shallow（shallow clone の History か）・boundaryRows（shallow clone の境界の、親を取得していないコミットの行） | |
| `historyMatching` | history・rows | `history` と同じ（一致した行だけの History） | |
| `historyRelease` | history | | |
| `historyRows` | history と、start・count か rows（行の配列）、recorded?（true なら mailmap で作者を変えた行を挙げる） | count・edges・recorded（行の何番目か、記録された名前とメール。頼まなければ空） | rowNumbers u32・nodes・ids・times i64・spans（件名と作者の `text` の中の始めと長さ、u32 × 4。作者は mailmap で置き換えたもの）・text（UTF-8）・signatures u8・folded u32（折りたたんだ History のとき）・parentStarts u32（行の数 + 1）・parents u32（親の行）・parentIds（親の ID）・edges・edgeIds u32（start・count のときだけ。その範囲に重なる線） |
| `historyGraph` | history | rows・edges | nodes・edges・ids・folded（全行） |
| `historySearch` | history・query（`crates/core/src/query.rs` の書き方） | count | rows u32（昇順） |
| `historySearchPartial` | history・query（このつながりで走っている `historySearch` のもの） | count・done（探し終えた割合、0〜1。内容の条件がなければ 0 のまま） | rows u32（昇順。そこまでに分かった一致で、最後の答えの一部。最後に始めた検索でなければ空） |
| `historyContentsToFetch` | history・query | count（partial clone で `contents:` を探す前に取る中身の数。なければ 0）・more（上限を超えた。`historySearch` は失敗する） | |
| `historyDensity` | history・buckets | buckets | density（f32 × 2: 頻度・活動量、0〜1） |
| `historyLineage` | history・row | | rows（u64 のビット列）・edgeFrom u32 |
| `historyLanes` | history・start・end・metrics（full・thin・minFull・minThin・available） | width（レーン全体の幅） | positions f32 |
| `historyRowOf` | history・id | row（なければ null） | |
| `historyOrder` | order（`date` か、枝のコミットを merge の下にまとめる `topo`。これから読む History すべてに効き、今の History は git を読み直さずに並べ直す） | order | |
| `refs` | | headBranch・head・entries（kind・name・target・detail・ahead・behind・gone） | |
| `status` | | generation・branch・upstream・ahead・behind・entries（kind・path・origPath・index・worktree・staged・unstaged・submodule。内容のコンフリクト（UU・AA）には、作業ツリーのファイルに残っているマーカーの数 markers と、git の rerere が前の解決を当てたか rerere も） | |
| `commitDiff` | id か stash（ID）、または history・rows・pair?（forward・reverse・mergeBase）。whitespace?（exact・at-eol・change・all。行を比べるときに無視する空白で、`git diff --ignore-space-at-eol`・`-b`・`-w` と同じ）。contexts?（ファイルの番号の文字列 → 前後の行数か、ファイル全体なら "all"。ほかのファイルは 3 行） | files（path・oldPath・kind・added・removed・binary・generated。generated は `linguist-generated`、なければ lock ファイルや縮めたファイルで、アプリは最初は畳む）・changes・selection・rows・view（`diffSyntax` 用） | 差分の行（下） |
| `workingDiff` | path・mode（unstaged・staged・combined）・whitespace?（`commitDiff` と同じ。combined とは使えず、空白を無視した差分は `stageRows`・`discardRows` に使えない）。context?（前後の行数か、ファイル全体なら "all"。省けば 3 行）。open?（`[{gap, top, bottom}]`。変更のない所を隙間の行にたたむ（下）。空の配列でもたたむ） | diff・path・mode・binary・added・removed・rows・lineEndings（下） | 差分の行 |
| `treeFile` | oid（40 桁）・path・plain?（true なら、そのコミットの変更に印を付けずファイルの中身だけ。Working Copy の All Files で HEAD から変えていないファイルを開くのに使う） | path・changed・binary | 差分の行: そのコミットの時点のファイル（History の All Files）。そのコミットで変わったファイルなら、前後の行をすべて出したそのコミットの差分（先頭に見出しの行）で、変わった行に印がつく |
| `setSignatureCheck` | local（daemon 越しに SSH 先で開いたリポジトリのコミットの署名を、true ならこの機械の鍵、false なら SSH 先の git と鍵で確かめる） | remote（効いたか。手元のリポジトリはいつも手元で確かめる） | — |
| `conflict` | path | path・rows | 差分の行 |
| `diffSyntax` | diff（`workingDiff` の）か view（`commitDiff` の） | spans | spans（row・start・end・kind の u32 × 4。差分の行、その行の文字列での位置（highlights と同じ）、キーワード 1・文字列 2・コメント 3・数 4 ほか `SyntaxSpan` の種類）。時間がかかることがあるので、差分を出してから頼む |
| `textSyntax` | path（言語を決める）・text | spans | spans（上と同じ。row は 0 で、文の先頭からの UTF-16 の位置） |
| `stage`・`unstage` | paths | | |
| `stageRows` | diff・rows・stage?（combined の差分のとき。false で外す） | | |
| `discardRows` | diff・rows（unstaged の差分の行） | | |
| `commit` | message・amend? | output（hooks が書いたもの） | |
| `run` | command（`crates/core/src/command.rs` の命令。checkout・merge・rebase・fetch・push・stash・undo・blame・設定など） | その命令の答え（ok・error ほか） | |
| `connection` | | sent・received・closed・engine（SSH 先のつながりと、そこで使っているエンジンとその用意のしかた。手元なら null） | |

差分の行のレコード: rows（差分の行 × 行の数）・textLengths u32・text（UTF-8）・highlights（行・始め・終わりの u32 × 3。変わった単語のバイト位置）。`commitDiff` は、ファイルごとに見出しの行（kind 8、text はパス、old・new は削除・追加の行数）から始まる。`workingDiff` の行の番号（0 始まり）が、`stageRows`・`discardRows` の rows で指す番号。

`open` があると、`workingDiff` に `@@` の見出しの行はない。代わりに、1 行以上ある変更のない所（ファイルの頭・hunk の間・ファイルの末尾）ごとに隙間の行を 1 つ置く。隙間の行は kind が `HUNK`（3）で、old は隠している行数、new は見せている行数、文字は次の hunk の関数名（なければ空。末尾の隙間も空）。ファイルの最初の行から変わっているときも、最初の hunk に隠した行が 0 の隙間の行を置く。隙間は隙間の行の順に 0, 1, … と数え、`{gap, top, bottom}` はその隙間の上（前の変更のすぐ後）から `top` 行、下（次の変更のすぐ前）から `bottom` 行を見せる。見せた行は変わっていない行で、上の分は隙間の行より前、下の分は後に並ぶ。top と bottom の和が隙間の行数以上なら隙間をすべて見せる（隙間の行は隠した行 0 で残る）。たたんだ差分・ファイル全体の差分の行も `stageRows`・`discardRows` に使える。

`workingDiff` の lineEndings（stage 済みの差分・サブモジュール・消したファイルでは null）: index・worktree・checkout は、それぞれ git に入っている形・作業ツリーのファイル・checkout したときの形の改行で、`none`・`lf`・`crlf`・`mixed` のどれか。rule はかかっている変換で、`git check-attr` と同じ書き方（`text=auto eol=crlf`・`-text`・`core.autocrlf=true`。ない・見せないときは空）。notable は、作業ツリーと git で改行が違うか、warning のとき true。warning は、改行が混ざっているか、作業ツリーが CRLF なのに checkout すると LF になるとき true。差分は両側を git に入れる形で比べるので、こうした違いは変わった行として出ない。FE は notable のとき差分のそばに見せる。

## 絵の手伝い（`amonhen-render`）

エンジンは絵の機能を持たない（[design.md](design.md) の「リポジトリ構成」）。VSCode 拡張は、すべてのファイルの Mermaid・DOT の図・数式・Android の VectorDrawable の絵と SVG のモバイル向けの確認を、`amonhen-render`（`crates/render`）で作る。エンジンの隣に `bin/amonhen-render-<os>-<arch>` として同梱する別の実行ファイルで、Remote-SSH ではエンジンと同じく SSH 先で動く。拡張は見せるファイルごとに、引数なしで `AMONHEN_LANG` に言語を入れて起動し、標準入力に求めを書いて標準出力から答えを読み、最後に入力を閉じる。手伝いは残りに答えて終わる。`amonhen-render --version` は `amonhen-render <版>` を書く。

やりとりはどれも JSON の 1 行（UTF-8）。求めには `id`（どんな JSON の値でもよく、そのまま返る）と `op` があり、まとめて送ってよい。答えは求めた順:

| `op` | 中身 | 答え |
|---|---|---|
| `diagram` | language（フェンスの言葉: `mermaid`・`dot`・`graphviz`・`gv`）・text・from?（Markdown のファイルのリポジトリの根からのパス。DOT の図の画像はそのフォルダから読む）・colors（dark・text・background・kinds: 画面の `#rrggbb` の色。kinds は `keyword` などの構文の種類）・scheme?（アプリの「図の配色」の鍵: `syntax`・`pele`・`mermaid`・`mermaid-default` などの名前つきのテーマ・`mermaid-import:名前`。なければ `mermaid`）・importedTheme?（`mermaid-import:` のときの mermaid-cli の設定ファイルの中身）・timeZone?（gantt の日付を数える時間帯: `""` は手伝いの OS の時間帯、`UTC`、IANA の名前。夏時間も数える。読めない名前は OS の時間帯）・utcOffset?（前の取り決めの決まった時差。分、地方時 − UTC。timeZone がないときだけ。どちらもなければ UTC） | svg（配色と画面の色か、図の中で指定した Mermaid のテーマで色を当てたもの）と note?（近いもので描いた）、か error（描けないわけ）、かどちらもない（描く図でない: コードとして見せる）。themeError?（読み込んだテーマが読めない。構文の色で描く） |
| `math` | formulas（tex・display?: 1 つの Markdown のファイルの式を出てくる順に。ファイルごとの上限はアプリと同じ）・color（`#rrggbb`）・timeLimit?（1 つのファイルで式を描く時間の上限の秒。0 は上限なし。なければ 2） | formulas: それぞれ svg・width・height・depth（本文の字の大きさ × 数式の大きさ（既定 1.21）を 1 とする em）か error、かどちらもない（TeX を見せる） |
| `vectorDrawable` | path（リポジトリの根から）・data（XML の base64）・fallback?（引けない色を描く色）・dark?（暗い外観の資源（`values-night` など）を先に引く） | svg（大きさは dp）・notes |
| `svgCheck` | data（SVG の base64） | issues（targets・feature・message・count）・previews（android・ios・flutter・reactNative ごとに svg?（SVG の文字列。部品が読めなければない）・title・caption・messages・error?・exact。アプリと同じく Android と flutter_svg は本物の取り込みを移したもの、iOS と react-native-svg は近似） |

答えは `{"id": …, "ok": true, …}`（上の中身つき）か、求めそのものが失敗したときの `{"id": …, "ok": false, "message": …}`（JSON でない行には `"id": null`）。

手伝いはリポジトリを読まない。図の画像や VectorDrawable の資源が要るときは、問いを書いて答えを待つ。問いはファイルなら `{"ask": n, "read": "path"}`、フォルダなら `{"ask": n, "list": "folder"}`（根は `""`）で、パスは同じコミットのリポジトリの根から。答えは `{"answer": n, "data": "<base64>"}`（ないファイルは `null`）か `{"answer": n, "entries": [{"name": …, "folder": true|false}]}`。問いは 1 つの求めを描く間に 1 つずつ来る。拡張はファイルを git から、フォルダをエンジンの `treeEntries` で読み、1 MB を超える画像はないものとして答える。

## 版

窓口の形を変えたら `amonhen_front::VERSION` を上げる（`hello` の答えの version で確かめる）。ソケットの名前にはビルドの指紋（`amonhen_remote::BUILD_ID`。ワークスペースのソースのハッシュ）が入るので、ビルドの違う daemon にはつながらない。

FE が共有の daemon を自分で起動するときは、言語を `AMONHEN_LANG` で渡す（daemon が自分で行うことに使う）。SSH 先からのエラーの文は、その SSH 先へのつながりを何人の FE で使っていても、頼んだ FE の言語になる。
