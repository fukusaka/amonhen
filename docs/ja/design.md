# Amon Hen Standalone 計画

[English](../design.md)

## 概要

VSCode 拡張の Amon Hen と同じ操作感を、VSCode の UI 部品の制約なしに作り込んだネイティブの Git クライアント。徹底的な低遅延を最優先にする。コアは Rust で書いて各 OS で使い回し、最初の FE（フロントエンド）は macOS で作る。拡張と同じく自分用で、GitHub Releases で配布する。

| 方針 | 内容 |
| --- | --- |
| 速度 | 入力から表示まで 1 フレーム（120Hz で 8.3 ms）以内。UI スレッドで git を待たない |
| 構成 | コアロジックは Rust、UI は各 OS のネイティブ。まず macOS（AppKit）だけ |
| 使い回し | コアは macOS / iOS / Windows / Linux / Android で動かす。FE に渡すデータの形式は FE ごとに最適なものを選べる |
| 妥協しない | SwiftUI・Qt・WebView は重い画面に使わない。History と差分は自前で描画する |
| 段階的に広げる | コアは同一プロセス・ローカル daemon・SSH 越しのどれでも動く形にしておき、最初は同一プロセスだけ作る |

### 要件

拡張の要件（vscode-amonhen の `docs/design.md` の「概要」）をすべて引き継ぎ、以下を足す。

| 要件 | 区分 |
| --- | --- |
| 10 万〜100 万コミットのリポジトリで History を 120fps でスクロールできる | 必須 |
| 起動直後に前回の History を即座に表示する（永続キャッシュ） | 必須 |
| 日本語入力・アクセシビリティは OS 標準の挙動 | 必須 |
| コアを macOS / iOS / Windows / Linux / Android で流用できる | 必須 |
| FE ごとに最適なデータ形式（文字列の符号化・列・GPU の頂点形式）を選べ、変換で無駄なコピーをしない | 必須 |
| History のグラフ表現の工夫（幹線の強調、祖先のハイライト、折りたたみ、アニメーション） | 必須 |
| CLI（`amonhen`）から同じコアを使える | 希望 |
| SSH 先のリポジトリを手元と同じ速さで操作できる | 希望 |
| VSCode 拡張の git 実行層・状態管理層をコアの daemon に置き換えられる | 希望 |

### 非目標

- macOS 以外の FE（コアと抽象化は最初から対応させ、全ターゲットのビルドを CI で通すが、FE は当面作らない）
- Qt・Electron・Tauri・Flutter による実装（検討の上で外した）
- 全画面を Metal で描くこと（入力欄・設定・ダイアログは標準部品）

## 技術選定

| 層 | 選択 | 理由 |
| --- | --- | --- |
| コア | Rust | GC 停止がなく、メモリ配置を制御できる |
| git の読み取り | gix（gitoxide）を同一プロセスで | spawn のコスト（1 回 約 6 ms）とパイプのコピーをなくす。commit-graph を直接読める |
| git の書き込み | デスクトップは `git` を spawn、iOS / Android は gix と libgit2 | rebase・merge・push などは本家に任せて正しさと互換性を保つ。モバイルでは git を起動できない。`GitBackend` で差し替える。git は 2.23 以降（`git switch`）。より新しい git の機能は、あるときだけ使い、なければ古いやり方に戻す（`amonhen-backend-cli` の `MIN_GIT_VERSION`・`git_version`） |
| 非同期 | tokio（I/O・プロセス）＋ rayon（CPU） | 古い要求は即キャンセルする |
| 差分 | imara-diff、色付けは tree-sitter | 色付けは後追いで、まず色なしで即表示 |
| 橋渡し | C ABI（cbindgen）を土台に、UniFFI（Swift / Kotlin）、Linux・Windows は Rust から直接 | 操作系は型安全に、行データはゼロコピーで |
| OS の機能 | `Platform` トレイトで FE から注入。ファイル監視は notify | メインスレッドの起こし方・認証情報・SSH・保存先を OS ごとに差し替える |
| UI の骨格・重い画面 | AppKit | 表示行の使い回し、描画範囲、レイヤーを直接制御できる |
| UI の周辺 | SwiftUI（`NSHostingView` で埋め込む） | 設定・ダイアログ・Welcome を手早く作る |
| GPU | History と差分表示を Metal で描く（文字のグリフアトラスを共有）。ほかは Core Animation と標準部品 | History のラベル用に Metal の文字描画は必ず作るので、差分表示でも使い回す |
| 最初の FE | macOS 14.4 以降、Apple Silicon | `os_sync_wait_on_address`、`NSView.displayLink` を使う |
| コアのビルド対象 | `aarch64-apple-darwin`、`aarch64-apple-ios`、`x86_64` / `aarch64-pc-windows-msvc`、`x86_64` / `aarch64-unknown-linux-gnu`、`aarch64-linux-android` | コアに OS 固有のコードが紛れ込むのを CI で防ぐ |

## 全体構成

```
┌──────────────────────── macOS アプリ（Swift / AppKit）────────────────────────┐
│ サイドバー        History               差分表示            コミット入力     │
│ NSOutlineView   NSTableView           自前の NSView        NSTextView        │
│                 + Metal のグラフ       Metal（文字は共有）  （IME は OS）     │
│ 3-way マージ（自前の NSView） · 設定・ダイアログ（SwiftUI）                  │
│ ──────────────────────────────────────────────────────────────────────────── │
│ Swift の薄い層: 見えている行だけ読む → 描く / 操作を Intent として送る       │
└──────────────▲───────────────────────────────────────────────┬───────────────┘
               │ 変更通知（変わった行の範囲） / 行データ（ゼロコピー）│ Intent
┌──────────────┴────────── CoreClient（通信手段を抽象化）──────────▼───────────────┐
│ ① 同一プロセス: C ABI ② 手元の daemon: ソケット＋共有メモリ ③ SSH: stdio     │
└──────────────▲───────────────────────────────────────────────┬───────────────┘
┌──────────────┴──────────────────── Rust コア ─────────────────▼───────────────┐
│ model      内部モデル（FE を知らない。UTF-8、展開済みバッファへの参照）       │
│ core       状態・スナップショット・Intent の処理（一方向のデータの流れ）       │
│ projection FE ごとの形への変換（変わったチャンクだけ・1 回だけ）              │
│ backend    GitBackend: gix / git CLI / libgit2                               │
│ platform   Platform: 起こし方・認証情報・SSH・保存先（FE から注入）          │
│ index   コミット情報の永続キャッシュ（mmap、メモリ上と同じ配置）              │
│ graph   レーンの順次計算・差分だけの再計算                                   │
│ diff    imara-diff・単語差分・tree-sitter の色付け                           │
│ watch   notify（FSEvents / inotify / ReadDirectoryChangesW）で監視            │
│ oplog   Undo 用の操作ログ（操作前の HEAD と関係する ref を記録）              │
│ search  log・pickaxe 検索（rayon で並列）                                    │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 画面構成

| 画面 | 左 | 右 |
| --- | --- | --- |
| Working Copy | コミットの入力（上）と、変更のあるファイルの一覧（下。チェックボックスで次のコミットに含めるか） | 差分 |
| History | コミットの一覧（グラフと件名） | 上から、メッセージとメタ情報（ref・作者・日時・ID・親）、変更ファイル、差分 |

コンフリクトのあるファイルは、Working Copy で選ぶと右にマージエディタが出る（別ウィンドウにも移せる）。左に Current、真ん中に Result（作業ツリーの内容から始める）、右に Incoming を並べ（左右は、すべてのコンフリクトをその側にした内容。diff3 なら Base も出せる）、列の間の帯がコンフリクトをつなぐ。帯の ≫・≪ やボタンでコンフリクトごとに採用するか、手で編集して、保存・解決済みにする。✨ Magic Merge は、両側が同じ・片側だけが変えた・空白だけが違う・同じ行の別々の単語を変えた、のどれかに当たるコンフリクトをまとめて解く。解決の計算はコア（`mergeSides`・`resolveText`・`conflictRegions`・`magicMerge`・`saveMerge`）が行う。merge・rebase などの途中は、ウィンドウのツールバーの下に帯を出す（コンフリクトの数と Continue・Skip・Abort）。

コンフリクトしたファイルに、マーカーがもう残っていないことがある。git の rerere が前の解決を当てた（`rerere.autoUpdate` が切れているとき）か、ほかのエディタで直して stage していないときである。コアは作業ツリーのファイルと、rerere がまだ解いていないパスを入れる `.git/MERGE_RR` を読むだけでこれを見分ける（`conflictFiles`。共有の daemon の `status` は、内容のコンフリクト（UU・AA）に `markers` と `rerere` を添える）。片側で消したファイルなど、マーカーで表せないコンフリクトは入れない。コンフリクトがなければ費用はなく、コンフリクトのファイル 1 MB あたり約 0.5 ms。Working Copy はそうしたファイルに橙の ✓ と「マーカーなし」か「rerere」を出し、帯はその数を数えてまとめて解決済みにでき、「手元のファイルを使う」「取り込む側のファイルを使う」は解決した内容を置き換える前に確かめる。マージエディタは、マーカーから作った左右の代わりに index に残っている両側（stage 2・3、Base は 1）を出し、何が起きたかの一文と「解決済みにする」を添える。`amonhen status` にも注記を出し、VSCode 拡張も一覧とコンフリクトの欄で同じようにする。

History の右の列の上の欄（コミットの詳細）は、メタ情報、件名、本文の残りの順に並べる。見せ方は 3 つから選ぶ（欄の右上、選んだものを覚える）。

- 簡潔: 小さなアバター・作者・作者の日時・ref・親の 1 行と件名。欄は文字の高さまで縮める。
- 標準: 上の帯に作者とコミッターを左右に並べ（Gravatar のアバター・名前・メール・日時）、その下に見出しを右揃えにした 2 列の表（ID、親、署名、ref、コミットを指すタグを 1 つ 1 行、merge したタグ、取り込んだコミットの数）、件名と本文。
- 詳細: 標準と同じ形で、表に子、`HEAD~2^2~1` の形の位置（そのまま git に渡せる）、含むブランチ・タグ、直前のタグ、push 済みか、取り込んだコミット 20 件、メッセージが参照するコミット（cherry-pick・revert の元、Fixes:）、notes を足す。

署名は 1 行（結果・署名者・鍵・信用度）、親・子は 40 桁で選んでコピーできる。日時は相対ではなく日時とタイムゾーン。行数（+/−）は変更ファイルの一覧の各行に出す。メッセージのトレーラーはメッセージの中で見せ、表には転記しない。アバターは gravatar.com からメールアドレスの MD5 で取り、アプリが動いている間だけ覚える（View > Show Avatars で切れる）。

すぐ出せるもの（ヘッダーとメッセージから読める分。`commitInfo`）を先に描き、時間のかかるもの（署名の検証・notes・行数は git、子・含むブランチとタグ・前後のタグ・HEAD との位置・取り込んだコミットは History のグラフ。コミットを指すタグと、署名付きのタグの検証。`commitExtras`、linux で 30〜55 ms、まだ検証していない署名付きのタグを merge したコミットはさらに 30 ms ほど）は裏で読んでから足す。本文は等幅とプロポーショナルを選べ（View メニュー）、長くても畳まずに全文を出す。詳細の欄はコミットを選ぶたびにメッセージの高さに合わせ（右の列の半分まで。続きは欄の中でスクロール）、右上の ⤢ で右の列いっぱいに広げられる。高さ合わせは右の列全体を並べ直す（linux で 5 ms ほど）ので、表示を終えた後にまとめて 1 回だけ行う。History で検索しているときは、件名と本文の一致を強調する。

操作: URL・コミット ID・`#123`（origin が GitHub・GitLab なら issue へ）はリンク。作者を押すとその人のコミットで絞り込み、ref やブランチを押すとその履歴に絞り込む。右クリックで ID・件名・メッセージ・`git show` をコピーする。⌥⌘↓・⌥⌘↑ で最初の親・最初の子へ移る。HEAD に含まれる merge でないコミットは、右クリックの「メッセージを書き直す…」でその場で書き直せる（⌘↩ で保存、push 済みなら確かめる。Undo で戻せる）。

UI と操作の見直し（タブ、ブックマーク、設定、ツールバー、Working Copy・History・マージエディタの作り込み、Activity、GitHub・AI との連携）は `docs/ux.md` にまとめた。

### リポジトリ構成

拡張（vscode-amonhen）とは別のこのリポジトリで作る。

```
crates/
  layout/            全 FE で共通の #[repr(C)] レコード（数値の列、グラフの Node / Edge）、Slab・Blob
  model/             内部モデル（FE のことを知らない）、GitBackend・Operations、検索の索引、Activity
  core/              状態・スナップショット・命令（JSON）の処理、レイアウト、差分、折りたたみ
  projection/        FE ごとの形への変換（文字列の符号化、列の選択、配置の揃え方）
  platform/          Platform トレイト
  backend-gix/       GixBackend（読み取り）
  backend-cli/       git CLI（書き込み・ネットワーク・署名の検証）
  watch/             リポジトリの変化の検知（notify）
  syntax/            tree-sitter による色付け
  remote/            daemon との通信（フレーム・要求の形）、RemoteBackend、Server、共有の daemon につなぐ・
                     共有メモリ（local・shm）、SSH で送り込む（ssh）
  daemon/            daemon（エンジン amonhen の --stdio: SSH 先、--serve: 手元の共有の daemon）
  ffi-c/             C ABI ＋ cbindgen のヘッダー
  cli/               CLI（amonhen）
apps/
  macos/             SwiftPM のパッケージ（AmonhenKit と、AppKit のアプリ）
```
後で足すもの: `backend-git2/`（モバイル向け）、Swift / Kotlin 向けのバインディング。
## データ設計

### 不変のスナップショット

- コアは状態が変わるたびに新しいスナップショットを作り、`arc-swap` で公開する。公開したものは書き換えない。UI の読み取りにロックは要らない。
- 行データは 4096 行ごとのチャンクに分け、変わったチャンクだけ作り直す（残りは前の世代と共有）。
- 各スナップショットは `generation` を持つ。UI は世代番号で古いデータでの描画を検出する。

### 内部モデルとプロジェクション

