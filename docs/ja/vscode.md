# VSCode 拡張の設計メモ

[English](../vscode.md)

## 概要

VSCode 上で快適に Git を操作するための自分用の拡張。Amon Hen のリポジトリの `apps/vscode` で作り（以下のパスはそこからの相対）、Amon Hen のエンジン
（アプリ・CLI と同じもの）を同梱する。Marketplace には公開せず、GitHub Releases で `.vsix` を配布する。

| 要件 | 区分 |
| --- | --- |
| サイドバー（Branches / Remotes / Tags / Stashes / Submodules / Subtrees） | 必須 |
| ブランチのドラッグ＆ドロップで merge / rebase / push | 必須 |
| コミットを柔軟に編集できる（reword / squash / fixup / drop / reorder / split） | 必須 |
| 差分ビューから hunk・行単位で細かくコミットを作れる | 必須 |
| コンフリクト解決 | 必須 |
| 充実した差分表示 | 希望 |
| Remote-SSH 越しで動く | 希望 |
| 大きなリポジトリでも速い | 希望 |

VSCode の UI 部品は自由度が低いため、専用 Git クライアントの画面を丸ごと作り込むのではなく、操作の核を VSCode の部品に割り当てて作る。今はナビをサイドバーの TreeView に、中身をエディタ領域の Webview（メインビュー）に置き、コンフリクトの解決は標準の 3-way マージエディタを使う（「UI を 1 つの Webview にまとめる」を参照）。

## 全体構成

`extensionKind: ["workspace"]` にすると、Remote-SSH 接続時に拡張ホストが SSH 先で動き、git とエンジンもそこで動く。
over SSH 対応はこれでほぼ済む。

```
ローカル（VSCode の UI）              リモート（拡張ホスト、Remote-SSH の接続先）
┌──────────────────────┐            ┌──────────────────────────────────────────────┐
│ TreeView（ナビ）      │            │ UI 層: Provider 群・Webview との通信          │
│ Webview（メインビュー）│──postMessage──▶      ▼                                   │
│ 3-way マージエディタ   │  (SSH 経由)  │ 状態管理層 ◀── vscode.git API（変更通知）     │
└──────────────────────┘            │        ▼                         ▼           │
                                    │ src/daemon/（既定）       git 実行層（代わり） │
                                    │ FE 向けの窓口のクライアント                    │
                                    │        ▼                         ▼           │
                                    │ 共有の daemon             git CLI             │
                                    │ (amonhen --serve)                            │
                                    │        ▼                         ▼           │
                                    │ リポジトリ                                   │
                                    └──────────────────────────────────────────────┘
```

Webview はローカルで描画され、拡張ホストとの通信だけが SSH を経由する。大きな差分やログは分割して送る。

## エンジン（共有の daemon）

既定の経路。`src/daemon/` が、
ホスト向けに同梱したエンジン（`bin/amonhen-<OS>-<CPU>`）を探して `amonhen install` で置き、共有の daemon
（`amonhen --serve`。Unix ドメインソケット、Windows は名前付きパイプ）と FE 向けの窓口で話す。命令と答えは JSON、
大きなデータ（History の行・グラフ・差分）はレコード。取り決めは[front.md](front.md)、
設計は[design.md](design.md) の 18。

- History・差分・Working Copy・書き込みは daemon を通す。daemon は History・検索の索引・SSH のつながりを持ち続け、
  アプリ・CLI と分け合う。
- History の行の数が分かっているので、Webview は画面に入る行だけを頼み（`historyRows` で 200 行ずつ）、離れた一致へも
  手前を読まずに移れる。History の検索はコアのもの（`historySearch`、アプリと同じ書き方）で、この経路だけにある。
  `contents:` の検索は `historySearchPartial` を 300 ms ごとに読んで分かった一致から色を付け、partial clone では
  `historyContentsToFetch` の数を出して先に確かめる（上限を超えれば絞るよう頼む）。Matching Only はコアの一致だけの
  History（`historyMatching`）に替え、選んでいるコミットは選んだままにする。
- Undo の記録は、コアと同じファイル・同じ形（`<git dir>/amonhen/undo.json`）。コアが記録する操作を拡張がもう一度
  記録しないよう、`delegation.ts` で分ける。どちらもファイルをまるごと置き換えて書き、読めない・壊れているときの
  扱いも同じ（`docs/ja/ux.md`）。
- 合うエンジンがないとき、`amonhen.daemon.enabled` を切ったときは、拡張の git 実行層で動く。下の git の起動・`cat-file`・
  rebase の組み立て・行単位の stage の節は、この経路の話。エンジンは同じことを Rust で行う。
