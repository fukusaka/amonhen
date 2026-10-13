# Amon Hen

VSCode 上で快適に Git を操作するための自分用の拡張。設計は [docs/design.md](docs/design.md) を参照。

## インストール・更新

```sh
dir=$(mktemp -d)
gh release download --repo fukusaka/vscode-amonhen --pattern '*.vsix' --dir "$dir"
code --install-extension "$dir"/*.vsix --force
rm -rf "$dir"
```

## 開発

```sh
npm install
npm run build      # dist/extension.js を生成
npm run typecheck
npm test
```

VSCode で F5 を押すと、拡張を読み込んだウィンドウが開く。Output の「Amon Hen」でログレベルを Trace にすると、git コマンドごとの所要時間が出る。