- 内部モデルは git の処理と計算に都合の良い形だけを持ち、FE を知らない。文字列は gix が展開した `Arc<[u8]>` を位置と長さで参照し、コピーしない。
- FE は接続時に `FrontendProfile`（文字列の符号化・必要な列・GPU の頂点形式・配置の揃え方）を宣言する。プロジェクション層がその形にチャンク単位で変換する。
- 変換するのは見えている範囲と先読みの範囲だけ。結果は「世代＋チャンク＋形式」でキャッシュし、変わっていないチャンクは前の世代と共有する。同じ形式の FE が複数あれば共有する。
- 変換結果は形式ごとに永続キャッシュにも書き、次の起動では mmap するだけにする。内部の永続キャッシュ（チェックポイント、ref の索引など Rust の中で閉じるもの）は rkyv を使ってもよいが、FE に渡す形は自前で定義する（rkyv は配置が言語をまたいで文書化されておらず、文字列も UTF-8 固定のため）。
- History のキャッシュのファイル（`crates/core/src/cache.rs`）の名前は、リポジトリと形式の指紋で決める。形式の指紋は中身を決めるソース（`cache.rs`、`graph.rs`、`CommitGraph`、amonhen-layout のレコード）のハッシュで、`crates/core/build.rs` が作る。並んで動く形式の違うビルド（開発中のビルドの daemon とリリースの daemon など）は、互いのファイルを書き換えずに別のファイルを使い、ほかの形式のファイルを読むこともない。同じリポジトリのほかの形式のファイルは、7 日書かれていなければ消す。

### ゼロコピーの規則

- 形が同じなら変換しない。数値の列（日時・添字・レーン・フラグ）とグラフの `Node` / `Edge` は全 FE で同じ `#[repr(C)]` レコードを共有し、GPU 側は頂点ディスクリプタで読み方を合わせる。
- データは列ごとの配列（SoA）で持つ。FE が列を選ぶのはポインタを選ぶだけで、詰め直さない。
- 文字列は「ASCII なら共通のバイト列、それ以外だけ FE の好む符号化」に統一する。ASCII はそのまま UTF-8 でも Latin-1 でもあるので、大半の文字列（ハッシュ、パス、英語のメッセージ、ブランチ名）は全 FE でコピーなし。変換が要るのは ASCII 以外の文字列を UTF-16 系の FE に渡すときだけで、展開済みバッファから FE 向けの領域へ直接 1 回だけ書く。
- 参照はポインタではなく領域の先頭からのオフセットで持つ（共有メモリ・ファイル・ネットワークでそのまま通用させるため）。別プロセス・リモートでは、連続した領域に集める処理と符号化の変換を同時に行い、書き込みを 1 回にする。
- チャンクの領域は全 FE でページ境界に揃えて確保する（`MTLBuffer(bytesNoCopy:)`、`VK_EXT_external_memory_host` でそのまま GPU に見せるため）。独立 GPU では PCIe 越しの転送 1 回だけは避けられない。
- 原理的に残る処理は「git オブジェクトの展開」と「描画」だけ。どちらも 1 回きりにしてキャッシュする。

| FE | 文字列 | コピーなしの受け渡し | 描画 |
| --- | --- | --- | --- |
| macOS / iOS | ASCII＋UTF-16 | `UnsafeBufferPointer` で直接読む（生ポインタは `withSnapshot { }` の外に出さない）。`CFStringCreateWithBytesNoCopy`（ASCII）/ `CFStringCreateWithCharactersNoCopy`（UTF-16） | Metal ＋ Core Text |
| Windows | ASCII＋UTF-16 | UTF-16 はそのまま `PCWSTR`。ASCII は描画時に見えている行だけスタック上で広げる | Direct2D / D3D ＋ DirectWrite |
| Linux | UTF-8 | UI も Rust（gtk-rs）で書き、FFI なしで参照する | GTK4（GSK）/ Vulkan |
| Android | ASCII（Latin-1）＋UTF-16 | JNI の `NewDirectByteBuffer`。Java の `String` は作らず、バッファを参照する `CharSequence` を描画 API に渡す | Compose / Canvas、必要なら Vulkan |
| VSCode / Web | UTF-16 または UTF-8 | daemon 経由の `ArrayBuffer`、または WASM のメモリを直接参照 | Canvas / WebGL |

同一プロセスでの書き込み回数は、数値の列・グラフ・ASCII の文字列が全 FE で 0 回、ASCII 以外の文字列が UTF-8 の FE で 0 回・UTF-16 系の FE で 1 回（変わったときだけ）になる。

### 変更通知

- コアの作業スレッドから SPSC のリングバッファ（`rtrb`）にイベントを積み、`Platform` から受け取った waker で FE のメインスレッドを起こす。macOS / iOS は `DispatchSource`（`.data_add`）で、合図が自動でまとまるので UI の更新は 1 フレームに 1 回で済む。ほかの OS は `PostMessage`（Windows）、GLib のメインループ（Linux）、Looper / Handler（Android）。

### 別プロセス・リモートにするとき

- ② 手元の共有の daemon（`amonhen --serve`）: 利用者ごとに 1 つ、Unix ドメインソケットで待つ。History は daemon が永続キャッシュのファイルに書き、つないだ側は mmap する。差分のファイルの中身は POSIX の共有メモリで渡す。命令と、Undo の記録を書く Working Copy の操作は daemon のコアで実行する。
  - SSH 先も開ける（`daemon:ssh://host/path`。CLI の `-C ssh://…` はいつもこれ）。手元の daemon が SSH 先でエンジン（`amonhen --stdio`）を用意してつなぎ、そのつながりとコア（History・検索の索引・差分）を、同じ SSH 先を開いた相手のあいだで共有する。SSH 先には git に触れる処理だけを頼む（アプリが直接 SSH 先を開く `ssh://…` と同じ分け方）。つながりが切れていれば、次に開くときにつなぎ直す。CLI で 1 回目はつなぐのに 1.6 秒、2 回目からは 0.01 秒（偽の ssh で手元を SSH 先に見立てて測った）。
  - XPC ではなく Unix ドメインソケットにした: Linux でも同じものが動き（VSCode 拡張・CLI からも使う）、往復の遅れは小さな要求で p50 0.08 ms（同一プロセス 0.04 ms）と十分小さい。大きなデータはソケットに乗せず、ファイルの mmap と共有メモリで渡すので、XPC の領域の受け渡しの利点もない。
  - 当初の案（要求と通知も共有メモリのリングで送り、寝ているときだけ `os_sync_wake_by_address_any` で起こす）は、ソケットの遅れが測って問題にならなかったので作っていない。
- ③ SSH: daemon（`amonhen --stdio <repo>`、`crates/daemon`。パスなしなら SSH 先のデータの置き場所の空のリポジトリを開き、その機械の設定だけを扱う。Settings > SSH Hosts のため）は SSH 先で `GitBackend`・`Operations`・`RepoFiles` を要求に応じて実行するだけにし、レイアウト・永続キャッシュ・差分・色付けは手元のコアで行う（`crates/remote` の `RemoteBackend`）。C ABI と FE はそのまま使える。UI が読むのは手元のメモリだけで、通信に乗るのは git に触れる所だけ。History は永続キャッシュを手元に持ち、つなぎ直したら前回の起点からの新しいコミットだけを受け取る（daemon は `git rev-list` で求める）。要求には番号を振って並行に送り、パス・内容の検索は取り消せる。
- どの動かし方でも、UI が読むのは手元のメモリだけ。通信は裏の同期にしか使わない。

### Windows の SSH 先

Windows の OpenSSH サーバは、送られたコマンドを `HKLM\SOFTWARE\OpenSSH\DefaultShell` のシェルで実行する。設定がなければ cmd.exe で、PowerShell にしていることも多い。そのため、いま `crates/remote/src/ssh.rs` が送るもの（`exec /bin/sh -c '…'`）は、どれもそこでは動かない。エンジンそのものは Windows でも動く（VSCode 拡張が使っている）ので、変わるのは届け方だけ。VSCode 拡張の Remote-SSH には関わらない。

- **Windows の SSH 先の見分け方。** 今と同じく、まず sh で調べる。それが失敗したら、`powershell -NoProfile -NonInteractive -EncodedCommand <base64>` でもう一度調べる。このコマンドは cmd.exe・Windows PowerShell・pwsh のどれでも同じ意味で、`powershell.exe` は Windows 10・11 のどれにもある。
  - 2 回目の調べは、sh のときと同じものを返す。
    - OS と CPU（`windows` と `PROCESSOR_ARCHITECTURE`）
    - エンジンの置き場所 `%LOCALAPPDATA%\amonhen\engine`（`amonhen install` が Windows で置くところ）
    - PATH の `amonhen` とその版
    - もう置いてあるエンジンとそのビルド
  - あわせて `DefaultShell` も返し、sh の側がエンジンの起動の前にする片付けも行う（このビルドのエンジンに使った印を付け、30 日使っていないエンジンを消す。動いているものは Windows では消せないので飛ばす）。
  - `$ProgressPreference` は切る。切らないと、モジュールを読み込むときの進捗（CLIXML）が出力に混ざる。
  - つなぐ側のプロセスは、接続先（ホストとポート）が Windows だったことを覚え、次からは PowerShell の調べを先に送る。
- **エンジンを送る。** 送るのはエンジンの置き場所の `amonhen-windows-x86_64.exe` か `-aarch64.exe`。.app には Linux のものと同じく同梱する。
  - scp（`scp -S <使っている ssh>`。同じ使い回しの接続の上で）で、一時的な名前で送る。Windows の OpenSSH は既定で sftp を持っていて、scp には `C:/…` の形で渡せる。
  - 標準入力では送れない。Windows PowerShell 5.1 は 1 MB ほどで読むのが止まる。pwsh 7 なら最後まで読めるが、どの機械にもあるわけではない。
  - そのあと PowerShell のスクリプトで名前を変え、ビルドの指紋と使った印を書き、SHA-1 を .NET で計算して返す。
  - どのスクリプトも、まず `PSModulePath` を Windows PowerShell 本来のものに戻す。pwsh から cmd.exe を挟んで起動されると pwsh のものを引き継ぎ、`Get-FileHash` などのコマンドが見つからない。
- **エンジンを起動する。** エンジンは ssh のセッションの標準入出力でやりとりするので、既定のシェルが直接起動する。間に PowerShell のスクリプトを挟むと、出力を取り込まれるおそれがある。コマンドは、調べで返ったシェルに合わせて書く。
  - cmd.exe: `""C:\…\amonhen.exe" --stdio "C:\src\repo""`。外側の引用符は `cmd /c` が外す。ここでは `%` を逃がせないので、`%` を含むパスは断る。
  - PowerShell: `& 'C:\…\amonhen.exe' --stdio 'C:\src\repo'`。

- **パス。**
  - `ssh://host/C:/src/repo`（ホストのすぐ後ろにドライブ）は、SSH 先では `C:/src/repo`。
  - `ssh://host/~/src/repo` は、展開する `$HOME` がないので、エンジンに `~/src/repo` のまま渡す。エンジンは `--stdio` と `find-repositories` でも、`listDirectory` と同じく `~` をホームとして読む。
  - `find-repositories` は区切りを `/` で返す。そのため、返す場所は `ssh://host/C:/…` の形のまま。
  - `RepositoryLocation.ssh` は、選んだ `C:\src\repo` を `ssh://host/C:/src/repo` にする。
- **Settings > SSH ホスト。**
  - エンジンのパスに Windows のパス（`C:\…\amonhen.exe`）も入れられる。
  - ログインシェルのスイッチは Windows の SSH 先には当てはまらないので、出さない。
  - 「接続を確認」は、sh で `git --version; command -v amonhen` を動かすのをやめ、エンジンに聞く。
- **Windows 11 の機械（OpenSSH、既定のシェルは pwsh 7.6）で試したこと。**
  - 既定のシェルが起動したプログラムは、セッションの標準入出力をそのまま受け取る。小さく書いたものは入力の終わりを待たずに返り、2 MB のでたらめなバイトもそのまま返り、入力の終わりで終わった。
  - 生の `cmd.exe /c` のコマンドで、`""C:\Program Files\…\x.exe" "a b" "C:\x y\z""` はその引数で動き、`"C:\…\x.exe" "a b"` は失敗する。
  - PowerShell の調べは、新しい接続で 1.3〜1.9 秒。その前の失敗する sh の調べも、新しい接続ならほぼ同じだけかかり、使い回しの接続ならわずか。
  - 止まったセッションが残った使い回しの接続（ControlMaster）は動かなくなり、その上の次のセッションも止まる。
  - アプリと CLI でそこのリポジトリ（`ssh://host/C:/src/repo`）を開ける。エンジンを送り、pwsh が起動し、2 回目からは覚えた PowerShell の調べで 1.4 秒ほどでつながる。
  - 既定のシェルが cmd.exe の場合は、SSH 先の設定を変えずにまねた。`AMONHEN_SSH` の包みが、どのコマンドもそのまま cmd.exe に渡し（`cmd --% /c <コマンド>`）、調べでは cmd.exe と答える。これでエンジンを送り、確かめ、起動して、`status` と `log` が動いた。pwsh から cmd.exe を挟んで起動した Windows PowerShell が pwsh の `PSModulePath` を引き継ぐことも、これでわかった（「エンジンを送る」）。
  - 試していないこと: Windows on ARM（`PROCESSOR_ARCHITECTURE` が `ARM64`）。

### コミットの署名の検証

- 署名があるか・どの種類か（GPG・SSH・X.509）は、コミットのヘッダーの `gpgsig` で分かるので、History の行を読むときに一緒に取る（`CommitText::signature`、チャンクの `signatures` の列）。gpg は使わない。
- 検証の結果（`git log --format=%G?` の 1 文字）は git に頼む。gpg を 1 件ずつ起動するので 1 件 15 ms ほどかかる。
  - SSH の署名は git 2.34 から。それより前の git に SSH で署名したコミットの `%G?` を頼むと `BUG:` で止まる（一緒に頼んだほかのコミットの結果も返らない）ので、git 2.34 より前ではコミットを先に読み（`git cat-file --batch`）、SSH の署名のコミットは git に渡さず、結果を V（git にない Amon Hen の独自の 1 文字）にする。
- 検証で gpg に鍵を取りに行かせない。gpg の `auto-key-retrieve` が有効だと、知らない鍵の署名を検証するたびに鍵サーバーや WKD に問い合わせて鍵束に鍵を加えてしまう（裏で何百も検証するので、ネットワークに出て、利用者の鍵束を勝手に増やす）。git は gpg に引数を足す設定を持たないので、`--no-auto-key-retrieve` を付けて設定どおりの gpg を呼ぶスクリプトを一時フォルダに置き、検証のときだけ `-c gpg.program=` でそれを指す。知らない鍵の署名は「No key」になる。
  - History: 見えている署名つきの行だけを、スクロールが止まって 0.15 秒たってから、8 件ずつ 1 回の git でまとめて検証する（`git log --no-walk=unsorted --format=%H%x00%G?%x00%GS%x00%GK%x00%GF%x00%GT --stdin`。正しい署名にはコミッターのメールを添えて返す）。終わった分からアイコンを替える。描画は覚えた結果を読むだけで、gpg を待たない。アイコンは SF Symbols を Metal の文字のアトラスに入れて描く。
  - 詳細の欄: 署名者・鍵・信用度を、ヘッダーを出した後に裏で読む（`commitExtras`）。
