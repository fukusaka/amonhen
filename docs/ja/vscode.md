# VSCode 拡張の設計メモ

[English](../vscode.md)

## 概要

VSCode 上で快適に Git を操作するための自分用の拡張。Amon Hen のリポジトリの `apps/vscode` で作り（以下のパスはそこからの相対）、Amon Hen のエンジン
（アプリ・CLI と同じもの）を同梱する。Marketplace には公開せず、GitHub Releases で `.vsix` を配布する。

| 要件 | 区分 |
| --- | --- |
| サイドバー（Branches / Remotes / Tags / Stashes / Submodules） | 必須 |
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
- Undo の記録は、コアと同じファイル・同じ形（`<git dir>/amonhen/undo.json`）。コアが記録する操作を拡張がもう一度
  記録しないよう、`delegation.ts` で分ける。
- 合うエンジンがないとき、`amonhen.daemon.enabled` を切ったときは、拡張の git 実行層で動く。下の git の起動・`cat-file`・
  rebase の組み立て・行単位の stage の節は、この経路の話。エンジンは同じことを Rust で行う。
- 人はどちらの経路でも mailmap の名前で出す。エンジンはコアで置き換える（詳細には記録された名前とメールも付き、違えば
  名前の下に出す）。git 実行層は `log.mailmap` を読み、History の行は `%aN`・`%aE` を使い（`%aN` は `log.mailmap` が
  false でも置き換える）、詳細は `git check-mailmap` で置き換える。`.mailmap`・設定ファイル・`mailmap.file` の指す
  ファイルが変わったら（読み直すたびに更新時刻を見る。`mailmap.file` の場所は設定ファイルが変わったときだけ git に聞く）、
  History と覚えている詳細を読み直す。

- Range Diff（ローカルブランチの Compare with Previous Version と、パレットの Range Diff…）は、`git range-diff` の形の
  読み取り専用の `amonhen:` の文書を出す。エンジンではコアの `previousVersion`・`rangeDiff` を使い、git 実行層では
  ブランチの reflog（`git log -g`）を読んで `git range-diff` を自分で動かす。

## 必須要素の実装方針

### サイドバー

- 独自の View Container に TreeView を置く。項目は Working Copy / Branches / Remotes / Tags / Stashes / Submodules。
- 状態変化の検知は `vscode.git` の `onDidChange` に任せ、自前の file watcher は持たない。

### ドラッグ＆ドロップによる merge / rebase / push

- `TreeDragAndDropController` で実装する。ブランチをブランチにドロップしたら「Merge A into B」「Rebase B onto A」を QuickPick で選ばせる。リモートにドロップしたら push。
- ドロップ先が現在のブランチでなければ checkout してから実行する。fast-forward で済む場合は checkout せず `update-ref` だけで処理する。
- 制約: TreeView と Webview の間ではドラッグ＆ドロップできない。History 内の並べ替えは Webview の中で完結させる。

### コミットの柔軟な編集

- reword / squash / fixup / drop / reorder / edit / split はすべて非対話の rebase で実現する。todo はこちらで丸ごと作り（`src/history/rebasePlan.ts`）、`GIT_SEQUENCE_EDITOR="cp <todo>"` で git の todo に上書きする。
  - メッセージの書き換えは、対象のコミットの直後に `exec git commit --amend -F <file>` を挟んで行う。どのコミットのメッセージかを `GIT_EDITOR` 側で判別しなくて済む。
  - squash は親の `pick` ＋ メッセージの amend ＋ `fixup`、fixup は `fixup` だけ、edit は git の `edit`（Continue のとき、stage した変更はそのコミットに amend され、git がそのコミットの notes を最後のコミットへ写す。`notes.rewriteRef`）。メッセージは `fixup` の前に親に付ける。後で amend すると、rebase が notes を写す先（`notes.rewriteRef`）が amend の前のコミットになり、まとめた側の notes が落ちる。
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
- マージエディタを開く公開 API は無いので、組み込みの Git 拡張も使っている内部コマンド `_open.mergeEditor` に、base・input1・input2（index の stage 1・2・3 を `amonhen:` で返す）と output（実ファイル）を渡して開く。失敗したら、または片側で削除されたコンフリクトなら、ファイルを開いて標準 UI の「Resolve in Merge Editor」とサイドバーの Accept Current / Incoming に任せる。
- 解決済みにするとき、コンフリクトのマーカーが残っていれば確認する。rebase 中は進み具合（`rebase-merge/msgnum`・`end`）を表示し、Skip もできるようにする。

## UI を 1 つの Webview にまとめる

ステップ 1〜5 では、Working Copy をサイドバーの TreeView と標準 diff エディタで、History をエディタ領域の Webview で作った。表現がまったく違う部品に分かれているため、Working Copy と History を同じ画面の中で切り替える操作感にならなかった。そこで UI 層を作り直し、エディタ領域の 1 つの Webview（メインビュー）にまとめる。git 実行層・状態管理層・Undo・rebase の仕組みはそのまま使う。

- メインビューは左にナビ（Working Copy / History / Branches / Remotes / Tags / Stashes）、右に内容を出す。切り替えは Webview の中だけで行い、拡張ホストとの往復を待たない。
- 差分は Webview の中に描く。軽さを優先してシンタックスハイライトは付けず、単語単位の差分の強調と、hunk・行の選択による stage / unstage を持たせる。差分の計算と index への書き込みは拡張ホスト側（`src/diff/lineDiff.ts`）で行う。
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
- Tags / Stashes / Submodules は展開されるまで取得しない。TreeView は前回との差分を取り、変わったノードだけ更新する。
- stage / unstage などは先に UI へ反映し、git の結果で確定させる（楽観的更新）。
- esbuild で 1 ファイルに bundle し、実行時の依存パッケージは持たない。Webview は React を使わず、仮想スクロールとグラフの canvas 描画で軽くする。
- git コマンドと再描画の所要時間を Output に記録し、「通常のリポジトリでサイドバーの更新は 30 ms 以内」を目安に計測しながら作る。
- git 実行層はインターフェースで切り離してあり、エンジンがあれば Rust（gix）のエンジンが代わる（「エンジン（共有の
  daemon）」を参照）。History と差分は、git を起動せずに daemon からレコードで受け取る。
- status は `git status --porcelain=v2 -z` で取得する。大きなリポジトリでは `core.untrackedCache` と fsmonitor の有効化を案内する。
- 読み取り系コマンドには `--no-optional-locks` を付け、ユーザー自身の git 操作とロックを奪い合わない。
- log は 200 件程度ずつ取得する。グラフのレーン計算は拡張ホスト側で少しずつ進め、Webview には最小限の配列だけ送る。Webview 側は仮想スクロールで描画する。
- `git commit-graph write --reachable` の実行を案内すると、グラフ表示と ancestry 判定が速くなる。
- Remote-SSH では postMessage が SSH を経由するため、大きな差分やログは分割して送る。

## 配布

- 正式版: ルートの `.github/workflows/vscode-release.yml`。`vscode-v*` タグ（アプリ・エンジンは `v*`）の push で、
  エンジン（Windows も）を作り、macOS のものは Developer ID で署名して公証に出し、`scripts/bundle-engine.sh` で同梱して `vsce package`、ビルドの由来の証明を付けて
  Release を作る。
- snapshot: ルートの `.github/workflows/snapshot.yml`。main の CI が通るたびに、`scripts/snapshot-version.mjs` が
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

- [ ] 12. History・差分・Working Copy・書き込みを共有の daemon で行う。
  [design.md](design.md) の 18 で進める
