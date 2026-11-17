# TUI の設計メモ

[English](../tui.md)

端末で動く FE: `amonhen tui`。アプリが届かない所（SSH のセッション、デスクトップのないマシン、エディタの横の端末）のためのもので、目指すところはアプリと同じ: 画面のどこも git を待たない。

状態: 段階 T1 を作った（History・スクロール・`--snapshot`・`--measure-scroll`）。残りは設計。

## 概要

- Rust で、ratatui（セルのバッファとフレームの差分。`ratatui-core` と `ratatui-crossterm` だけ。画面はどれも自前で描くので、部品の crate は要らない）と crossterm（入力・色・代替画面。Unix と Windows）を使う。
- エンジンの一部にする: `amonhen tui [<path> | ssh://host/path | daemon:<path>]`。実行ファイルは増やさないので、エンジンのあるホストには TUI がある。
- コアへは FE 向けの窓口（`amonhen-front`、[front.md](front.md)）だけを通る。CLI と同じで、手元のパスは同じプロセス、`daemon:` と `ssh://` は共有の daemon。History はアプリと同じ永続キャッシュを使う。
- キーボードが主。マウスは補助（ホイール・クリックで選択・仕切りのドラッグ）。
- 画面・名前・文言は [ux.md](ux.md) に従う。端末でアプリと同じにできない所は TUI なりの形にし、[frontends.md](frontends.md) に TUI の列を足してそこに書く。

作らないもの:

- アバター、窓の背景と画像つきのテーマ、GitHub と AI の機能、設定のページ（設定はアプリと `amonhen run` で変える。TUI は読むだけ）。
- ドラッグ＆ドロップ。アプリがドラッグで行う操作（merge・rebase・並べ替え）は、選択に対する命令にする。
- Unicode の罫線が出ない端末と、256 色に満たない端末。

## 置き場所

| 部分 | 場所 | 補足 |
| --- | --- | --- |
| TUI | `crates/tui`（`amonhen-tui`、ライブラリ） | `Arc<dyn Front>` を受け取る。リポジトリをどう開いたかは知らない。`amonhen-front`・`amonhen-layout`・`amonhen-model`・`amonhen-projection`・ratatui・crossterm に依存する（`amonhen-commands` は T4 から） |
| 入口 | `crates/cli` のサブコマンド `tui` | ほかのサブコマンドと同じ開き方（`open_repository`）で開き、窓口を `amonhen_tui::run` に渡す |
| 切り替え | `amonhen-cli` のフィーチャー `tui`（既定で入れる） | `--no-default-features` で作るターゲットごとの検査では外れる（`syntax` と同じ） |
| セルのグラフ | `amonhen-projection::terminal` | いま `crates/cli/src/graph.rs` にある、レーンを列に詰める処理と罫線の格子をここへ移し、`amonhen log` と TUI が同じグラフを描く。CLI は格子を ANSI の文字列にし、TUI は ratatui のバッファに書く |

コアに端末のコードは入れない。最初の数段階では `amonhen-front` に命令を足さない（TUI に要るものは、VSCode 拡張と CLI のためにもうある）。

## 構造

```
 crossterm のイベント ─┐
 front.subscribe ──────┼─▶ 1 本のチャネル ─▶ メインスレッド: 状態を更新 ─▶ 変わっていれば描く ─▶ 端末
 ワーカーの答え ───────┘                           │
                                                   └─▶ ワーカー: front.call（待つ）。古くなれば取り消す
```

- メインスレッドが状態と端末を持つ。1 本のチャネルで待ち、入力・ワーカーの答え・変化の知らせ・タイマー（何かが動いている間と、検索が進み具合を出す間だけ）で起きる。何も変わらなければフレームを描かない。
- `Front::call` は答えを待つので、呼び出しはすべてワーカースレッドで行い、答えはチャネルで返す。要求の種類ごと（History の行・差分・status・検索）に世代を持たせ、今より古い答えは捨てる。走っている検索は `call_until` で取り消す。
- 状態はただのデータ（どの画面か・選択・スクロールの位置・下に書くキャッシュ）。描画は状態の関数（ratatui の想定どおり）。部品の木は持たない。
- フレームは synchronized update（`CSI ? 2026`）で包んで書く。対応する端末ではフレームがまとめて出る。ratatui は変わったセルだけを送る。

### History

