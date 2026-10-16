# Amon Hen Standalone 計画

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
| git の書き込み | デスクトップは `git` を spawn、iOS / Android は gix と libgit2 | rebase・merge・push などは本家に任せて正しさと互換性を保つ。モバイルでは git を起動できない。`GitBackend` で差し替える |
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
│ ① 同一プロセス: C ABI   ② ローカル daemon: XPC＋共有メモリ   ③ SSH: stdio     │
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

History の右の列の上の欄（コミットの詳細）は、メタ情報、件名、本文の残りの順に並べる。見せ方は 3 つから選ぶ（欄の右上、選んだものを覚える）。

- 簡潔: 小さなアバター・作者・作者の日時・ref・親の 1 行と件名。欄は文字の高さまで縮める。
- 標準: 上の帯に作者とコミッターを左右に並べ（Gravatar のアバター・名前・メール・日時）、その下に見出しを右揃えにした 2 列の表（ID、親、署名、ref、merge したタグ、取り込んだコミットの数）、件名と本文。
- 詳細: 標準と同じ形で、表に子、`HEAD~2^2~1` の形の位置（そのまま git に渡せる）、含むブランチ・タグ、直前のタグ、push 済みか、取り込んだコミット 20 件、メッセージが参照するコミット（cherry-pick・revert の元、Fixes:）、notes を足す。

署名は 1 行（結果・署名者・鍵・信用度）、親・子は 40 桁で選んでコピーできる。日時は相対ではなく日時とタイムゾーン。行数（+/−）は変更ファイルの一覧の各行に出す。メッセージのトレーラーはメッセージの中で見せ、表には転記しない。アバターは gravatar.com からメールアドレスの MD5 で取り、アプリが動いている間だけ覚える（View > Show Avatars で切れる）。

すぐ出せるもの（ヘッダーとメッセージから読める分。`commitInfo`）を先に描き、時間のかかるもの（署名の検証・notes・行数は git、子・含むブランチとタグ・前後のタグ・HEAD との位置・取り込んだコミットは History のグラフ。`commitExtras`、linux で 30〜55 ms）は裏で読んでから足す。本文は等幅とプロポーショナルを選べ（View メニュー）、長くても畳まずに全文を出す。詳細の欄はコミットを選ぶたびにメッセージの高さに合わせ（右の列の半分まで。続きは欄の中でスクロール）、右上の ⤢ で右の列いっぱいに広げられる。高さ合わせは右の列全体を並べ直す（linux で 5 ms ほど）ので、表示を終えた後にまとめて 1 回だけ行う。History で検索しているときは、件名と本文の一致を強調する。

操作: URL・コミット ID・`#123`（origin が GitHub・GitLab なら issue へ）はリンク。作者を押すとその人のコミットで絞り込み、ref やブランチを押すとその履歴に絞り込む。右クリックで ID・件名・メッセージ・`git show` をコピーする。⌥⌘↓・⌥⌘↑ で最初の親・最初の子へ移る。HEAD に含まれる merge でないコミットは、右クリックの「メッセージを書き直す…」でその場で書き直せる（⌘↩ で保存、push 済みなら確かめる。Undo で戻せる）。

UI と操作の見直し（タブ、ブックマーク、設定、ツールバー、Working Copy・History・マージエディタの作り込み、Activity、GitHub・AI との連携）は `docs/ux.md` にまとめた。

### リポジトリ構成

拡張（vscode-amonhen）とは別のこのリポジトリで作る。

```
crates/
  model/             内部モデル（FE のことを知らない）
  core/              状態・スナップショット・Intent の処理（通信手段に依存しない）
  layout/            全 FE で共通の #[repr(C)] レコード（数値の列、グラフの Node / Edge）
  projection/        FE ごとの形への変換（文字列の符号化、列の選択、配置の揃え方）
  platform/          Platform トレイト
  backend-gix/       GixBackend
  backend-cli/       CliBackend
  backend-git2/      Git2Backend（モバイル向け、後で）
  client/            CoreClient の共通 API
  transport-inproc/  ① C ABI
  transport-local/   ② XPC / Unix ドメインソケット＋共有メモリ（後で）
  transport-remote/  ③ stdio（後で）
  ffi-c/             C ABI ＋ cbindgen のヘッダー
  ffi-uniffi/        Swift / Kotlin 向けのバインディング
bins/
  amonhend/          daemon（後で）
  amonhen/           CLI
apps/
  macos/             Xcode プロジェクト（AppKit）。ffi を xcframework にして取り込む
bench/               criterion のベンチと大規模リポジトリでの計測
```

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

### 別プロセス・リモートにするとき（後で）

