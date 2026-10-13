# Amon Hen

VSCode 上で快適に Git を操作するための自分用の拡張。設計は [docs/design.md](docs/design.md) を参照。

## インストール・更新

`gh` でこのリポジトリにアクセスできる必要がある（private リポジトリのため）。

```sh
scripts/install.sh            # 正式版（最新の Release）
scripts/install.sh snapshot   # main の最新
```

リポジトリを clone していない端末では:

```sh
gh api repos/fukusaka/vscode-amonhen/contents/scripts/install.sh --jq .content | base64 -d | sh -s -- snapshot
```

Remote-SSH の接続先に入れるときは、接続した VSCode の統合ターミナルで実行する。

## リリース

| チャンネル | 作られるタイミング | バージョン |
| --- | --- | --- |
| stable | `v*` タグの push | `0.0.2` |
| snapshot | main への push のたび（`snapshot` という pre-release を置き換える） | `0.0.2-snapshot.<前回リリースからのコミット数>+<sha>` |

正式版を出すときは `npm version patch && git push --follow-tags`。snapshot は次の patch の pre-release 扱いなので、正式版を入れると snapshot より新しいと判定される。

## 開発

```sh
npm install
npm run build      # dist/extension.js を生成
npm run typecheck
npm test
```

VSCode で F5 を押すと、拡張を読み込んだウィンドウが開く。Output の「Amon Hen」でログレベルを Trace にすると、git コマンドごとの所要時間が出る。