- `history` が番号つきの History と行数を返す。更新が来ても、スクロール中はその番号が使える。
- 行は `historyRows` で範囲ごとに取る（ノード・エッジ・ID・時刻・件名と作者）。見えている範囲の前後（数画面ずつ）の行を持っておき、端に着く前に次の分を頼むので、スクロールの 1 歩は手元のデータだけで描ける。まだない行（遠くへ飛んだ直後）は、答えが来るまで空のまま。頼むのは一度に 1 つで、次に何を頼むかは答えが返ってから決める。
- 1 コミットは必ず端末の 1 行。行と画面の位置は引き算で求まり、ページ単位のスクロールで行の高さを測らなくてよい。`amonhen log` がコミットの間の「つなぎの行」に描いている曲線は、コミットの行に描く: コミットから出る曲線はそのコミットの行で曲げ、親に入る曲線は親の行で曲げる。隣の行へまっすぐつなぐ曲線は、first-parent なら親の行（ブランチが分かれた所へ戻る形）、マージなら子の行で曲げる。ほかのレーンを横切る線は `┼` になる。
- グラフの列は、要るだけ取る。上限は幅の一部（既定で 3 分の 1）。通るだけのレーンは、`amonhen log` と同じく細い 1 列にまとめる。
- ref の札は `refs` から取り、コミットの ID で行に付ける。選んだコミットの祖先は `historyLineage` で強調する（ほかを薄くする。アプリと同じ）。
- 検索は History の検索の書き方で `historySearch` を使い、見つかった分から出す（`historySearchPartial`）。

### 差分

- `commitDiff` と `workingDiff` は行のレコードと文字の表を返す。TUI は答えをそのまま持ち、見えている行だけをそこから描く（行ごとに文字列を作らない）。
- 変わった語は `highlights` の区画を使う。構文の色は差分を出してから頼み（`diffSyntax`）、届いたら塗る。
- 色は、端末が対応を知らせていれば（`COLORTERM`）24 ビット、なければ 256 色の近い色。アプリで選んだ差分と構文のテーマに従うか、端末の色に従うかは T2 で決める（テーマは今はアプリの設定で、コアのものではない）。
- 最初は 1 列（unified）だけ。左右に並べる表示は 160 桁ほど要るので後にする。

### 変化

`front.subscribe` が変化のビット（refs・index・作業ツリー・config）を届ける。続けて来たビットはまとめ、ビットが指すものだけを読み直す（index と作業ツリーなら `status`、refs なら `refs` と `history`）。画面の History は新しいものができるまで番号を保ち、できたら選択をコミットの ID で引き継ぐ（`historyRowOf`）。

## 画面

```
 amonhen-standalone  main ↑2 ↓0                                   ⟳ Fetching origin…
┌ Workspace ─┬ History ───────────────────────────────┬ a1b2c3d4 ──────────────────────┐
│ Changes  5 │ ● ─╮  (HEAD → main) Fix the graph  Ann │ Fix the graph                  │
│ History    │ │  ●  Add a test               Bob  2d │ Ann <ann@example.com>  2d ago  │
│ Stashes  2 │ ●─╯  Merge branch 'topic'      Ann  3d │                                │
│ Branches   │ ●    Bump the version          Ann  5d │ M src/graph.rs         +12 -3  │
│  ★ main    │ ┆                                      │ A tests/graph.rs       +40     │
│    topic   │                                        ├────────────────────────────────┤
│ Remotes    │                                        │ @@ -10,7 +10,9 @@ fn layout()   │
│ Tags       │                                        │ -    let lane = 0;             │
│            │                                        │ +    let lane = trunk();       │
└────────────┴────────────────────────────────────────┴────────────────────────────────┘
 Rebase 3/7 · 2 conflicts   c continue  s skip  a abort                    ? help  : commands
```

- 端末が広ければ（140 桁ほどから）3 列: サイドバー・メインの画面・詳細。狭ければ、詳細はメインの画面と入れ替わりで開く（Enter で入り、Esc で戻る）。さらに狭ければ（80 桁ほど未満）、サイドバーは呼んだときだけメインの画面に重ねる。
- サイドバーはアプリと同じ節（メインの画面を切り替える Workspace の行、続いて Branches・Remotes・Tags・Worktrees）と、絞り込みの欄を持つ。
- 一番上の行は、リポジトリ・今のブランチと ahead / behind・動いている処理。一番下の行は、merge・rebase・cherry-pick の途中ならその状態と操作、そうでなければ、フォーカスのある所のキーの案内。
- Working Copy はアプリと同じ 1 つの一覧で、ファイルごとにチェック（Space で stage / unstage）。右に差分（hunk・行ごとの stage。`stageRows`・`discardRows`）、一覧の下にメッセージの欄。
- シート（確認・Branch・Merge・Rebase・Push…）は画面に重ねる枠で、欄と文言はアプリと同じ。壊す操作は先に尋ね、Undo で戻せるかを書く。