- どこで検証するか:
  - 手元のリポジトリ: 手元の git と鍵（`~/.gnupg`、`gpg.ssh.allowedSignersFile`、`gpg.minTrustLevel` など）。
  - SSH 先のリポジトリ: 設定で選ぶ（既定は手元）。
    - 手元（既定）: コミットのオブジェクト（数 KB）を受け取り、手元の作業用の bare リポジトリ（`$TMPDIR/amonhen-verify.git`）に書いて、手元の git に `%G?` で検証させる。木やほかのオブジェクトは要らない。手元のリポジトリと同じ規則・鍵・信用度になる。書いたオブジェクトの ID が元の ID と違えば断る。
    - SSH 先: daemon に要求を送り、SSH 先の git と鍵で検証する。SSH 先に公開鍵がなければ「検証できない」になる。
  - 信用度は「自分がその鍵をどれだけ信用するか」なので、手元の鍵束で判断するのを既定にした。
- 見せ方（`SignatureStatus`）は、`%G?` に信用度（`%GT`）と「署名者のメールアドレスがコミッターと一致するか」を加えて決める。`%GT` は git 2.26 から。それより前の git では、メールアドレスと一緒に gpg で読む、署名した鍵の有効性を信用度にする。言葉は GitHub に合わせて英語。

  | 状態 | アイコン（SF Symbols） | 色 | 言葉 | 信用度の目盛り |
  | --- | --- | --- | --- | --- |
  | G・信用 ultimate・full（SSH は allowed signers にあれば） | checkmark.seal.fill | 緑 | Verified | 3 |
  | G・信用 marginal | checkmark.seal | 緑 | Verified | 2 |
  | U、または信用 undefined・never | checkmark.shield | 黄 | Untrusted key | 1 |
  | G で、鍵のどのメールアドレスもコミッターと違う | person.crop.circle.badge.questionmark | 黄 | Unverified signer | — |
  | X・Y | clock.badge.exclamationmark | 橙 | Expired | — |
  | R | exclamationmark.shield.fill | 赤 | Revoked key | — |
  | B | xmark.seal.fill | 赤 | Bad signature | — |
  | E | questionmark.circle | 灰 | No key | — |
  | V（git 2.34 より前、SSH の署名） | exclamationmark.circle | 灰 | Can’t verify | — |
  | 検証中 | seal | 薄い灰 | Checking… | — |

  署名者がコミッターと一致するかは、git の `%GS`（鍵の主なユーザー ID だけ）ではなく、署名した鍵に登録されたすべてのメールアドレスで決める。GPG は主鍵の指紋（`%GP`）で `gpg --list-keys --with-colons` を引き（鍵がいくつあっても gpg は 1 回）、ユーザー ID ごとの有効性も読む。SSH は git が allowed signers から見つけたプリンシパル（`%GS`）を使う。吹き出しの「コミッターのメール」の行に、コミッターのメールアドレスが鍵に登録されているかを出す。有効なユーザー ID（u・f・m）にあれば「鍵に登録されている」、有効性が不明なものにだけあれば「鍵に登録されている。ただし、そのユーザー ID は確認されていない」（誰でも他人のアドレスをユーザー ID に書けるため）、失効・期限切れのものにだけある、またはどこにもなければ「鍵に登録されていない」、鍵のメールアドレスを読めなければ「確かめられない」。

  詳細の欄の Signature の行は、アイコン・言葉・目盛り・方式と署名者。かざすと短い説明、押すと吹き出し（説明、署名者、コミッターとの一致、方式、鍵 ID、4 文字ずつ区切った指紋、信用度、検証した場所）を出す。History の行はアイコンだけ（信用度はアイコンの形で分かる）。
- 結果はコア（手元の `Repository`）がコミットごとに覚える（コミットは変わらないため）。アプリを終えると消える。
- 検証する側を変えたら、覚えていた結果はすべて捨てる。鍵や信用度が違うので、結果を引き継がない。コアは番号を進め、変える前に始めた検証の結果が後から届いても覚えない。History の丸と詳細の欄も、結果を捨てて検証し直す。

### タグの署名の検証

- 注釈付きのタグにも署名できる（`git tag -s`。OpenPGP・SSH・X.509）。署名付きのタグを merge したコミットは、ヘッダーの `mergetag` にそのタグを持つ（linux はメンテナーの署名付きのタグを merge するので、マージの半分ほどにある）。署名があるか・どの種類かは、タグのオブジェクト（メッセージの後ろの `-----BEGIN … SIGNATURE-----` の行）で分かる。gpg は使わない。
- git にはタグの `%G?` がない（`for-each-ref` の `%(signature:grade)` はコミットだけ）。`git verify-tag --format` は状態行を出さず、通ったタグだけを出す。そこで署名付きのタグを 1 つずつ `git verify-tag --raw` で確かめ、出力を git がコミットで読むのと同じ読み方で読む（OpenPGP と X.509（gpgsm）は `[GNUPG:]` の状態行、SSH は ssh-keygen の文。どちらも翻訳されない）。結果はコミットと同じ形（`%G?` の 1 文字・署名者・鍵・指紋・信用度・鍵のメールアドレス）で、アイコンと言葉も同じ。署名者と比べるのはコミッターでなくタガー。1 回の `verify-tag` に渡したタグの出力は区切りなく続くので、git はタグごとに起動し、8 つまで並べて動かす。git 2.34 より前では、SSH で署名したタグは git に頼まずに V（Can't verify）。
- `mergetag` はタグのオブジェクトそのもの（ID も元のタグと同じ）だが、リポジトリにないことが多い。タグを一時的なオブジェクトの置き場に書き（`GIT_OBJECT_DIRECTORY`、`hash-object -t tag -w --literally`）、そこを足して（`GIT_ALTERNATE_OBJECT_DIRECTORIES`）リポジトリで `verify-tag` を動かす。リポジトリの設定が効き、リポジトリのオブジェクトは増えない（git 2.23 でも動く）。中身が ID と合わないものは No key。
- 結果はコミットの結果と一緒に、タグのオブジェクトの ID で覚える（タグと、それを merge したコミットで 1 つ）。捨てるときも同じ。daemon 越しでは、タグのオブジェクトを daemon から読み（`TagObjects`）、コミットの署名と同じところで確かめる（この Mac の鍵なら手元の作業用のリポジトリの git、SSH 先の git と鍵なら `VerifyTags`）。
- 見せる場所:
  - 詳細の欄: コミットを指すタグを 1 つ 1 行の Tag の行（注釈付きのタグは名前・タガー・日時・件名と署名、軽量タグは軽量タグと出す）と、Merged Tag の行ごとの署名。どちらも `commitExtras` のほかの分と一緒に裏で読む。署名を押すと、コミットと同じ吹き出し（タグの名前、「タガーのメール」、タグのメッセージ）を出す。
  - サイドバー: 署名付きのタグの行の右端に印（リモートの印の前）。見えているタグだけを、スクロールが止まって 0.15 秒たってから 8 つずつ確かめ（`tagSignatures`）、refs が変わったら確かめ直す（タグを作り直したかもしれない。コアが結果を覚えているので速い）。ツールチップに言葉と説明。
  - History の ref の札はそのまま。CLI: `amonhen refs --verify` でタグごとに結果を添える（`--json` では `signed` と `signature`）。`amonhen show` は、コミットの署名と、指すタグ・merge したタグの行をそれぞれの署名と一緒に出す（`--json` では `signature`・`tags`・`mergedTagSignatures`）。
- linux（注釈付きのタグ 949 個がすべて署名付き）で計ると、1 つのタグに 25〜30 ms（git と gpg）。全部なら 1 つずつで約 25 秒、8 つずつ並べて 4 秒ほど。CLI で新しい 8 つは 0.07 秒。署名があるかを全部のタグのオブジェクトから読むのは 0.38 秒。

### mailmap

- 人は mailmap（作業ツリーの `.mailmap`、bare なら `HEAD:.mailmap`、`mailmap.blob`、`mailmap.file`。git と同じ順に gix で読む）の名前とメールで見せる（`git log` と同じ）。切るのは git の `log.mailmap`（既定は true）で、Amon Hen に独自の設定はない。「作者と署名」のページに、その切り替えと `mailmap.file` の行を両方の段に置く。Blame は git と同じくいつも置き換える（`git blame --porcelain` を読む）。
- コミットには記録された名前を持ったまま（`CommitText` はコミットの中を指す。作者のメールも指すようにした）、見せるときにコアが置き換える（`Repository::mailmap`、`amonhen_model::Mailmap`）。だから mailmap を変えても行を読み直すだけで、History のキャッシュは作り直さない。
- 読むのはリポジトリのある側（`GitBackend::mailmap`）。手元は gix で読んで覚えておき、もと（ファイルと設定ファイル、blob のオブジェクト）が変わったかを `stat` 数回で調べる。daemon 越し・SSH 先はその側から受け取る（`Request::Mailmap`。変化の知らせか 5 秒で聞き直し、指紋を送って変わっていなければ答えは空）。FE はリポジトリが変わったときに `mailmap_fingerprint` を比べ、変わっていれば行と詳細を読み直す。
- History の行: 作者の列はコミットの中を指し、mailmap で名前が変わる行だけ、チャンクごとに 1 つの置き換えた名前の置き場を指す。詳細（`commitInfo`）: 置き換えた作者とコミッター、違えば `recordedAuthor`・`recordedCommitter`。詳細の欄は名前の下に記録のもの（「記録: …」）を添え、署名は記録されたコミッターのメールと比べる。File History もコアで置き換える（git からは `%an`・`%ae`。`%aN` は `log.mailmap` を見ないため）。
- 検索: `author:`（と欄を決めない語）は、置き換えた名前と記録された名前のどちらかに合えばよい（git の `--author` は置き換えた名前にだけ合うので、それより広い）。検索の索引は記録された人（名前とメール）を 1 つずつ持ち、行はそれを指し、条件は人ごとに調べるので、mailmap の分は行ごとではなく人ごとに 1 回引くだけ。
- linux（148 万コミット、mailmap 1008 行。`crates/core/benches/authors.rs`）で入れる前 → 後: History の 60 行 365 → 382 µs、`author:torvalds` 6.6 → 6.7 ms、`author:"tejun heo"` 5.6 → 6.4 ms、`fix memory leak` 63 → 64 ms、コミットの詳細 17 → 24 µs、検索の索引を作る 1.37〜1.49 → 1.46〜1.50 秒。

## マルチプラットフォーム

### GitBackend

iOS / Android では `git` を起動できないので、git の操作をトレイトで抽象化して差し替える。

| 実装 | 用途 |
| --- | --- |
| `GixBackend` | 読み取りは全 OS。書き込みも gix が対応した操作から順に移す |
| `CliBackend` | デスクトップでの書き込み（rebase・merge・push）。正しさを優先 |
| `Git2Backend`（libgit2） | モバイルで gix が未対応の書き込み（merge・push など）を補う |

非対話 rebase も、デスクトップは `GIT_SEQUENCE_EDITOR` で todo を差し込む方式、モバイルは gix / libgit2 でコミットを積み直す方式と、バックエンドごとに実装する。

### Platform

FE が実装してコアに渡す。

| 機能 | macOS / iOS | Windows | Linux | Android |
| --- | --- | --- | --- | --- |
| メインスレッドの起こし方 | DispatchSource | `PostMessage` | GLib のメインループ | Looper / Handler |
| ファイル監視（notify） | FSEvents | `ReadDirectoryChangesW` | inotify | inotify |
| 認証情報 | キーチェーン | 資格情報マネージャー | libsecret | Android Keystore |
| SSH | OS の ssh / russh（iOS） | OS の ssh | OS の ssh | russh |
| 保存先 | `~/Library/Caches/<バンドル ID>` など | `%LOCALAPPDATA%` | `$XDG_CACHE_HOME` | アプリの cache ディレクトリ |

### バインディング

- C ABI（`cbindgen` でヘッダーを生成）がすべての土台。速さが要る経路（行データ・グラフ・通知）はここを通す。
- UniFFI で Swift（macOS / iOS）と Kotlin（Android）向けの型付き API を生成する。
- Linux（gtk-rs）と Windows（windows-rs で UI も Rust にする場合）は Rust から直接使う。

## History のグラフ

### レイアウト（Rust）

- commit-graph から親と世代番号を読む。メッセージ・作者は見えている行の分だけ後で取る。
- 新しい順に順次レイアウトし、最初の 1 画面分ができた時点で表示する。4096 行ごとにレーンの状態をチェックポイントとして保存する。
- 新しいコミットが増えたら先頭から計算し直し、チェックポイントと状態が一致した時点で打ち切る。
- first-parent の幹線はレーン 0 に固定する。色はレーン番号ではなくブランチの出自で決め、スクロールや再計算で変わらないようにする。
- 長い線は行ごとに分けず、区間（`row_start..row_end`）1 つとして持つ。見えている範囲の線は区間の索引で O(log n) で引く。

### 描画（Metal）

- 点・線・曲線・ラベルの背景を四角形のインスタンスとして、1 回の描画命令で描く。形は fragment シェーダーの SDF で描き、解析的にアンチエイリアスする。
- インスタンスは表示範囲 ± 数画面分を GPU に置き、スクロールでは定数バッファのオフセットだけを変える。
- `CAMetalLayer.maximumDrawableCount = 2`。入力を受けたら次の vsync を待たずに描いて表示する。

### 入力

- `scrollWheel` を直接受け、イベントのタイムスタンプと速度から表示時刻の位置を予測して描く。慣性は `momentumPhase` に従う。
- 当たり判定は行 = `y / 行の高さ`、レーン = `x / レーンの幅` で O(1)。

### 表現の工夫

- 幹線を太く、枝を細く描く。
- ホバーしたコミットの祖先の道筋を強調し、ほかを薄くする（祖先判定は Rust でビット集合を作り、GPU にはテクスチャ 1 枚で渡す）。
- ブランチに焦点を当てるモード、マージ済みブランチの折りたたみ。
- レーンが多いときは未使用レーンを詰め、ホバー位置の周辺を魚眼で広げる。
- 未コミットの変更を先頭の点線の仮ノードで、stash を専用の形で、ahead / behind の区間を色分けで示す。
- レイアウトが変わったら新旧をコミット ID で対応付け、頂点シェーダーで位置を補間してアニメーションする。
- スクロールバーにグラフの密度（コミットの頻度、ブランチの活動量）を描く。

## 差分表示

- `NSTextView` は使わず、最初から Metal で描く。文字の描画の仕組みは History のラベルと共有する。
  1. 文字の並べ方は Core Text の CTLine に任せる（日本語・合字・フォントの代替を OS と同じ品質で）。
  2. CTRun からグリフ番号・位置・フォントを取り出す。
  3. 未登録のグリフだけ `CTFontDrawGlyphs` でラスタライズしてアトラスに足す（OS と同じ描画処理なので見た目が揃う）。絵文字はカラーのアトラスに分ける。
  4. グリフ 1 つを四角形のインスタンス 1 つとして、1 回の描画命令で描く。横位置は 1/4 ピクセル単位に丸める。