- 人はどちらの経路でも mailmap の名前で出す。エンジンはコアで置き換える（詳細には記録された名前とメールも付き、違えば
  名前の下に出す）。git 実行層は `log.mailmap` を読み、History の行は `%aN`・`%aE` を使い（`%aN` は `log.mailmap` が
  false でも置き換える）、詳細は `git check-mailmap` で置き換える。`.mailmap`・設定ファイル・`mailmap.file` の指す
  ファイルが変わったら（読み直すたびに更新時刻を見る。`mailmap.file` の場所は設定ファイルが変わったときだけ git に聞く）、
  History と覚えている詳細を読み直す。

- Range Diff（ローカルブランチの Compare with Previous Version、パレットの Range Diff、History で選んだ 2 つのコミット、
  Reflog の行の Compare with Current HEAD）は、アプリと同じくメインビューのページに出す。上に比べる範囲（直せる）、
  左に組の一覧（= ! < >、Changes Only）、右に変わった組の差分の差分か、ほかの組のコミット（詳細の欄）。開いている
  あいだはサイドバーに Range Diff の行を出す。エンジンではコアの `previousVersion`・`rangeDiff` を使い、git 実行層
  ではブランチの reflog（`git log -g`）を読んで `git range-diff` を自分で動かす。
- エンジンのあとに足した機能は、エンジンだけで作る（`run` でコアの命令を使う）。エンジンがなければ、その行とコマンドは
  出さない（コンテキストキー `amonhen.engine`）。