- ② ローカル daemon: スナップショットを共有メモリに作り、領域は XPC で 1 回だけ渡す。Intent と通知も共有メモリのリングで送り、相手が寝ているときだけ `os_sync_wake_by_address_any` で起こす。要求処理のスレッドは `QOS_CLASS_USER_INTERACTIVE`。共有メモリの領域は使い回す。
- ③ SSH: daemon（`amonhend --stdio <repo>`、`crates/daemon`）は SSH 先で `GitBackend`・`Operations`・`RepoFiles` を要求に応じて実行するだけにし、レイアウト・永続キャッシュ・差分・色付けは手元のコアで行う（`crates/remote` の `RemoteBackend`）。C ABI と FE はそのまま使える。UI が読むのは手元のメモリだけで、通信に乗るのは git に触れる所だけ。History は永続キャッシュを手元に持ち、つなぎ直したら前回の起点からの新しいコミットだけを受け取る（daemon は `git rev-list` で求める）。要求には番号を振って並行に送り、パス・内容の検索は取り消せる。
- どの動かし方でも、UI が読むのは手元のメモリだけ。通信は裏の同期にしか使わない。

### コミットの署名の検証

- 署名があるか・どの種類か（GPG・SSH・X.509）は、コミットのヘッダーの `gpgsig` で分かるので、History の行を読むときに一緒に取る（`CommitText::signature`、チャンクの `signatures` の列）。gpg は使わない。
- 検証の結果（`git log --format=%G?` の 1 文字）は git に頼む。gpg を 1 件ずつ起動するので 1 件 15 ms ほどかかる。
- 検証で gpg に鍵を取りに行かせない。gpg の `auto-key-retrieve` が有効だと、知らない鍵の署名を検証するたびに鍵サーバーや WKD に問い合わせて鍵束に鍵を加えてしまう（裏で何百も検証するので、ネットワークに出て、利用者の鍵束を勝手に増やす）。git は gpg に引数を足す設定を持たないので、`--no-auto-key-retrieve` を付けて設定どおりの gpg を呼ぶスクリプトを一時フォルダに置き、検証のときだけ `-c gpg.program=` でそれを指す。知らない鍵の署名は「No key」になる。
  - History: 見えている署名つきの行だけを、スクロールが止まって 0.15 秒たってから、8 件ずつ 1 回の git でまとめて検証する（`git log --no-walk=unsorted --format=%H%x00%G?%x00%GS%x00%GK%x00%GF%x00%GT --stdin`。正しい署名にはコミッターのメールを添えて返す）。終わった分からアイコンを替える。描画は覚えた結果を読むだけで、gpg を待たない。アイコンは SF Symbols を Metal の文字のアトラスに入れて描く。
  - 詳細の欄: 署名者・鍵・信用度を、ヘッダーを出した後に裏で読む（`commitExtras`）。
- どこで検証するか:
  - 手元のリポジトリ: 手元の git と鍵（`~/.gnupg`、`gpg.ssh.allowedSignersFile`、`gpg.minTrustLevel` など）。
  - SSH 先のリポジトリ: 設定で選ぶ（既定は手元）。
    - 手元（既定）: コミットのオブジェクト（数 KB）を受け取り、手元の作業用の bare リポジトリ（`$TMPDIR/amonhen-verify.git`）に書いて、手元の git に `%G?` で検証させる。木やほかのオブジェクトは要らない。手元のリポジトリと同じ規則・鍵・信用度になる。書いたオブジェクトの ID が元の ID と違えば断る。
    - SSH 先: daemon に要求を送り、SSH 先の git と鍵で検証する。SSH 先に公開鍵がなければ「検証できない」になる。
  - 信用度は「自分がその鍵をどれだけ信用するか」なので、手元の鍵束で判断するのを既定にした。
- 見せ方（`SignatureStatus`）は、`%G?` に信用度（`%GT`）と「署名者のメールアドレスがコミッターと一致するか」を加えて決める。言葉は GitHub に合わせて英語。

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
  | 検証中 | seal | 薄い灰 | Checking… | — |

  署名者がコミッターと一致するかは、git の `%GS`（鍵の主なユーザー ID だけ）ではなく、署名した鍵に登録されたすべてのメールアドレスで決める。GPG は主鍵の指紋（`%GP`）で `gpg --list-keys --with-colons` を引き（鍵がいくつあっても gpg は 1 回）、ユーザー ID ごとの有効性も読む。SSH は git が allowed signers から見つけたプリンシパル（`%GS`）を使う。吹き出しの「コミッターのメール」の行に、コミッターのメールアドレスが鍵に登録されているかを出す。有効なユーザー ID（u・f・m）にあれば「鍵に登録されている」、有効性が不明なものにだけあれば「鍵に登録されている。ただし、そのユーザー ID は確認されていない」（誰でも他人のアドレスをユーザー ID に書けるため）、失効・期限切れのものにだけある、またはどこにもなければ「鍵に登録されていない」、鍵のメールアドレスを読めなければ「確かめられない」。

  詳細の欄の Signature の行は、アイコン・言葉・目盛り・方式と署名者。かざすと短い説明、押すと吹き出し（説明、署名者、コミッターとの一致、方式、鍵 ID、4 文字ずつ区切った指紋、信用度、検証した場所）を出す。History の行はアイコンだけ（信用度はアイコンの形で分かる）。