- CTLine は「世代＋行番号」でキャッシュし、スクロールでは History と同じく定数のオフセットだけを変える。
- ファイル内容は gix が展開した `Arc<[u8]>` を差分計算・色付け・表示で共有する。
- テキスト選択・コピー・検索・右クリックメニュー・hunk / 行単位の stage を自前で実装する。

## 書き換える操作と、積み重ねたブランチ

rebase と History の編集（reword・squash・fixup・drop・edit・split・move）は、コミットを書き換える。書き換えるコミットに先頭があるブランチ（今のブランチの下に積んだブランチ）は、一緒に動くか、古いコミットに残るかのどちらかになる。Amon Hen はどちらにするかを git の設定に任せ、何かが変わる前に、どうなるかを見せる。どちらになっても、後からは見えにくい形で履歴が書き換わったり分かれたりするため。

- **設定。** git の `rebase.updateRefs` を 2 段で使う。ユーザー（`--global`、アプリの設定）と、リポジトリ（`--local`、リポジトリの Settings の画面）。書かれていなければ git の既定（オフ）。Amon Hen だけの既定やブランチごとの設定は持たないので、端末の `git rebase` とふるまいが同じになる。どの欄にも、今効いている値と、どこで決まっているかを出す。
- **古い git。** ブランチを一緒に動かすには git 2.38 が要る。古い git（SSH 先のリポジトリなら SSH 先の git）では欄を使えなくし、要る版と見つかった版を出す。見込みには、ブランチが動かずに残ることを出す。
- **見込み。** コアの命令（`rewritePreview`）が、どこへの rebase か、どの History の編集かを受け取り、次のものを返す。
  - 書き換わるコミットと、その前と後の位置
  - 一緒に動くブランチと、動く先
  - 取り残されるブランチと、その理由（設定がオフ、git が古い、ここやほかの worktree でチェックアウト中）
  - それらのブランチの上流（push 済みのブランチが動くと、force push が要る）
  - コンフリクトの予測（`predictMerge`。rebase では目安）

  作業ツリー・index・ref には触らない。CLI でも同じものを出す（`--json` も）。
- **確認のシート。** コミットを書き換えるので、rebase（メニュー・サイドバーの右クリック・ドラッグのどれからでも）と History の編集の前に、必ず出す。前と後のコミットを、History と同じ描き方（レーン・点・札・件名）で左右に並べる。
  - 書き換わるコミットは、前では薄く、後では強調して描き、A → A′ と追えるようにする
  - 一緒に動くブランチの札は、動いていく様子を描く
  - 取り残されるブランチの札は、古いコミットに残したまま描き、理由を添える
  - push 済みのブランチは、`origin/<ブランチ>` の札が古いコミットに残る
  - コンフリクトの予測は、図の下に出す

  チェックボックス「積み重ねたブランチも一緒に動かす」は、設定の値から始まり、この操作にだけ効く（`--update-refs`／`--no-update-refs`、History の編集では todo の `update-ref` の行）。git が古ければ使えない。切り替えると、図もそれに合わせて描き直す。
- **操作の後。** どのブランチが動き、どれが残ったかを出す。Undo 1 回で、すべて戻る。
- **結果を確かめる。** Range Diff（ux.md の 5d 節）は、ブランチを書き換える前の版と比べる（`git range-diff` と同じ）。
  作り直したスタックを、コミットごとに確かめられる。前の版は、コアの `previousVersion` がブランチの reflog から探す
  （端末でした rebase も）。`rangeDiff` は `git range-diff`（2.19 から。最低の 2.23 より前）を動かし、出力を完全な
  コミット ID つきの組に読む。
- **リポジトリの設定。** シート（⌘;）をやめ、サイドバーの Workspace に、Working Copy・History・Stashes・Reflog と並ぶ Settings の画面として置く。⌘; はその画面を開く。
- **独自の既定を持たない。** Amon Hen は自分で `rebase.updateRefs` を有効にしない。rebase は、同じ設定の `git rebase` と同じにふるまう。History の編集は、設定がオンで git が 2.38 以降のときだけ、todo の `update-ref` の行を残す。

## パッチ

- コミットをパッチにするのは `git format-patch`、当てるのはメールの形（format-patch のファイル・mbox・.eml）なら
  `git am`、ただの diff なら `git apply`。コアがパッチを読むのは、何を変えるかと触るファイルを見せるためだけ
  （`crates/core/src/patch.rs`）で、当てるのは git。
- パッチのファイルは、コアが動く機械に置いたまま（アプリなら Mac、VSCode なら拡張ホスト）。バックエンドには中身を
  標準入力で渡し、format-patch は git dir の中の一時フォルダに書かせてファイルを返す（名前は git が付ける。
  `format.filenameMaxLength` なども効く）。daemon 越しでも SSH 先でも同じに動く。
- 既定は git に合わせる: 1 コミット 1 ファイル（1 つの mbox は選べる）、am は `am.threeWay`、apply は作業ツリーだけ。
  format-patch のほかのこと（番号の付け方・宛先・Signed-off-by）は `format.*` に任せる。
- `git am` の途中は、それだけの種類にする（`rebase-apply/applying`。前は rebase と見なしていた）。3-way なしで
  当たらなかったパッチは、Retry with 3-Way Merge で `rebase-apply/patch` を `git apply --3way` で当て直し
  （`git am -3` と同じ）、きれいに当たれば続ける。当て直したパッチはコアが `rebase-apply/amonhen-retried` に書く
  （am が終わると git がフォルダごと消す）。git の `rebase-apply/threeway` を書き換えると、後のパッチまで 3-way になるので使わない。
- Undo: am は全体で 1 つ。止まっている間は保留し、終わったときに記録する（Abort なら残らない）。当てた diff は
  触ったファイルで記録する。index は戻らない。
- format-patch にはコミットを `--no-walk=unsorted` と `--max-count` で渡す（format-patch は逆の順に書き出すので逆に
  して渡す。数を渡さないと、コミットが 1 つのときに `<rev>..HEAD` の意味になる）。どれも git 2.23 にある。

## 速度目標と計測

| 項目 | 目標 |
| --- | --- |
| 起動して History を表示 | 数 ms（永続キャッシュあり） |
| 130 万コミット（linux）の全レイアウト | 裏で 100 ms 前後 |
| スクロール中の GPU へのデータ転送 | 定数 1 つ |
| 1 フレームの描画命令 | History は 1 回 |
| 入力から GPU が描き終えるまで | 8.3 ms 以内（表示はさらに WindowServer の合成で 2 フレーム後） |
| Rust と Swift の境界のコスト | 1 フレームあたり µs 未満 |

- Rust は criterion、Swift は `os_signpost` ＋ Instruments で計測する。
- 入力イベントのタイムスタンプから `MTLDrawable.addPresentedHandler` の表示時刻までを常に計測し、デバッグ表示に出す。
- CI で linux・chromium 規模のリポジトリを使い、起動時間・フレーム落ち・入力遅延の悪化を検出する。
- `CFString` の NoCopy が実際にコピーしていないことを確かめる。`apps/macos/Tests` で、UTF-16 は `CFStringGetCharactersPtr` が元の配列を返すこと、ASCII は元のバッファを書き換えると内容も変わることをテストしている。短い ASCII 文字列は CoreFoundation が tagged pointer（ポインタの値に文字を埋め込む）にするので参照はしないが、ヒープ確保もないので問題ない。

### 計測の記録

linux（1,485,021 コミット、ref 952 個、Apple Silicon、release ビルド）:

| 項目 | 時間 |
| --- | --- |
| History の読み込み（commit-graph なし） | 2.5 s（`git rev-list --all --topo-order` は 11 s） |
| 同上（commit-graph あり、`git clone --shared` した複製で計測） | 0.97 s（git は 2.1 s） |
| グラフのレイアウト（レーン 671 本） | 0.23 s |
| 起点が変わらないときの読み直し | 9 ms |
| 1 コミット足した後の読み直し | 139 ms |
| アプリで開いて History を表示（キャッシュあり、102 MB） | 18 ms |
| 同上（キャッシュなし） | 2.4 s |
| スクロール中のメインスレッドの処理 p50 / p95 | 2.0 / 2.6 ms（フレーム落ち 0） |

ここに至るまでに直したこと:

- gix のトポロジカル順の走査は全コミットを 2 回読み、2 回目もオブジェクトを展開し直していた（28 s）。各コミットを 1 回だけ読み（commit-graph があればそれを使う）、同じ深さのコミットを並列に読んで、並べ替えはメモリ上で行うようにした（`model::topo`）。
- 並べ替えはハッシュ表を 3 回引いていて、150 万件の表ではキャッシュミスが重かった（0.91 s）。オブジェクト ID の先頭 8 バイトをそのままハッシュ値にし（`OidMap`）、親を 1 回だけ添字に直すようにした（0.22 s）。
- レイアウトは各行で全レーンを走査していた（821 本で 1.0 s）。待っている行からレーンを引く索引と空きレーンの集合を持つようにした（0.18〜0.23 s）。
- 起点が変わったら全履歴を読み直していた。前回の History に当たったら止めて、新しいコミットだけを足すようにした（`model::topo::extend`）。

キャッシュがない初回の 2.4 s は、commit-graph があれば約 1.2 s になる。commit-graph がないリポジトリで `git commit-graph write --reachable` を裏で実行するかは設定で選ぶ（`writeCommitGraph`、既定は行わない。リポジトリの `.git` に書き込むため）。パスごとの履歴を速くする、変更のあったパスの Bloom フィルタ（`--changed-paths`）も書く。2.27 より前の git はこのオプションを知らないので、付けずに書き直す。

commit-graph がすでにあれば、開いたときと fetch・pull の後に、commit-graph にないコミットだけを新しい層として書き足す（`--split`。小さい層が積み重なると git がまとめる）。増えていなければ git は何も書かない（30 ms）。書き足さないと、commit-graph を書いた後に増えたコミットは、git の自動 gc が書き直すまで、オブジェクトから読まれ、パスでの検索でも Bloom フィルタが効かない。サポートの最低の git 2.23 も `--split` を知っている。linux で、先頭からその数だけ前に commit-graph を書いた状態（`git clone --shared` した複製）:

| commit-graph にないコミット | 書き足し | History の読み込み | File History の最初の 100 件（`kernel/sched/core.c` / `MAINTAINERS`） | それらのコミットの `git rev-list --count` |
| --- | --- | --- | --- | --- |
| 17,180（マージウィンドウ 1 回分） | 1.7 s | 542 → 521 ms | 0.09 → 0.08 s / 0.12 → 0.06 s | 0.24 → 0.06 s |
| 98,901（1 年分） | 13 s | 771 → 548 ms | 0.15 → 0.09 s / 0.13 → 0.07 s | 1.23 → 0.11 s |

書き足しは増えた数に比例し（1: 0.05 s、1,526: 0.3 s、5,212: 0.7 s）、全体の書き直しの 141 s よりずっと短いので、数でのしきい値は設けない。

ファイル単位の履歴（`fileHistory`）は `git log --follow` を使わない（履歴の単純化も Bloom フィルタも効かず、全コミットを歩く）。`git log --no-merges -- <path>` で辿り（`merges` なら `--no-merges` を付けない。マージにはファイル名が出ないので、その時点のパスを添える）、いちばん古いコミットでそのパスが改名かコピーで生まれていたら（`git diff-tree -C`）、そのコミットの親から元の名前で続ける。`limit` があれば残りの数を各 `git log` に渡し、その先があるかを `more` で返す。macOS の File History の窓は、まず 100 コミットを、次に全部を求める。linux（上と同じく `git clone --shared` した複製）での `amonhen run` の時間を、`git log --follow` と比べる:

| ファイル | `--follow` | 今 | 今（Bloom フィルタあり） | コミットの数（`--follow` / 今） |
| --- | --- | --- | --- | --- |
| `kernel/sched/core.c`（改名あり） | 86 s | 1.6 s | 0.74 s | 2,924 / 2,909 |
| `drivers/net/ethernet/intel/e1000/e1000_param.c`（改名あり） | 28 s | 1.7 s | 0.48 s | 38 / 38 |
| `mm/page_alloc.c` | 13 s | 1.2 s | 0.52 s | 2,285 / 2,279 |
| `MAINTAINERS` | 10 s | 3.7 s | 1.4 s | 12,808 / 12,757 |
| `COPYING` | 9.5 s | 0.56 s | 0.25 s | 6 / 4 |

- `--follow` だけが出すコミット: マージで捨てられた枝の変更（取り消された、両側に同じ変更がある）、別の根からマージした履歴の同じ名前のファイル、改名と並行して別の枝で元の名前のファイルに加え、改名の後にマージした変更。`--follow` はそのコミットで変えていないファイルからのコピーも追うが、こちらは同じコミットで変えたファイルからのコピーだけを追う。

History の読み込み（起点の収集＋トポロジカル順の走査、`crates/backend-gix/examples/walk.rs`、Apple Silicon、release ビルド）:

| リポジトリ | コミット数 | gix | `git rev-list --all --topo-order` |
| --- | --- | --- | --- |
| android/development（commit-graph なし） | 34,261 | 267 ms | 286 ms |
| 同上（commit-graph あり） | 34,261 | 31 ms | 111 ms |
| coreutils（commit-graph なし） | 30,176 | 157 ms | 225 ms |

- gix のオブジェクトキャッシュを有効にしないと、commit-graph なしで 1.3 秒かかる（`object_cache_size_if_unset` で 64 MB を指定）。
- commit-graph の有無で 1 桁違う。commit-graph がないリポジトリでは、書き出しを案内するか、コア自身の永続キャッシュで補う。

グラフのレイアウト（`crates/core/benches/graph.rs`、合成した履歴、criterion）:

| 項目 | 時間 |
| --- | --- |
| 10 万コミットの全レイアウト | 9.4 ms |
| 100 万コミットの全レイアウト | 90 ms |
| 100 万コミットのうち最初の 60 行 | 1.3 ms |
| 100 万コミットに 10 コミット足した後の再計算 | 6.9 ms |
| 見えている 60 行に重なる線の検索 | 0.9 µs |

History 画面のスクロール（`AmonHen <repo> --measure-scroll`、600 フレーム、1.5 行ずつ、60Hz のディスプレイ、release ビルド）:

| リポジトリ | メインスレッドの処理 p50 / p95 | GPU が描き終えるまで p50 / p95 | 表示まで | フレーム落ち |
| --- | --- | --- | --- | --- |
| coreutils | 1.9 / 2.5 ms | 2.2 / 2.8 ms | 33.3 ms | 0 |
| android/development | 2.2 / 4.2 ms | 2.3 / 4.2 ms | 33.3 ms | 0 |

