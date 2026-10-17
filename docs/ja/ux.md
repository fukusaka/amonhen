# UI / UX の再検討

[English](../ux.md)

`docs/design.md` の「画面構成」を土台に、日常の操作を SourceTree・Fork・Tower・JetBrains（IDE の Git）と同じかそれ以上に楽にするための見直し。速度の方針（UI スレッドで git を待たない、1 フレーム以内）は変えない。

## 原則

| 原則 | 内容 |
| --- | --- |
| 速さを削らない | 新しい画面も「見えている分だけ計算・描画」。重いもの（複数コミットの差分、AI、GitHub）は後追いで足し、先に出せるものを先に出す |
| macOS の作法に従う | ウィンドウのタブ、Settings（⌘,）、ツールバーのカスタマイズ、メニューの網羅、macOS 標準のショートカットは崩さない。独自の作法は History と差分のような自前の画面の中だけ |
| 1 つの操作は 1 か所で完結 | 選ぶ → 確かめる → 実行 を同じ画面で。別ウィンドウ・シートは確認が要る操作だけ |
| すべて Undo できる | 書き込み系は操作ログ（oplog）に積み、ツールバーと Activity から戻せる |
| キーボードで全部できる | すべての操作にメニュー項目を持たせ、ショートカットはメニューから引く |
| 外部との通信は明示 | AI・GitHub は設定で有効にしたときだけ。送る内容を見せてから送る |

## 全体のレイアウト

```
┌ タブ: amonhen-standalone │ linux │ vscode-amonhen │ + ─────────────────────────────────────────┐
│ [◧] main ↑2↓0 │ Fetch Pull Push │ Branch Merge Rebase │ Stash Pop │  ⟳ 2 ops  ↶  🔍____ │
├────────────┬──────────────────────────────────────────────────────────────────────────────┤
│ WORKSPACE  │ （Working Copy / History / Conflicts のどれか）                               │
│  Changes 5 │                                                                              │
│  History   │                                                                              │
│  Conflicts │                                                                              │
│ BRANCHES   │                                                                              │
│ REMOTES    │                                                                              │
│ PULL REQS  │                                                                              │
│ TAGS       │                                                                              │
│ STASHES    │                                                                              │
│ SUBMODULES │                                                                              │
│ WORKTREES  │                                                                              │
└────────────┴──────────────────────────────────────────────────────────────────────────────┘
 merge・rebase・cherry-pick の途中は、ツールバーの下に帯: 「Rebase 3/7 · 2 件のコンフリクト  [Continue] [Skip] [Abort]」
```

- Conflicts はコンフリクトがあるときだけサイドバーに出し、件数を付ける。
- Pull Requests・Worktrees は該当するものがあるときだけ出す。

## 1. 複数のリポジトリ: タブと別ウィンドウ

参考: Fork（リポジトリごとのタブ）、Tower（ウィンドウのタブ）。

