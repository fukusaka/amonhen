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

- 起動するとリポジトリを選ぶ。File > Open…（⌘O）でほかのリポジトリを別のウィンドウで開く。
- サイドバー: Working Copy / History、ブランチ（フォルダ・上流との差 ↑↓・gone）、リモート、タグ、stash、サブモジュール。右クリックで checkout・merge・rebase・作成・名前の変更・削除・stash の操作・サブモジュールの更新と別ウィンドウ。ブランチを別のブランチにドロップすると merge / rebase（fast-forward で済めば checkout しない）、リモートにドロップすると push。
- ツールバー: Fetch・Pull・Push・Stash・Undo。履歴の書き換え・変更の破棄・stash の削除などは Undo で戻せる（VSCode 拡張版と記録を共有する）。
- Working Copy: 差分の行をクリックで選び（hunk の見出しで hunk 全体、Shift で範囲、⌘ で追加）、Stage Lines / Discard Lines。Space・Enter で stage / unstage。Summary と Description を書いて ⌘↩ でコミット。コンフリクトはコンフリクトごとに Use Current / Use Incoming / Use Both。
- History: グラフ付きの一覧。右クリックでコミットの編集（Edit Message・Squash・Fixup・Edit・Split・Drop）、cherry-pick・revert、ブランチ・タグの作成。ドラッグで並べ替え。上部で All Branches / Current Branch / 選んだ ref を切り替える。

## 開発

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
scripts/build-core.sh && (cd apps/macos && swift test)
scripts/build-app.sh snapshot   # dist/ に .app と zip を作る
```

確認用の起動オプション: `AmonHen <repo> --snapshot out.png [--select-first] [--working-copy]`（画面を PNG に書き出して終了）、`--measure-scroll`（History を 600 フレーム自動でスクロールして速さを表示）。

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