- 表示までは 2 フレーム。ウィンドウ表示では、WindowServer が次の vsync で合成し、その次の vsync で画面に出るので、アプリ側では縮められない。`presentsWithTransaction` の有無でも変わらなかったので、表とグラフを同じフレームで動かせる有効の方を使う。
- 最大値（8〜12 ms）は、新しいチャンクの件名を同期で読む瞬間だった。裏で読んで前後のチャンクも先読みするようにして、p95 2.0 ms・最大 2.6 ms になった。
- linux（140 万コミット）、120Hz のディスプレイ、1 フレームに 40 行ずつの速いスクロールでは、表のセル（NSTextField）を CPU で描く処理が CA のコミットの約 6 割を占め、600 フレームのうち表示できたのは 532（49 フレーム落ち）だった。描画先を 3 枚にし、件名・作者・日時をグラフと同じく Metal で描く（差分表示と共有するグリフアトラス、行ごとに並べたグリフをキャッシュ）ようにして、599 フレーム表示（1 フレーム落ち）、メインスレッドの処理は p50 2.1 / p95 2.5 ms になった。
- グラフの点と線は、見えている範囲だけ毎フレーム GPU のバッファに書いている（数 KB）。レイアウトの配列をページ境界に揃えて `MTLBuffer(bytesNoCopy:)` で渡し、スクロールでは定数だけ変える形にするのは後で。

FE とコアの境目（`--measure-scroll` の内訳、`--measure-select N`、linux、release ビルド）:

| 項目 | 前 | 後 |
| --- | --- | --- |
| 速いスクロール（40 行/フレーム）のメインスレッド p50 / p95 | 3.35 / 3.93 ms | 1.09 / 1.72 ms |
| コミットを選んでから差分が出るまで p50 / p95（履歴の離れた行へ） | 24.4 / 46.9 ms | 8.1 / 15.4 ms |
| コミットを選んでからメッセージが出るまで p50 / p95 | 12.4 / 39.2 ms | 6.9 / 11.1 ms |

- コアの呼び出し（線の検索・レーンの位置・コミット ID）は 1 フレームで合わせて 0.03 ms 以下。重かったのは FE 側の Core Text での行の並べで、行の文字はチャンクを裏で読むときに同じスレッドで並べておき、メインスレッドではアトラスを引くだけにした。
- コミットの詳細の読み込み自体（変わったファイル 1.1 ms、差分 0.7 ms）は軽く、遅れは待ち時間だった。詳細のたびに ref をディスクから読み直していた（15〜30 ms）のをやめ、先読みのチャンクと色付けを詳細より低い優先度にし、読み込みはメインスレッドの描画を待たずに始め、最初のファイルの差分は変わったファイルと一緒に読む。

daemon 越し（`cargo run --release -p amonhen-remote --example daemon_cost -- <amonhend> <repo>`、手元の linux に子プロセスでつないだもの。SSH の遅延と帯域は含まない）:

| 項目 | 時間 | 受け取った量 |
| --- | --- | --- |
| History の初回の読み込み | 2.4 s | 54 MB（以後は手元のキャッシュ） |
| History の読み直し（変化なし） | 29 ms | 19 KB |
| 件名などのチャンク（256 行） | 3 ms | 502 KB |
| 変わったファイルと最初の差分 | 3 ms | 127 KB |
| 件名の検索 | 1.4 s | 1.37 GB |

- 転送量を減らした後（通信路の上の大きさ）: チャンク 502 KB → 13 KB、変わったファイルと最初の差分 127 KB → 56 KB、件名の検索 1.37 GB → 5 MB。History の初回は 47 MB（コミット ID はほとんど縮まない）で、以後は手元のキャッシュから差分だけ。
- 圧縮は zstd ではなく LZ4（`lz4_flex`、Rust だけで書かれている）にした。zstd は C のコードを含み、全ターゲットの型検査と Linux 用の静的リンクで C コンパイラが要るため。
- エンジンの置き場所はどのマシンでも同じ形にする: `${XDG_DATA_HOME:-~/.local/share}/amonhen/engine/<中身のハッシュ>/amonhen`（版ごとに並べて置け、隣の `engines` に SSH 先に送るほかの OS と CPU のものも置く）、`current` がいちばん新しい版を指し、`~/.local/bin/amonhen` が `current/amonhen` を指す。アプリ（起動するたびに裏で）・`scripts/install-app.sh`・tar.gz を展開した人は `amonhen install` で自分を置き、今の `current` より古くなければ `current` を向け直す（`0.1.0` < `0.1.1-snapshot.N` < `0.1.1`）。共有の daemon は `amonhen engine` が出すもの（`current` が同じビルドならそれ、違えば自分）で起動するので、どの FE が先に起動しても同じ daemon になる（`crates/cli/src/install.rs`）。開発中のアプリからは置かない。
- SSH 先で動くのはエンジン（`amonhen`。CLI と daemon を兼ねる 1 つの実行ファイル）で、x86_64 と aarch64 の Linux 向けは musl で静的リンクしたもの（`scripts/build-engine.sh`、リンクは zig、5〜6 MB）。使う順は `AMONHEN_REMOTE_ENGINE` で指定したもの → SSH 先の `PATH` の `amonhen`（同じビルドなら）→ そこに置いてある同じビルドのもの（エンジンのフォルダの `build` にビルドの指紋を置く。送ったときと `amonhen install` が書くので、ほかの機械や VSCode 拡張が置いたものも、手元のエンジンを読まず送らずに使う）→ 前に送ったもの（中身が同じ）→ 送り込む（`AMONHEN_PUSH=0` で止める）。送るのは、同梱の SSH 先の OS と CPU 向けのもの、SSH 先が手元と同じ種類なら動いているエンジンのうち、このビルドのもの。送る先は `~/.local/share/amonhen/engine/<中身のハッシュ>/amonhen`（手元の置き場所と同じ形）で、書いたら SSH 先で `sha1sum`・`shasum` で中身を確かめる。ほかの版は消さず（違う版の Amon Hen が同じ SSH 先を使っても取り合いにならない）、使うたびに `used` の印を更新し、30 日使われていないものだけを片付ける。使っているエンジンと用意のしかたは `amonhen daemon status` と Activity に出す（VSCode の Remote-SSH・Zed のリモート開発と同じ形）。
- 一緒に動くビルド: FE・共有の daemon・SSH 先のエンジンは、同じビルドどうしだけがつながる。ビルドの指紋（`amonhen_remote::BUILD_ID`）はワークスペースのソース（`crates/*/src` の下のすべて、各 crate の `Cargo.toml` と `build.rs`、ルートの `Cargo.toml` と `Cargo.lock`。改行は LF にそろえて読む）のハッシュで、`crates/remote/build.rs` が作る。`amonhen --version`（`(build …)`）、共有の daemon のソケットの名前、送る前に確かめるエンジンの中の印、挨拶に入れる。前は手で上げる通信の形の版を使っていたが、並んで進むブランチが違う形に同じ数を取り（#62 と #63 がどちらも 30）、上げ忘れると違う形を通してしまった。同じリリースのアプリ・VSCode 拡張・エンジンは同じソースから作るので、今までどおり 1 つの daemon を使う。ソースの違うビルドは、それぞれの daemon・SSH 先のそれぞれのエンジンで並んで動く。

構文の色付け（`cargo run --release -p amonhen-syntax --example highlight -- <file>`、linux のファイル）:

| ファイル | 大きさ | 時間 |
| --- | --- | --- |
| kernel/fork.c | 3,431 行・84 KB | 16 ms |
| kernel/workqueue.c | 8,644 行・245 KB | 29 ms |
| drivers/gpu/drm/amd/amdgpu/gfx_v10_0.c | 10,253 行・451 KB | 58 ms |

- 言語ごとの最初の 1 回は、クエリの読み込みで 8〜14 ms 足される。
- 差分は色なしで先に表示し、変更前・変更後のファイル全体を裏で解析してから描き直す。断片ではなく全体を解析するので、hunk の外から続くコメントや文字列にも正しく色が付く。

History の行の文字（件名・作者・日時）のメモリ:

- 256 行ずつのチャンクで読み、見えている所に近い 24 個（約 6,000 行）だけを覚える。遠いものは捨て、戻ってきたら読み直す（1 チャンク数 ms、裏で）。チャンクはコミットのオブジェクトの中身を指しているので、全部持つと linux では数 GB になる。
- linux を上から下まで速くスクロールしたときの物理メモリ（`footprint`）は、捨てないと 8 秒で 4 GB まで増え続けたが、捨てるようにして 300〜380 MB で止まる。スクロールの速さは変わらない（メインスレッド p50 1.14 ms）。

永続キャッシュ（`crates/core/src/cache.rs`）:

| 項目 | 時間 |
| --- | --- |
| 100 万コミットの書き込み | 19 ms |
| 100 万コミットの読み込み（mmap ＋ 壊れていないかの検査 ＋ 線の索引の組み立て） | 8.1 ms |
| android/development を開いて History を表示（キャッシュあり） | 3〜4 ms |
| 同上（キャッシュなし、裏で読んで表示） | 約 400 ms |

- 起点と HEAD が前回と同じなら、走査もレイアウトもしない（コミットは不変なので History も同じ）。
- 読み込み時間の大半は検査と索引の組み立て。mmap 自体はほぼ 0。必要なら索引もキャッシュに入れる。

Working Copy の status（`crates/core/examples/status.rs`、2 回目以降）:

| リポジトリ | ふだんの git status | untracked cache | ＋ fsmonitor |
| --- | --- | --- | --- |
| linux（96,080 ファイル、手元の macOS） | 約 500 ms | 125 ms | 45 ms |
| minix（73,545 ファイル、SSH 先の Linux、daemon 越し） | 115 ms | 32 ms | 使えない |

- 時間の大半は追跡していないファイルを探すこと（linux で 373 ms）。untracked cache は `status.showUntrackedFiles` が `--untracked-files=all` と揃っていないと使われないので、両方を渡す。
- ふだんの status は index のロックを取らない（利用者の git と奪い合わない）ので、キャッシュは書き残されない。初回と、その後は 60 秒に 1 回だけ書き残す。
- fsmonitor は macOS と Windows だけで、リポジトリごとに `git fsmonitor--daemon` が常駐する。アプリの設定で切れる（既定は使う）。
- 読み直しは重ねて走らせない。途中で変化の知らせが来たら、終わってから 1 回だけ読み直す。

祖先の強調（`crates/core/src/lineage.rs`、`crates/core/examples/lineage.rs`）:

- linux（148 万コミット、線 174 万本）で、選んだコミットの祖先と光らせる線を求めるのに 9〜25 ms。コミットを選んだときに裏で求め、求まったら描き直す。カーソルを合わせるたびに求めるには重いので、選んだときだけにしている。
- 線は子と親の行を持つ（`Edge::child`・`Edge::parent`、合流する線は `Edge::RIDER`）。線が 16 バイトから 24 バイトになり、linux のキャッシュは 103 MB から 117 MB になった（形式 2）。読み込み・書き込みの時間はほぼ変わらない。
- 同じ親を待つ線は、レーンの持ち主の縦線を共有する。合流した子だけが祖先なら、縦線は合流したところから下だけを光らせる（描くときに 2 つに分ける）。
- 描くのは見えている範囲の線だけなので、速いスクロールのメインスレッドの時間は変わらない（p50 1.07 ms）。

linux（148 万コミット）では、1 行を横切る線が中央値で 365 本、最大のレーン番号は中央値で 452（`crates/core/examples/layout.rs`）。トポロジー順（`git log --topo-order` と同じ並べ方）でも 303 本・637 で、並べ方では解決しない。そこで、見えている範囲に点のあるレーンだけを広く取り、線だけのレーンは細く薄い帯に詰め、何もないレーンは幅 0 にする（魚眼）。範囲が変わったときのレーンの動きは約 0.15 秒のアニメーションでつなぐ。行の文字はその行のグラフ（点と、その行を通る線。そのフレームに描いた位置）のすぐ右から始め、文字の左端は行ごとに違う（グラフから離れない）。グラフの列の幅も見えているレーンに要る分だけ取り（上限 120 pt）、広がるときはレーンと同じ速さで、狭まるときはゆっくり動かし、スクロール中にヘッダーが左右に揺れないようにする。

実際のリポジトリ（`crates/core/examples/layout.rs`）では、coreutils（30,176 コミット）で 1.2 ms、android/development（34,261 コミット、起点 318 個）で 17 ms。後者はリリースブランチが多くレーンが 673 本まで広がるので、未使用レーンを詰める・魚眼で広げるなどの表現の工夫（実装順序の 13）が要る。

## 配布

- arm64 の `.app` を GitHub Releases に置く。拡張と同じく `v*` タグの push で GitHub Actions がビルドして添付し、main への push で `snapshot` の pre-release を作り直す。
- 署名は自己署名のコード署名用証明書で行う。ad-hoc 署名だとビルドのたびに身元が変わり、macOS のアクセス許可（フォルダへのアクセス、キーチェーンなど）が更新のたびにリセットされるため。証明書と秘密鍵は GitHub Actions の secret に置く。
- アプリ名は `Amon Hen`、バンドル ID は `io.github.fukusaka.amonhen`。バンドル ID は設定・キャッシュ・アクセス許可のひも付け先なので変えない。snapshot 版は `Amon Hen Snapshot`（`io.github.fukusaka.amonhen.snapshot`）にして、正式版と並べて入れられるようにする。
- `.app` の組み立ては `scripts/build-app.sh [stable|snapshot]`（`dist/` に zip）。証明書は secret の `AMONHEN_CERT_P12`・`AMONHEN_CERT_PASSWORD`・`AMONHEN_CERT_NAME` から CI の一時キーチェーンに入れる（`scripts/import-signing-certificate.sh`）。
- インストール・更新は `scripts/install-app.sh [--user|--system|--dir <フォルダ>] [stable|snapshot]`。`curl` で取得して（`gh` はいらない。clone せずに `curl … | sh` でも動く）`/Applications` に置く（`--user` なら `~/Applications`、`--dir` なら指定したフォルダ。どれも付けなければ今入っているところ）。quarantine 属性は付かず、アプリは公証もしてある。
- git の環境: Finder や Dock から起動したアプリはターミナルの環境を引き継がない（launchd の PATH は `/usr/bin:/bin:/usr/sbin:/sbin`、`SSH_AUTH_SOCK` は macOS の agent、`GIT_*` はなし）ので、Homebrew の git・credential helper・1Password などの agent が見つからず、ターミナルでは通る fetch・pull・push がアプリからは通らなかった。起動時にログインシェルを、端末が開くときと同じ素の環境から一度動かし（`$SHELL -ilc 'env -0'`、3 秒まで。`AMONHEN_NO_SHELL_ENV` で止められる）、git と ssh がいつも使うもの（`PATH`（前に足す）・`SSH_AUTH_SOCK`・`SSH_AGENT_PID`・`XDG_CONFIG_HOME`・`XDG_DATA_HOME`・`GIT_*`）と、設定でチェックした名前を自分の環境に取り込む。git・共有の daemon・gh はそれを引き継ぐ。結果は defaults に覚えておき、次回からはすぐ使って裏で読み直す（`ShellEnvironment`。設定 > Git > 環境変数に変数と、読んだ時刻、SSH の agent があるかが出て、読み直しと切る設定がある。`AmonHen --print-environment` でも見える）。git と ssh にはパスワードやパスフレーズを尋ねる端末がないので、`GIT_ASKPASS`・`SSH_ASKPASS`（と `SSH_ASKPASS_REQUIRE=prefer`）をダイアログで尋ねるスクリプト `Contents/Resources/amonhen-askpass` に向ける（`AMONHEN_ASKPASS` で渡し、`crates/backend-cli` が git を動かすときに付ける）。
- Developer ID の署名と公証: 環境 `release` の secret に Developer ID Application の証明書（`AMONHEN_CERT_P12`・`AMONHEN_CERT_PASSWORD`・`AMONHEN_CERT_NAME`）と App Store Connect の API キー（`AMONHEN_NOTARY_KEY`・`AMONHEN_NOTARY_KEY_ID`・`AMONHEN_NOTARY_ISSUER`）があれば、snapshot と正式版の CI で、アプリとエンジンを hardened runtime と時刻の印つきで署名し、正式版の CI はさらに公証に出す（snapshot は自分用で、quarantine の印が付かない curl で入れるので公証しない。`scripts/build-app.sh`・`scripts/notarize.sh`）。アプリには結果を綴じ込み、単体のエンジンは公証に出すだけ（初めて動かすときに Mac が確かめる）。なければ ad-hoc のまま作る（`gh` や `curl` で取れば quarantine の印が付かないので、`scripts/install-app.sh` ではそのまま動く）。公開のリポジトリなので、署名する仕事だけが環境 `release` の secret を読み、リリースを作る CI の外部の Action はコミットのハッシュで固定する。フォークからの pull request には secret が渡らない。自動更新（Sparkle）は、人に配りたくなったときに検討する。