- エンジンは絵を描かない。すべてのファイルの Mermaid・DOT の図・数式・Android の VectorDrawable の絵と SVG のモバイル
  向けの確認は、絵の手伝い `amonhen-render` が作る。エンジンの隣に同梱し（`bin/amonhen-render-<os>-<cpu>`。
  `amonhen.daemon.path` でエンジンを指していればその隣）、見せるファイルごとに起動する（`src/daemon/render.ts`。
  取り決めは [front.md](front.md#絵の手伝いamonhen-render)）。エンジンの `markdownBlocks` は Markdown を読むだけで、
  拡張はその図のフェンスと式を Webview のテーマの色で手伝いに送り、手伝いが求める同じコミットの DOT の画像や
  VectorDrawable の資源に答える。手伝いがない・失敗したときは、コードとテキストのまま。図の配色・gantt の時間帯・
  数式の大きさと時間の上限は設定 `amonhen.diagrams.*`・`amonhen.math.*`（アプリの Settings > Theme の Diagrams・Math と
  同じ選択肢と既定。読み込んだ Mermaid のテーマはファイルのパス、OS の時間帯は Webview のもの）で、変えると開いている
  ファイルを描き直す。SVG のモバイル向けの確認は、元の絵と選んだ部品での見え方（Android と flutter_svg は本物の取り込み
  を移したもの、iOS と react-native-svg は近似）を画像の差分と同じ比較の欄で比べる。
- サイドバーのセクションの下の Reflog は、メインビューのページを開く。左に HEAD の reflog（コアの `reflog`）、右に選んだ行の
  コミットを History と同じ詳細の欄に出す。どの ref からも辿れない行には「not on any branch」と付ける。ボタンと右クリック
  から、そこにブランチ・タグを作る、checkout する（detached）、そこへ reset する、History で見せる（今の範囲になければ
  All Branches にする。`historyRowOf`）、ID・名前をコピーする。HEAD が動いたら読み直す。
- Show Daemon（サイドバーの ⋯ とコマンドパレット）は、アプリの Activity の Daemon と同じメインビューのページを開く。
  共有の daemon が動いているか（pid・版・動いている時間・つながりの数・ソケット）と Start・Stop、daemon が開いている
  リポジトリ（SSH の送受信量・つながり・History・監視・検索の索引）、この窓の daemon とのつながり。出している間は
  `amonhen daemon status` を 2 秒ごとに読む。Stop で窓のリポジトリのつながりは切れ、Start でつなぎ直す（アプリの Reconnect と同じ）。
- Reset Current Branch to Here…（History のコミットのメニューと Reflog）は、Mixed・Soft・Hard をモーダルで選んでもらい、
  コアの `reset` を使う。コアは hard の前に作業中の変更を stash に残し、Undo を記録する。Checkout (Detached) はどちらの
  経路でも動く。

## 必須要素の実装方針

### サイドバー

- 独自の View Container に TreeView を置く。項目は Working Copy（worktree が 2 つ以上あれば、その下に worktree）/ History /
  Conflicts（コンフリクトがある間）/ 途中の merge・rebase / Branches / Remotes / Tags / Stashes（stash があるとき）/
  Submodules / Subtrees（エンジンが `git subtree` を見つけたとき）、続いて Range Diff（開いている間）、Reflog と
  Repository Settings（エンジンがあるとき）。
- 状態変化の検知は `vscode.git` の `onDidChange` に任せ、自前の file watcher は持たない。

### ドラッグ＆ドロップによる merge / rebase / push

- `TreeDragAndDropController` で実装する。ブランチをブランチにドロップしたら「Merge A into B」「Rebase B onto A」を QuickPick で選ばせる。リモートにドロップしたら push。
- ドロップ先が現在のブランチでなければ checkout してから実行する。fast-forward で済む場合は checkout せず `update-ref` だけで処理する。
- 制約: TreeView と Webview の間ではドラッグ＆ドロップできない。History 内の並べ替えは Webview の中で完結させる。
- コミットグラフが変わる操作の確認（Push・Force Push・Pull・Merge・Rebase・Reset・Cherry-pick・Revert・Fast-Forward、ほかのブランチにないコミットのあるブランチの削除、Undo、パッチをコミットにする取り込み、History の編集の Apply Changes）は、メインビューが開いていれば、その Webview の上に重ねたシートで、アプリと同じ前と後の絵を添えて出す（docs/design.md の「コミットグラフの変化を見せる」）。絵はコアの `movePreview`・`deletePreview`・`mergePreview`・`pickPreview`・`octopusPreview`・`rewritePreview` から拡張が組み（`src/history/drawing.ts`）、Webview は描くだけ。シートにはアプリのシートと同じものも載せる: 取り込み方・Reset の動かし方は選ぶもの（選ぶと絵が替わる）、送る・取り込むコミットの一覧（押すとシートを閉じずに差分を開く）。ネイティブのモーダルには画像を出せず、VSCode のモーダルエディタは拡張から開けないので、メインビュー（またはエンジン）がなければこれまでどおりモーダルか QuickPick で確かめ、Fast-Forward と Apply Changes は確かめない。
- rebase（ドロップ、またはブランチの右クリックから）と History の編集は、コミットを書き換えるので、いつも先に確かめる（docs/design.md の「書き換える操作と、積み重ねたブランチ」）。共有の daemon では、確認にコアの `rewritePreview` を出す: 一緒に動く・残る積み重ねたブランチとその理由、あとの push に Force Push が要る push 済みのブランチ、コンフリクトの見込み（git 2.40 以降）。メインビューが開いていれば、上と同じくその上に重ねたシートで前と後の絵を添えて出し（`src/ui/rewriteConfirm.ts`）、なければ modal の詳しい欄に出す。積み重ねたブランチに関わり、git が 2.38 以降なら、2 つ目のボタンでその操作だけ `rebase.updateRefs` と逆に動かす・動かさないを選べる（コアの `rebase`・`rewrite` の `updateRefs`）。見通しは今のブランチについてだけで、ほかのブランチの rebase は、代わりに merge したときのコンフリクトの見込み（`predictMerge`。git 2.38 以降、どちらの経路でも）を添えて modal で確かめる。git 実行層では、今のブランチの rebase は見通しなしで確かめる。
- コマンドパレットの Merge Branches… で、いくつものブランチ・リモートブランチ・タグを `git merge a b c` と同じく 1 つのコミットで今のブランチに merge する（octopus）。1 つ目の QuickPick で一覧の順に選び、2 つ目の QuickPick に、前のものまでの結果に重ねて調べたそれぞれのコンフリクトの見込み（`predictMerge`。git 2.38 以降）を出して、1 つのコミットにまとめる・最後より前でぶつかるものを最後に回す・1 つずつ merge する、から選ばせる。最後より前のものがコンフリクトすると git はコンフリクトを残さずに止まるので、git が断ったら改めて 1 つずつを勧める。1 つずつはコンフリクトで止まり、残りを知らせる。どちらも Undo 1 回で全部戻る。共有の daemon ではコアが記録し（配列を渡す `merge`、`oneByOne`）、git 実行層では拡張が記録する。git 2.37 より前が途中まで合わせた変更を残したときは、`reset --merge` で戻す。

### コミットの柔軟な編集

- 共有の daemon では、History の編集はコアの `rewrite` で行う。積み重ねたブランチを動かすときは git の `update-ref` の行を残し、Undo もコアが記録する。この節の残りは git 実行層のこと。
- reword / squash / fixup / drop / reorder / edit / split はすべて非対話の rebase で実現する。todo はこちらで丸ごと作り（`src/history/rebasePlan.ts`）、`GIT_SEQUENCE_EDITOR="cp <todo>"` で git の todo に上書きする。
  - メッセージの書き換えは git の `reword`・`squash` そのもので行い、git が開くエディタに `GIT_EDITOR="cp <file>"` でメッセージを書かせる。todo に入るのは 1 つだけで、git がエディタを開くのも 1 回（`squash` では最後の 1 つ）なので、どのコミットのメッセージかをエディタの側で判別しなくて済む。notes は git が設定のとおりに写し（`notes.rewriteRef`・`notes.rewrite.*`）、フックも git と同じく走る（`reword` では pre-commit と commit-msg）。`commit.cleanup` を設定していなければ `commit.cleanup=whitespace` で rebase し、`git commit -F` と同じく `#` で始まる行を残す。
  - squash は親の `pick` ＋ `squash`、fixup は `fixup` だけ、edit は git の `edit`（Continue のとき、stage した変更はそのコミットに amend され、git がそのコミットの notes を最後のコミットへ写す。`notes.rewriteRef`）。
  - 編集できるのは HEAD から first-parent で辿り、最初の merge コミットの手前まで。merge をまたぐ rebase は扱わない。
  - `--autostash` で作業中の変更を退避し、`--no-autosquash` でユーザーの設定に todo を並べ替えさせない。
- split は git の手順と同じく `edit` で止め、`git reset HEAD^` で変更を作業ツリーに戻す。Working Copy で分割してコミット → Continue の流れ。`edit` から続けるので、git がコミットの notes を新しいコミットの最後に写す（`notes.rewriteRef`）。
- merge や rebase が止まっている間は、Working Copy の先頭に Continue / Abort を出す。edit や split で止まった操作の Undo の記録は保留しておき（`undo-pending.json`）、終わった時点で確定する。
- 操作前に `HEAD` と関係する ref を記録し、1 操作単位で Undo できるようにする。
  - 記録は操作の前後の両方を取り、`<gitDir>/amonhen/undo.json` に最大 50 件残す。Undo するときは今の状態が「操作後」と一致することを確かめ、違えば何もしない。
  - checkout 中のブランチは `reset`（commit は `--soft`、merge や rebase は `--keep`）で戻し、そうでないブランチは `update-ref` に旧値を条件として渡して戻す。操作の中でブランチを切り替えていたら、元のブランチに戻る。
  - コンフリクトで止まった操作は記録しない。戻すのは abort の役目。push は Undo の対象にしない。

### 差分ビューから細かくコミットを作る

- stage はメインビューの Working Copy で行う。差分の行を選び（Shift+クリックで範囲）、または hunk・ファイルのボタンを使う。
  標準の diff エディタは読むためだけに使う。
- エンジンがあれば、選んだ行の stage・unstage・破棄は daemon が行う（`stageRows`・`discardRows`）。
- git 実行層では、index と working tree の行差分を自前で計算し（Myers 法、`src/diff/lineDiff.ts`）、選択範囲の変更だけを
  当てた新しい index の内容を作って `hash-object -w` → `update-index --cacheinfo` で書き込む。unstage は HEAD との差分で
  同じことをする。部分パッチを `git apply` に渡す方式より、文脈行や改行の扱いでつまずかない。

### パッチ

- History のコミットのメニューに Save as Patch… と Save Patches from Here to HEAD…（Webview は 1 つずつしか選べない
  ので、ひと続きはそのコミットから HEAD まで first-parent で辿ったもの）。クイックピックで、1 コミット 1 ファイル・
  送り状つきの 1 コミット 1 ファイル・1 つの mbox から選ぶ。
- Apply Patch…（コマンドパレットと、エクスプローラーの .patch・.diff・.mbox・.eml のメニュー）は、パッチの中身と
  当たるかをモーダルのダイアログで見せる。最初のボタンは git の既定のまま（メールの形は `am.threeWay`、diff は作業
  ツリー）で、ほかは 3-way merge と、diff なら Apply and Stage。
- `git am` の途中は、ナビに「Applying patches (1/3)」と止まった理由、Skip、Retry with 3-Way Merge を出す。共有の
  daemon があればコアの命令を使い、git 実行層では拡張がパッチを読んで（`src/git/patch.ts`。読み方はコアと同じ）
  git を動かす。

### コンフリクト解決

- 標準の 3-way マージエディタを流用する。拡張側は、コンフリクト中のファイル一覧、ファイル単位の ours / theirs 採用（`checkout --ours/--theirs`）、merge / rebase 中の表示と continue / abort / skip ボタンを担当する。
- マージエディタを開く公開 API は無いので、組み込みの Git 拡張も使っている内部コマンド `_open.mergeEditor` に、base・input1・input2（index の stage 1・2・3 を `amonhen:` で返す）と output（実ファイル）を渡して開く。失敗したら、または片側で削除されたコンフリクトなら、ファイルを開いて標準 UI の「Resolve in Merge Editor」と Working Copy の Take Current File・Take Incoming File に任せる。
- 解決済みにするとき、コンフリクトのマーカーが残っていれば確認する。rebase 中は進み具合（`rebase-merge/msgnum`・`end`）を表示し、Skip もできるようにする。

## UI を 1 つの Webview にまとめる

ステップ 1〜5 では、Working Copy をサイドバーの TreeView と標準 diff エディタで、History をエディタ領域の Webview で作った。表現がまったく違う部品に分かれているため、Working Copy と History を同じ画面の中で切り替える操作感にならなかった。そこで UI 層を作り直し、エディタ領域の 1 つの Webview（メインビュー）にまとめる。git 実行層・状態管理層・Undo・rebase の仕組みはそのまま使う。

- メインビューは左にナビ（Working Copy / History / Branches / Remotes / Tags / Stashes）、右に内容を出す。切り替えは Webview の中だけで行い、拡張ホストとの往復を待たない。
- 差分は Webview の中に描き、単語単位の差分の強調と、hunk・行の選択による stage / unstage を持たせる。エンジンがあれば、差分と Blame にコアの tree-sitter による構文の色を付ける（`diffSyntax`・`textSyntax`。差分を出したあとに頼む）。git 実行層では付かない。差分の計算と index への書き込みは拡張ホスト側（`src/diff/lineDiff.ts`）で行う。
- コミット欄は複数行のメッセージと amend を扱う。
- History の詳細にも同じ差分の部品を使い、変更ファイルの差分をスクロールで続けて見られるようにする。
- ブランチのドラッグ＆ドロップ（merge / rebase / push）はナビの中で行う。
- コンフリクトの解決は、引き続き標準の 3-way マージエディタを開く。
- Remote-SSH では postMessage が SSH を通るので、差分はファイル単位で求められたときに送り、大きなファイルは分割する。
- その後、ナビ（Working Copy / History の入口、途中の merge / rebase、ブランチ類）はサイドバーの TreeView に移した（ステップ 11）。メインビューは中身だけを広く使い、サイドバーとの役割を「選ぶ」と「見る・操作する」に分ける。サイドバーは TreeView なので、テーマや操作がほかのサイドバーと揃い、ドラッグ＆ドロップと右クリックのメニューも標準の仕組みで扱える。
- stage の手段はメインビューに一本化し、標準の diff エディタから行単位で stage する経路はなくした。アクティビティバーのアイコンでサイドバーが表示されたら、メインビューも開く。

## 速度対策

git 実行層では、小さな git コマンドの所要時間は、ほぼプロセス起動のコスト（macOS で 1 回 約 6 ms）。常駐 `cat-file` への問い合わせは 約 0.08 ms。言語より「spawn の回数」と「更新範囲」が効くので、以下を徹底する。

- `git cat-file --batch` を常駐させ、オブジェクト取得のたびに spawn しない（`--batch-command` は git 2.36 からで、サポートする最低の版より新しい）。cat-file は index を最初の 1 回しか読まないので、`:path` を問い合わせる前に index の更新時刻を確かめ、変わっていれば起動し直す。
- ref・config・stash の変化は `.git` 内のファイル（`HEAD`、`refs/` 以下のディレクトリ、`packed-refs`、`reftable/tables.list`、`config`、`logs/refs/stash`）の更新時刻で検知し、spawn しない。git は ref を lock ファイルの rename で更新するので、ディレクトリの更新時刻で足りる。
- 変化があった部分だけ取り直す。ref は ahead/behind も含めて `for-each-ref` 1 回で取る。worktree の変化は検知できないので status は毎回取る。独立したコマンドは並列で実行する。
- Tags / Submodules は展開されるまで取得しない。Stashes は stash があるときだけ出すので、stash の一覧は先に読む（`git stash list` は軽い）。TreeView は前回との差分を取り、変わったノードだけ更新する。
- stage / unstage などは先に UI へ反映し、git の結果で確定させる（楽観的更新）。
- esbuild で 1 ファイルに bundle し、実行時の依存パッケージは持たない。Webview は React を使わず、仮想スクロールとグラフの canvas 描画で軽くする。
- git コマンドと再描画の所要時間を Output に記録し、「通常のリポジトリでサイドバーの更新は 30 ms 以内」を目安に計測しながら作る。
- git 実行層はインターフェースで切り離してあり、エンジンがあれば Rust（gix）のエンジンが代わる（「エンジン（共有の
  daemon）」を参照）。History と差分は、git を起動せずに daemon からレコードで受け取る。
- status は `git status --porcelain=v2 -z` で取得する。大きなリポジトリでは `core.untrackedCache` と fsmonitor の有効化を案内する。
- 読み取り系コマンドには `--no-optional-locks` を付け、ユーザー自身の git 操作とロックを奪い合わない。
- log は 200 件程度ずつ取得する。グラフのレーン計算は拡張ホスト側で少しずつ進め、Webview には最小限の配列だけ送る。Webview 側は仮想スクロールで描画する。
- グラフのレーンは Webview がアプリと同じ決め方（`crates/core/src/lanes.rs`）で並べる。見えている範囲に点があるレーンは広く、線だけのレーンは細い束に詰め、列は 120px までに収めるので、文字が一覧の外に押し出されない。
- `git commit-graph write --reachable` の実行を案内すると、グラフ表示と ancestry 判定が速くなる。
- Remote-SSH では postMessage が SSH を経由するため、大きな差分やログは分割して送る。

## 配布

- 正式版: ルートの `.github/workflows/vscode-release.yml`。`vscode-v*` タグ（アプリ・エンジンは `v*`）の push で、
  エンジン（Windows も）を作り、macOS のものは Developer ID で署名して公証に出し、`scripts/bundle-engine.sh` で同梱して `vsce package`、ビルドの由来の証明を付けて
  Release を作る。
- snapshot: ルートの `.github/workflows/ci.yml` の snapshot のジョブ。main の先頭の CI が通るたびに、`scripts/snapshot-version.mjs` が
  `<次の patch>-snapshot.<前回リリースからの apps/vscode のコミット数>+<sha>` を作り、エンジンを同梱した `.vsix` を
  アプリ・エンジンと同じ `snapshot` という pre-release に載せる。SemVer の比較では正式版より古いので、正式版を入れると
  上書きされる。
- 各マシンへの導入・更新: `.vsix` を取ってきて `code --install-extension`、またはルートの `scripts/install-vscode.sh [stable|snapshot]`
  （Releases の API から `curl` で `.vsix` を探して取る → `code --install-extension --force`。`curl … | sh` でも動く）。

## 実装順序

毎日使う Working Copy と差分ビューからの stage を早めに作り、手応えを確かめる。

- [x] 1. git 実行レイヤーと状態の管理、サイドバーの TreeView
- [x] 2. Working Copy と diff の表示、hunk・行単位の stage
- [x] 3. ドラッグ＆ドロップによる merge / rebase / push と Undo の基盤
- [x] 4. History の Webview とコミット編集（rebase 操作）
- [x] 5. コンフリクト解決の UI

UI を 1 つの Webview にまとめる（「UI を 1 つの Webview にまとめる」を参照）:

- [x] 6. メインビューの土台（左ナビと右の領域）。History をその中に移す
- [x] 7. Working Copy: ファイル一覧、差分、ファイル・hunk・行単位の stage、複数行のコミット欄
- [x] 8. History の詳細に差分を表示する
- [x] 9. ナビのブランチ類とドラッグ＆ドロップ、ツールバー（Undo・Fetch など）
- [x] 10. サイドバーの TreeView の整理とドキュメント
- [x] 11. ナビをサイドバーの TreeView に移し、メインビューは中身だけにする

エンジンを使う:

- [x] 12. History・差分・Working Copy・書き込みを共有の daemon で行う。
  [design.md](design.md) の 18 で進める

macOS アプリに追いつく（1 つの機能を 1 つの pull request で、エンジンを使って。まだ足りないものは
[frontends.md](frontends.md) にある）。GitHub・AI・Undo の記録のほかの Activity・機械全体の git の設定・日本語化は、
今は入れない。VSCode にそのための拡張と設定がある。

- [x] 13. Reflog、コミットへの Reset、コミットの Checkout (Detached)
- [x] 14. タグとリモートのメニュー、タグのリモートの印と署名の印
- [x] 15. Worktrees
- [x] 16. File History
- [x] 17. Blame
- [x] 18. アプリと同じブランチの並べ方、お気に入り、アーカイブ、複数選択、絞り込み
- [x] 19. History の署名の印、Go to HEAD・Parent・Child
- [x] 20. shallow clone の取り足し、partial clone の中身をすべて取る
- [x] 21. Stashes の残り（Branch…・`--index`・行の詳しい情報・Pop の前の確認）
- [x] 22. Undo の前の確認と、Undo の記録の一覧
- [x] 23. リポジトリの設定
- [x] 24. submodule: 印・Update…・URL の編集・追うブランチ・外す・Update All・Sync URLs・Add Submodule…
- [x] 25. subtree: Add・Pull・Push・Split
- [x] 26. ファイルをコミットの版・その前の版に戻す
- [x] 27. stage した変更だけ・stage した変更を残して・1 つのファイルだけ stash する
- [x] 28. worktree のロックと解除・移動・移したフォルダを探す
- [x] 29. submodule のデータを .git/modules に移す・それぞれの submodule で命令を動かす
- [x] 30. rerere の解決を忘れる・すべて忘れる
- [x] 31. 履歴のない新しいブランチ（空から・ファイルを残して）
- [x] 32. ignore されたファイルを一覧してから消す
- [x] 33. Working Copy: パスでの絞り込み、パス・木・ファイル名の見せ方
- [x] 34. コミット欄: 件名の長さ、下書きとテンプレート、過去のメッセージ、型とトレーラー
- [x] 35. Commit and Push、Fixup・Squash Commit For、hooks の出力
- [x] 36. Working Copy: ファイルごとの行数、⌫、J/K、.gitignore に足す、sparse checkout の知らせ
- [x] 37. History: いくつもの ref、worktree の札、順、行の項目、merge を畳む、祖先を目立たせる
- [x] 38. History の詳細: ⌘F で探す、ファイルを畳む・Viewed、変更したファイルの木、1 ファイルずつ
- [x] 39. History の編集モード: いくつものコミットを 1 回の rebase で、近道から開く、マージを保つ（`--rebase-merges`）
- [x] 40. Export Archive、Save as Bundle、Fetch from Bundle、Clone from Bundle
- [x] 41. Switch Branch: 最近のブランチを上に、リモートのブランチは追跡するブランチを作って、打った名前は新しいブランチで
- [x] 42. Fetch の前に確かめる: すべてのリモートか 1 つか、どれが prune するか
- [x] 43. Pull の前に確かめる: コミット、取り込み方（git の設定・merge・rebase・fast-forward のみ）、なければ上流
- [x] 44. Push の前に確かめる: 注意、コミット、上流が進んでいれば Pull… か見せたものの上だけの Force Push
- [x] 45. Rebase の確認に、今と後のコミットをブランチの札と一緒に出す
- [x] 46. Rebuild History
- [x] 47. 拡張自身の fetch・push も、120 秒何も出さなければ止める
- [x] 48. Working Copy: HEAD → 作業ツリーの 1 つの差分、変わった行と hunk ごとのチェック
- [x] 49. Working Copy: コミットの欄を一覧の上に
- [x] 50. Save as Patch で版（-v）を尋ねる
- [x] 51. 一部のフォルダだけのとき、Working Copy の行に出す
- [x] 52. Checkout・Merge・Cherry-pick・Revert・Apply・Pop の前に確かめる
- [x] 53. History と Working Copy の差分・Blame の構文の色（エンジンの tree-sitter）
- [x] 54. コンフリクトがあるときのサイドバーの Workspace の Conflicts
- [x] 55. 失敗は原因を出し、Details で全文を
- [x] 56. History: 2 行の行（One Line per Commit）と、グラフの線ごとの祖先の強調
- [x] 57. History の詳細: メッセージをファイルの上の別の欄に、ドラッグできる境と ⤢ で広げる
- [x] 58. Range Diff をメインビューのページに（組の一覧・差分の差分・直せる範囲）。History と Reflog からも開く
- [x] 59. Push のシートから、送る変更すべてやコミットひとつの差分を開く
- [x] 60. Keys and Values の列を Combined / Levels で切り替える
- [x] 61. 右クリックのメニューの名前をアプリに合わせる
- [x] 62. パスワード・パスフレーズを VSCode の入力欄で尋ねる（askpass）
- [x] 63. Run in Each Submodule… の命令の出力を出す
- [x] 64. ほかのブランチの rebase でコンフリクトの見込みを出す
- [x] 65. status で git の fsmonitor を使う（`amonhen.fsmonitor`）
- [x] 66. commit-graph を自動で書く（`amonhen.writeCommitGraph`）
- [x] 67. Merge の取り込み方を選ぶ（fast-forward だけ・いつも merge コミット・squash）
- [x] 68. Daemon のページ（様子・Start と Stop・つながり）。アプリの Activity の Daemon と同じ
- [x] 69. History の詳細をアプリと同じ形に: ファイルの見出しが変更ファイルを兼ねる（最初は畳む、⋯ でメニュー）、リストか、フォルダを畳める木、切り替えとすべて開く・畳むは帯 1 本。Viewed と 1 ファイルずつはなくした
- [x] 70. History の詳細: ファイルの前後の行を広げる（hunk の見出しの ⇕）・ファイル全体を出す、空白を無視する（エンジンがあれば）
- [x] 71. History の詳細で画像と SVG を比べる: 大きさを添えて並べる・スワイプ・オニオンスキン・違うピクセル
- [x] 72. History の詳細の「すべてのファイル」（その時点の木と、コミットの行に印を付けた中身。拡大縮小できる画像、SVG の絵とモバイルの確認、整形した Markdown、そのほかのバイナリはエディタで）。エンジンがあれば
- [x] 73. 長い行の折り返し（`amonhen.wrapLongLines`、Alt+Z、差分の上の ↩︎）、コミット情報の高さを保つ（仕切りのダブルクリックで合わせ直す）、Markdown の GitHub 独自の絵文字を薄い札に
- [x] 74. Working Copy の差分で空白を無視する（エンジンがあれば。その間は行・hunk を stage・破棄しない）
- [x] 75. アプリのメニューの Commit・Commit and Push・Stage All・Set Upstream…・Rebase…・Go to HEAD・Find in History… をコマンドに。Merge Branches… と Rebase… で History で選んでいるコミットも選べる
- [x] 76. コミット情報の右クリック（ID・短い ID・件名・メッセージ・git show のコピー、Edit Message…）と、名前を押してその人で History を探す
- [x] 77. コミットのメッセージの URL とコミット ID をリンクに、トレーラーを分けて並べ、git notes（エンジンがあれば）、History の検索の語に印
- [x] 78. コミット情報の Tag・Merged Tag の行（タガー・件名・署名）、merge で取り込んだコミットの数、署名の形式と指紋
- [x] 79. merge を 1 つずつ畳む・開く（右クリック・←/→）、すべて畳む・すべて開く。アプリ・TUI と同じファイルにリポジトリごとに覚える
- [x] 80. History と編集モードで、コミットを別のコミットに重ねて落とすとそれにまとめる（その上へ動かして squash にした編集モードを開く）
- [x] 81. Working Copy でいくつものファイルを選ぶ（⌘・Ctrl クリック、Shift クリック、Shift+↑↓）。Space・⌫・行のボタン・Stash… は選んだものすべてに効く
- [x] 82. 途中の操作の行で、何をどこへしている途中か（何手目か）と、なぜ止まり次に何をするかを言う（エンジンがあれば。アプリの帯と同じ）。revert には Skip を出さない
- [x] 83. 見分けられる失敗を分かる言葉で知らせ、アプリと同じ次の一手（Pull…・Open Working Copy・Set Upstream…・Open Worktree・Open in Terminal・Try Again）と、Details に助言を
- [x] 84. 詳細の帯の表示の設定: 表示項目（簡潔・標準・詳細）、高さを自動で決める、コミット情報だけを表示、長い行を折り返す（アプリと同じ）
- [x] 85. 編集モードで p・r・s・f・d・e で行のすることを決め、行の下の線を引いて古いコミットを取り込む・手を付けていない古いものを外す
- [x] 86. History をアプリと同じに: 最初はすべてのブランチ、ブランチの History は上流も、「N branches」、検索の ▾、「3 / 10」の件数、祖先を最初から目立たせる、中身を取りすぎる検索から Fetching and Folders へ
- [x] 87. History のメニューをアプリと同じに: Copy Branch or Tag Name…・Copy Short IDs、Squash into Previous… と Interactive Rebase from Here…、どのコミットにも Cherry-pick、Split と複数選択の Merge・Save Patches は使えるときだけ、Go to Parent / Child のキー、編集モードの「drop — Remove」、パッチの形とカバーレターの組み合わせ、Reflog の worktree
- [x] 88. History の札をアプリと同じに（3 つまで・+N・真ん中を切る・押すと名前の一覧）、一覧の横のコミットの頻度とブランチの動きの帯
- [x] 89. 画像の比べ方をアプリと同じに: 2-Up、拡大縮小（−・倍率・+・合わせる・⌘ スクロール）、違うピクセルの割合、SVG は絵で出して Show as Text
- [x] 90. すべてのファイルをアプリと同じに: リストか木、変えたフォルダを開く、消したファイルは前の版、「@ コミット」、Show This Change、どのファイルも Open in Editor、見方を覚える、モバイルの確認の部品の切り替え、Markdown の相対リンク
- [x] 91. 詳細の小さな差: ファイルの見出しの Expand・Collapse、Open File at This Commit、Show Changes Only、選んだときのファイルのメニュー、取っていない親、帯の「All Folders」「All Diffs」、空のときの文言、表示のメニューの Monospaced Commit Message と Changed Files as Tree
- [x] 92. Working Copy の小さな差: ファイルの並べ方の List と「File Name」、ファイルのメニューの項目と並び、untracked のファイルの「Delete …」、行の破棄は確かめない、Description だけのコミット、「Amend Commit」、差分を読めないわけは差分の場所に、Take Current・Incoming File は失うものがあるときだけ確かめる
- [x] 93. Stashes・Restore・File History・進行中の操作の小さな差: Stash の選び方を組み合わせる、stash の行のいつ・元の ID・Copy Name、File History の Restore This Version… と Copy Short ID、Skip・Abort は尋ねない（revert に Skip なし）
- [x] 94. サイドバーの小さな差: Copy Branch / Tag Name と Copy Names、いくつものタグの Push・Replace・削除、タグのフォルダ、中身のある節だけをアプリと同じ組み方で（Remotes はリモートのブランチから）、絞り込みは平らに、ほかで checkout しているブランチの Open Worktree、ブランチの印、先に聞く Prune、リモートの Edit Push URL… と Check Connection、取り出してある submodule だけの Open in New Window、ドラッグでの push の確認とアプリと同じ upstream の設定
- [x] 95. 同期とブランチの操作の小さな差: メインビューなしでも Fast-Forward を尋ねる、いくつも消すときは未マージのコミットを添えて 1 度だけ尋ねる、上流のないブランチの Force Push は Publish、今のブランチの Rename Branch…、octopus merge の Change the Order…
- [x] 96. すべてのファイルで Mermaid・DOT の図・数式・Android の VectorDrawable の絵を描く（エンジンの隣に同梱した絵の手伝い `amonhen-render` で。エンジンは絵の機能を持たない）
- [x] 97. 絵の下の市松模様と整形した Markdown の検索: 絵の色から選ぶ明るい・暗い市松模様と切り替えのボタン、すべてのファイルの Markdown のプレビューの ⌘F（キーの札・表のマスも）
- [x] 98. Working Copy をアプリの並べ方に（一部はエンジンがあれば）: 一覧は左の列いっぱい、コミットの欄は右上（差分の上）で件名の行だけに畳め、高さを覚える。⊟ で一覧を Mixed（☑ ▣ ☐ の 1 つの一覧）と Staged・Unstaged の節（それぞれの側の行数）で切り替える。差分の帯で Unstaged・Staged・All を選び（見る側は Working Copy で 1 つ）、選んだ側に変更がない・All で出せないときはそう言う
