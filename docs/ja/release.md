# リリース

[English](../release.md)

リリースの作り方と、署名・公証・ビルドの由来の証明。

## タグと snapshot

- `v<数字>` のタグ（`v0.1.1` など）を push すると、GitHub Actions がアプリの zip と、エンジンだけの tar.gz（macOS arm64・
  Linux x86_64・Linux aarch64）と zip（Windows x86_64・aarch64）を Release に載せる。タグは `Cargo.toml` の
  `workspace.package.version` と一致させる。
- VSCode 拡張は `vscode-v*` のタグで、エンジンを同梱した .vsix を載せる（下の「[VSCode 拡張](#vscode-拡張)」を参照）。
- main の CI（`ci.yml`）が通るたびに、そのコミットで `snapshot` という pre-release を作り直す（アプリ・エンジン・拡張。
  バージョンは `<次の patch>-snapshot.<コミット数>+<sha>`）。Windows のエンジンは macOS のビルドと並んで作り、最後の
  仕事で同梱して載せる。
- 変更は [CHANGELOG.md](../../CHANGELOG.md)（英語。アプリとエンジン、VSCode 拡張の節に分けた 1 つのファイル）に書く。
  タグを打つ前に、その節の `Unreleased` を版と日付の見出しに改めてコミットする。

## VSCode 拡張

| チャンネル | 作られるタイミング | バージョン |
| --- | --- | --- |
| stable | `vscode-v*` タグの push（アプリ・エンジンの `v*` とは別） | `0.0.2` |
| snapshot | main の CI が通るたび（アプリ・エンジンと同じ `snapshot` という pre-release に載せる） | `0.0.3-snapshot.<前回リリースからの apps/vscode のコミット数>+<sha>` |

snapshot は次の patch の pre-release 扱いなので、正式版を入れると snapshot より新しいと判定される。

正式版を出す手順:

1. [CHANGELOG.md](../../CHANGELOG.md) の「VSCode extension」の節の `Unreleased` を、出すバージョンと日付の見出しに改めてコミットする
2. `npm version patch --no-git-tag-version` を、1. と同じコミットに入れる
3. `git tag vscode-v<版> && git push origin main vscode-v<版>`

## ビルドの由来の証明

Release のファイルには、このリポジトリのどのコミットから、どの CI で作ったかの証明（artifact attestations）が付く
（v0.1.0 より後のもの）。

```sh
gh attestation verify Amon-Hen-0.1.1.zip --repo fukusaka/amonhen
```

## 署名と公証（macOS）

環境 `release` の secret に Developer ID Application の証明書と App Store Connect の API キーがあれば、アプリと
macOS のエンジンを Developer ID で署名し、Apple の公証に出す（アプリには結果を綴じ込む）。ブラウザで取っても
そのまま開ける。正式版だけを公証に出し、snapshot は署名だけにする（`scripts/install-app.sh` で入れれば
quarantine の印が付かない）。

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
