# Amon Hen 設計メモ

## 概要

VSCode 上で快適に Git を操作するための自分用の拡張。Marketplace には公開せず、GitHub Releases で `.vsix` を配布する。

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

VSCode の UI 部品は自由度が低いため、専用 Git クライアントの画面を丸ごと作り込むのではなく、操作の核を VSCode の部品（TreeView、標準 diff エディタ、Webview）に割り当てて作る。

## 全体構成

`extensionKind: ["workspace"]` にすると、Remote-SSH 接続時に拡張ホストが SSH 先で動き、git もそこで実行される。over SSH 対応はこれでほぼ済む。

```
ローカル（VSCode の UI）              リモート（拡張ホスト、Remote-SSH の接続先）
┌──────────────────────┐            ┌─────────────────────────────────────┐
│ TreeView             │            │ UI 層                               │
│ 標準 diff エディタ    │──postMessage──▶ Provider 群・Webview との通信      │
│ Webview              │   (SSH 経由) │        ▼                            │
└──────────────────────┘            │ 状態管理層 ◀──── vscode.git API      │
                                    │ status キャッシュ・Undo 記録 (変更通知)│
                                    │        ▼                            │
                                    │ git 実行層                          │
                                    │ 常駐 cat-file・非対話 rebase         │
                                    │        ▼                            │
                                    │ git CLI / リポジトリ                 │
                                    └─────────────────────────────────────┘
```

Webview はローカルで描画され、拡張ホストとの通信だけが SSH を経由する。大きな差分やログは分割して送る。

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
  - squash は `fixup` ＋ メッセージの amend、fixup は `fixup` だけ、edit は `pick` ＋ `break`。
  - 編集できるのは HEAD から first-parent で辿り、最初の merge コミットの手前まで。merge をまたぐ rebase は扱わない。
  - `--autostash` で作業中の変更を退避し、`--no-autosquash` でユーザーの設定に todo を並べ替えさせない。
- split は「`pick` → `exec git reset HEAD^` → `break`」で止め、Working Copy で分割してコミット → Continue の流れ。
- merge や rebase が止まっている間は、Working Copy の先頭に Continue / Abort を出す。edit や split で止まった操作の Undo の記録は保留しておき（`undo-pending.json`）、終わった時点で確定する。
- 操作前に `HEAD` と関係する ref を記録し、1 操作単位で Undo できるようにする。
  - 記録は操作の前後の両方を取り、`<gitDir>/amonhen/undo.json` に最大 50 件残す。Undo するときは今の状態が「操作後」と一致することを確かめ、違えば何もしない。
  - checkout 中のブランチは `reset`（commit は `--soft`、merge や rebase は `--keep`）で戻し、そうでないブランチは `update-ref` に旧値を条件として渡して戻す。操作の中でブランチを切り替えていたら、元のブランチに戻る。
  - コンフリクトで止まった操作は記録しない。戻すのは abort の役目。push は Undo の対象にしない。

### 差分ビューから細かくコミットを作る

- 差分表示は標準の diff エディタを使う。`TextDocumentContentProvider` で `amonhen:` スキームを作って HEAD 版と index 版を提供し、`vscode.diff` で開く。シンタックスハイライト、単語単位の差分、インライン／左右分割の切り替えがそのまま使える。
- hunk・行単位の stage は、index と working tree の行差分を自前で計算し（Myers 法）、選択範囲の変更だけを当てた新しい index の内容を作って `hash-object -w` → `update-index --cacheinfo` で書き込む。unstage は HEAD との差分で同じことをする。部分パッチを `git apply` に渡す方式より、文脈行や改行の扱いでつまずかない。
- 標準 diff エディタにはチェックボックスを置けないので、エディタ上部のボタン、キーバインド、ガターのデコレーションで補う。全 hunk を 1 画面でスクロールしたい場合は、Webview の差分ビューを別途用意する。

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

## 速度対策

小さな git コマンドの所要時間は、ほぼプロセス起動のコスト（macOS で 1 回 約 6 ms）。常駐 `cat-file` への問い合わせは 約 0.08 ms。言語より「spawn の回数」と「更新範囲」が効くので、以下を徹底する。

- `git cat-file --batch-command` を常駐させ、オブジェクト取得のたびに spawn しない。cat-file は index を最初の 1 回しか読まないので、`:path` を問い合わせる前に index の更新時刻を確かめ、変わっていれば起動し直す。
- ref・config・stash の変化は `.git` 内のファイル（`HEAD`、`refs/` 以下のディレクトリ、`packed-refs`、`reftable/tables.list`、`config`、`logs/refs/stash`）の更新時刻で検知し、spawn しない。git は ref を lock ファイルの rename で更新するので、ディレクトリの更新時刻で足りる。
- 変化があった部分だけ取り直す。ref は ahead/behind も含めて `for-each-ref` 1 回で取る。worktree の変化は検知できないので status は毎回取る。独立したコマンドは並列で実行する。
- Tags / Stashes / Submodules は展開されるまで取得しない。TreeView は前回との差分を取り、変わったノードだけ更新する。
- stage / unstage などは先に UI へ反映し、git の結果で確定させる（楽観的更新）。
- esbuild で 1 ファイルに bundle し、実行時の依存パッケージは持たない。Webview は React を使わず、仮想スクロールとグラフの canvas 描画で軽くする。
- git コマンドと再描画の所要時間を Output に記録し、「通常のリポジトリでサイドバーの更新は 30 ms 以内」を目安に計測しながら作る。
- git 実行層はインターフェースで切り離す。計測して TS で足りない箇所が出たら、その部分だけ Rust（gitoxide）などの WASM／ネイティブ実装に差し替えられるようにする。
- status は `git status --porcelain=v2 -z` で取得する。大きなリポジトリでは `core.untrackedCache` と fsmonitor の有効化を案内する。
- 読み取り系コマンドには `--no-optional-locks` を付け、ユーザー自身の git 操作とロックを奪い合わない。
- log は 200 件程度ずつ取得する。グラフのレーン計算は拡張ホスト側で少しずつ進め、Webview には最小限の配列だけ送る。Webview 側は仮想スクロールで描画する。
- `git commit-graph write --reachable` の実行を案内すると、グラフ表示と ancestry 判定が速くなる。
- Remote-SSH では postMessage が SSH を経由するため、大きな差分やログは分割して送る。

## 配布

- 正式版: `.github/workflows/release.yml`。`v*` タグの push で、タグと `package.json` の version が一致するか確かめ、テスト → `vsce package` → Release に添付する。
- snapshot: `.github/workflows/snapshot.yml`。main への push のたびに `scripts/snapshot-version.mjs` で `<次の patch>-snapshot.<前回リリースからのコミット数>+<sha>` を作り、`snapshot` という pre-release を置き換える。SemVer の比較で正式版より古くなるので、正式版を入れると上書きされる。
- 各端末でのインストール・更新: `scripts/install.sh [stable|snapshot]`。`gh release download` → `code --install-extension --force`。

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
- [ ] 9. ナビのブランチ類とドラッグ＆ドロップ、ツールバー（Undo・Fetch など）
- [ ] 10. サイドバーの TreeView の整理とドキュメント