- 結果はコア（手元の `Repository`）がコミットごとに覚える（コミットは変わらないため）。アプリを終えると消える。
- 検証する側を変えたら、覚えていた結果はすべて捨てる。鍵や信用度が違うので、結果を引き継がない。コアは番号を進め、変える前に始めた検証の結果が後から届いても覚えない。History の丸と詳細の欄も、結果を捨てて検証し直す。

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

キャッシュがない初回の 2.4 s は、commit-graph があれば約 1.2 s になる。commit-graph がないリポジトリで `git commit-graph write --reachable` を裏で実行するかは設定で選ぶ（`writeCommitGraph`、既定は行わない。リポジトリの `.git` に書き込むため）。

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
- SSH 先に送る amonhend は、x86_64 と aarch64 の Linux 向けに musl で静的リンクしたもの（`scripts/build-linux-daemon.sh`、リンクは zig、約 3.5 MB）。名前に中身のハッシュを付け、アプリの版が変われば送り直す。

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

linux（148 万コミット）では、1 行を横切る線が中央値で 365 本、最大のレーン番号は中央値で 452（`crates/core/examples/layout.rs`）。トポロジー順（`git log --topo-order` と同じ並べ方）でも 303 本・637 で、並べ方では解決しない。そこで、見えている範囲に点のあるレーンだけを広く取り、線だけのレーンは細く薄い帯に詰め、何もないレーンは幅 0 にする（魚眼）。範囲が変わったときのレーンの動きは約 0.15 秒のアニメーションでつなぐ。

実際のリポジトリ（`crates/core/examples/layout.rs`）では、coreutils（30,176 コミット）で 1.2 ms、android/development（34,261 コミット、起点 318 個）で 17 ms。後者はリリースブランチが多くレーンが 673 本まで広がるので、未使用レーンを詰める・魚眼で広げるなどの表現の工夫（実装順序の 13）が要る。

## 配布

- arm64 の `.app` を GitHub Releases に置く。拡張と同じく `v*` タグの push で GitHub Actions がビルドして添付し、main への push で `snapshot` の pre-release を作り直す。
- 署名は自己署名のコード署名用証明書で行う。ad-hoc 署名だとビルドのたびに身元が変わり、macOS のアクセス許可（フォルダへのアクセス、キーチェーンなど）が更新のたびにリセットされるため。証明書と秘密鍵は GitHub Actions の secret に置く。
- アプリ名は `Amon Hen`、バンドル ID は `io.github.fukusaka.amonhen`。バンドル ID は設定・キャッシュ・アクセス許可のひも付け先なので変えない。snapshot 版は `Amon Hen Snapshot`（`io.github.fukusaka.amonhen.snapshot`）にして、正式版と並べて入れられるようにする。
- `.app` の組み立ては `scripts/build-app.sh [stable|snapshot]`（`dist/` に zip）。証明書は secret の `AMONHEN_CERT_P12`・`AMONHEN_CERT_PASSWORD`・`AMONHEN_CERT_NAME` から CI の一時キーチェーンに入れる（`scripts/import-signing-certificate.sh`）。
- インストール・更新は `scripts/install.sh [stable|snapshot]`。`gh release download` で取得して `/Applications` に置く。quarantine 属性が付かないので、公証なしで起動できる（最初の配布で確かめる。付いていたら `xattr -d com.apple.quarantine` で外す）。
- Developer ID の取得と公証、自動更新（Sparkle）は、人に配りたくなったときに検討する。

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
- [ ] 13. グラフ表現の工夫（祖先の強調、折りたたみ、魚眼、アニメーション、密度表示）。魚眼（`crates/core/src/lanes.rs`）、レーンの動きのアニメーション、選んだコミットの祖先の強調（`crates/core/src/lineage.rs`）は済み
- [x] 14. 配布（GitHub Actions、snapshot、自己署名、install.sh）

拡張:

- [ ] 15. CLI（`amonhen`）
- [ ] 16. ローカル daemon（XPC＋共有メモリ）。SSH には要らない macOS 向けの最適化なので、17 の後に回す
- [x] 17. SSH 越しの daemon と手元の複製。段階 A0（コアのファイル操作を `RepoFiles` に）・A（`amonhen-remote` と `amonhend`、手元の子プロセスでの確認）・B（アプリから `daemon:/path` で開く。amonhend はアプリに同梱）・C（`ssh://host/path` で、SSH 先の OS と CPU に合う amonhend を `~/.cache/amonhen/` に送り込んで起動する。チャンクは件名などだけ、件名の検索は daemon の側、大きなフレームは LZ4 で圧縮）・D（リポジトリの一覧の「Add Remote…」、つなぐ間の接続中のウィンドウ、初回の読み込みの受信量、切れたときの帯と「つなぎ直す」）は済み。実際の SSH 先（Linux x86_64）でも開けることを確かめた
- [ ] 18. VSCode 拡張からの daemon 利用
- [ ] 19. ほかの OS の FE（iOS / Linux / Android / Windows）。プロジェクションの形式と Platform の実装、モバイルは Git2Backend を足す
