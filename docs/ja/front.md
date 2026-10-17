# FE 向けの窓口（`amonhen-front`）

[English](../front.md)

コアのすべての働きを、言語を問わず使える形で出す取り決め。VSCode 拡張（TypeScript）と CLI が使う。コアは手元の共有の daemon（`amonhen --serve`）で動き、FE はコアを持たない。SSH 先のリポジトリも daemon が開く（FE からは手元と同じに見える）。CLI は手元のリポジトリを同じプロセスのコア（`amonhen_front::InProcess`）でも開き、同じ命令を通す。

macOS のアプリは、同じプロセスのコアを C ABI（`crates/ffi-c`）で使う（共有メモリと mmap でコピーしない、いちばん速い道）。

## つなぐ

1. Unix ドメインソケット（Windows は名前付きパイプ `\\.\pipe\amonhen-<利用者の SID のハッシュ>-<通信の形の版>`。Node は `net.connect` でそのままつなげる）につなぐ。場所は `amonhen --socket` が出す（macOS は `$TMPDIR/amonhend-<通信の形の版>.sock`、ほかは `$XDG_RUNTIME_DIR` か `/tmp/amonhen-<uid>`。`AMONHEN_DAEMON_DIR` で変えられる）。つながらなければ `amonhen --serve <socket>` を、親から切り離して起動し、つながるまで待つ。
2. 最初に、開く場所を送る: 長さ（u32、LE）と UTF-8 の文字列 `"\u0001" + 場所`。場所は手元のリポジトリの絶対パスか `ssh://host/path`。
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
| `history` | rebuild?・fold?・expanded?（ID の配列）・tips?（ID の配列。そこから辿れるものだけ） | history・generation・rows・head・headRow・folded | |
| `historyMatching` | history・rows | `history` と同じ（一致した行だけの History） | |
| `historyRelease` | history | | |
| `historyRows` | history と、start・count か rows（行の配列） | count・edges | rowNumbers u32・nodes・ids・times i64・spans（件名と作者の `text` の中の始めと長さ、u32 × 4）・text（UTF-8）・signatures u8・folded u32（折りたたんだ History のとき）・parentStarts u32（行の数 + 1）・parents u32（親の行）・parentIds（親の ID）・edges・edgeIds u32（start・count のときだけ。その範囲に重なる線） |
| `historyGraph` | history | rows・edges | nodes・edges・ids・folded（全行） |
| `historySearch` | history・query（`crates/core/src/query.rs` の書き方） | count | rows u32（昇順） |
| `historyDensity` | history・buckets | buckets | density（f32 × 2: 頻度・活動量、0〜1） |
| `historyLineage` | history・row | | rows（u64 のビット列）・edgeFrom u32 |
| `historyLanes` | history・start・end・metrics（full・thin・minFull・minThin・available） | | positions f32 |
| `historyRowOf` | history・id | row（なければ null） | |
| `refs` | | headBranch・head・entries（kind・name・target・detail・ahead・behind・gone） | |
| `status` | | generation・branch・upstream・ahead・behind・entries（kind・path・origPath・index・worktree・staged・unstaged・submodule） | |
| `commitDiff` | id か stash（ID）、または history・rows・pair?（forward・reverse・mergeBase） | files（path・oldPath・kind・added・removed・binary）・changes・selection・rows | 差分の行（下） |
| `workingDiff` | path・mode（unstaged・staged・combined） | diff・path・mode・binary・added・removed・rows | 差分の行 |
| `conflict` | path | path・rows | 差分の行 |
| `stage`・`unstage` | paths | | |
| `stageRows` | diff・rows・stage?（combined の差分のとき。false で外す） | | |
| `discardRows` | diff・rows（unstaged の差分の行） | | |
| `commit` | message・amend? | | |
| `run` | command（`crates/core/src/command.rs` の命令。checkout・merge・rebase・fetch・push・stash・undo・blame・設定など） | その命令の答え（ok・error ほか） | |
| `connection` | | sent・received・closed（SSH 先のつながり。手元なら null） | |

差分の行のレコード: rows（差分の行 × 行の数）・textLengths u32・text（UTF-8）・highlights（行・始め・終わりの u32 × 3。変わった単語のバイト位置）。`commitDiff` は、ファイルごとに見出しの行（kind 8、text はパス、old・new は削除・追加の行数）から始まる。`workingDiff` の行の番号（0 始まり）が、`stageRows`・`discardRows` の rows で指す番号。

## 版

窓口の形を変えたら `amonhen_front::VERSION` を上げる（`hello` の答えの version で確かめる）。ソケットの名前には daemon との通信の形の版（`amonhen_remote::PROTOCOL_VERSION`）が入るので、版の違う daemon にはつながらない。

FE が共有の daemon を自分で起動するときは、言語を `AMONHEN_LANG` で渡す（daemon が自分で行うこと、たとえば SSH 先へつなぐときに使う）。
