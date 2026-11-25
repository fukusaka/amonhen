# アイコン

[English](../icons.md)

どの FE も同じアイコンを描く。形は 1 組で、`crates/icons/svg` に SVG として置き、各 FE が自分の描画で描く。このページには、決まりと各形の意味、どう決めたかを残す。

## なぜ

もとは各 FE がそれぞれの土台のアイコンを使っていて、同じ操作が FE ごとに違う形だった。

- macOS のアプリは SF Symbols。
- GTK4 のアプリは、約 75 種を GSK のパスで描いていた。アイコンのテーマにブランチやタグがないことがあるためで、形は macOS のアプリの SF Symbols に寄せていた。
- Windows のアプリは WinUI の `Symbol` のフォント。Git の形がなく、Merge は Shuffle、Rebase は Sort、Stash は Save、Pop は OpenFile、Terminal は Keyboard だった。タグの署名は絵文字。
- VSCode 拡張は codicon と記号（✎ ⌂ ⑂）。
- 端末の FE は文字。

たとえば Pull は、macOS では丸で囲んだ線への矢印、VSCode では `repo-pull`、Windows では Import だった。SF Symbols は Apple のプラットフォームのアプリでしか使えないので、その形を写してほかの FE をそろえることはできない。

2 つの方向を比べた。どの FE も描く共通の独自セットにするか、各土台のセットに意味の対応表を添える（土台にない形だけ自前で描く）か。後者は各土台の見た目になじむが形の差が残り、[frontends.md](frontends.md) の決まりでは、土台が強いない差は △ になる。共通のセットにした。

## 決まり

- 16×16 の格子、太さ 1.5 の線、端も角も丸。塗りではなく線。
- 色は 1 色で、FE が決める（ふつうは文字の色。チェックアウト中のブランチは強調の色、など）。
- 丸はコミット。上の横線はリモート。矢印は動く向き。点線はまだないもの、または前の位置。
- 形は新しく描き、どの土台のセットも写さない。誰でも知っている道具（検索・更新・端末）は、そのほうが早く読めるので見慣れた形のままにする。

![各形の意味](../images/icons/intent.ja.png)

## どう決めたか

1. 最初にツールバーの 19 種を描き、16・20・48 の大きさで明るい背景と暗い背景に並べた。16 では最初の Merge が人の形に見え、Rebase の点線が消えたので描き直した。Pull と Push は矢印を小さくし、間を空けた。
2. それでも意味が伝わらない 5 つ（Merge はよくある図案、Rebase は動いたことしか言えない、Stash と Pop はダウンロードとアップロードに見える、Repositories はケーキやトレーに見える）に、3〜4 個ずつ別の案を描いた。
3. 選んだ案: Merge は C（Branch の裏返しで、Branch と対になる）、Rebase は B（前の根元を点で残す）、Stash と Pop は B（引き出し）、Repositories は B（本棚）。

![描いた案と選んだ案](../images/icons/alternatives.ja.png)

## 作り

`amonhen-icons`（`crates/icons`）が、ビルドのときに `svg/*.svg` を読み、各アイコンを線として持つ。線は SVG のパス（円と四角はパスに直す）と、あれば破線の長さ。SVG の弧を描けない描画のために、同じ線を移動・直線・3 次ベジェ・閉じるの列（弧はベジェで近づける）にしたものも持つ。塗ることしかできない描画（VSCode をアイコンのフォントにするならそのフォント）のために、`Icon::outline(size)` が線を描く大きさの塗る形にする。線の 1 本ずつ（曲線は短い直線に分ける）を幅 1.5 の長方形と両端の円にし、どれも同じ向きに回すので、nonzero で塗ると線を引いたときと同じ形にまとまる。自分で描いたファイルが使う要素と属性（`path`・`circle`・`rect`、`d`・`cx`・`cy`・`r`・`x`・`y`・`width`・`height`・`rx`・`stroke-dasharray`）だけを読み、ほかがあればビルドを止める。FE はパスを `STROKE_WIDTH` の太さ、丸い端と角で引く。長さ 0 の破線と丸い端で点になる。

- GTK4: `gsk::Path::parse` でパスを読み、`apps/gtk4/src/icon.rs` が共通のセットにある種類を線で引く（形ごとに 1 度だけ読む）。GSK がすべての形を読めることをテストで確かめる。
- macOS: C ABI（`amonhen_icon_shape`）で移動・直線・ベジェの列を渡し、AmonhenKit の `SharedIcons` が `CGPath` にし、アプリの `AppIcons` が線を引いてテンプレート画像にする。ツールバーとサイドバーが SF Symbols と同じように色を付ける。今の macOS の `NSImage` は SVG を読めるが、対応する最も古い 14.4 で読めるかは確かめていないので、それには頼らない。ツールバーのサイドバーの切り替えと検索欄は AppKit の部品なのでシステムの見た目のまま、Finder は Finder のフォルダのまま。
- Windows: WinUI のアイコンの部品（`PathIcon`・`FontIcon`）は塗るだけなので、ツールバーとそのカスタマイズのパレットでは、形ごとに XAML の図形の `Path` で線を引く（`Shape::markup(20)`、太さは大きさに合わせ、端と角は丸、破線は太さを 1 とした長さ、色はテーマの文字の色）。windows-reactor に `Path` がなかったので、vendor の写しに足した（`Shape` と `Path` のバインディングは、Windows App SDK の winmd から windows-bindgen で作った）。Files は Segoe Fluent Icons の Explorer のフォルダのまま。
- VSCode（これから）: `package.json` のアイコンは、明るいテーマと暗いテーマの SVG のファイルか、`contributes.icons` で登録するフォント（テーマの色に追従するが、フォントを作る道具が要る。字形には塗る形を使える）。未決。

このページの図は `crates/icons/tools/figures.swift`（macOS で）と、その横のデータから作る。描いた案は `crates/icons/tools/alternatives` に残す。

```bash
cd crates/icons/tools
swift figures.swift intent intent.ja.tsv ../svg ../../../docs/images/icons/intent.ja.png
swift figures.swift alternatives alternatives.ja.tsv alternatives ../../../docs/images/icons/alternatives.ja.png
```

## 進み具合

- [x] 決まりと、ツールバーの 19 種
- [x] GTK4: ツールバー（と、サイドバーの Branch など、ほかの所の同じ種類）
- [x] macOS: ツールバーと、サイドバーのブランチ
- [x] Windows: ツールバー
- [ ] 残りのアイコン（約 55 種: サイドバーの節、状態、署名の印など）
- [ ] VSCode
- 未決: アプリのアイコンも描き直すか、VSCode の形式、端末の FE に何かするか（文字で表している）。