## キー

- 移動: 矢印と `j` `k`、`g` `G`、PageUp と PageDown、`Ctrl-d` `Ctrl-u`。枠の間は `Tab` と `Shift-Tab`。Workspace の行は `1`…`5`。
- `/` で検索（History。サイドバーでは絞り込み）、`n` `N` で次と前。
- `:` で命令の一覧: 共通の表（`amonhen-commands`）の操作のうち TUI にあるものを、打った文字で絞り込み、キーを横に出す。`?` でフォーカスのある所のキーの一覧。
- アプリのツールバーにある操作は 1 文字のキーを持つ（`f` fetch、`p` pull、`P` push、`b` branch、`m` merge、`r` rebase、`s` stash、`u` undo）。アプリがシートを通すものは、TUI もシートを通す。
- 操作の表のショートカットは窓のアプリ向け（`Primary+Shift+B`）で、ほとんどは端末で打てない。表に TUI のキーの列を足す（GTK の列と同じ）。
- kitty のキーボードプロトコルは、端末が答えたときだけ使う（ふつうの端末で区別できないキーのため）。これに頼る機能は作らない。

## 端末だから違うところ

- パスワードとパスフレーズ: git と ssh は端末なしで動く（端末は TUI が使っている）。TUI は `AMONHEN_ASKPASS` を、`amonhen tui --askpass` を呼ぶ小さなスクリプトにする。そのプロセスは、動いている TUI が待っているソケットにつなぎ（場所はスクリプトに書く。daemon は `AMONHEN_ASKPASS` を引き継ぐが、ほかの `AMONHEN_*` は引き継がない）、TUI がシートで尋ね、答えを返す。daemon は FE の `AMONHEN_ASKPASS` を使うので、共有の daemon が git を動かすときも同じに働く。
- コミットメッセージ: ふだんは TUI の中の複数行の欄。`Ctrl-e` で代替画面を抜け、git のエディタ（`GIT_EDITOR`・`core.editor`・`VISUAL`・`EDITOR`）でメッセージを開き、戻る。
- マウス: 取り込むと端末の文字の選択が効かなくなる（多くの端末は Shift を押しながらなら選択できる）。既定で取り込み、切るキーと設定を持つ。
- コンフリクト: 一覧にし、ファイルごとのコンフリクトを出し（`conflict`）、アプリにあるコンフリクトごと・ファイルごとの選択を、コアが命令として出している範囲で使えるようにする（T6 で確かめる）。3 ペインのエディタは作らない。`e` でファイルをエディタで開く。
- シートの前と後の絵（rebase・merge・reset）は、同じグラフのセルで描く（動きはなし）。後の段階。
- 一時停止（`Ctrl-z`）と端末の大きさの変更では、画面を戻して描き直す。panic のときは、端末を戻してから出す。

## 速さ

[design.md](design.md) の目標を、TUI の手が届く所まで当てはめる: 端末に渡すバイト列まで。その先で端末エミュレータがすることは TUI からは変えられないので、測らない。

| 項目 | 目標 |
| --- | --- |
| キーのイベントからフレームのバイト列を flush するまで（History を 1 行スクロール） | 8.3 ms 以内。余裕を持たせ、手元のリポジトリの 200 × 60 で 1 ms 未満 |
| 同じことを `ssh://`・`daemon:` で | 同じ（行が手元の範囲にあるとき）。答えを待って止まらない |
| スクロール 1 歩のバイト数 | 測って、増えないよう保つ（SSH のセッションを通る量） |
| 起動から History の最初の画面まで | `amonhen log` より数 ms 多い程度（同じ永続キャッシュ） |