## 既存の拡張との関係

- 拡張は今のまま保守する。Standalone は別製品として作る。
- 設計の考え方（spawn を減らす、分割取得、楽観的更新、Undo の操作ログ、非対話 rebase の todo 生成）はそのまま移す。非対話 rebase の手順（vscode-amonhen の `src/history/rebasePlan.ts`）は Rust に移植する。
- daemon ができたら、拡張の git 実行層・状態管理層を daemon への接続に置き換えることを検討する（Remote-SSH では SSH 先で `amonhend --stdio` を起動する）。

## リスクと検証

| リスク | 検証の方法 |
| --- | --- |
| gix が書き込み系・特殊な設定（reftable、sparse checkout、partial clone など）に追いつかない | 読み取りだけ gix、書き込みは git。読めない場合は git の spawn に落とす経路を残す |
| 自前の差分表示で日本語入力・選択・アクセシビリティの作り込みが重い | 入力欄は標準の `NSTextView`。差分表示は読み取り専用にして、`NSAccessibility` は行単位で提供する |
| UniFFI と C ABI の二本立てで境界が複雑になる | 行データ以外は UniFFI に寄せ、C ABI は History・差分・status の行取得だけにする |
| プロジェクションの形式が増えるとテストの組み合わせが増える | 形式ごとに「内部モデルから変換 → 元に戻して比較」するテストを用意する。試作では macOS の形式だけ実装する |
| モバイルで gix・libgit2 の書き込みが git と食い違う | 同じ操作を CliBackend と比べるテストを用意する。未対応の操作はモバイルでは出さない |
| 共有メモリ・リングバッファでのメモリ安全性 | Swift 側は範囲を限定した API で包み、Rust 側は Miri と AddressSanitizer で境界をテストする |
| macOS 14.4 未満の端末 | 対象外とする |

## 実装順序

いちばん重い History を最初に試作し、境界の設計と速度目標を満たせるかを確かめてから広げる。

試作:

- [x] 1. Rust の workspace と `model` / `core` / `layout` の骨組み、`GitBackend` / `Platform` / `FrontendProfile` の形、gix による commit-graph の読み取り。全ターゲットのビルドを CI で通す
- [x] 2. グラフのレイアウト（順次計算、チェックポイント、区間表現）と criterion のベンチ
- [x] 3. プロジェクション（macOS の形式だけ）と C ABI でのスナップショット公開、Swift からのゼロコピー読み取り
- [x] 4. AppKit の History（`NSTableView`）と Metal のグラフ描画、入力から表示までの計測
- [x] 5. linux リポジトリで速度目標を検証。満たせなければ設計を見直す

本体:

- [x] 6. 永続キャッシュ（mmap）と起動直後の即時表示
- [x] 7. サイドバー（Branches / Remotes / Tags / Stashes / Submodules）と FSEvents による変更検知
- [x] 8. 差分表示（Metal、History と共有するグリフアトラス、CTLine キャッシュ）と後追いの色付け（tree-sitter、`crates/syntax`）。History の件名・作者・日時も Metal で描く。差分の文字の選択・コピー・検索
- [x] 9. Working Copy: status、ファイル・hunk・行単位の stage、コミット入力
- [x] 10. 書き込み系の操作（checkout、merge、rebase、push、fetch）と Undo の操作ログ
- [x] 11. コミット編集（reword / squash / fixup / drop / reorder / split）とドラッグ＆ドロップ
- [x] 12. 3-way マージ（コンフリクトごと・ファイルごとの解決、3 ペインのマージエディタ）
- [x] 13. グラフ表現の工夫（祖先の強調、折りたたみ、魚眼、アニメーション、密度表示）。魚眼（`crates/core/src/lanes.rs`）、レーンの動きのアニメーション、選んだコミットの祖先の強調（`crates/core/src/lineage.rs`）、マージ済みブランチの折りたたみ（`crates/core/src/fold.rs`。各先端から最初の親だけを辿った幹を残し、取り込んだ側の数を merge の行に出す。右クリックで merge ごとに開く。View > Fold Merged Branches）、History の右端の密度の帯（区間ごとのコミットの頻度とブランチの活動量、見えている範囲の枠、押すとそこへ飛ぶ）も済み。linux（148 万コミット）で折りたたみとレイアウトが 47 ms（7.7 万行に）、密度が 10 ms（`crates/core/examples/fold.rs`）
- [x] 14. 配布（GitHub Actions、snapshot、自己署名、install.sh）

拡張:

