# リリース

[English](../release.md)

リリースの作り方と、署名・公証・ビルドの由来の証明。

## タグと snapshot

- `v<数字>` のタグ（`v0.2.0` など）を push すると、GitHub Actions がアプリの zip、GTK4 のアプリの tar.gz（Linux x86_64・aarch64）、エンジンだけの tar.gz（macOS arm64・
  Linux x86_64・Linux aarch64）と zip（Windows x86_64・aarch64）を Release に載せる。タグは `Cargo.toml` の
  `workspace.package.version` と一致させ、タグのコミットで CI が通っていて、`CHANGELOG.md` にその版の節があること
  （見出しのすぐ下に短い要約の段落を書く。要約が、CHANGELOG 全体へのリンクと一緒に Release の説明になる）。
- 作るのは `.github/workflows/build.yml`: エンジンは OS と CPU ごとにそのマシンで作り、配る前にその場で一度動かして
  確かめる（`scripts/check-engine.sh`）。GTK4 のアプリは Linux の CPU ごとに Ubuntu 24.04 で作り、SSH 先に送る Linux のエンジンを入れる
  （`scripts/package-gtk4.sh`）。アプリは macOS で並べてコンパイルし（`scripts/compile-app.sh`）、最後に
  組み立てて署名する（`scripts/package-app.sh`）。手元では `scripts/build-app.sh` が同じことをまとめて行う。
- VSCode 拡張は `vscode-v*` のタグで、エンジンを同梱した .vsix を載せる（下の「[VSCode 拡張](#vscode-拡張)」を参照）。
- main の CI（`ci.yml`）が通るたびに、そのコミットで `snapshot` という pre-release を作り直す（アプリ・エンジン・拡張。
  バージョンは `<次の patch>-snapshot.<コミット数>+<sha>`。版を上げてからタグを打つまでは `<版>-snapshot.…` で、snapshot は
  そのあとの正式版より必ず古くなる）。Windows のエンジンは macOS のビルドと並んで作り、最後の
  仕事で同梱して載せる。
- 変更は [CHANGELOG.md](../../CHANGELOG.md)（英語。アプリとエンジン、VSCode 拡張の節に分けた 1 つのファイル）に書く。
  タグを打つ前に、その節の `Unreleased` を版と日付の見出しに改め、その下に要約を書いてコミットする。
- リリースのあとは、Windows の実機で `scripts/check-windows.ps1` を流して確かめる（[windows.md](windows.md#実機でリリースを確かめる)）。

## VSCode 拡張

| チャンネル | 作られるタイミング | バージョン |
| --- | --- | --- |
| stable | `vscode-v*` タグの push（アプリ・エンジンの `v*` とは別） | `0.0.2` |
| snapshot | main の CI が通るたび（アプリ・エンジンと同じ `snapshot` という pre-release に載せる） | `0.0.3-snapshot.<前回リリースからの apps/vscode のコミット数>+<sha>` |

snapshot は次の patch の pre-release 扱いなので、正式版を入れると snapshot より新しいと判定される。

正式版を出す手順:

1. [CHANGELOG.md](../../CHANGELOG.md) の「VSCode extension」の節の `Unreleased` を、出すバージョンと日付の見出しに改め、その下に要約を書いてコミットする
2. `npm version patch --no-git-tag-version` を、1. と同じコミットに入れる
3. main に push し、CI が通ったら `git tag vscode-v<版> && git push origin vscode-v<版>`（リリースは、タグのコミットで
   CI が通っていることを確かめ、CHANGELOG のその節を説明にする）

## ライセンス

アプリ（`Contents/Resources`）、エンジンのどの tar.gz・zip、`.vsix` にも、`LICENSE-MIT`・`LICENSE-APACHE`・`NOTICE` と、
中に組み込んだ依存のライセンスの一覧 `THIRD-PARTY-NOTICES.md` を入れる（`about.toml` で cargo about が作る。musl libc
は `about.hbs` に手で足す）。`deny.toml` で許していないライセンスの依存が入ると、CI が落ちる。

## ビルドの由来の証明

Release のファイルには、このリポジトリのどのコミットから、どの CI で作ったかの証明（artifact attestations）が付く
（v0.1.0 より後のもの）。

```sh
gh attestation verify Amon-Hen-0.2.0.zip --repo fukusaka/amonhen
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
証明書で署名する。Windows・Linux のエンジンは署名していない（下を参照）。

## Windows のエンジンは署名しない

Windows のエンジンは、いまは署名しない。2026 年 10 月に v0.2.5 と vscode-v0.2.3 を Windows 11 Pro で確かめた:

- ブラウザで取った zip には Windows が Mark of the Web を付ける。エクスプローラーの「すべて展開」は `amonhen.exe` に
  引き継ぎ、`Expand-Archive`（Windows PowerShell 5.1 と PowerShell 7）は引き継がない。`Invoke-WebRequest` と
  `curl` は、そもそも付けない。
- SmartScreen（「Windows によって PC が保護されました」）が出るのは、印の付いた `amonhen.exe` をエクスプローラーから
  （ダブルクリックで）起動したときだけ。端末から起動すれば、印があっても警告なしで動く。
- `amonhen.exe install` はエンジンを `%LOCALAPPDATA%\amonhen` に新しいファイルとして書くので、入れたエンジン（CLI と
  共有の daemon）に印は付かない。
- .vsix に同梱したエンジンにも印は付かず（Remote-SSH でも同じ）、拡張は警告なしで起動する。
- Smart App Control（Windows 11）は、どこから来たかによらず署名のない実行ファイルを止める（同梱のエンジンも）。有効
  なのは一部のまっさらなインストールだけで、確かめた機械では切れていた。

署名で何が変わるかと比べた:

- Azure Artifact Signing（もとの Trusted Signing、月 $9.99）は、個人は米国とカナダしか受け付けない。
- 個人（IV・OV）の証明書と CI から使えるクラウド署名（SSL.com eSigner など）は年に約 $300 かかり、証明書の期間は
  いまは最長 460 日。
- 署名しても SmartScreen の警告はすぐには消えない（EV でも消えなくなった）。評判は積み上げることになる。変わるのは、
  Smart App Control がエンジンを止めなくなること。

Smart App Control が利用者のところでエンジンを止めたとき、Artifact Signing が米国・カナダ以外の個人を受け付けたとき、
Windows の FE（ダブルクリックで起動するアプリ）を出すときに見直す。