- `NSWindow.tabbingMode = .preferred` で macOS 標準のウィンドウのタブにする。タブのドラッグで別ウィンドウにでき、Window メニューの「すべてのウィンドウを結合」も標準で動く。自前のタブバーは作らない。
- ⌘T・タブバーの「+」: リポジトリの一覧を出し、選んだものを前のウィンドウのタブで開く（Quick Open ができたらそれに替える）。⌘N は新しいウィンドウ。Open… や一覧から開いたものも、前のウィンドウのタブに入れる。タブの切り替えは ⌃Tab・⌃⇧Tab と ⌘⇧] ⌘⇧[。
- 開いていたタブとウィンドウの並びを次の起動で戻す（今の `RepositoryList.open` をウィンドウ単位の配列に広げる）。
- 裏のタブはメモリを絞る: History のチャンクを見えている付近だけ残し（済みの仕組みを使う）、ファイル監視は続けるが status の再計算は前に出たときまで遅らせる。タブのタイトルに未コミットの変更の有無（●）と ahead/behind を出す。

## 2. ブックマーク（リポジトリの一覧）

今の「Repositories…」（開いた順の一覧）を、Fork の Repository Manager・SourceTree のブックマークに相当するものに広げる。

| 要素 | 内容 |
| --- | --- |
| 個別登録 | 「追加…」でフォルダを選ぶ・Finder からドロップ・開いたリポジトリを自動で追加（設定で切れる）。SSH 先（`ssh://`）もここに |
| フォルダから自動取得 | 「スキャンするフォルダ」を複数登録（例 `~/prj`）。深さ（既定 3）・除外パターン（`node_modules` など）を指定。見つけたリポジトリはそのフォルダ名のグループに入る。フォルダは FSEvents で見張り、増減を反映する。消えたものは灰色にして残し、手で外す |
| グループ | 手動のグループ（フォルダのような入れ子）と、スキャンのグループ。ドラッグで並べ替え・移動 |
| 状態 | 各行に今のブランチ・ahead/behind・変更のあるファイルの数。`.git/HEAD` だけは即読み、ほかは見えている行だけ裏で `git status --porcelain=v2 --branch --untracked-files=no`（ロックを取らない）を 4 つずつ。untracked は数えない |
| 付加情報 | 色ラベル、お気に入り（★）、表示名の上書き、最終オープン日時 |
| 操作 | 開く（⏎ 今のウィンドウのタブ、⌘⏎ 新しいウィンドウ）、Finder で表示、ターミナルで開く、エディタで開く、Fetch（選んだもの全部を一括で） |
| Quick Open | ⌘⇧O で全ブックマークをあいまい検索するパネル（Spotlight 風） |

一覧は 2 か所に出し、中身と操作は同じ（`RepositoryBrowser`）。

- **リポジトリのページ**: リポジトリを開いていないタブ（`RepositoryHome.swift`）。⌘T・⌘N・⌘O・⌘⇧R で出し、起動したとき・
  最後のリポジトリを閉じたときに何も開いていなければ出す。左に分類（すべて・お気に入り・場所（このMac と SSH のホスト
  ごと）・手で作ったグループ（作ってから出す）。それぞれ件数付き）。場所の下には、スキャンするフォルダ（🔍。探すのはここだけで、そのマシンの
  上で探す。SSH のホストは、そこのエンジンが `amonhen find-repositories` で探す）と、それ以外のリポジトリ（開いた・手で
  入れたもの）のフォルダが、リポジトリのあるフォルダの木（枝分かれもリポジトリもない途中のフォルダは `~/prj/oss` のように
  1 つにまとめる）で並び、フォルダを選ぶとその下のリポジトリを出す（場所の下のフォルダは、🔍 の外のものだけ。同じリポジトリを 2 か所で数えない。省いた名前はツールチップにフルパス）。加えるのは左の下の「＋」か右クリック: 場所なら手元の
  リポジトリ・SSH 接続、このMac なら手元のリポジトリ・スキャンするフォルダ、ホストならそのホストのリポジトリ・スキャンする
  フォルダ、グループならグループ。🔍 のフォルダでは、探す深さ・探し直す・スキャンをやめる。右に、選んだ分類の中のリポジトリを 1 回ずつ、
  並べ替えられる 5 つの列で出す: 名前とパス、ブランチ、状態（「変更 n」・上流との差 ↑↓・変更なしは ✓。列の見出しのツールチップに説明。SSH 先は開くまで読まないので「SSH」）、
  最後のコミット（いつ・件名）、最後に開いた日時（既定の並び。開いていなければ —）。行をお気に入り・グループにドラッグすると入れられる。
  選ぶ（ダブルクリックか ⏎）とこのタブがそのリポジトリになる（⌘⏎ で新しいウィンドウ）。右は絞り込みと一覧だけで、
  下にボタンは置かない。人が一度に把握できるのは 5〜6 個なので、
  ページのどの部分もその数に収める。ページにもリポジトリのタブと同じツールバーを付ける（リポジトリへの操作は押せない）
  ので、タブを切り替えてもタブの並びの位置が動かない。別ウィンドウは持たない。
- **ツールバーの左の Repositories**: リポジトリのページに切り替える（⌘⇧R）。
- **その隣のリポジトリ名**（GitHub Desktop と同じ）: 押すと一覧を名前と状態だけで出し（お気に入り、そのあと最後に
  開いた順）、選ぶと今のタブをそのリポジトリに切り替える。

保存は UserDefaults ではなく `~/Library/Application Support/Amon Hen/bookmarks.json`（構造があり、手で直せるように）。スキャンの結果は保存せず、起動時に裏で取り直す。

## 3. 設定画面（Settings…、⌘,）

macOS 標準の、上にアイコン付きのタブが並ぶ形（`NSTabViewController` の `.toolbar` スタイル、中身は SwiftUI の `Form`）。今アプリメニューにあるトグルはここへ移す。

| タブ | 項目 |
| --- | --- |
| General | 起動時に前回のリポジトリを開く、開いたリポジトリをブックマークに足す、リポジトリを開く既定（タブ / ウィンドウ）、外部エディタ・ターミナル・差分ツールの選択 |
| Git | git の実行ファイルの場所と版、グローバルの `user.name`・`user.email`・署名の鍵（`git config --global` を読み書き）、pull の既定（merge / rebase / ff-only）、fetch の `--prune`、自動 fetch の間隔、push 前の確認 |
| Accounts | GitHub・GitLab などのアカウント（後述）。トークンはキーチェーン |
| Appearance | テーマ（システム / ライト / ダーク）、差分とメッセージのフォントと大きさ、History の 1 行表示・アバター・祖先の強調、グラフの色 |
| Diff & Merge | unified / split の既定、空白の無視、前後の行数、折り返し、タブ幅、マージエディタの既定の並び（後述）、diff3 |
| Commit | 件名の文字数の目安（50 / 72）、Sign-off を付ける、Conventional Commits の補完、コミット後に push |
| AI | 使う AI（なし / Apple Intelligence（端末内）/ Claude Code / Codex。既定は、端末内のモデルが使えればそれ、なければ「なし」）、選んだ CLI の場所・版・ログインの状態（見つからなければ入れ方を案内）、モデル、翻訳先の言語、リポジトリごとに送らない設定。詳細で用途ごとのプロンプト（9 章） |
| Performance | commit-graph の自動書き出し、fsmonitor、永続キャッシュの場所と大きさ・消去、裏のタブの扱い |
| Advanced | SSH 先の署名の検証、daemon の送り込み先、ログの詳しさ、計測の記録 |

## 4. リポジトリの設定（Repository Settings…、⌘;）

リポジトリごとのシート。git の設定は「実際に効いている値」と「どこで決まっているか」（`git config --show-origin`：system / global / local / worktree）を並べ、local で上書き・上書きをやめる、を選べるようにする。

| タブ | 項目 |
| --- | --- |
| General | 表示名・色ラベル、既定のブランチ、自動 fetch の有無 |
| Identity | `user.name`・`user.email`・`user.signingkey`・`commit.gpgsign` の上書き（仕事用と個人用を分けるため） |
| Remotes | 追加・名前の変更・URL の編集・削除・prune。GitHub などに紐づくかを表示 |
| Branches | 上流の設定、`pull.rebase`・`rebase.autoStash`・`push.autoSetupRemote` |
| Ignore | `.gitignore` と `.git/info/exclude` の編集（パターンに合うファイルの例を横に） |
| Advanced | `core.autocrlf`・`core.fileMode`・LFS・hooks の一覧（有効 / 無効）、submodule の更新方針、すべての local 設定の表（キーと値を直接編集） |

## 5. ツールバー

参考: Fork（Fetch・Pull・Push・Stash・Pop の並び）、Tower（操作ごとのメニュー付きボタン）。

| 群 | 項目 | クリック | ▾ のメニュー |
| --- | --- | --- | --- |
| 場所 | ブランチ（タイトルの位置） | ブランチの切り替えパネル（あいまい検索、最近のブランチが上） | —（名前の変更・上流は Repository メニュー） |
| 同期 | Fetch | 既定のリモートを fetch | すべてのリモート、prune 付き、タグも |
| | Pull | 確認のシート: 取り込むコミットと手元にだけあるコミット、取り込み方（merge / rebase / ff-only、既定は設定）とその結果（マージコミットができる・ID が変わる・ff-only では取り込めない）、作業中の変更があれば注意 | — |
| | Push | 確認のシート: 送るコミット。上流に手元にないコミットがあれば Pull を勧める（Force Push… はそこからだけ。上流から消えるコミットを見せる）。上流がなければ作るか確かめる | — |
| 統合 | Merge | 選んでいるコミット・ブランチを今のブランチへ（確認シートで no-ff・squash を選ぶ） | 最近のブランチの一覧 |
| | Rebase | 今のブランチを選んだものの上へ | interactive（History の編集モードに入る）、onto |
| 退避 | Stash | 退避（メッセージ入力つき、⌥ クリックで即退避） | untracked も、stage 済みだけ、選んだファイルだけ |
| | Pop | stash の名前とメッセージを見せて確かめてから、最新を pop | —（選んで apply・pop するのはサイドバー） |
| 状態 | Activity | 実行中の操作の数とスピナー。押すと Activity | — |
| | Refresh | status・ref・History を読み直す（⌘R） | 作業ツリーを全部読み直す（fsmonitor と status のキャッシュを使わない）、永続キャッシュを捨てて History を作り直す |
| | Undo | 操作の名前を見せて確かめてから戻す | —（操作ログは Activity） |
| 検索 | 検索欄（`NSSearchToolbarItem`） | ⌘F で History の検索、⌘⇧F で範囲つき | 件名・本文 / 作者 / ID / パス / 内容（pickaxe）の切り替え |

- Refresh は残す。変更は自動で検知するが、ネットワーク上のファイルシステム・SSH 先・ほかのツールが `.git` を直接書き換えたときなど、検知が漏れる場面で確実に揃えるため。押したら読み直しの間だけボタンを回す。
- `allowsUserCustomization = true` にして、Branch・Merge・Rebase・Cherry-pick・Terminal・Finder・GitHub で開く などを足し引きできるようにする。
- ahead/behind は Push・Pull のバッジ（macOS 26 以降）とブランチ名の横の両方に出す。
- 使えないときは無効にし、ツールチップに理由（「上流がありません」「コンフリクトの解決中です」）。
- Pull・Push・ブランチに ▾ のメニューは付けない。アイコンを 1 回押しただけで、外に送ったり履歴を書き換えたりしないこと。外に出る・ブランチが動く操作（Pull・Push・Pop・Undo）は、何が起きるかをシートで見せる。↩ で実行するので、キーボードからは 1 打増えるだけ。Fetch・Refresh はすぐ実行する（手元のものを変えない）。メニューのショートカット（⌃⌘L・⌃⌘P）も同じシートを出す。Commit and Push は重ねて聞かない。
- 同じものは 1 か所に: ツールバーに検索欄があれば History の中の検索欄は隠す（一致の件数・前・次・絞り込みは History に残す）。ツールバーに Branch があれば、ウィンドウのサブタイトルにブランチ名を出さない。
- ネットワークを使う git が 120 秒なにも出さなければ（ふつうは誰にも見えない認証の入力待ち）、そう書いたエラーで止める。Activity から取り消すこともできる。

## 6. Working Copy

参考: Tower・JetBrains（チェックボックスの 1 群の一覧、コミットの入力が上）。

### 左の列（上から）

```
┌ コミット ───────────────────────────────┐
│ [件名                         ] 42/50   │
│ ┌ 本文（入力に合わせて伸びる）───────┐ │
│ └────────────────────────────────────┘ │
│ ☐ Amend  ☐ Sign-off   [Commit ▾] ⌘↩   │
├ 変更 ─────────────── [≡ List|🌲 Tree] ┤
│ 🔍 絞り込み                            │
│ ☐ Changes (4 / 2 を含める)             │
│   ☑ M  src/lib.rs            +12 −3    │
│   ▣ M  src/main.rs           +5  −1    │  ← 一部の行だけ含める
│   ☐ M  README.md             +1  −1    │
│   ☐ ?  notes.txt                       │
└────────────────────────────────────────┘
```

### チェックボックスの 1 群

Staged / Unstaged の 2 群をやめ、変更のあるファイルを 1 つの一覧にして、各行のチェックボックスで「次のコミットに含めるか」を示す。チェックボックスは index（stage）の状態そのもので、FE だけの選択状態は持たない（ターミナルで `git add` してもそのまま反映される）。

| 状態 | 意味 | 押したとき |
| --- | --- | --- |
| ☑ | 作業ツリーの変更をすべて stage 済み | ファイルごと unstage |
| ▣ | 一部だけ stage 済み（行・hunk 単位で含めた） | すべて stage |
| ☐ | stage していない | すべて stage |

- 見出しの行（Changes）とディレクトリの行も 3 状態のチェックボックスで、配下をまとめて切り替える。
- 見出しに「全体の数 / 含める数」。Commit は含めるものがないと押せない（Amend を除く）。
- 新しいファイル（?）・削除・名前の変更も同じ扱い。コンフリクトのファイルはチェックボックスの代わりに ⚠ を出し、押すとマージエディタを開く。

- コミットの入力を上に置く（JetBrains・GitHub Desktop と同じく、「何を書くか」を先に見せる）。入力欄は畳める（件名の 1 行だけにする）。
- 一覧の見せ方を 3 つから選ぶ: List（パスを 1 列）、Tree（ディレクトリの木、1 つしか子がない段はまとめる。済み）、Flat（ファイル名と、薄い字で親ディレクトリ）。並び順はパス / 状態 / 拡張子。
- 絞り込み欄（パスのあいまい検索）。数百ファイル変わったときに効く。
- 各行に +/− の行数、状態のアイコン（M / A / D / R / ? / U）、行のホバーで Discard のボタン。Space で選んだファイル（複数可）のチェックを切り替え、⌫ で破棄（確認つき）。

### コミットを書きやすくする

| 機能 | 内容 |
| --- | --- |
| 文字数 | 件名の残りを表示し、50 で注意・72 で警告の色。本文は 72 桁の目安線 |
| 下書き | リポジトリごとに自動保存。コミットするまで消えない。`commit.template` があれば初期値に |
| 過去のメッセージ | 件名の欄で ⌥↑ / ⌥↓ で最近のメッセージを呼び出す。▾ で一覧 |
| 補完 | `Co-authored-by:`（過去の作者から）、`#123`（GitHub の issue）、Conventional Commits の型（`feat:` など）、ブランチ名に含まれる issue 番号 |
| Commit ▾ | Commit / Commit and Push / Amend / Amend（メッセージはそのまま）/ Fixup! 選んだコミット |
| Amend | オンにすると前のメッセージを入れ、差分の上に「HEAD との差」を出す。push 済みなら警告 |
| hooks | pre-commit などの出力は進捗をシートで見せ、失敗したら出力を残してメッセージは消さない |
| AI | 「メッセージを提案」（stage 済みの差分から）。後述 |
| 無効の理由 | Commit を押せないときは、ボタンの横に理由（「stage したファイルがありません」） |

### 右の差分で細かく stage する

参考: GitHub Desktop の行のガターのチェック、JetBrains の行ごとの含める・含めない。

一覧が 1 群になるので、差分も 1 本にする。右には HEAD → 作業ツリー の差分を出し、変わった行ごとに「次のコミットに含めるか」（index に入っているか）をガターのチェックで示す。

```
 ☑ ▾ @@ fn parse @@                         [Discard hunk]
     12   let a = 1;
 ☑ − 13   let b = 2;
 ☑ + 13   let b = 3;
 ☐ + 14   dbg!(b);          ← 作業ツリーにだけある行
```

- コアで HEAD → index と index → 作業ツリーの 2 つの差分を重ね、HEAD → 作業ツリーの各変更行に「stage 済みか」を付けて返す（`working_diff_combined`、行の `flags` の `STAGED`）。index が「HEAD に変更の一部を当てたもの」として表せないファイル（index にだけある変更が作業ツリーで書き戻されたなど）は、ファイルごと従来の Unstaged / Staged の切り替えで見せる。
- 切り替えは「HEAD にどの変更を当てたものを index にするか」を作り直して書く（`set_rows_staged`）。すべて含めるなら `git add`、すべて外すなら `git reset` と同じにする（新しいファイル・消したファイルのため）。
- hunk の見出しのチェックは 3 状態で、押すと hunk ごと含める・外す。
- 行のチェックを押すとその行を切り替える。ガターをドラッグで範囲、⌘ クリックで追加して選び、Space でまとめて切り替える。
- 変更のまとまり（連続する − と ＋）の境目をクリックすると、そのまとまりだけを選ぶ。
- キー: Space 選んだ行（なければ今の hunk）のチェックを切り替え、`D` 破棄、`J` / `K` で次・前の hunk、`⌥J` / `⌥K` で次・前のファイル。
- 切り替えた後は同じ位置に留まり、次の hunk にカーソルを移す（楽観的に表示を更新し、status の確定で合わせる）。
- 表示の切り替え（差分の上の帯）: unified / split、空白の無視、前後の行数（3 / 10 / 全部）、折り返し。
- Discard は oplog に内容を残し、Undo で戻せる（今の作業ツリーの内容を blob に書いておく）。

## 7. History の右の列

### メッセージを全文読める

- 16 行で畳むのをやめ、既定は全文。右の列は上（詳細）・中（変更ファイル）・下（差分）の境目をドラッグでき、位置を覚える。
- 詳細の欄の右上に「⤢」: 押すと詳細だけを右の列いっぱいに広げる（もう一度でもとに戻す）。
- 本文は選択・コピー・検索（⌘F が右の列にフォーカスがあるときはこの中を探す）ができる。

### GitHub の PR のような差分表示

今の「ファイルを選ぶと、その差分だけ出す」に加えて、「全ファイルを縦に連ねる」表示を選べるようにする。

```
┌ 変更ファイル（木）──┬ 差分（全ファイルを縦に）──────────────────────────────┐
│ ▾ src               │ ▾ src/lib.rs                      +12 −3  ☐ Viewed  ⋯ │ ← 見出しは上に張り付く
│    lib.rs   +12 −3  │   @@ fn main @@                                       │
│    main.rs  +1  −0  │   …                                                   │
│ ▾ docs              │ ▾ src/main.rs                     +1 −0   ☐ Viewed  ⋯ │
│    design.md +40    │   …                                                   │
│ 3 files +53 −3      │ ▸ Cargo.lock（生成物のため畳んでいます）  +210 −180   │
└─────────────────────┴───────────────────────────────────────────────────────┘
```

- 左の木で押したファイルへ飛ぶ。スクロールに合わせて木の選択が動く。
- ファイルごとに畳める。lockfile・`linguist-generated`・バイナリ・1000 行を超えるものは最初から畳む。
- 「Viewed」（見たもの）を付けると畳み、木に ✓。コミットごとに覚える（複数のコミットをまたいだレビューに使う）。
- 見出しの ⋯: blame、ファイルの履歴、このコミットの時点の内容を開く、エディタで開く。
- 仕組み: コアで全ファイルの差分を一度だけ取って持ち（`StackedDiff`、ファイルが多ければ並列に取る）、「ファイルの見出しの行（`DiffRow::FILE`）＋各ファイルの行」を 1 本につないだ表示を作る。文字列はファイルの中身を共有したまま。畳む・開くたびに表示だけを作り直す。差分表示の Metal の仕組みはそのまま使える。linux でコミットを選んでから差分が出るまでの時間は、1 ファイルずつのときと変わらない（p50 15〜16 ms）。
- 単体のファイル表示（今のもの）は残し、見出しの切り替えで選ぶ。

### 複数のコミットを選んだとき

| 選び方 | 見せるもの |
| --- | --- |
| 連続する範囲（⇧ クリック、1 本の first-parent の上） | 一番古いものの親から一番新しいものまでの差分（squash した結果と同じ）。上に「5 commits · 2 authors · 3 days · +120 −40」と、含むコミットの一覧（押すと単体に） |
| 2 つ（⌘ クリック） | 2 つの間の差分（A..B）。⇄ で向きを入れ替え、「merge-base から」（A...B）も選べる |
| 3 つ以上で飛び飛び | コミットごとの差分をファイル単位でまとめる（ファイルの見出しの下に、どのコミットの変更かを小見出しで） |

- 選んでいる間の右クリック: cherry-pick（並びを保って）、revert、squash（連続のとき）、ID をまとめてコピー、patch として保存。
- 範囲の端が merge をまたぐときは first-parent の範囲として扱い、そう書いて出す。
- 仕組み: コアの `Selection::of` が History のグラフで first-parent のつながりを調べ、`selection_diff` がバックエンドの `changes_between`（2 つのコミットの木の差分）で全ファイルの差分を作る。とびとびのときは、各コミットの差分を「パス — 短い ID」の見出しでパス順・古い順に並べる。

## 8. コンフリクトの解決（埋め込みのマージエディタ）

### 流れ

1. merge・rebase・cherry-pick・stash pop でコンフリクトが出たら、ツールバーの下に帯を出し、サイドバーの Conflicts を選んだ状態にする。
2. Conflicts の画面は「左: コンフリクトのファイルの一覧（未解決 / 解決済み）」「右: マージエディタ」。別ウィンドウにはせず、⤢ で切り出せるだけにする。
3. すべて解決したら帯の Continue が強調される。Continue は rebase なら次のコミットへ進み、新しいコンフリクトがあればそのまま次のファイルを開く。
4. 帯からいつでも Abort（oplog に残るので Undo でも戻せる）。

### エディタの並び（JetBrains に合わせる）

```
┌ Ours（main）─────────┬ Result ──────────────────┬ Theirs（feature）────┐
│                      │                          │                      │
│  変更 A   ≫ ✕ ───────┼──▶                       │                      │
│                      │      ◀──────────────────┼─ ✕ ≪   変更 B       │
│  衝突     ≫ ✕ ═══════╪══ ⚠ 衝突 ═══════════════╪═ ✕ ≪   衝突         │
└──────────────────────┴──────────────────────────┴──────────────────────┘
 ✨ Magic Merge   ← → 次・前の変更   Base を表示   Accept Ours / Accept Theirs   変更 4・衝突 1   [Save & Mark Resolved]
```

- 3 列の間を変更ごとの帯（曲線）でつなぎ、色で種類を示す: 追加は緑、変更は青、削除は灰、衝突は赤。
- 各変更に「≫ / ≪ 取り込む」「✕ 捨てる」。衝突は両方を取り込むと順に並ぶ（どちらを先にするかは ⌥ で）。
- Result は直接編集できる（`NSTextView`、日本語入力は OS 標準）。
- Base は既定では隠し、「Base を表示」で 4 列目（または Ours・Theirs の各列に Base との差として重ねる）。
- 今の「上に 3 ペイン・下に Result」の並びはやめ、3 列だけにする。3 列の縦のスクロールは揃え、変更の位置を合わせる（行数の差は帯で吸収する）。

### Magic Merge（自動で片づける）

JetBrains の「Apply non-conflicting changes」と「Resolve simple conflicts」に当たるもの。✨ を押すと次を順に適用し、適用したものは帯の色を薄くして残す（1 つずつ戻せる）。

1. 衝突しない変更（片方だけが変えたところ）をすべて取り込む。
2. 両方が同じ変更をしたところは 1 つにする。
3. 空白・改行コードだけが違う衝突は、設定の側（既定は Theirs の内容を Ours の空白で）にする。
4. 同じ行の中で別々の単語を変えた衝突は、単語単位で 3-way マージする（単語の境目は差分表示の単語差分と同じ）。
5. 片方が行を足しただけ（import の追加など）で並びが衝突したものは、両方を並べる（言語の文法が分かるときは tree-sitter で import・use の並びだと判定できたときだけ）。

- ファイルを開いたときに 1・2 だけは自動で適用する（設定で切れる）。残った衝突の数を一覧とタブに出す。
- 計算はすべてコア（`mergeSides`・`resolveText`・`conflictRegions` に足す）。単語単位のマージと判定はテストで固める。
- 一覧の右クリックで「このファイルは Ours / Theirs で解決」「外部のマージツールで開く」。
- Magic Merge の後に残った衝突は、AI で解消を提案できる（9 章）。エディタの下の帯の ✨ Magic Merge の隣に「AI で解消」。

## 9. AI（翻訳・解説・メッセージの提案・コンフリクトの解消）

| 場所 | ボタン | 動き |
| --- | --- | --- |
| History の詳細 | 翻訳 | 件名と本文を設定の言語に訳し、原文の下に折りたためる形で出す |
| History の詳細 | 解説 | コミットの差分とメッセージから「何をなぜ変えたか」「影響しそうな箇所」を要約。複数選んだときは範囲全体を |
| Working Copy | 提案 | stage 済みの差分からメッセージを提案（リポジトリの過去の書き方に合わせる） |
| 差分の hunk | 解説 | その hunk だけ |
| マージエディタ | 解消 | 衝突 1 つ・ファイルの残りの衝突すべて・全ファイルのどれかを対象に、解決案を作る（下の「コンフリクトの解消」） |
| マージエディタ | 解説 | 衝突 1 つについて、Ours と Theirs それぞれの意図（各側のコミットのメッセージと差分から）を説明する |

### コンフリクトの解消

- 順番は Magic Merge が先、AI は残った衝突だけ。機械的に決まるものを AI に渡さない（速く、間違いが入らない）。
- 送るもの: 衝突の Base・Ours・Theirs と前後の行、ファイルのパスと言語、両側で衝突に関わるコミットの件名と本文（merge なら `merge-base..` の各側、rebase なら当てているコミット）。
- 返させる形は決めておく（衝突ごとの解決後のテキストと、短い理由。JSON）。形が合わない・衝突の数が合わない返答は捨てて、エラーとして出す。
- 結果は Result に「提案」として入れ、紫の帯と理由の吹き出しで示す。衝突ごとに「採用」「却下」「編集して採用」。採用するまで解決済みにしない。一括採用はファイル単位で、確認つき。
- 採用した提案は oplog に残し、Undo で戻せる。
- 提案した内容にコンフリクトの印（`<<<<<<<` など）が残っていたら採用できなくする。言語が分かるときは tree-sitter で構文の壊れを確かめ、壊れていたら警告を出す。
- ファイル全体を送るのは、衝突がファイルの大部分を占めるときだけ。それ以外は衝突の前後に絞る。

- 接続先は手元の CLI で、Claude Code（`claude -p --output-format stream-json --verbose --include-partial-messages --tools "" --no-session-persistence`、空の一時ディレクトリで起動してリポジトリの CLAUDE.md を読ませない）か Codex（`codex exec --json --sandbox read-only --skip-git-repo-check -`）のどちらかを Settings > AI で明示的に選ぶ。見つかったものを勝手に使うことはせず、選ぶまで AI のボタンは出さない。用途ごとには分けず、全体で 1 つ。
- どちらも子プロセスとして起動し、プロンプトと差分は標準入力で渡して、流れてくる JSON を読む。鍵の管理はアプリで持たず、CLI のログインに任せる。
- Apple Intelligence（端末内。macOS 26 で Apple Intelligence を入にした Mac）は Foundation Models を直接使う。Mac の外に何も出ないので、リポジトリごとの確認はしない。一度に読めるのは約 4,000 トークンなので、いちばん長い変数（ふつうは差分）を詰める。翻訳や短い解説に向き、大きなコミットやコンフリクトの解消は Claude Code・Codex のほうがよい。
- モデルは CLI ごとに覚え、プルダウンで選ぶ（Claude Code は Haiku・Sonnet・Opus・Fable、Codex は `~/.codex/models_cache.json` に一覧で出ているもの。「その他…」で名前を書ける）。`--model`（Codex は `-m`）で渡す。Claude Code の既定は速い `claude-haiku-5-5`、解説だけ `claude-sonnet-5-5` を選べる。
- ツールを使わせない設定で起動する（Claude Code は使えるツールを空に、Codex は sandbox を読み取り専用に）。解説・翻訳でファイルを書いたりコマンドを実行したりする必要はないため。フラグの正確な名前は実装時に手元の版の `--help` で確かめる。
- 呼び出しはコアの外の小さな crate（`amonhen-assist`）の `Assistant` トレイトに閉じ込め、実装を CLI ごとに 1 つ置く。出力の読み取りは CLI ごとに小さく分け、版を確かめる（Codex の `exec` のイベントの形は変わりやすい）。プロセスの起動は `Platform` から注入する（SSH 先のリポジトリでも、CLI は手元で動かす）。
- 選んだ CLI が見つからない・ログインしていないときはボタンを無効にし、理由を出す。

### プロンプトの設定（Settings > AI > 詳細）

用途ごと（翻訳・コミットの解説・hunk の解説・メッセージの提案・コンフリクトの解消・コンフリクトの解説）にプロンプトを編集できる。

- 各用途に組み込みの既定のプロンプトを持ち、編集したものだけを保存する。「既定に戻す」と、既定との差分の表示がある。
- プロンプトは差し込みの変数を使うテンプレート。使える変数は用途ごとに一覧で見せ、知らない変数はエラーにする（例: `{{diff}}`・`{{message}}`・`{{files}}`・`{{language}}`・`{{base}}`・`{{ours}}`・`{{theirs}}`・`{{ours_commits}}`・`{{theirs_commits}}`・`{{path}}`・`{{recent_messages}}`）。
- 返させる形（JSON の形）はアプリが決めるもので、プロンプトの編集では変えられない。プロンプトの末尾に自動で足す。
- 「試す」ボタン: 今のリポジトリの今の選択で、編集中のプロンプトを実行し、送る内容と結果を並べて見せる（キャッシュは使わない）。
- 保存は `~/Library/Application Support/Amon Hen/prompts/<用途>.md`（手で直せる・dotfiles で持ち歩ける）。リポジトリの `.amonhen/prompts/<用途>.md` があればそちらを優先する（チームやリポジトリごとの書き方に合わせるため）。どちらを使っているかは設定画面とボタンのツールチップに出す。リポジトリのプロンプトは clone したものにも入っているので、初めて使うとき・内容が変わったときに中身を見せて確かめる（許可はリポジトリとプロンプトのハッシュで覚える）。
- キャッシュの鍵にはプロンプトの内容のハッシュも入れる（編集したら取り直す）。
- 送る前に送る内容（差分の大きさ、ファイル数）を見せ、リポジトリごとに「このリポジトリでは使わない」を選べる。既定は無効。
- 結果は流しながら表示し、コミット ID＋種類＋言語＋CLI＋モデル＋プロンプトのハッシュで永続キャッシュに入れる（2 回目は即表示）。
- 大きな差分は、ファイルの一覧と行数だけ＋見えているファイルの差分に絞って送る。

## 10. Activity（内部の動作・履歴・性能）

Window > Activity（⌥⌘A）。ツールバーのスピナーからも開く。リポジトリごとではなく、アプリ全体で 1 つ（左でリポジトリを絞り込める）。

| タブ | 内容 |
| --- | --- |
| Operations | 実行中・済みの操作。git のコマンド行、開始時刻、所要時間、終了コード、標準出力・エラー。実行中のものは進捗（fetch・clone の受信量）と取り消し。失敗したものは赤く、帯の通知からここへ飛ぶ |
| History | 操作ログ（oplog）。操作の名前・前後の HEAD・変わった ref。行を選んで「ここまで戻す」 |
| Performance | 入力から表示までの時間（History のスクロール・選択、差分の表示）のヒストグラムと直近の値、取りこぼしたフレーム、コアの Intent ごとの所要時間、History のチャンクのメモリ、永続キャッシュの命中率。`FrameProfile`・os_signpost と同じ値を表示する |
| Events | FSEvents の通知、status の再計算、daemon との通信量（SSH のとき） |

- コアは操作・計測をリングバッファ（件数の上限つき）に記録し、Activity を開いているときだけ FE が読む（閉じていれば描画のコストはない）。
- 右クリックで JSON に書き出す（不具合の報告と計測の比較用）。

## 11. メニュー

すべての操作をメニューから引けるようにする。ショートカットはほかのクライアントに合わせず、独自に一貫した規則で決める。

| 修飾 | 割り当てる操作 |
| --- | --- |
| ⌘＋文字 | macOS 標準（⌘N・⌘T・⌘O・⌘W・⌘F・⌘Z・⌘,）と、画面の切り替え（⌘1〜3）・Refresh（⌘R）・Commit（⌘↩） |
| ⌃⌘＋文字 | リモートとの同期（Fetch・Pull・Push） |
| ⌥⌘＋文字 | ローカルの書き込み（Stash・Pop・Branch・Tag・Merge・Rebase） |
| ⌘⇧＋文字 | 開く・探す（Quick Open・Find in History・Go to Ref） |
| 修飾なし | 一覧・差分にフォーカスがあるときだけ（Space・`D`・`J`・`K` など） |

- 頭文字に揃える（Fetch は F、Push は P）。同じ頭文字がぶつかったら、使う頻度の高いほうに渡す。
- macOS の「キーボード > キーボードショートカット > アプリのショートカット」でメニュー名から変えられるので、アプリに割り当ての編集画面は作らない。
- 下の表は規則に合わせた案。

| メニュー | 項目 |
| --- | --- |
| Amon Hen | About、Settings… ⌘,、Services、隠す、終了 |
| File | New Window ⌘N、New Tab ⌘T、Open… ⌘O、Quick Open… ⌘⇧O、Open Recent ▸、Clone… ⌃⌘C、Add Remote Repository…、Init…、Close Tab ⌘W、Close Window ⌘⇧W |
| Edit | 標準（Undo・Redo はテキストの取り消し）、Find ⌘F、Find in History ⌘⇧F、Find Next / Previous ⌘G / ⌘⇧G |
| View | Working Copy ⌘1、History ⌘2、Conflicts ⌘3、Refresh ⌘R、Show Sidebar ⌃⌘S、Diff: Unified / Split ⌥⌘D、Ignore Whitespace、Wrap Lines、Font Size ⌘+ / ⌘−、History の表示（1 行・アバター・祖先の強調）、Commit Details: Compact / Standard / Detailed |
| Navigate | Go to HEAD ⌘⇧H、Go to Ref… ⌘⇧J、Go to Parent / Child ⌥⌘↓ / ⌥⌘↑、Next / Previous Hunk、Next / Previous File、Next Conflict |
| Repository | Fetch ⌃⌘F、Pull ⌃⌘L、Push ⌃⌘P、Stash ⌥⌘S、Pop ⌥⌘⇧S、New Branch… ⌥⌘B、New Tag… ⌥⌘T、Merge… ⌥⌘M、Rebase… ⌥⌘R、Interactive Rebase…、Continue / Skip / Abort、Undo（操作名） ⌥⌘Z、Repository Settings… ⌘;、Open in Terminal、Reveal in Finder、Open in Editor、Open on GitHub |
| Commit | Commit ⌘↩、Commit and Push ⌘⇧↩、Amend ⌥⌘↩、Include All、Exclude All、Toggle Selection（Space）、Discard…、Cherry-pick、Revert、Reword…、Create Patch… |
| Window | 標準（最小化・タブ・すべてのウィンドウを結合）、Repositories ⌘⇧R、Activity ⌥⌘A |
| Help | ショートカットの一覧、ログの場所を開く |

- 右クリックのメニューは、同じ操作をメニューバーと同じ名前で出す（覚える名前を 1 つにする）。
- 使えない項目は消さずに無効にする（ある場所を覚えられるように）。
- git の操作の Undo は Repository > Undo（⌥⌘Z）に置き、Edit の ⌘Z（テキストの取り消し）とは分ける。⌘Z で思わぬ git の操作が戻らないように。

## 12. GitHub などとの連携

| 機能 | 内容 |
| --- | --- |
| アカウント | 手元の `gh`（GitHub CLI）に任せる。アプリはトークンに触れず、`gh` を起動して読み書きする（ログインは `gh auth login`）。gh がなければ連携の機能を出さない。GitLab・Gitea は後で |
| リモートの判定 | origin などの URL から GitHub のリポジトリを判定（今の `#123` のリンクと同じ） |
| Pull Requests | サイドバーに一覧（自分のもの・レビュー依頼・すべて）。状態（draft・レビュー・CI）。チェックアウト、ブラウザで開く |
| PR を作る | Push の後に「Create Pull Request…」。タイトル・本文をコミットから（AI の要約も選べる）、base の選択、draft |
| CI の状態 | History のブランチの先頭と各コミットに ✓ / ✗ / ● を出す（見えている範囲だけ、ETag で条件付きで取得し、永続キャッシュに入れる） |
| アバター | GitHub のアカウントがあれば GitHub のアバター、なければ今の Gravatar |
| Clone | 自分のリポジトリの一覧から選んで clone |

- 通信は gh を裏で起動して行い、UI スレッドを待たせない（今は macOS の FE の中。GitLab などを足すときに、`HostingProvider` で差し替えられる形にしてコアの外の crate に移す）。読めないときは Activity の Events に出し、ほかの操作には影響させない。

## 13. 言語

- アプリは英語で書いてローカライズする。最初の訳は日本語（`apps/macos/Localization/ja.lproj/Localizable.strings`。
  `Localization.swift` の `L()` で使う）。言語はシステムの優先する言語に従う。
- git の操作とアプリの画面の名前は、どの言語でも英語のまま（調べるときの言葉で、git 自身の出力にも出るため）:
  Working Copy・History・Activity・Repositories・Branches・Remotes・Tags・Stashes・Pull Requests・Summary・
  Description・Diff・Blame・Commit・Amend・Push・Pull・Fetch・Merge・Rebase・Stash・Pop・Cherry-pick・Revert・Reset・
  Squash・Fixup・Reword・Stage・Unstage・Discard・Clone・Undo・Continue・Skip・Abort・Force Push・Fast-Forward・HEAD。
  そういう語だけのボタンやメニューの項目（「New Branch…」「Stage Lines」）は訳さない。
- それ以外は訳す: macOS の標準のメニューと項目（Apple の訳語）、説明・確認・プレースホルダ・ツールチップ。日本語の文では
  ブランチ・タグ・リモート・上流・コミット・作業ツリー・コンフリクトを名詞として使う。
- コア（Rust）も同じやり方で訳す（`amonhen_model::i18n` の `tr!`、日本語は `crates/model/src/i18n/ja.strings`）。
  言語は文を見せる側が決める: アプリは画面の言語を FFI で渡し、CLI は `LC_ALL`・`LC_MESSAGES`・`LANG` を見て、
  共有の daemon と SSH 先は、接続ごとに挨拶で伝えられた言語で答える（daemon が自分で行うことには、起動した側が
  渡す `AMONHEN_LANG`）。git 自身の文は英語のまま（`LC_ALL=C` で動かす）。
- VSCode 拡張の画面は、まだローカライズしていない。

## 14. Worktree

Claude Code などは worktree をしきりに作る（`.claude/worktrees/` の下など）。そのたびに別のリポジトリとして開くのは
煩わしいので、リポジトリの worktree はそのリポジトリのタブの中に置く。考え方は git の worktree に合わせる。

git の考え方と、アプリが守ること:

- worktree は対等。それぞれが作業ツリーと HEAD・index の組で、元のものは `.git` を持っているだけ。アプリは
  `git worktree list` と同じく、元のものを先頭にするほかは同じに並べる。
- ブランチ・タグ・リモート・オブジェクト・stash の一覧・設定は全部の worktree で共有。HEAD・index・作業ツリーの
  ファイル・途中の操作（rebase・merge など）は worktree ごと。
- git の命令は、実行した worktree に対して働く。アプリは**書き込む先の worktree** を 1 つ持ち（git の「今いる
  worktree」）、HEAD・index・作業ツリーに触れる命令だけをそこに向ける: checkout・commit・amend・stage・discard・
  merge・rebase・pull・reset・cherry-pick・revert・stash（退避と pop）・Undo。リポジトリ全体の命令（fetch、
  ブランチ・タグの作成・名前の変更・削除、リモート）は、どの worktree からでも同じに働く。
- 1 つのブランチは 1 つの worktree でしか checkout できない。アプリは押し通さない（`--ignore-other-worktrees` は
  使わない）: ほかの worktree が checkout しているブランチを選んだら、書き込む先をその worktree に移す。
- 「今のブランチ」は書き込む先の worktree のブランチ: Branches で太字・色付きのアイコン、History で塗った札、
  ツールバーの Branch。ほかの worktree が checkout しているブランチは、Branches に「⌂ worktree」、History で緑の札、
  ブランチの切り替えのパネルに「worktree … で使用中」と出す。
- worktree の作成・削除は git で行う（`git worktree add`・`remove`・`prune`、一覧は
  `git worktree list --porcelain`）。`.git/worktrees` を直接いじらない。変更が残っている worktree の削除は確かめて
  から `--force`。ロック中のものはロック中と見せ、片付けない。フォルダがなくなったものは片付け待ちと見せる。
- Undo の記録は worktree ごと（`<git dir>/amonhen/undo.json`。追加した worktree は自分の git dir を持つ）で、HEAD
  と同じ。共有のブランチを動かした操作を戻すと、どの worktree からもそう見える。

見るときは混ぜ、書くときは 1 つを名指す:

- **見る（全部の worktree をまとめて）**。サイドバーの Worktrees に、全部の worktree をブランチと変更の数つきで
  並べる。History では、各 worktree の HEAD のコミットに札を付ける（緑。ブランチの札の前。書き込む先は ✎、ほかは ⌂、
  変更のあるファイルの数つき）。どの worktree がどこにいて、どれだけ書き換えているかが 1 画面で分かる。Working Copy には全部の worktree の変更を worktree ごとの見出しで
  並べ（変更のないものは畳む）、どの差分も読める。
- **書く（1 つの worktree。いつも見せる）**。ツールバーに書き込む先（✎ 名前）をブランチと並べて出す。サイドバーか
  ✎ で worktree を選ぶと、それが書き込む先になる。ほかの worktree のファイルを stage・discard しようとしたら、先に
  「<worktree> で編集しますか？」と聞く。読むだけなら変えない。確認のシートには worktree を書く（「gui-gtk4 に
  Pull」）。
- **ほかの場所**。リポジトリのページと一覧では、worktree を別のリポジトリとして出さず、そのリポジトリの下に出す
  （スキャンで見つかったものも）。worktree のフォルダを開いたら（CLI・Finder）、そのリポジトリのタブを、その
  worktree を書き込む先にして開く。CLI は git と同じく実行した worktree に対して働き、`amonhen worktree` は
  `git worktree` と同じ並び（list・add・remove・prune）。
- New Worktree…（ブランチかコミットから、フォルダを選んで）、Remove Worktree、Prune、Finder で表示、ターミナルで開く。

## 実装の順序

体感の効果が大きく、依存の少ないものから。各ステップで `docs/design.md` の「実装順序」と同じくチェックボックスを更新する。

- [x] U1. メニューの網羅とショートカットの整理、Settings ウィンドウ（今のトグルを移す）。メニュー、規則に沿ったショートカット、Settings、File > Clone…（⌃⌘C。gh があれば自分の GitHub のリポジトリから選べる。clone したらブックマークに入れてタブで開く）は済み
- [x] U2. ツールバーの再編（メニュー付きボタン、Merge・Rebase・Pop、Activity のスピナー、検索欄、カスタマイズ）。Pull ▾・Push ▾・Merge・Rebase・Stash・Pop・Activity・Refresh・Undo、ブランチ（押すと切り替えのパネル ⌘⇧B: あいまい検索、最近切り替えたものが上、リモートは追跡ブランチを作る、一致しなければ新しいブランチ。▾ で New・Rename・Set Upstream）、History の検索欄（⌘⇧F、▾ で author:・path:・contents:）、カスタマイズは済み。のちに Pull・Push・ブランチの ▾ のメニューはやめ、確認のシートにした（5 章）
- [x] U3. Working Copy の左の列の再配置（コミットの入力を上、チェックボックスの 1 群、List / Tree / Flat、絞り込み）とコミットの入力の強化。List / Tree / Flat、行ごとの +/−（コアの `workingStats`）、ホバーの「変更を捨てる」、リポジトリごとの下書き、`commit.template` から始める、⌥↑・⌥↓ と 🕘 の一覧で最近のメッセージ、⊕ で Conventional Commits の型・Co-authored-by（最近の作者）・Signed-off-by、Commit ▾（Commit and Push ⌘⇧↩、Amend）、本文の 72 桁の目安線、`#` を打つと開いている Issue・PR の一覧（gh、5 分覚える。IME で打ったときは確定してから）、Commit ▾ の Fixup Commit For・Squash Commit For（最近のコミットから選ぶと fixup!・squash! の件名を入れる）、hooks の出力のシート（成功しても hooks が書いたものがあれば見せる。止まったときも等幅で読める）も済み
- [x] U4. HEAD → 作業ツリーの 1 本の差分と行ごとのチェック（コアで 2 つの差分を重ねる）、ガターでの行選択・キー操作。行・hunk のチェック、Space、重ねられないファイルでの Unstaged / Staged の切り替えは済み。J / K で次・前の hunk、⌥J / ⌥K で次・前のファイル、含める・外すの後に次の hunk を選ぶ、も済み
- [x] U5. History の詳細の全文表示と、境目のドラッグ。本文は畳まずに全文、境目の位置を覚える、⤢ で詳細だけを広げる。詳細の欄にフォーカスがあるときの ⌘F（欄の中を探す）も済み
- [x] U6. History の PR 風の差分（全ファイルを縦に、Viewed）。ファイルの見出し（畳む・開く、行数、Viewed）、上に張り付く見出し、一覧の選択とスクロールの連動、lockfile と 1000 行超えを最初は畳む、200 ファイルを超えたら 1 ファイルずつ、View メニューと Settings で切り替えは済み。Viewed を次の起動まで覚える（500 コミットまで）、見出しの右クリック（畳む・Viewed・Blame・File History・パスのコピー・Finder・既定のアプリ）、変更ファイルを差分の左に木で出す（View > Changed Files as Tree と Settings で切り替え。子が 1 つのフォルダはつなげる）も済み
- [x] U7. History の複数選択（範囲・2 つ・飛び飛び）。⇧・⌘ クリックで選び、コアで見せ方を決めて（`Selection`）全ファイルを並べた差分で見せる。上の欄に選んだコミットの一覧と差分の取り方。2 つのときの取り方の切り替え（A..B・向きを入れ替える・merge-base から A...B。上の欄のリンクで）、複数選択の右クリック（古い順に cherry-pick、新しい順に revert、連続した範囲の squash（メッセージは古い順に並べたものから編集）、ID のコピー）も済み
- [x] U8. マージエディタの埋め込み・3 列の並び・帯、Magic Merge（コアの単語単位のマージ）。Working Copy の右に埋め込み（⧉ で別ウィンドウへ）、Current・Result・Incoming の 3 列（Base は切り替え）、コンフリクトをつなぐ帯と ≫・≪、コンフリクトの位置で揃えるスクロール、Magic Merge の規則 1〜5（import の並びの判定を含む）、ウィンドウの帯（途中の操作・コンフリクトの数・Continue / Skip / Abort）、サイドバーの Conflicts（数のバッジ、押すと最初のファイルを開く）、開いたときに間違えようのない規則だけを自動でかける（設定で切れる）、Magic Merge をコンフリクト 1 つずつ ⌘Z で戻せる、まで済み
- [x] U9. ウィンドウのタブと、開いていたタブの復元。開くリポジトリは前のウィンドウのタブに入れ、⌘N だけ新しいウィンドウ。タブバーの「+」と ⌘T、Window メニューのタブの操作、ウィンドウごとのタブの並びの記録と復元、裏のタブでは作業ツリーの読み直しを前に出るまで待つ、タブの名前の ●・↑↓、裏になったタブの History は見えている所のチャンクだけを残す（linux を 6,000 行ほど読んだ後に裏にすると 397 → 356 MB）、まで済み
- [x] U10. ブックマーク（グループ、フォルダのスキャン、状態の表示、Quick Open）。Favorites・Recent・Bookmarks・手で作ったグループ・スキャンするフォルダの見出し、右クリック（お気に入り・名前・グループ・Finder・ターミナル）、スキャン（深さ・除外、一覧を開くたびに探し直す）、行の状態、Quick Open（⌘⇧O・⌘T・⌘N、あいまい検索）、スキャンするフォルダの FSEvents での見張り（リポジトリが増えた・消えたら探し直す。見つけたリポジトリの中の変化は見ない）、ドラッグでの並べ替え（ブックマーク・グループ・お気に入り）、色ラベル、選んだもの・見出しの中のすべての一括 Fetch（4 つずつ、失敗は行に赤で）、開いたリポジトリの自動の登録（スキャンするフォルダの下は除く。Settings で切れる）も済み
- [x] U11. リポジトリの設定（git の設定と出どころ、Remotes、Ignore）。Repository > Repository Settings…（⌘;）のシートに Identity・Branches・Remotes・Ignore・All Settings。各項目に今効いている値とどこで決まっているか、このリポジトリ（local）での上書き・取り消し、リモートの追加・名前の変更・URL・削除・prune、`.gitignore` と `.git/info/exclude` の編集、General（表示名・色ラベル・開いている間の自動 fetch。アプリの中だけの設定でブックマークに置く）、Hooks・LFS（.git/hooks か core.hooksPath の hook と実行できるか、git lfs が入っているか、.gitattributes の LFS のパターン）、Ignore の各行に合うファイルの例と数（保存する前の中身で、git ls-files -i に聞く）も済み
- [x] U12. Activity（コアのリングバッファ、Operations・History・Performance・Events）。Window > Activity（⌥⌘A）とツールバーの Activity（操作の間は回る）。Operations は git の起動とコアの命令（リポジトリ・内容・時間・成否・エラー、絞り込み、JSON に書き出し）、History は操作ログと「ここまで戻す」、Performance は History の入力 → 表示・選択 → 差分・Working Copy の読み直しの p50 / p95 / 最大、Events はファイルの変化の知らせ。SSH 先の daemon で起動した git の記録（2 秒ごとに daemon から読む）、fetch・pull・push・clone の進み具合（`--progress` の行を流しながら拾う）と取り消し（プロセスグループごと止める。SSH 先も daemon に頼む）、操作の失敗をウィンドウの帯で知らせる（fatal:・error: の行、詳細・Activity・閉じる。取り消したときも）も済み
- [x] U13. GitHub 連携（アカウント、PR の一覧と作成、CI の状態）。gh に任せる。origin の URL から GitHub のリポジトリを判定し、サイドバーの Pull Requests に開いている PR（CI の ✓・✗・●、draft、レビューの状態）、右クリックでチェックアウト・ブラウザで開く・URL のコピー、選ぶとそのブランチの履歴。Repository > Create Pull Request…（今のコミットのメッセージから、base は既定のブランチ）と Open on GitHub、History のコミットごとの CI の状態（今のブランチと既定のブランチの直近 100 コミットを GraphQL で 1 回に読み、署名の印の右に ✓・✗・時計）、GitHub のアバター（同じ問い合わせで作者の avatarUrl を取り、Gravatar より優先）、自分のリポジトリから Clone（U1）も済み
- [x] U14. AI（Claude Code / Codex の CLI 経由の翻訳・解説・メッセージの提案、コンフリクトの解消、用途ごとのプロンプトの設定）。Settings > AI で CLI（既定は使わない）・モデル・言語を選び、用途ごとのプロンプトをファイルで編集・既定に戻す。History の詳細の翻訳・解説（複数選択ではまとめた差分）、Working Copy の ✨ 提案（「メッセージに入れる」）、マージエディタの「AI で解消」（JSON で受け、マーカーが残れば捨てる。「Result に入れる」で置き換え、⌘Z で戻せる）と「AI で解説」、結果は流しながら出し、キャッシュする。リポジトリごとの初回の確認、リポジトリのプロンプトの確認、差分の hunk の右クリックからの解説（用途「hunk の解説」、History・Working Copy）、Settings のプロンプトの「試す」（見本の差分・メッセージで今のプロンプトを送る）も済み。Codex はこの Mac に入っていないので、出力の読み取りは確かめていない
- [x] U15. コアと CLI の worktree: 一覧（`git worktree list --porcelain`）・作成・削除・片付け（SSH 先も）、
  `amonhen worktree`。
- [x] U16. worktree を見る: サイドバーの Worktrees（ブランチと変更の数、増減をすぐ反映）、リポジトリのページで
  リポジトリの下にまとめる、worktree のフォルダを開いたらそのリポジトリのタブを開く。
- [x] U17. 1 つの worktree に書く: 書き込む先（ツールバーの ✎）、HEAD・index・作業ツリーに触れる命令をそこへ、
  確認のシートに worktree を書く、ほかの worktree が checkout しているブランチで書き込む先を移す、New Worktree…・
  Remove Worktree・Prune。
- [x] U18. Working Copy に全部の worktree の変更を worktree ごとの見出しで並べる、ほかの worktree に書く前に聞く。
- [x] U19. History で、各 worktree の HEAD に札（名前と変更のあるファイルの数、書き込む先は ✎）。worktree が
  コミットしたり、ファイルを書き換えたりしたら付け直す。

## 決めたこと

| 論点 | 決定 |
| --- | --- |
| Working Copy の一覧 | チェックボックスの 1 群（index の状態をそのまま示す。一部だけのときは ▣） |
| History の差分の既定 | 全ファイルを縦に。巨大なコミット（ファイル数 200 超など）は自動で単体のファイル表示に（案のまま進め、使ってみて見直す） |
| マージエディタの並び | Ours・Result・Theirs の 3 列だけ |
| AI の接続先 | 手元の CLI。Claude Code と Codex のどちらかを設定で明示的に選ぶ（既定は「なし」、用途ごとには分けない） |
| ショートカット | 独自の規則で決める（ほかのクライアントには合わせない） |
| Refresh | ツールバーに残す（⌘R） |
