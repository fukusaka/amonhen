# Amon Hen

徹底的な低遅延を最優先にした Git クライアント。コア（エンジン）は Rust で、macOS のアプリ（AppKit と Metal）、
CLI（`amonhen`）、VSCode 拡張（[apps/vscode](apps/vscode)）が同じエンジンと共有の daemon を使う。
設計は [docs/design.md](docs/design.md) を参照。

## 動作環境

- アプリ: macOS 14.4 以降、Apple シリコン（arm64）。
- エンジン（CLI と daemon）: macOS arm64、Linux x86_64・aarch64（musl の静的リンクなので、どの Linux でも動く）、
  Windows x86_64・aarch64。
- どれも `git` を使う（書き込みと一部の読み取りは `git` を起動する）。

## インストール

アプリ（[GitHub CLI](https://cli.github.com/) で Release から取ってきて `/Applications` に入れる。入っていれば置き換える）:

```sh
scripts/install.sh            # 最新のリリース（Amon Hen）
scripts/install.sh snapshot   # main の最新（Amon Hen Snapshot、正式版と並べて入れられる）
```

アプリを入れると、エンジン（CLI と daemon）も `~/.local/share/amonhen` に置かれ、`~/.local/bin/amonhen` から使える。
エンジンだけを入れるときは、Release の `amonhen-<版>-<OS>-<CPU>`（Linux・macOS は tar.gz、Windows は zip）を展開して
`amonhen install` を実行する（Windows は `%LOCALAPPDATA%\amonhen` に置き、`%LOCALAPPDATA%\amonhen\bin` を PATH に足す）。

VSCode 拡張は [apps/vscode](apps/vscode) の `scripts/install.sh [stable|snapshot]` で入れる。拡張はエンジンを同梱していて、
入れれば共有の daemon を使う。

## 使い方

- 起動するとリポジトリの一覧（File > Repositories…、⇧⌘O）が出る。開いたことのあるリポジトリが新しい順に並び、Add Repository… で加え、Remove で外す（フォルダには触れない）。File > Open…（⌘O）でほかのリポジトリを別のウィンドウで開く。最後のウィンドウを閉じると一覧に戻る。
- サイドバー: Working Copy / History、ブランチ（フォルダ・上流との差 ↑↓・gone）、リモート、タグ、stash、サブモジュール。右クリックで checkout・merge・rebase・作成・名前の変更・削除・stash の操作・サブモジュールの更新と別ウィンドウ。ブランチを別のブランチにドロップすると merge / rebase（fast-forward で済めば checkout しない）、リモートにドロップすると push。
- ツールバー: Fetch・Pull・Push・Stash・Undo。履歴の書き換え・変更の破棄・stash の削除などは Undo で戻せる（CLI・VSCode 拡張と記録を共有する）。
- Working Copy: ファイルはパスの一覧かディレクトリの木で表示（一覧の上で切り替え）。差分の行をクリックで選び（hunk の見出しで hunk 全体、Shift で範囲、⌘ で追加）、Stage Lines / Discard Lines。Space・Enter で stage / unstage。Summary と Description を書いて ⌘↩ でコミット。コンフリクトはコンフリクトごとに Use Current / Use Incoming / Use Both、または Merge Editor…（3 ペインのマージエディタ）。
- History: グラフ付きの一覧（件名とブランチ・タグのラベル、その下に作者・日時・ID）。右クリックでコミットの編集（Edit Message・Squash・Fixup・Edit・Split・Drop）、cherry-pick・revert、ブランチ・タグの作成。ドラッグで並べ替え。上部で All Branches / Current Branch / 選んだ ref を切り替える。⌘F でコミットを検索（Enter・⌘G で次へ、⇧Enter・⇧⌘G で前へ。一致したコミットだけを出すこともできる）。
- 差分: ドラッグで文字を選んで ⌘C、⌘F で検索。構文の色付けは表示の後に裏で付く。
- History の検索の構文: 語は件名・作者・ID から探す。空白で区切ると AND、`or` で OR、括弧でまとめる。`author:` `subject:` `commit:` `path:` `contents:"..."`（例: `path:fs/ (author:linus or author:greg)`）。
- ファイルの右クリック（Working Copy・History の変更ファイル）で Blame と File History（名前の変更も追う）。行やコミットをダブルクリックすると History で選ぶ。
- SSH 先のリポジトリ: リポジトリの一覧の Add Remote Repository… で接続先とパスを入れるか、`AmonHen ssh://[user@]host[:port]/path/to/repo`（ホームからなら `/~/src/repo`）。鍵か ssh-agent でパスワードなしにつながること。SSH 先では git の処理だけを `amonhen --stdio` が行う。SSH 先の `PATH` に通信の形の版が合う `amonhen` があればそれを使い、なければ同梱のエンジン（Linux x86_64・aarch64、macOS arm64）を `~/.local/share/amonhen/engine/<中身のハッシュ>/` に送り込む（送ったら中身を確かめる。`AMONHEN_PUSH=0` で送らない、`AMONHEN_REMOTE_ENGINE` で場所を指定）。
- 共有の daemon: Settings > Advanced の「リポジトリを共有の daemon 越しに開く」か、一覧の右クリックの Open via Daemon で、手元の共有の daemon 越しに開く（`daemon:/path`・`daemon:ssh://host/path`）。CLI・VSCode 拡張と History・検索の索引・SSH のつながりを共有する。様子は Window > Activity の Daemon のタブか `amonhen daemon status`。
- View > One Line per Commit で History の行を 1 行にする。終了したときに開いていたリポジトリは、次の起動で開き直す。

### CLI

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

## 開発

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
scripts/build-core.sh && (cd apps/macos && swift test)
scripts/build-engine.sh          # dist/engine にエンジン（macOS・Linux。Linux 用は brew install zig が要る）
scripts/build-app.sh snapshot   # dist/ に .app と zip を作る
cargo build -p amonhen-cli && (cd apps/vscode && npm ci && npm test)   # VSCode 拡張
```

確認用の起動オプション: `AmonHen <repo> --snapshot out.png [--select-first | --select-row N] [--working-copy] [--find TEXT] [--search TEXT] [--merge-editor PATH]`、`--blame PATH`・`--file-history PATH`（そのウィンドウを開く）（画面を PNG に書き出して終了）、`--measure-scroll [--scroll-rows N]`（History を 600 フレーム自動でスクロールして速さと処理ごとの内訳を表示）、`--measure-select N`（コミットを N 個選び、詳細が出るまでの段階ごとの時間を表示）、`--log-scroll`（トラックパッドでのスクロールの減速を記録）、`--show-latency`（入力から表示までの時間をタイトルの下に出す）。

## リリース

- `v<数字>` のタグ（`v0.1.1` など）を push すると、GitHub Actions がアプリの zip と、エンジンだけの tar.gz（macOS arm64・
  Linux x86_64・Linux aarch64）と zip（Windows x86_64・aarch64）を Release に載せる。タグは `Cargo.toml` の
  `workspace.package.version` と一致させる。
- VSCode 拡張は `vscode-v*` のタグで、エンジンを同梱した .vsix を載せる（[apps/vscode](apps/vscode) を参照）。
- main に push するたびに、`snapshot` という pre-release を作り直す（アプリ・エンジン・拡張。バージョンは
  `<次の patch>-snapshot.<コミット数>+<sha>`）。

### 確かめ方（ビルドの由来の証明）

Release のファイルには、このリポジトリのどのコミットから、どの CI で作ったかの証明（artifact attestations）が付く
（v0.1.0 より後のもの）。

```sh
gh attestation verify Amon-Hen-0.1.1.zip --repo fukusaka/amonhen
```

### 署名と公証（macOS）

環境 `release` の secret に Developer ID Application の証明書と App Store Connect の API キーがあれば、アプリと
macOS のエンジンを Developer ID で署名し、Apple の公証に出す（アプリには結果を綴じ込む）。ブラウザで取っても
そのまま開ける。

```sh
base64 -i DeveloperID.p12 | gh secret set AMONHEN_CERT_P12 --env release
gh secret set AMONHEN_CERT_PASSWORD --env release
gh secret set AMONHEN_CERT_NAME --env release --body "Developer ID Application: 名前 (チーム ID)"
base64 -i AuthKey_XXXXXXXXXX.p8 | gh secret set AMONHEN_NOTARY_KEY --env release
gh secret set AMONHEN_NOTARY_KEY_ID --env release --body "<Key ID>"
gh secret set AMONHEN_NOTARY_ISSUER --env release --body "<Issuer ID>"
```

Developer ID の証明書は期限（いまは約 1 年）が来る前に作り直し、`AMONHEN_CERT_P12` と `AMONHEN_CERT_PASSWORD` を
入れ替える。secret がなければ ad-hoc 署名で作る。手元では `AMONHEN_SIGN_IDENTITY` に証明書の名前を渡すと、同じ
証明書で署名する。Windows・Linux のエンジンは署名していない（Windows でブラウザから取ると SmartScreen が警告する。
VSCode 拡張に同梱したものは警告なしで動く）。

## ライセンス

[MIT](LICENSE-MIT) か [Apache License 2.0](LICENSE-APACHE) の、好きなほうを選んで使える（`MIT OR Apache-2.0`）。