- [x] 15. CLI（`amonhen`、`crates/cli`）。UI なしでコアを使う。`amonhen [<path>]`・`amonhen open <path|ssh://…>` でアプリのタブに開く（アプリはフォルダと `amonhen://open?location=…` を受け取る）、`amonhen log`（コアのレイアウトで罫線のグラフ、点か曲線のあるレーンだけ列を取り、線が通るだけのレーンは ┆ にまとめる。`--fold`・`--branch`・検索の書き方）、`amonhen undo`（`--list`・`--peek`。アプリと同じ操作ログ）、`amonhen merge-file`（Magic Merge。残れば終了コード 1 で git の mergetool に使える）。History はアプリの永続キャッシュから始めるので、linux（148 万コミット）でも log が 85 ms。`scripts/install-app.sh` が `~/.local/bin/amonhen` にリンクを作る。コアでできることは CLI からすべて使える（VSCode 拡張やほかの FE が通る道を、先に CLI で通して確かめる）: `status`・`diff`（stage していない・`--staged`・`--combined` で行ごとの stage の印、コミット 1 つ・2 つ）・`show`・`stage`/`unstage`（`--rows` で差分の行ごと）・`discard`（行ごとにも。Undo できる）・`commit`・`refs`・`watch`（つないだまま変化の知らせを受ける）・`clone`・`run`（`run_json` の命令をそのまま。引数がなければ標準入力の 1 行 1 命令に 1 行ずつ答える、拡張が CLI を窓口にする形の試作）。どれも `--json` で FE が読む形を出す。`crates/cli/tests/cli.rs` で、手元で直接・`daemon:`・`ssh://`（偽の ssh）の 3 つの開き方に同じ命令を流し、同じに動くことを確かめる
- [x] 16. 手元の共有の daemon（当初は XPC＋共有メモリの案。Unix ドメインソケット＋共有メモリにした。理由は「別プロセス・リモートにするとき」）。段階的に進める: ① 手元に常駐する共有の daemon（`amonhend --serve`、`crates/daemon/src/serve.rs`）。利用者ごとに 1 つ、Unix ドメインソケット（`$TMPDIR/amonhend-<ビルドの指紋の頭 8 文字>.sock`、0600）で待ち、つないだ相手が最初に送るパスのリポジトリを開く。同じリポジトリの `GitBackend` と監視はつないでいる相手のあいだで共有し、変化は全員に知らせる。だれもつないでいない状態が 10 分続いたら終わり、使うときにまた起動される。つなぐときに FE の環境のうち git・ssh が使うもの（`PATH`・`AMONHEN_ASKPASS`・SSH の agent・`GIT_*`）を送り、daemon はそのつながりの git・ssh をその環境で動かすので、どの FE が先に daemon を起動したかで変わらない（`amonhen_model::process::ClientEnv`）。通信は SSH と同じフレームで、圧縮はしない。アプリの `daemon:/path` と CLI の `-C daemon:/path` がこれを使う。手元のリポジトリを daemon 越しに開く: Settings > パフォーマンスの切り替え（いつも）、リポジトリの一覧の右クリックの Open via Daemon（1 つだけ）、`amonhen open --daemon`。様子を見る: `amonhen daemon status [--json]`・`start`・`stop`（つなぐときに最初に送るものを `\0status`・`\0stop` にすると、リポジトリではなく daemon への指示になる）と、Window > Activity の Daemon のタブ（daemon のプロセス・開いているリポジトリごとのつながり・History・監視・検索の索引と、このアプリの daemon とのつながりの送受信量）。linux で、2 回目からの `amonhen log` は同一プロセスと同じ 0.09 秒（初回は daemon の起動と全コミットの受け渡しで 1.7 秒）、は済み。往復の遅れは小さな要求で p50 0.08 ms（同一プロセス 0.04 ms）、256 行のチャンクは同じ 2 ms（`crates/daemon/examples/latency.rs`）で、ロジックを daemon に移しても遅くならないので、設計どおり順に移す。② History（読み込み・レイアウト）を daemon で作る: daemon がリポジトリごとに 1 つだけ作って永続キャッシュのファイルに書き、つないだ側にはその場所を返す（`Request::PreparedHistory`、`GitBackend::prepared_history`）。つないだ側はレイアウトを計算せず、ファイルを mmap して使うので、中身はコピーせずにページキャッシュで共有される（実質の共有メモリ）。書き換えは名前の付け替えなので、読んでいる途中のものは壊れない。FE は変えずに済む。linux で `amonhen log` は 2 回目から 0.12 秒（同一プロセス 0.09 秒）、daemon の再起動後 0.17 秒、初回（daemon が作って書く）2.3 秒、は済み。③ 操作を daemon に移す: `run_json` の命令（操作・Undo・設定・Magic Merge など）と、Undo の記録を書く Working Copy の操作（コミット、変更を捨てる）は、手元の共有の daemon につないでいれば daemon のコアで実行する（`GitBackend::delegate`、`Request::Delegate`、`crates/core/src/delegate.rs`）。操作と Undo の記録が 1 か所になる（2 つのコアが同じ記録のファイルを別々に書き換えない）。行ごとの stage は記録を書かず、git の処理はもともと daemon で動くので任せない。daemon の記録を読む・取り消す命令だけは手元で実行する、は済み。手元のリポジトリを daemon 越しに開くのは、今は Settings で選ぶ（既定は開かない）。差分も daemon で計算して 1 回の往復で返し、ファイルの中身は共有メモリで渡す（`crates/remote/src/shm.rs`。daemon が POSIX の共有メモリの 1 つの領域に並べて書き、ソケットでは領域の名前と各部分の位置だけを送る。受け取った側は読むだけで写し、`amonhen_layout::Blob` としてコピーせずに表示まで使う）。linux で p50 1.6 ms・p95 6.3 ms・最大 17 ms（同一プロセス 1.4・5.3・15 ms、ファイルごとに中身を取りに行く前の形 1.7・8.3・27 ms、ソケットで中身を送る形 1.6・9.2・34 ms）、は済み。Working Copy の status は git がもともと daemon で動いていて差がない（124 ms）。History の検索（件名・作者名・ID）は、件名と作者名を初めて探すときに全部読んで索引（`amonhen_model::search::TextIndex`）に置き、次からはそれを探すだけにする（毎回 git から読み直していたのが遅さのほとんどだった）。コミットが増えたら前の索引から引き継いで増えた分だけ読む。手元のコア・SSH 先の daemon・共有の daemon で同じものを使い、共有の daemon では同じリポジトリの相手のあいだで索引と数えたコミットを共有し、最後の相手が離れても 10 分は残す。linux で 1 回の検索が 1.1 秒 → 6〜31 ms（初回は索引を作るので 1.4 秒、索引は約 100 MB）、`amonhen log -C daemon:… <query>` は 2 回目から 0.1 秒（`crates/core/examples/search.rs`）。SQLite の全文検索（FTS5 の trigram）も比べたが、まれな語でだけ速く（0.5 ms）、索引が 950 MB・作るのに 22 秒かかるので採らなかった。一致したコミットの間は、↩・⇧↩（History とツールバーの検索欄）、⌘G・⌘⇧G（どの欄にいても。差分・詳細の欄の中を探しているときはそちら）、件数の横の ∧ ∨ で移る。件数の横の切り替えで、一致したコミットだけを出せる（`CommitGraph::retain_linked`。一致しないコミットは飛ばし、線は最初の親をたどって次に一致したコミットへつなぐ。linux で fix の 24.9 万件のレイアウトまで 49 ms）。History の作り直し: Repository > Rebuild History、`amonhen log --rebuild`、`amonhen daemon rebuild [<path>|ssh://…]`（`Repository::rebuild_history`。永続キャッシュを捨て、前回の History から引き継がずに作る。共有の daemon 越しなら daemon のコアに作り直させ、そのファイルを読み直す）
- [x] 17. SSH 越しの daemon と手元の複製。段階 A0（コアのファイル操作を `RepoFiles` に）・A（`amonhen-remote` と `amonhend`、手元の子プロセスでの確認）・B（アプリから `daemon:/path` で開く。amonhend はアプリに同梱）・C（`ssh://host/path` で、SSH 先の OS と CPU に合う amonhend を `~/.cache/amonhen/` に送り込んで起動する。チャンクは件名などだけ、件名の検索は daemon の側、大きなフレームは LZ4 で圧縮）・D（リポジトリの一覧の「Add Remote…」、つなぐ間の接続中のウィンドウ、初回の読み込みの受信量、切れたときの帯と「つなぎ直す」）は済み。実際の SSH 先（Linux x86_64）でも開けることを確かめた
- [x] 18. VSCode 拡張からの daemon 利用。先にやること: CLI の `log --json`・`diff --json` は大きなデータを JSON にしていて、拡張が本来通る道（`repr(C)` のレコードと文字列表を `ArrayBuffer`・WASM のメモリでコピーせずに読む）を通していない。CLI にレコードをそのまま出す口（例: `amonhen log --records <file>`、VSCode 向けのプロファイル）を足し、linux で JSON と比べて（書き出し・大きさ・Node での読み込みの時間とメモリ）から、つなぎ方（CLI をはさむ・napi・WASM）を決める。計測（linux の 148 万コミット、`crates/core/examples/transport.rs` と Node 26）: 画面 1 枚分（256 行の点・線・ID・件名・作者・日時）は JSON が作る 0.51 ms・82 KB・Node で読む 0.18 ms、レコードが作る 0.013 ms・54 KB・読む 0.16 ms（文字列にする分が大半）で、どちらでも足りる。全行（点・線・ID）は JSON が作る 1.1 秒・137 MB・読む 0.53 秒・ヒープ +386 MB、レコードが作る 7.8 ms・89 MB・読む（型付き配列で見るだけ）0.001 ms。別スレッドへの受け渡し（webview の postMessage と同じ構造化複製）は、全行で JSON のオブジェクト 1.1 秒、ArrayBuffer の複製 12 ms、移す 0.75 ms。よって、大きなデータはレコード（ArrayBuffer）、命令は JSON にする。つなぎ方は、コアを残らず共有の daemon に移し、拡張は TypeScript で daemon の FE 向けの窓口に直接つなぐ（napi は使わない。コアが daemon にあれば napi に残る利点は mmap・共有メモリだけで、webview へはどのみちプロセス間のコピーになる）。① FE 向けの窓口（`crates/front`、`docs/front.md`）は済み: 命令と答えは JSON、大きなデータは `repr(C)` のレコード、フレームは daemon と同じ（圧縮なし）、つないで最初に `\u0001` ＋場所を送る。History と Working Copy の差分は番号で持ち、検索は相手ごとに取り消せる（`Repository::search_history_until`）。CLI はすべての命令をこの窓口で通す（手元は同じプロセスの `InProcess`、`daemon:`・`ssh://` は daemon）。linux で、daemon 越しの `amonhen log -n 30` は 2 回目から 0.03 秒（同じプロセスで毎回開くと 0.09 秒）、`log fix` の検索は 2 回目から 0.05 秒（同 1.5 秒）。② 拡張の TS のつなぐ側（vscode-amonhen の `src/daemon/`。`amonhend --socket` でソケットの場所を聞き、いなければ起動する）は済み。③ 設定 `amonhen.daemon.path` があれば、History（グラフはコアのレイアウトを行ごとの線分に直す）・コミットの詳細と差分・Working Copy の差分・行単位の stage と破棄を daemon で行う、は済み（Extension Development Host で ktor を開いて確かめた）。④ Undo の記録はもともと共通（コアと拡張が同じ `<git_dir>/amonhen/undo.json` を同じ形で読み書きする）で、daemon で捨てた行を拡張の Undo で戻せることを試験で確かめた。⑤ 書き込み（コミット・ファイル単位の stage・ブランチとタグ・merge・rebase・cherry-pick・revert・stash・fetch・push・続ける／中止／飛ばす・サブモジュール）は daemon のコアの命令で行う（vscode-amonhen の `src/daemon/writes.ts`。拡張の git 実行層の書き込みを置き換え、読み取りと拡張に固有の手順（untracked を消す破棄・index の書き込み・rebase の todo）は git 実行層のまま）、は済み。コアが Undo を記録する操作は、拡張では記録しない（`AsyncLocalStorage` で伝える）。拡張の Undo の記録が中身を覚えたままにしていて、アプリ・CLI・daemon が書いた記録を見落とし、次の保存で消していたので、毎回読み直すようにした（コアはもとから毎回読む）。⑥ 配り方を整理した: エンジンは CLI と daemon を兼ねる 1 つの実行ファイル `amonhen`（darwin-aarch64・musl の linux-x86_64・linux-aarch64。リリースに tar.gz を載せる）、置き場所はどのマシンでも `~/.local/share/amonhen`（`amonhen install`・`amonhen engine`）、SSH 先にも同じ形で置く。拡張は 3 つのエンジンを同梱した共通の .vsix 1 つ（約 8 MB）で、起動時に合うものを置き場所に置き、設定なしで共有の daemon を使う（`amonhen.daemon.enabled` で切れる。合うエンジンがなければ git 実行層で動く）。拡張の CI がスタンドアロン版のリリースを読むには、非公開のリポジトリなので読める鍵（`AMONHEN_STANDALONE_TOKEN`）が要る。⑦ Remote-SSH: 拡張の daemon まわりの試験を、Remote-SSH の先と同じ手元の Linux のマシン（x86_64、Node 24）で、配る Linux 用のエンジンで流して通した（ソケットは `$XDG_RUNTIME_DIR`、共有メモリは `/dev/shm`）。Windows: 共有の daemon は名前付きパイプで待つ（git の fsmonitor・VSCode・Docker と同じ使い分け。`crates/remote/src/endpoint.rs`、`interprocess` crate。重ねた I/O で読み書きを同時にでき、名前に利用者の SID のハッシュとビルドの指紋を入れ、権限は自分の SID だけ、最初の作成は `FILE_FLAG_FIRST_PIPE_INSTANCE`）。共有メモリは後で（Node は使えず、Rust の FE で効く。取り決めはもう両方に対応）。エンジンの置き場所は `%LOCALAPPDATA%\amonhen`（`current` はハッシュを書いたファイル、CLI は `bin\amonhen.exe` に写す）、History のキャッシュは `%LOCALAPPDATA%\amonhen\cache`。Windows 用のエンジン（x86_64・aarch64）は CI の Windows のマシンで作り（`.github/workflows/windows-engine.yml`）、リリースに zip で載せ、.vsix に同梱する。CI の Windows のテストで、共有の daemon 越しの CLI（`works_through_the_shared_daemon_on_every_platform`）を流す。Remote-SSH の画面でも確かめた（Extension Development Host を手元の Linux のマシンに Remote-SSH でつなぎ、拡張が SSH 先で同梱の Linux 用エンジンを置き場所に置いて `~/.local/bin/amonhen` を張り、`$XDG_RUNTIME_DIR` の共有の daemon で toybox の History・コミットの詳細・差分を出した）。Windows は CI の Windows のマシンで、共有の daemon 越しの CLI の試験が名前付きパイプの上で通った。拡張の試験は CI で macOS・Ubuntu・Windows のすべてで流す（Windows では、git の返す `C:/…` のパスをその OS の形にそろえ、止めた直後の git がつかんでいる一時ディレクトリは待って消し直す）。Windows の実機（x64、Windows 11 Pro。[windows.md](windows.md)）で確かめた。VSCode の拡張が同梱のエンジンを `%LOCALAPPDATA%\amonhen` に置き、名前付きパイプの共有の daemon（CLI と共有）で History、コミットの詳細と差分、Working Copy の行単位の stage、commit と Undo を行えた。見つかった 3 つの問題は直した。コンソールを持たない daemon が git や ssh を起動するたびにコンソールの窓が一瞬開いていた（起動はすべて `CREATE_NO_WINDOW` を付ける。`amonhen_model::process::command`）。Working Copy の差分が git の改行の変換を無視していて、`core.autocrlf=true` では変えたファイルがすべて全体の書き換えに見え、行単位の stage で index に CR が入っていた（作業ツリーを git に入れる形で読む。gix の filter pipeline で、git の組み込みのフィルターだけをかける。行単位の破棄はほかの行のバイトを変えない。SSH 先で変換するため protocol を 18 に上げた）。拡張のテストが、常駐の `git cat-file` が終わる前に一時的なリポジトリを消していて、Windows では消せなかった。次: WASM（エンジンがない環境で、コアを WASM で動かす。wasm32-wasip1 にビルドでき、Node の `node:wasi` で動いたが、gix がパックを mmap で読むので WASI では読めない。git の読み書きを拡張の git 実行層に任せる形が本命）
- [ ] 19. ほかの OS の FE（iOS / Linux / Android / Windows）。プロジェクションの形式と Platform の実装、モバイルは Git2Backend を足す。Linux（GTK4、`apps/gtk4`、gtk4-rs。Linux に固有のところはないので BSD でも動き、GTK の動く Windows・macOS でも動く（そちらはそれぞれのネイティブの FE を配る）。ルートの workspace に GTK が要らないよう、別の Cargo の workspace にする）: 着手。手元のリポジトリを開いて History を出す（グラフは GSK のパスで、色と太さごとにまとめて描く。ref のラベル・件名・作者・日時・ID は Pango。見えている行だけを 1 つの `GtkScrollable` のウィジェットが描き、件名などは `FrontendProfile::GTK` で 256 行ずつ裏で読み、見えている所に近い 24 チャンクだけを持つ）。クリックとキーでの選択、同じアニメーションの魚眼のレーン、同じ永続キャッシュ、ref が変わったら読み直す（`amonhen-watch`）。コミットを選ぶと、macOS 版の「詳細」の並びで詳細を出す（上に作者とコミッター、英語の見出しの表（Commit・Parent・Child・Rev・Signature・Refs・Branches・Tags・Previous Tag・Pushed・Merged）、件名・本文・notes。`commit_info` を先に出し、`commit_extras` は届いたら足す。欄は中身の高さに合わせ、右の列の半分まで）と全ファイルの差分（`selection_diff`。History と同じく見えている行だけを描く。変わった単語の強調と構文の色は、位置がもう UTF-8 のバイトなので、そのまま Pango の属性にする。ファイルの見出しをクリックすると畳む）を出す。サイドバー（`GtkTreeListModel` の `GtkListView` なので、見えている行の部品しか作らない）: Workspace（Working Copy と変更のあるファイルの数、History）、Branches（`feature/x` のような名前はフォルダにまとめる。今のブランチは太字、上流との ↑↓、gone）、Remotes（リモートごと、その中もフォルダ）、Tags（多ければ最初は閉じる）、Stashes（メッセージで）、Submodules。アイコンは GSK のパスで文字の色に描く（アイコンのテーマにブランチやタグのものがないことがあるため）。ref を選ぶと、History でそのコミットを選ぶ（行の索引は初回に裏で作る）。ref の右クリックで、macOS 版と同じ操作を出す（ローカルブランチ: チェックアウト・merge・rebase・上流への push・新しいブランチやタグ・名前の変更・削除。リモートブランチ: ローカルブランチとしてチェックアウト・merge・rebase・新しいブランチ。タグ: merge・新しいブランチ。stash: apply・pop・drop。サブモジュール: 更新）。ブランチかタグをローカルブランチに落とすと merge と rebase、ローカルブランチをリモートかリモートブランチに落とすと push と強制 push（確かめてから）。操作はコアの命令（`run_json`）を裏で実行し、失敗とコンフリクトを知らせ、History・ref・status を読み直す。取り消す（見出しのボタン。入力欄が使わないときは Ctrl+Z も）は、次に取り消す操作を説明に出す。メニューの項目は、名前・並び・区切りを macOS 版と同じ（英語）にし、置き場所は GNOME に合わせる: 見出しの左の「Repository」のボタンが macOS の Repository メニュー（Fetch・Pull・Pull With・Push・Force Push (with Lease)…・New Branch…・Rename Branch…・Set Upstream…・New Tag…・Merge…・Rebase…（macOS 版と同じく一覧から選ぶ）・Continue・Skip・Abort・Stash…・Pop Stash・Undo・Rebuild History・Open in Terminal・Reveal in Files）、右端の主メニュー（≡）に File・View・Navigate・Commit を同じ名前のサブメニューで入れ、最後に Settings・Activity・Keyboard Shortcuts・About Amon Hen。GTK4 版にまだ機能のない項目（Edit・検索・表示の切り替え・GitHub・タブ）は入れない。項目は窓のアクションで、ショートカットは ⌘ を Ctrl に、⌃・⌥ を Alt に読み替える（Ctrl+O・Ctrl+1・Ctrl+2・Ctrl+R・Ctrl+Shift+B・Ctrl+Shift+H・Alt+↑↓・Ctrl+Alt+F・L・P・B・M・R・S・Z・A・Ctrl+Return・Ctrl+,）。Ubuntu が全体で使うキー（New Tag の Ctrl+Alt+T）は外す。一覧は `GtkShortcutsWindow`。見出しには macOS 版のツールバーの既定のボタンも同じ並びで置く（今のブランチの名前とメニューの Branch、Fetch、メニューと上流との ↓・↑ の数の付いた Pull と Push、Merge、Rebase、Stash、Pop、右に Refresh と Undo。説明も同じ）。アイコンは文字の色で描く。リポジトリは、パス、`daemon:/path`（手元の共有の daemon。`~/.local/share/amonhen/current` のエンジンか PATH の `amonhen` で起動する）、`ssh://host/path`（SSH 先の daemon。送り込むエンジンは `AMONHEN_ENGINES` か、実行ファイルの隣の `engines`（macOS 版の Resources/engines と同じ）から。History は場所ごとに手元にキャッシュする）のどれでも、起動の引数か File > Add Remote… で開ける。daemon 越しなら、ファイルの監視ではなく daemon の知らせで読み直す。macOS から SSH で luna.local の openbsd-src を開き、History・詳細・差分が出た。History の検索は macOS 版のツールバーの検索の欄と同じ動きで、GNOME の形の検索バーとして History の上に出す（見出しの検索のボタンか Ctrl+F）。Enter でコアの検索の書き方で探し（`search_history`、裏で）、一致した行を塗り、何番目 / 全体を出し、Enter・Shift+Enter（Ctrl+G・Ctrl+Shift+G）で一致の間を移る。History が新しくなったら探し直す。∧∨ の右の切り替えで、macOS 版と同じく一致したコミットだけを出す（`history_matching`。first-parent をたどって次の一致につなぐ）。探すのはいつも元の History。サイドバーの右クリックも macOS 版と同じ項目（Checkout main・Merge X into Y・Rebase Y onto X・New Branch from X…・サブモジュールの Open X in New Window など）で、メニューのモデルから作る `GtkPopoverMenu`。History のコミットを右クリックすると、そのコミットを選んで macOS 版と同じコミットのメニューを出す（New Branch at・New Tag at・Checkout (Detached)、merge でないコミットには Edit Message…・Squash into Parent…・Fixup into Parent・Edit (Stop Here)・Split…・Drop（確かめてから）、Cherry-pick・Revert、Copy Commit ID）。コアの `rewrite`・`commitMessage`・`squashMessage` を、macOS 版と同じダイアログで使う。コミットは複数選べる（Ctrl+クリック・Shift+クリック・Shift+↑↓）。そのときは詳細の欄に、選んだ数、差分の取り方（続いていれば一番古いものの親からの 1 つの差分、離れた 2 つなら A..B と、向きの入れ替え・merge-base からへの切り替え、とびとびならコミットごとの差分をファイルごとに）、各コミットの 1 行を出し、差分は `selection_diff` から。右クリックは macOS 版と同じく Cherry-pick N Commits・Revert N Commits・Squash N Commits…（続いていて merge を含まないとき）・Copy Commit IDs・Copy Short IDs。Working Copy（History と並ぶページで、サイドバーで切り替える）: コミットメッセージ（件名・本文・直前のコミットの修正。修正なら HEAD のメッセージを入れる）、変更のあるファイルの一覧（次のコミットに含めるかのチェック。index の状態そのもので、一部だけなら混在の印。コンフリクトが先）、選んだファイルの差分。差分は HEAD と作業ツリーの 1 本の差分に、変更の行ごとに index に入っているかの印を付ける（`working_diff_combined`。印のクリックでその行を含める・外し、クリック・Shift・Ctrl や hunk の見出しで選んだ行をボタンで含める・外す・破棄する）。そう表せないファイルは、作業ツリーの側と stage 済みの側を切り替える。index か作業ツリーが変わったら status を読み直す。文言は macOS 版と同じ（Summary・Amend・Commit / Amend Commit と Commit and Push・Amend Last Commit、Changes と含める数 / 全体、Unstaged / Staged、Stage Lines / Unstage Lines、Discard Lines / Discard File）。ファイルごとに行数（`workingStats`）と、マウスを載せたときに変更を捨てるボタン（確かめてから。untracked は削除で、Undo では戻せない）を出し、パスで絞り込める。一覧は macOS 版と同じく、パスの一覧、ディレクトリの木（子が 1 つだけのディレクトリは名前をつないで 1 段にし、クリックで畳む）、ファイル名を先に親のディレクトリを薄く、のどれかで見せる。コンフリクトのファイルは、マーカー付きの中身（`conflict_view`。Current と Incoming を色分け）と Take Current File・Take Incoming File・Open Merge Editor・Mark Resolved（コンフリクトが残っていれば確かめてから）を出す。マージエディタは macOS 版と同じく別の窓で、Current・Result（編集できる。今の作業ツリーの内容から始める）・Incoming を並べ、共通の祖先（diff3）があれば Base を左端に出せる。コンフリクトはどの欄でも色を付け、解決済みのものは左右の欄で薄くし、欄のスクロールはコンフリクトの位置で揃える。選んだコンフリクト（∧∨ か Ctrl+Alt+↑↓、欄のクリックで選ぶ）を Accept Current・Accept Incoming・Accept Both・Accept Base で解き、✨ Magic Merge はコアの規則で解けるものを解く。どれも Result の変わった所だけを置き換えるので、Ctrl+Z で 1 つずつ戻せる。開いたときに、間違えようのないコンフリクト（両側が同じ、片側だけが変えた）を解く（Settings > Git で切れる）。Save（Ctrl+S）と Save & Mark Resolved（Ctrl+Return。コンフリクトが残っていれば確かめてから）は `saveMerge` で書き、保存していない変更があれば閉じる前に確かめる。コンフリクトの計算は文字だけを扱うのでアプリの中で `amonhen_core::conflict` で行い、両側は `mergeSides` で読むので、daemon 越しでも使える。AI のボタンと、欄の間をつなぐ帯は入れない。`--snapshot out.png` と `--measure-scroll`。macOS の GTK で測った値（linux の 148 万コミット、1 フレームに 40 行）: History の snapshot は p50 3.2 / p95 4.1 ms で、グラフは約 0.1 ms、残りのほとんどは新しく見えた行の Pango の shaping（メインスレッド）。cairo の描画器ではメインスレッドが 1 フレーム p50 5.7 / p95 6.8 ms で 60 Hz（600 フレーム中 11 フレームを落とした）。macOS の GL の描画器は処理の量によらず半分の速さになるので、GPU の描画器は本物の Linux で測る必要がある。Ubuntu 24.04（GTK 4.14、GNOME on Xorg、NVIDIA RTX 5060 Ti、60 Hz。openbsd-src の 24.8 万コミット、1 フレームに 40 行）では、既定の描画器でメインスレッドが 1 フレーム p50 1.6 / p95 2.2 ms（History の snapshot は p50 1.1 ms）で、vulkan・ngl・cairo でもほぼ同じ。600 フレーム中 約 220 フレームを落とすが、1 フレームに 1 行（仕事は 0.8 ms）でも同じだけ落とすので、アプリではなく X11 のセッションのフレームの同期による。同じマシンの Wayland のセッションでは、フレームをほとんど落とさない: 既定の描画器（ngl）で、1 フレームに 40 行ならメインスレッドが p50 1.5 / p95 2.2 ms、600 フレーム中 2 フレームを落とした（1 行なら 0.6 / 1.0 ms で落としたのは 0）。ngl・cairo もほぼ同じで、vulkan は 40 行で p50 2.5 / p95 3.6 ms、落としたのは 0（1 行では最初に 146 ms 止まり、19 フレームを落とした）。200%（Wayland は拡大率をコンポジタから受け取るので、Xwayland で `GDK_SCALE=2`。窓は 1920×1080 の画面いっぱいで、100% の 1280×760 の窓の約 2 倍のピクセル）で 1 フレームに 40 行なら、cairo が p50 1.7 / p95 2.5 ms、vulkan が p50 2.3 / p95 3.9 ms で、どちらも 100% とほぼ同じ。この CPU（Ryzen 9 9950X3D）では cairo のソフトウェアの描画は 2 倍のピクセルでも増えず、vulkan は CPU の仕事が多く、最初に 1 回止まる（191 ms）。cairo ではアプリは GPU を使わない（`nvidia-smi` の一覧に出ない。CPU で共有メモリに描き、mutter が GPU に上げる）ので、メインスレッドの時間には、ngl・vulkan では GPU が描く時間が、cairo ではコンポジタが上げる手間が入っていない。フレームの始まり（スクロールを進めた時点）から、コンポジタが知らせる画面に出た時刻（`GdkFrameTimings` の presentation time）までは、どの描画器でも同じフレーム数になる: Wayland で p50 33.3 ms（2 フレーム）、p95 は ngl 33.7・vulkan 33.9・cairo 34.4 ms。Xwayland の 200% では 3 つとも約 48.6 ms（3 フレーム）。遅れを決めるのはコンポジタの流れで、アプリの描き方ではない（macOS 版も WindowServer を通して 2 フレーム）。アプリでは描画器を決めず、GTK に任せる（`GSK_RENDERER` で替えられる）。Ubuntu のフォントは macOS より大きく、大きさがピクセルで決まっているので、History の行の高さはフォントから決め、小さくするフォントも同じ単位で縮める。History と差分はテーマの `view` の背景を使い、暗いテーマでも読めるようにした。Blame と File History は macOS 版と同じく別の窓で、History の差分のファイルの見出し（Blame at <コミット>・File History）と Working Copy のファイルの右クリック（Blame・File History）から開く。Blame は同じコミットの行をまとめ、まとまりの 1 行目に ID・作者・日時、2 行目に件名を出し、まとまりごとに背景を交互に変える。File History は、ファイルを変えたコミット（名前の変更も追う）と、そのコミットでのファイルの差分。どちらも `GtkListView` で、行をダブルクリックするとそのコミットを History で選ぶ。Activity（主メニューか Ctrl+Alt+A）は macOS 版と同じタブの窓で、開いている間は 1 秒ごとに読み直し、中身が変わったときだけ作り直す: Operations（`amonhen_model::activity` の git の起動とコアの命令、daemon 越しなら `remoteActivity` で裏で読んだ daemon の分も。新しい順で ✓・✗・●（実行中。進み具合と取り消すボタン）、種類・失敗だけ・文字で絞り込み、JSON に書き出せる）、History（Undo の記録。一番新しいものに Undo、ほかに「ここまで戻す」）、Performance（History・status の読み直しと差分の読み込みの回数・p50・p95・最大・直近）、Events（監視が知らせた ref・index・作業ツリーの変化）、Daemon（共有の daemon の pid・版・動いている時間・つながり・リポジトリと、起動・停止）。Settings（主メニューか Ctrl+,）は macOS 版と同じタブで、GTK4 版に機能のある項目を同じ鍵で持つ: Git（Pull の既定。merge・rebase・fast-forward のみ。Pull と Ctrl+Alt+L で使う。マージエディタを開いたときに、間違えようのないコンフリクトを解く）、Appearance（本文を等幅の字体で表示する）、Performance（手元のリポジトリを開いたとき、commit-graph がなければ裏で書き出す）、Advanced（リポジトリを共有の daemon 越しに開く）。`~/.config/amonhen/gtk.ini`（GLib の KeyFile。GSettings はスキーマを入れる必要があり、展開しただけの実行ファイルにはない）に置き、Working Copy の一覧の見せ方も覚えて次の窓で使う。配布: `scripts/package-gtk4.sh` が Linux で `dist/amonhen-gtk-<版>-linux-<arch>.tar.gz` を作る（実行ファイル、SSH 先に送る `dist/engine` の Linux のエンジン、`.desktop`、アイコン、`install.sh`。`install.sh` は `~/.local` に置き、デスクトップのアプリの一覧に出し、同梱のエンジンを共有の daemon 用に置く）。GTK は同梱しないので、GTK 4.14 以降が要る（Ubuntu 24.04・Debian 13 以降）。CI は Ubuntu 24.04 でパッケージまで作り、正式版と snapshot に x86_64・aarch64 の分を載せる（`build.yml` の `gtk`。版は About に出す）。
- [ ] 20. ブランチの操作。済み: サイドバーの Branches を探しやすく並べる（お気に入り ★ をドラッグで決めた順に、今のブランチをその土台ごと、既定のブランチ、動いているブランチを新しい順に `/` のフォルダで、そのあとに畳んだ Inactive と Archived）、ほかのブランチの上に積んだブランチをその下に入れ子で出す（↳。土台は、上流に当たる手元のブランチ、なければ先端から first-parent を辿って最初に当たるブランチ。先端がそこにあるもの、または push したブランチで固有のコミットがそこにあるもの。固有のコミットは first-parent の鎖のうち作った点（reflog）より新しいもので、土台が後から進んでも積んだほうと取り違えない。既定のブランチの履歴はどのブランチの印にもしない。push したブランチもほかと同じように積まれる。`crates/core/src/branches.rs`、`{"op":"branchGroups"}`）、行の右端の印（既定のブランチに取り込み済み、上流が gone、ほかの worktree でチェックアウト中、↑↓、編集中）を押すと意味と次の一手（ブランチを消す、アーカイブ、worktree を開く）の吹き出し、アーカイブとお気に入りは git の設定に記録（`branch.<name>.amonhenArchived`・`amonhenPinned`）、上流と分かれたブランチを選ぶと History に上流も一緒に出す、git の `rebase.updateRefs` が true なら、rebase と History の編集で、今のブランチの下に積んだブランチも一緒に動かす（rebase は git に任せ、History の編集では todo に `update-ref` の行を入れる。git 2.38 から。ここやほかの worktree でチェックアウト中のブランチは、git と同じく動かさない。Undo ではそれらも戻し、チェックアウトしていない ref は 1 つの `update-ref --stdin` のトランザクションでまとめて書く。ブランチの改名は逆向きの改名で戻すので、設定と reflog も戻る）、merge・pull・rebase でコンフリクトするファイルを前もって見せる（`predictMerge`。`git merge-tree --write-tree --name-only` で、作業ツリーと index には触らない。Pull のシートでは両方に新しいコミットがあるとき、Merge…・Rebase… のシートでは選び直すたび、ブランチの吹き出しでは既定のブランチへのマージを開いたときに 1 回だけ調べる。rebase は最後の結果での目安。git が 2.38 より古ければできないので、分からないと出す。`amonhen merge-check` からも。linux で 1 回 50〜90 ms）、いくつものブランチ・タグ・コミットを `git merge a b c` と同じく 1 つのコミットで merge する（octopus。サイドバーか History で複数の行を選んだ「N 個を … に merge…」から、merge する順に並べたシートを出す。順はドラッグで変えられ、それぞれのコンフリクトの見込みを前のものまでの結果に重ねて調べ（配列を渡した `predictMerge`。ここまでの結果を ref のない仮のコミットにして merge-tree）、git が書くメッセージも見せる。最後より前のものがコンフリクトすると git はコンフリクトを残さずに止まるので、シートでそれを最後に回すか、1 つずつ merge することを勧め（`oneByOne`。1 つずつ merge のコミットにし、コンフリクトで止まったら残りを知らせる。Undo 1 回で全部戻る）、それでも git が断ったら 1 つずつを改めて勧める。git 2.37 より前は、断ったときにそこまで合わせた変更を作業ツリーに残し、`merge --abort` に要る MERGE_HEAD もないので、新しい git と同じくバックエンドが `reset --merge` で戻す。no-ff・squash は 1 つの merge と同じく git の設定に任せる。`amonhen merge-check a b c` からも）。予定: 取り込み済みのブランチをまとめて消す（既定のブランチに取り込み済みのブランチを一覧にしたシート。今のブランチ・お気に入り・ほかの worktree が使っているものは除く。`git branch -d` なので取り込んでいないものは消えない。Undo に `git branch <名前> <前の先端>` として記録）、GitHub で Squash か Rebase でマージした PR のブランチも取り込み済みにする（`gh pr list --state merged` でマージ済みの PR を読み、head のブランチを名前と先端で突き合わせ、吹き出しに「PR #123 でマージ」と出し、シートではそれらだけ `-D` を許す）、gone の上流の片付け（Fetch は `fetch.prune` がオンなら `--prune` を付ける。上流が gone で取り込み済みの手元のブランチを消すことを勧める）、複数のブランチをまとめてアーカイブ（サイドバーの複数選択）、`origin/HEAD`・`main`・`master` のないリポジトリ（取り込み済みと幹が分からない）のための、リポジトリごとの既定のブランチの設定
- [ ] 21. 書き換える操作と、積み重ねたブランチ（「書き換える操作と、積み重ねたブランチ」）。済み: Amon Hen が自分で `rebase.updateRefs` を有効にするのをやめた。ユーザー（この Mac の `--global`。リポジトリの行を「すべてのリポジトリに設定」で）とリポジトリ（リポジトリの Settings の画面。書くのは `--local` だけで、ユーザーの `--global` が効いているときは札でそう示す。`{"op":"gitVersion"}` と、`"scope":"global"` 付きの `configSet`）の 2 段の設定。git 2.38 より古ければ、見つかった版を添えて選べなくする。リポジトリの設定をシートからサイドバーの Workspace の Settings の画面へ移し、使う人から作り直した（ux.md の 4 章、U20）。予定（この順に進める）: `rewritePreview` の命令と CLI。History と同じ描き方で前と後を並べ、この操作だけのチェックボックスを持つ確認のシート。VSCode 拡張でも同じにする。