- `amonhen tui --measure-scroll` は、出力を数えるだけの先に向け、本物のイベントの経路で History をスクロールして、フレームごとの時間とバイト数を出す（アプリの同じ名前の引数）。
- `amonhen tui --snapshot <file>` は、ratatui の試験用のバックエンドに 1 フレーム描いて文字で書き出す（`--size <桁>x<行>`）。テストはこのフレームを比べるので、CI に端末は要らない。
- フレームの組み立てと、グラフをセルに詰める所の criterion のベンチは、まだ書いていない。

### 計測の記録

T1、Apple Silicon の Mac の release ビルド、linux のリポジトリ（1,485,021 コミット）、`--measure-scroll`（600 フレーム、8.3 ms おきにホイール 1 目盛り = 3 行）:

| 大きさ | フレームの時間（中央値 / 95% / 最大） | 1 フレームのバイト数（平均 / 最大） | 行が足りなかったフレーム |
| --- | --- | --- | --- |
| 200 × 60 | 1.48 ms / 3.49 ms / 12.3 ms | 25,974 / 52,601 | 0 |
| 120 × 40 | 0.81 ms / 2.18 ms / 13.2 ms | 12,828 / 23,813 | 0 |

- 最も遅いフレームを除けば 8.3 ms に収まるが、200 × 60 で 1 ms の目標には届いておらず、最も遅いフレーム（12〜13 ms）は超えている。どこに時間がかかっているかは、まだ調べていない。
- スクロールの 1 歩で、ほぼすべてのセルを描き直している（どの行も動くため）。端末に History の領域のスクロールを送り、新しく見える行だけを送れば、バイト数も時間も減る。ratatui の `Terminal` は前のフレームを外に出さないので、`ratatui-core` のバッファの上に自前のフレームの出力が要る（T1b）。
- 最初の画面の `--snapshot` は、キャッシュがある状態の linux で 0.34 秒（プロセス全体。大半はリポジトリを開く所）。`amonhen log` とはまだ比べていない。

## 配布

- TUI は、エンジンを作るすべてのターゲットでエンジンに入れる。crossterm は Windows のコンソールを直接扱うので、Windows のエンジンにも入る。
- エンジンは約 0.36 MB 大きくなる（darwin-aarch64 の release ビルドで 40.28 MB から 40.64 MB、0.9%）。そのため、SSH 先へ送るものも含め、どのエンジンにも入れる。
- terminfo も C のライブラリも要らないので、musl の静的なエンジンは静的なまま。

## 実装の順番

- [x] T1. `crates/tui`、イベントループ、`amonhen tui`、History（グラフ・札・件名・作者・日付）、行の範囲を持つスクロール、`amonhen log` と共有する `amonhen-projection::terminal`、`--snapshot` と `--measure-scroll`。速さ・1 行のグラフ・エンジンの大きさを linux のリポジトリで測った（計測の記録）
- [ ] T1b. 端末のスクロール領域を使う自前のフレームの出力（スクロールの 1 歩で、新しく見える行だけを送る）。最も遅いフレームの原因を調べる。criterion のベンチ
- [ ] T2. コミットの詳細と差分（ファイルの一覧・unified の差分・変わった語・構文の色）、狭いときのレイアウト
- [ ] T3. Working Copy: 一覧、ファイル・hunk・行ごとの stage、discard、メッセージの欄とエディタ、commit と amend。変化の知らせ
- [ ] T4. サイドバー（Branches・Remotes・Tags・絞り込み）、ref ごとの History、検索、祖先の強調、命令の一覧とヘルプ
- [ ] T5. シートつきの操作（fetch・pull・push・branch・checkout・merge・rebase・stash・reset・undo）、askpass、動いている処理の表示。マウスの取り込みを切るキーと設定、一時停止（`Ctrl-z`）、kitty のキーボードプロトコル
- [ ] T6. コンフリクト、Stashes、Reflog、Worktrees、History の編集（命令として）、前と後の絵
- [ ] T7. frontends.md の TUI の列、usage.md、CHANGELOG

## 決めたこと

2026-11-17 に決めた:

1. TUI はエンジンの一部（`amonhen tui`）にする。別のプログラムにはしない。
2. 引数なしの `amonhen` は TUI を始めない。今までどおりアプリで開く。
3. マウスの取り込みは既定で入れ、切るキーと設定を持つ。
4. コミットメッセージは TUI の欄に書き、キーで git のエディタを開ける。

未決:

5. 最初に出す範囲: T3 まで（読むことと commit）か、T5 まで（ふだんの操作）か。
