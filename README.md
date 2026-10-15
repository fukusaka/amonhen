# Amon Hen Standalone

徹底的な低遅延を最優先にした、macOS 向けの Git クライアント。コアは Rust、UI は AppKit と Metal。
設計は [docs/design.md](docs/design.md) を参照。

## インストール

[GitHub CLI](https://cli.github.com/) で Release から取ってきて `/Applications` に入れる（入っていれば置き換える）。

```sh
scripts/install.sh            # 最新のリリース（Amon Hen）
scripts/install.sh snapshot   # main の最新（Amon Hen Snapshot、正式版と並べて入れられる）
```

`gh` で取ってきたファイルには quarantine 属性が付かないので、公証なしで起動できる。ブラウザで落とした場合は
`xattr -dr com.apple.quarantine "/Applications/Amon Hen.app"` で外す。

## 使い方

- 起動するとリポジトリの一覧（File > Repositories…、⇧⌘O）が出る。開いたことのあるリポジトリが新しい順に並び、Add… で加え、Remove で外す（フォルダには触れない）。File > Open…（⌘O）でほかのリポジトリを別のウィンドウで開く。最後のウィンドウを閉じると一覧に戻る。
- サイドバー: Working Copy / History、ブランチ（フォルダ・上流との差 ↑↓・gone）、リモート、タグ、stash、サブモジュール。右クリックで checkout・merge・rebase・作成・名前の変更・削除・stash の操作・サブモジュールの更新と別ウィンドウ。ブランチを別のブランチにドロップすると merge / rebase（fast-forward で済めば checkout しない）、リモートにドロップすると push。
- ツールバー: Fetch・Pull・Push・Stash・Undo。履歴の書き換え・変更の破棄・stash の削除などは Undo で戻せる（VSCode 拡張版と記録を共有する）。
- Working Copy: ファイルはパスの一覧かディレクトリの木で表示（一覧の上で切り替え）。差分の行をクリックで選び（hunk の見出しで hunk 全体、Shift で範囲、⌘ で追加）、Stage Lines / Discard Lines。Space・Enter で stage / unstage。Summary と Description を書いて ⌘↩ でコミット。コンフリクトはコンフリクトごとに Use Current / Use Incoming / Use Both、または Merge Editor…（3 ペインのマージエディタ）。
- History: グラフ付きの一覧（件名とブランチ・タグのラベル、その下に作者・日時・ID）。右クリックでコミットの編集（Edit Message・Squash・Fixup・Edit・Split・Drop）、cherry-pick・revert、ブランチ・タグの作成。ドラッグで並べ替え。上部で All Branches / Current Branch / 選んだ ref を切り替える。⌘F でコミットを検索（件名・作者・ID、Enter・⌘G で次へ）。
- 差分: ドラッグで文字を選んで ⌘C、⌘F で検索。構文の色付けは表示の後に裏で付く。
- History の検索の構文: 語は件名・作者・ID から探す。空白で区切ると AND、`or` で OR、括弧でまとめる。`author:` `subject:` `commit:` `path:` `contents:"..."`（例: `path:fs/ (author:linus or author:greg)`）。
- ファイルの右クリック（Working Copy・History の変更ファイル）で Blame と File History（名前の変更も追う）。行やコミットをダブルクリックすると History で選ぶ。
- daemon 越しに開く（SSH 対応の準備中）: `AmonHen daemon:/path/to/repo` で、同梱の `amonhend` を起動してつなぐ。
- View > One Line per Commit で History の行を 1 行にする。終了したときに開いていたリポジトリは、次の起動で開き直す。

## 開発

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
scripts/build-core.sh && (cd apps/macos && swift test)
scripts/build-app.sh snapshot   # dist/ に .app と zip を作る
```

確認用の起動オプション: `AmonHen <repo> --snapshot out.png [--select-first | --select-row N] [--working-copy] [--find TEXT] [--search TEXT] [--merge-editor PATH]`、`--blame PATH`・`--file-history PATH`（そのウィンドウを開く）（画面を PNG に書き出して終了）、`--measure-scroll [--scroll-rows N]`（History を 600 フレーム自動でスクロールして速さと処理ごとの内訳を表示）、`--measure-select N`（コミットを N 個選び、詳細が出るまでの段階ごとの時間を表示）、`--log-scroll`（トラックパッドでのスクロールの減速を記録）、`--show-latency`（入力から表示までの時間をタイトルの下に出す）。

## リリース

- `v*` タグを push すると、GitHub Actions がビルドして Release に zip を添付する。タグは `Cargo.toml` の `workspace.package.version` と一致させる。
- main に push するたびに、`snapshot` という pre-release を作り直す（バージョンは `<次の patch>-snapshot.<コミット数>+<sha>`）。

### 署名（自己署名の証明書）

ad-hoc 署名だとビルドのたびにアプリの身元が変わり、macOS のアクセス許可（フォルダへのアクセスなど）が更新のたびにリセットされる。自己署名のコード署名用証明書を 1 つ作って CI に渡すと、身元が固定される。

1. キーチェーンアクセス > 証明書アシスタント > 証明書を作成…で、名前を `Amon Hen`、固有名の種類を「自己署名ルート」、証明書のタイプを「コード署名」にして作る。
2. キーチェーンアクセスでその証明書と秘密鍵を選び、.p12 として書き出す（パスワードを付ける）。
3. リポジトリの secret に登録する。

   ```sh
   base64 -i amonhen.p12 | gh secret set AMONHEN_CERT_P12
   gh secret set AMONHEN_CERT_PASSWORD
   gh secret set AMONHEN_CERT_NAME --body "Amon Hen"
   ```

secret がなければ ad-hoc 署名で作る。手元では `AMONHEN_SIGN_IDENTITY="Amon Hen" scripts/build-app.sh` で同じ証明書を使える。
