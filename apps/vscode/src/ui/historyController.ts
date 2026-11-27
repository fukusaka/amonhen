import type { EditRow } from '../history/editPlan.ts';
import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename } from 'node:path';
import * as vscode from 'vscode';
import { daemonLanguage, type DaemonClient, type Reply } from '../daemon/client.ts';
import { daemonFor, renderPath } from '../daemon/connection.ts';
import { RenderSession } from '../daemon/render.ts';
import { applySyntax, commitFiles, splitCommitDiff, textSyntaxByLine } from '../daemon/diff.ts';
import { readRows, toRowData } from '../daemon/history.ts';
import { contentsToFetch, searchesContents, searchHistory } from '../daemon/search.ts';
import { computeDiff, type DiffLine, type FileDiff } from '../diff/fileDiff.ts';
import { parseCommitObject } from '../git/commitObject.ts';
import { signatureExplanation, signatureLabel, signatureStatus, type SignatureCheck } from '../git/signature.ts';
import type { Commit } from '../git/log.ts';
import { GraphBuilder } from '../history/graph.ts';
import type {
  CommitDetails,
  DetailsExtras,
  DiffChoices,
  DiffWhitespace,
  FileView,
  HistoryHostMessage,
  ImageBlob,
  PictureColors,
  SvgCheckView,
  TreeEntry,
  ViewLine,
  HistoryWebviewMessage,
  PairMode,
  Person,
  RefLabel,
  RowData,
  Scope,
  SearchState,
  SelectionView,
  SignatureView,
  CommitSummaryView,
  FullExtras,
  TagView,
  HistoryItem,
  HistoryOrder,
} from '../history/protocol.ts';
import { samePath, type Worktree } from '../git/worktrees.ts';
import {
  historyHidden,
  historyHighlight,
  historyOneLine,
  historyOrder,
  setDetailsLayout,
  detailsLayout,
  monospacedMessage,
  setMonospacedMessage,
  setHistoryHidden,
  setHistoryHighlight,
  setHistoryOneLine,
  setHistoryOrder,
} from './preferences.ts';
import { editableChain, refLabels, worktreeLabels } from '../history/rows.ts';
import { scopeRefs } from '../history/scope.ts';
import type { Repository, Section } from '../state/repository.ts';
import { openCommitChanges, openCommitFile } from './commitChanges.ts';
import { gitUri } from './uri.ts';
import { repositorySlug } from '../git/github.ts';
import { isVectorDrawable } from '../history/vectorDrawable.ts';
import { drawMarkdownPictures } from '../history/markdownPictures.ts';
import { pictureSettings, type PictureSettings } from '../history/pictureSettings.ts';
import { onDidChangeSignatureConfig } from './signatureConfig.ts';
import { showError } from './stageCommands.ts';

/** 1 回に読み込むコミットの数 */
const PAGE = 200;

/** mailmap で置き換えて記録と違えば、記録されたもの */
function recordedIfMapped(recorded: Person, mapped: Person): Person | undefined {
  return recorded.name !== mapped.name || recorded.email !== mapped.email
    ? { name: recorded.name, email: recorded.email }
    : undefined;
}

/** daemon の答えの `{name, email}`（なければ undefined） */
function person(value: unknown): Person | undefined {
  const v = value as { name?: unknown; email?: unknown } | null | undefined;
  return v && typeof v.name === 'string' ? { name: v.name, email: String(v.email ?? '') } : undefined;
}
/** 編集できるかを調べる範囲 */
const EDITABLE_LIMIT = 500;
/** 詳細を覚えておくコミットの数 */
const DETAILS_CACHE = 100;
/** 差分を覚えておくファイルの数 */
const DIFF_CACHE = 200;
/** いくつも選んだときに並べるコミットの数 */
const SELECTION_LIST = 200;

/** 共有の daemon の History: そのつながりと History の番号と行の数 */
interface DaemonHistory {
  client: DaemonClient;
  history: unknown;
  rows: number;
  /** shallow clone の History なら、境目のコミットの行 */
  boundary?: number[];
  /** 畳んだ merge が行ごとに隠している数（畳んでいなければなし） */
  folded?: Uint32Array;
}

/** 昇順の `rows` で、`row` 以上の最初の位置（なければ最後） */
function lowerBound(rows: readonly number[], row: number): number {
  let lo = 0;
  let hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (rows[mid]! < row) lo = mid + 1;
    else hi = mid;
  }
  return Math.min(lo, Math.max(0, rows.length - 1));
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** partial clone で中身を探す前に、取る中身の数を出して断る。上限を超えるなら探さず、絞るよう頼む */
async function confirmContentsSearch(count: number, more: boolean): Promise<boolean> {
  const n = count.toLocaleString();
  if (more) {
    const open = await vscode.window.showWarningMessage(
      'Too Many Contents to Fetch',
      {
        modal: true,
        detail: `This search needs more than ${n} file contents not fetched yet. Narrow it with path:, after: or before:, or fetch all contents in Fetching and Folders.`,
      },
      'Open Fetching and Folders',
    );
    if (open) await vscode.commands.executeCommand('amonhen.showRepositorySettings');
    return false;
  }
  const answer = await vscode.window.showWarningMessage(
    'Search the Contents?',
    { modal: true, detail: `This search fetches ${n} file contents not fetched yet.` },
    'Search',
  );
  return answer === 'Search';
}

/**
 * メインビューの History の拡張ホスト側。
 * log は分割して読み込み、グラフのレーンはこちらで計算して、描画に必要な最小限の配列だけ送る。
 */
/** `a/./b/../c` を `a/c` に。根より上に出れば undefined */
function normalizePath(path: string): string | undefined {
  const out: string[] = [];
  for (const part of path.split('/')) {
    if (part === '' || part === '.') continue;
    if (part === '..') {
      if (out.length === 0) return undefined;
      out.pop();
    } else {
      try {
        out.push(decodeURIComponent(part));
      } catch {
        out.push(part);
      }
    }
  }
  return out.join('/');
}

/** 詳細で比べる画像の、片側の大きさの上限 */
const MAX_IMAGE_BYTES = 20 * 1024 * 1024;

/** 画像の拡張子の MIME の型 */
/** Markdown のブロックに描いた図（描けなかったものも）があるか */
function hasDiagrams(blocks: readonly unknown[]): boolean {
  return JSON.stringify(blocks).includes('"kind":"diagram"');
}

function imageType(path: string): string {
  const ext = path.slice(path.lastIndexOf('.') + 1).toLowerCase();
  const types: Record<string, string> = { jpg: 'image/jpeg', jpeg: 'image/jpeg', svg: 'image/svg+xml', ico: 'image/x-icon' };
  return types[ext] ?? `image/${ext}`;
}

export class HistoryController implements vscode.Disposable {
  private static readonly edits = new vscode.EventEmitter<
    Extract<HistoryWebviewMessage, { type: 'saveMessage' | 'move' | 'squashOnto' | 'editPreview' | 'editApply' | 'editExtend' }>
  >();
  /** Webview でメッセージの編集や並べ替えが行われた。処理は historyCommands が受け持つ */
  static readonly onDidReceiveEdit = HistoryController.edits.event;

  /** Webview へ送る。historyCommands からも使う */
  readonly post: (message: HistoryHostMessage) => void;
  private repoListener: vscode.Disposable | undefined;
  private repo: Repository | undefined;
  /** History の範囲。最初はすべてのブランチ（アプリと同じ） */
  private scope: Scope = 'all';

  private commits: Commit[] = [];
  /** 読み込んだコミット（daemon の History は飛び飛びに読むので、ID から） */
  private readonly known = new Map<string, Commit>();
  private graph = new GraphBuilder();
  private labels = new Map<string, RefLabel[]>();
  /** 編集できるコミットと、その位置（新しい順） */
  private chain = new Map<string, number>();
  private done = false;
  private loading = false;
  /**
   * 読み直しが、daemon と git のどちらで読むかを決めて最初の行を送るまで。その間に Webview が頼む続き（開いた直後に
   * 頼む最初の行）は、HEAD も読み方もまだ決まっていないので受けない（空の行を送って読み終えたことにしてしまう）
   */
  private opening = false;
  /** reset のたびに増やし、古い読み込みの結果を捨てる */
  private generation = 0;
  private headOid: string | undefined;
  private resetTimer: NodeJS.Timeout | undefined;
  /** 最後に始めた読み直し */
  private resetting: Promise<void> = Promise.resolve();
  /** 一度取ったコミットの詳細。コミットの内容は変わらないので、ref のラベル以外はそのまま使える */
  private readonly details = new Map<string, Omit<CommitDetails, 'refs'>>();
  /** 一度求めたファイルの差分。キーは `oid:path` */
  private readonly diffs = new Map<string, FileDiff>();
  /** Webview に送ったコミットの差分（`<oid か選択の key>:<path>`）。構文の色が後から届いたら、これだけ送り直す */
  private readonly sentDiffs = new Set<string>();
  /**
   * partial clone で中身が手元になく、差分を取っていないファイル（`<oid か選択の key>` → 取る中身の ID とファイルの
   * パス）。読む画面は勝手に取りに行かず、「取ってくる」の帯で押されたときだけ取る（覚えない。次のコミットではまた聞く）
   */
  private readonly missing = new Map<string, { ids: string[]; paths: Set<string> }>();
  /** partial clone か（リポジトリの形を読むまでは undefined。「すべてのファイル」でファイルを開く前に確かめる） */
  private partialClone: boolean | undefined;
  /** 行の署名の印（null は署名なし）。コミットは変わらないので、読み直しても覚えておく */
  private readonly signatures = new Map<string, SignatureView | null>();
  /** 詳細の残り（署名・子のコミット）。子は増えるので、History を読み直したら捨てる */
  private readonly extras = new Map<string, DetailsExtras>();
  /** 詳細を読んだコミットの署名の種類（`gpg`・`ssh`・`x509`。署名がなければ null） */
  private readonly signedKinds = new Map<string, string | null>();
  /** daemon の ref が古いかもしれない（ref・HEAD が動いた。詳細の残りを読む前に読み直させる） */
  private daemonRefsStale = true;
  /** コミットが merge したタグ（コアの `commitInfo` の mergedTags。署名の結果は `commitExtras` から） */
  private readonly mergedTagInfo = new Map<string, { name: string; tagger?: string; taggerEmail?: string; subject?: string; signed?: string | null }[]>();
  /** 共有の daemon で History を読むとき（設定 `amonhen.daemon.path`）、出している History */
  private daemon: DaemonHistory | undefined;
  /** 一致だけの History を出しているとき、もとの History */
  private base: DaemonHistory | undefined;
  /** 今の検索。`rows` は `history`（もとの History）の行 */
  private search: { query: string; history: unknown; rows: number[] } | undefined;
  /** いくつも選んでいる行（昇順）と、その差分。`token`・`pair` は頼まれたときのもの（空白を選び直したら読み直す） */
  private selection: { key: string; token: number; rows: number[]; pair: PairMode; diffs: Map<string, FileDiff> } | undefined;
  /** 差分で無視する空白（エンジンだけ。覚えない。選び直したら、出している詳細を読み直す） */
  private whitespace: DiffWhitespace = 'exact';
  /** コミット（か選択の key）の差分のファイルの並び（`commitDiff` の contexts はファイルの番号で頼む） */
  private readonly fileOrder = new Map<string, string[]>();
  private selectionRequest = 0;
  private searchAbort: AbortController | undefined;
  /**
   * merge の畳み方（コアの `foldState`。リポジトリごとに `<git dir>/amonhen/fold.json` に覚え、アプリ・TUI と同じ）。
   * foldDefault なら merge を畳み、foldExcept はそれと逆にした merge
   */
  private foldDefault = false;
  private readonly foldExcept = new Set<string>();
  /** 畳み方を読んだリポジトリ（開き直したら読み直す） */
  private foldLoadedFor: string | undefined;
  /**
   * 畳むか・祖先を目立たせるか。選んだらすぐこの値で読み直し、覚えるのは裏で（覚え終わるのを待つと、その間に始まった
   * 読み直しが前の値で読む）。まだ選んでいなければ、覚えている値
   */
  private highlightChoice: boolean | undefined;
  private get highlighting(): boolean {
    return this.highlightChoice ?? historyHighlight();
  }
  /** 選んでいるコミット（祖先を目立たせるため） */
  private selectedOid: string | undefined;

  /** History で 1 つだけ選んでいるコミット（いくつも選んでいれば undefined。Merge…・Rebase… で選べる） */
  get selectedCommit(): string | undefined {
    return this.selection ? undefined : this.selectedOid;
  }
  private lineageRequest = 0;
  /** 探している途中の検索の文（History を読み直したら、終わっていなくても探し直す） */
  private searching: string | undefined;
  /** 一致だけを出す切り替え（最後に頼まれたもの） */
  private matchingOnly = false;
  /** ref をまだ読み終えていない読み直しの数（その間の ref の知らせは、その読み直しが拾う） */
  private readingRefs = 0;

  /** 署名の検証の設定（`gpg.*`）の知らせ */
  private readonly signatureConfigListener: vscode.Disposable;

  constructor(post: (message: HistoryHostMessage) => void) {
    this.post = post;
    this.signatureConfigListener = onDidChangeSignatureConfig(() => this.forgetSignatures());
  }

  /** 検証の設定が変わった。覚えていた検証の結果を捨て、確かめた行と出している詳細を確かめ直す（コアは設定を書いたときに捨てている） */
  private forgetSignatures(): void {
    // 印を出していた行は署名がある（古いエンジンで署名の有無を知らない行は印なしだった）
    const rows = [...this.signatures].map(([oid, view]) => ({ oid, ...(view ? { signed: true } : {}) }));
    this.signatures.clear();
    this.extras.clear();
    void (async () => {
      // 設定はほかのリポジトリ（Settings の段が global のときも）で書いたかもしれない（コアが捨てるのは書いた
      // リポジトリの分だけ）ので、このリポジトリのコアの分も捨ててもらう（知らない古いエンジンはそのまま）
      await (await this.engine())?.call({ op: 'run', command: { op: 'forgetSignatures' } }).catch(() => undefined);
      const daemon = this.daemon;
      if (daemon) await this.verifyRows(daemon, rows, true);
      if (this.selectedOid) await this.sendDetails(this.selectedOid);
    })().catch(() => undefined);
  }

  /**
   * サイドバーから History を開いたときの範囲。ref を選んだならその ref、そうでなければ
   * ref の履歴を出していたときだけすべてのブランチに戻す（Current Branch は保つ）
   */
  showScope(scope: Scope | undefined): void {
    const next = scope ?? (scopeRefs(this.scope) ? 'all' : this.scope);
    if (next !== this.scope) {
      this.scope = next;
      void this.reset();
    }
  }

  /** 表示するリポジトリを切り替えて読み直す */
  setRepository(repo: Repository | undefined): void {
    this.repoListener?.dispose();
    this.repo = repo;
    this.repoListener = repo?.onDidChange((changed) => this.onRepositoryChange(changed));
    this.details.clear();
    this.missing.clear();
    this.partialClone = undefined;
    void this.reset();
  }

  get repository(): Repository | undefined {
    return this.repo;
  }

  commit(oid: string): Commit | undefined {
    return this.known.get(oid);
  }

  /** 編集できる範囲での位置（新しい順） */
  chainIndex(oid: string): number | undefined {
    return this.chain.get(oid);
  }

  /**
   * ブランチやタグ、HEAD が動いたときと、mailmap が変わったかもしれないとき（覚えた詳細も捨てる）だけ読み直す。
   * stage などでは読み直さない
   */
  private onRepositoryChange(changed: ReadonlySet<Section>): void {
    const headOid = this.repo?.current.status?.branch.oid;
    // ref が動いた: 詳細の残り（タグ・含むブランチ）は覚え直し、daemon にも ref を読み直させる（daemon は頼まれるまで
    // ref を読み直さないので、端末で作ったタグを知らない）
    if (changed.has('branches') || changed.has('remotes') || changed.has('tags') || (headOid !== undefined && headOid !== this.headOid)) {
      this.daemonRefsStale = true;
      this.extras.clear();
    }
    const refsChanged =
      this.readingRefs === 0 && (changed.has('branches') || changed.has('remotes') || changed.has('tags'));
    if (changed.has('mailmap')) {
      this.details.clear();
    }
    // status をまだ読んでいなければ、HEAD が動いたかは分からない（読み直さない）
    const headMoved = this.readingRefs === 0 && headOid !== undefined && headOid !== this.headOid;
    if (refsChanged || changed.has('mailmap') || headMoved) {
      clearTimeout(this.resetTimer);
      this.resetTimer = setTimeout(() => {
        this.resetTimer = undefined;
        void this.reset();
      }, 100);
    }
  }

  async handle(message: HistoryWebviewMessage): Promise<void> {
    switch (message.type) {
      case 'more':
        await this.loadMore();
        break;
      case 'range':
        // 開いている途中なら、読み直しが行を送る
        if (this.daemon && !this.opening) {
          await this.loadRange(this.daemon, message.start, message.count);
        }
        break;
      case 'search':
        await this.runSearch(message.query.trim(), message.matchingOnly, message.selected);
        break;
      case 'detailsLayout':
        await setDetailsLayout(message.layout);
        this.post({ type: 'detailsLayout', layout: detailsLayout() });
        break;
      case 'displayMenu':
        await this.displayMenu();
        break;
      case 'scope':
        this.scope = message.scope;
        await this.reset();
        break;
      case 'select':
        this.clearSelection();
        this.selectedOid = message.oid;
        void this.sendLineage().catch(() => undefined);
        await this.sendDetails(message.oid).catch(showError);
        break;
      case 'selectRows':
        await this.sendSelection(message.token, message.rows, message.pair);
        break;
      case 'commitDiff':
        await this.sendCommitDiff(message.oid, message.path).catch(showError);
        break;
      case 'treeEntries':
        await this.sendTreeEntries(message.oid, message.path).catch(showError);
        break;
      case 'treeFile':
        await this.sendTreeFile(message.oid, message.path, message.colors, message.timeZone).catch((err: unknown) =>
          this.post({ type: 'treeFile', oid: message.oid, path: message.path, view: { kind: 'error', message: errorMessage(err) } }),
        );
        break;
      case 'openTreeFile':
        if (this.repo) {
          // 行があればその行を選んで開く（差分の行の右クリック。「すべてのファイル」がないとき）
          const line = message.line ? Math.max(0, message.line - 1) : undefined;
          const options: vscode.TextDocumentShowOptions | undefined =
            line === undefined ? undefined : { selection: new vscode.Range(line, 0, line, 0) };
          await vscode.commands
            .executeCommand('vscode.open', gitUri(this.repo.root, message.path, 'rev', message.oid), options)
            .then(undefined, showError);
        }
        break;
      case 'commitImage':
        await this.sendCommitImage(message.oid, message.path).catch(showError);
        break;
      case 'fileContext':
        await this.sendFileContext(message.oid, message.path, message.context).catch(showError);
        break;
      case 'whitespace':
        await this.setWhitespace(message.whitespace).catch(showError);
        break;
      case 'openFile': {
        const details = this.details.get(message.oid);
        const file = details?.files.find((f) => f.path === message.path);
        if (this.repo && details && file) {
          await openCommitFile(this.repo, details.oid, details.parents[0], file).catch(showError);
        }
        break;
      }
      case 'open': {
        // History にないコミット（Reflog の、どのブランチからも辿れないもの）は、出した詳細から
        const details = this.details.get(message.oid);
        const commit =
          this.commit(message.oid) ??
          (details && {
            oid: details.oid,
            parents: details.parents,
            author: details.author.name,
            email: details.author.email,
            date: details.author.date,
            subject: details.message.split('\n', 1)[0] ?? '',
          });
        if (this.repo && commit) {
          await openCommitChanges(this.repo, commit).catch(showError);
        }
        break;
      }
      case 'saveMessage':
      case 'move':
      case 'squashOnto':
      case 'editExtend':
      case 'editPreview':
      case 'editApply':
        HistoryController.edits.fire(message);
        break;
      case 'goTo':
        await this.goTo(message.to, message.oid).catch(showError);
        break;
      case 'density': {
        // 密度の帯（アプリと同じ。daemon の History だけ）
        const shown = this.daemon;
        if (!shown) break;
        try {
          const reply = await shown.client.call({ op: 'historyDensity', history: shown.history, buckets: Math.max(1, Math.floor(message.buckets)) });
          if (shown !== this.daemon) break;
          const values = reply.f32('density');
          this.post({ type: 'density', values: Buffer.from(values.buffer, values.byteOffset, values.byteLength).toString('base64') });
        } catch {
          // 古いエンジン。帯は出さない
        }
        break;
      }
      case 'toggleFold':
        await this.toggleFold(message.oid).catch(showError);
        break;
      case 'fetchContents':
        await this.fetchContents(message.oid, message.path);
        break;
      case 'reloadDiff':
        await this.reloadDiff(message.oid).catch(showError);
        break;
      case 'fetchForm': {
        const command = { more: 'amonhen.fetchMoreHistory', all: 'amonhen.fetchAllHistory', contents: 'amonhen.fetchAllContents' };
        await vscode.commands.executeCommand(command[message.what]);
        break;
      }
    }
  }

  private clearSelection(): void {
    this.selection = undefined;
    this.selectionRequest++;
  }

  /** いくつも選んだ行の差分を読む。差分はとっておき、ファイルが画面に入ったら送る */
  private async sendSelection(token: number, rows: readonly number[], pair: PairMode): Promise<void> {
    const daemon = this.daemon;
    if (!daemon) return;
    const request = ++this.selectionRequest;
    const sorted = [...rows].sort((a, b) => a - b);
    this.selection = { key: `selection:${token}`, token, rows: sorted, pair, diffs: new Map() };
    const current = this.selection;
    try {
      const [diff, listed] = await Promise.all([
        daemon.client.call({ op: 'commitDiff', history: daemon.history, rows: sorted, pair, ...this.whitespaceArg() }),
        daemon.client.call({ op: 'historyRows', history: daemon.history, rows: sorted.slice(0, SELECTION_LIST) }),
      ]);
      if (request !== this.selectionRequest) return;
      const index = new Map<number, DiffLine>();
      current.diffs = splitCommitDiff(diff, index);
      this.rememberOrder(current.key, diff.json['files']);
      this.rememberMissing(current.key, diff);
      void this.loadSyntax(daemon.client, diff.json['view'], index, current.key, current.diffs);
      const selection = diff.json['selection'] as { kind: SelectionView['kind'] };
      const commits = readRows(listed).map((r) => ({ row: r.row, oid: r.oid, subject: r.subject, author: r.author }));
      this.post({
        type: 'selection',
        token,
        view: {
          key: current.key,
          count: sorted.length,
          kind: selection.kind,
          pair,
          files: (diff.json['files'] as SelectionView['files'] | undefined) ?? [],
          commits,
        },
        choices: this.choices(),
      });
    } catch (err) {
      if (request === this.selectionRequest) {
        this.post({ type: 'selection', token, error: errorMessage(err) });
      }
    }
  }

  /** いくつも選んでいるなら、そのコミット（新しい順） */
  async selectedCommits(): Promise<Commit[]> {
    const daemon = this.daemon;
    const rows = this.selection?.rows;
    if (!daemon || !rows) return [];
    const reply = await daemon.client.call({ op: 'historyRows', history: daemon.history, rows: [...rows] });
    return readRows(reply).map((r) => ({
      oid: r.oid,
      parents: r.parentOids,
      author: r.author,
      email: '',
      date: r.date,
      subject: r.subject,
    }));
  }

  /** merge するときの名前: ブランチ・タグがあればその名前（merge のメッセージが git と同じになる）、なければ ID */
  mergeName(oid: string): string {
    return this.labels.get(oid)?.find((l) => !l.current)?.name ?? oid;
  }

  get headCommit(): string | undefined {
    return this.headOid;
  }

  /** 続いたコミットをまとめるときのメッセージと範囲（コアが、続いているか・編集できるかも確かめる） */
  /**
   * 編集モードの行: HEAD から first-parent で merge を越えて辿り、`from` とそれより `older` 個古いもの、少なくとも `atLeast`
   * 行まで（古いものにまとめる・古い方へ動かすため）。新しい順。エンジンがないか、`from` を辿れなければ undefined
   */
  async editRows(from: string, older = 0, atLeast = 0): Promise<EditRow[] | undefined> {
    const client = await this.engine();
    if (!client) return undefined;
    const run = async (command: Record<string, unknown>) => {
      const reply = (await client.call({ op: 'run', command })).json as { ok?: boolean; error?: string; text?: string | null };
      if (!reply.ok) throw new Error(reply.error ?? 'Can’t read the commits');
      return reply.text ?? '';
    };
    const line = (await run({ op: 'editableCommits', throughMerges: true }))
      .split('\n')
      .filter(Boolean)
      .map((l) => ({ oid: l.slice(0, l.indexOf(' ') < 0 ? undefined : l.indexOf(' ')), merge: l.endsWith(' merge') }));
    let end = line.findIndex((c) => c.oid === from);
    if (end < 0) return undefined;
    end = Math.max(end + older, atLeast - 1);
    if (end >= line.length) return undefined;
    return Promise.all(
      line.slice(0, end + 1).map(async (c): Promise<EditRow> => ({
        oid: c.oid,
        message: await run({ op: 'commitMessage', oid: c.oid }),
        author: this.known.get(c.oid)?.author ?? '',
        ...(c.merge ? { merge: true } : {}),
        action: 'pick',
      })),
    );
  }

  async combineMessage(oids: readonly string[]): Promise<{ oldest: string; newest: string; message: string }> {
    const client = await this.engine();
    if (!client) throw new Error('This needs the engine');
    const reply = await client.call({ op: 'run', command: { op: 'squashRangeMessage', oids: [...oids] } });
    const answer = reply.json as { ok?: boolean; error?: string; info?: { oldest: string; newest: string; message: string } };
    if (!answer.ok || !answer.info) throw new Error(answer.error ?? 'These commits can’t be combined');
    return answer.info;
  }

  /** HEAD・親（最初の親）・子のコミットに移る。子がいくつもあれば選んでもらう */
  async goTo(to: 'head' | 'parent' | 'child', oid?: string): Promise<void> {
    let target: string | undefined;
    if (to === 'head') {
      target = this.headOid;
    } else if (oid && to === 'parent') {
      const client = this.details.has(oid) ? undefined : await this.engine();
      target = (this.details.get(oid) ?? (client && (await this.daemonDetails(client, oid))))?.parents[0];
    } else if (oid) {
      const children = (await this.readExtras(oid))?.children ?? [];
      if (children.length > 1) {
        const picked = await vscode.window.showQuickPick(
          children.map((c) => ({ label: c.slice(0, 8), description: this.known.get(c)?.subject, oid: c })),
          { placeHolder: 'Child commit' },
        );
        target = picked?.oid;
      } else {
        target = children[0];
      }
    }
    if (target && !(await this.reveal(target))) {
      void vscode.window.showInformationMessage(`${target.slice(0, 8)} isn’t in History`);
    }
  }

  /** リポジトリの形（shallow の境目・partial clone）を送る */
  private async sendForm(daemon: DaemonHistory, generation: number): Promise<void> {
    const reply = (await daemon.client.call({ op: 'run', command: { op: 'repositoryForm' } })).json;
    if (generation !== this.generation || reply['ok'] !== true) return;
    const partial = (reply['info'] as { partial?: { remote: string; filter: string } | null } | undefined)?.partial;
    this.partialClone = !!partial;
    // 一致だけの History（検索）のときは、帯も境目も出さない
    const shallow = daemon.boundary !== undefined && !this.base;
    this.post({ type: 'form', shallow, boundary: shallow ? daemon.boundary! : [], ...(partial ? { partial } : {}) });
  }

  /**
   * 行の署名を確かめて印を送る（一度確かめたものは覚えている。コアも覚えている）。`reset` なら Webview の前の印を
   * 捨てさせる（署名が無効・なしになった行の印は送る分に含まれないため）
   */
  private async verifyRows(
    daemon: DaemonHistory,
    rows: readonly { oid: string; signed?: boolean }[],
    reset = false,
  ): Promise<void> {
    const todo = rows.filter((r) => !this.signatures.has(r.oid));
    if (todo.length === 0) {
      if (reset) this.post({ type: 'signatures', marks: {}, reset });
      return;
    }
    const reply = (await daemon.client.call({ op: 'run', command: { op: 'verifySignatures', oids: todo.map((r) => r.oid) } }))
      .json;
    if (reply['ok'] !== true) return;
    const info = reply['info'] as { checks?: (SignatureCheck & { committerEmail?: string })[] } | undefined;
    const checks = info?.checks ?? [];
    const marks: Record<string, SignatureView> = {};
    todo.forEach(({ oid, signed }, i) => {
      const check = checks[i];
      // 署名があるのに確かめられなければ（N）No key（アプリと同じ）。署名の有無を知らない行（古いエンジン）の N は印なし
      const view = check && (check.status !== 'N' || signed) ? signatureView(check, check.committerEmail ?? '') : null;
      this.signatures.set(oid, view);
      if (view) marks[oid] = view;
    });
    if (reset || Object.keys(marks).length > 0) this.post({ type: 'signatures', marks, ...(reset ? { reset } : {}) });
  }

  /**
   * このリポジトリのエンジンへのつながり（使わないなら undefined）。History を読み直している間は `this.daemon` が
   * ないので、History によらないコアの命令はこちらで頼む
   */
  private async engine(): Promise<DaemonClient | undefined> {
    return this.daemon?.client ?? (this.repo ? await daemonFor(this.repo.root) : undefined);
  }

  /** 詳細の残り（署名・子のコミット）を読む */
  private async readExtras(oid: string): Promise<DetailsExtras | undefined> {
    const cached = this.extras.get(oid);
    if (cached) return cached;
    const client = await this.engine();
    if (!client) return undefined;
    if (this.daemonRefsStale) {
      this.daemonRefsStale = false;
      await client.call({ op: 'refs' }).catch(() => undefined);
    }
    const reply = (await client.call({ op: 'run', command: { op: 'commitExtras', oid } })).json;
    if (reply['ok'] !== true) return undefined;
    const info = reply['info'] as
      | {
          signature?: SignatureCheck | null;
          children?: string[];
          notes?: string;
          headRev?: string | null;
          branches?: string[];
          tags?: string[];
          tagCount?: number;
          previousTag?: string | null;
          originUrl?: string | null;
          remoteBranches?: string[];
          merged?: CommitSummaryView[];
          mergedCount?: number;
          references?: Record<string, CommitSummaryView>;
          tagObjects?: { name: string; id: string | null; tagger?: string; taggerEmail?: string; time?: number; subject?: string; signed?: string | null; signature?: SignatureCheck | null }[];
          mergedTagSignatures?: (SignatureCheck | null)[];
        }
      | undefined;
    const committer = this.details.get(oid)?.committer.email ?? '';
    // 署名があるのに確かめられなければ（N か結果なし）No key（アプリと同じ）
    const signed = this.signedKinds.get(oid);
    const check = info?.signature && info.signature.status !== 'N' ? info.signature : signed ? { status: 'N' } : undefined;
    const signature = check ? signatureView(check, committer, signed) : undefined;
    // タグの署名は、タガーのメールと比べる。署名のないタグには付けない
    const tagSignature = (s: SignatureCheck | null | undefined, email: string | undefined, kind: string | null | undefined) =>
      kind ? signatureView(s && s.status !== 'N' ? s : { status: 'N' }, email ?? '', kind) : undefined;
    const tags: TagView[] = (info?.tagObjects ?? []).map((t) => {
      const view = tagSignature(t.signature, t.taggerEmail, t.signed);
      return {
        name: t.name.replace(/^refs\/tags\//, ''),
        ...(t.id ? { tagger: t.tagger, time: t.time, subject: t.subject } : {}),
        ...(view ? { signature: view } : {}),
      };
    });
    const mergedTags: TagView[] = (this.mergedTagInfo.get(oid) ?? []).map((t, n) => {
      const view = tagSignature(info?.mergedTagSignatures?.[n], t.taggerEmail, t.signed);
      return { name: t.name, ...(t.tagger ? { tagger: t.tagger } : {}), ...(t.subject ? { subject: t.subject } : {}), ...(view ? { signature: view } : {}) };
    });
    const extras: DetailsExtras = {
      oid,
      ...(signature ? { signature } : {}),
      children: info?.children ?? [],
      full: fullExtras(info ?? {}, this.details.get(oid)?.message ?? ''),
      ...(info?.notes?.trim() ? { notes: info.notes } : {}),
      ...(tags.length > 0 ? { tags } : {}),
      ...(mergedTags.length > 0 ? { mergedTags } : {}),
      ...(info?.mergedCount ? { mergedCount: info.mergedCount } : {}),
    };
    this.extras.set(oid, extras);
    if (this.extras.size > DETAILS_CACHE) this.extras.delete(this.extras.keys().next().value!);
    return extras;
  }

  private async sendDetails(oid: string): Promise<void> {
    const repo = this.repo;
    if (!repo) {
      return;
    }
    let details = this.details.get(oid);
    if (!details && this.daemon) {
      details = await this.daemonDetails(this.daemon.client, oid);
      this.details.set(oid, details);
    }
    if (!details) {
      const obj = await repo.backend.object(oid);
      if (!obj) {
        return;
      }
      const commit = parseCommitObject(obj.data.toString('utf8'));
      const [files, [author, committer]] = await Promise.all([
        repo.backend.commitFiles(oid, commit.parents[0]),
        repo.backend.mapPeople([commit.author, commit.committer]),
      ]);
      details = {
        oid,
        parents: commit.parents,
        author: { ...commit.author, ...author },
        committer: { ...commit.committer, ...committer },
        recordedAuthor: recordedIfMapped(commit.author, author!),
        recordedCommitter: recordedIfMapped(commit.committer, committer!),
        message: commit.message,
        files,
      };
      this.details.set(oid, details);
      if (this.details.size > DETAILS_CACHE) {
        this.details.delete(this.details.keys().next().value!);
      }
    }
    // shallow clone なら、取っていない親を確かめる（詳細に「(not fetched)」と出す。アプリと同じ）
    const missingParents =
      this.daemon?.boundary !== undefined && this.repo
        ? (await Promise.all(details.parents.map(async (p) => ((await this.repo!.backend.object(p).catch(() => undefined)) ? undefined : p)))).filter(
            (p): p is string => p !== undefined,
          )
        : [];
    this.post({
      type: 'details',
      details: { ...details, refs: this.labels.get(oid) ?? [], ...(missingParents.length > 0 ? { missingParents } : {}) },
      ...(this.daemon ? { choices: this.choices() } : {}),
    });
    // 署名の検証と子のコミットは時間がかかるので、後から送る（エンジンだけ）
    void this.readExtras(oid)
      .then((extras) => extras && this.post({ type: 'detailsExtras', extras }))
      .catch(() => undefined);
  }

  /** 共有の daemon で、コミットの詳細と全ファイルの差分を読む（差分は覚えておき、ファイルを選んだときに送る） */
  private async daemonDetails(client: DaemonClient, oid: string): Promise<Omit<CommitDetails, 'refs'>> {
    const [info, diff] = await Promise.all([
      client.call({ op: 'run', command: { op: 'commitInfo', oid } }),
      client.call({ op: 'commitDiff', id: oid, ...this.whitespaceArg() }),
    ]);
    const i = info.json['info'] as Record<string, unknown>;
    if (info.json['ok'] !== true || !i) {
      throw new Error(String(info.json['error'] ?? 'Can’t read the commit'));
    }
    this.signedKinds.set(oid, typeof i['signed'] === 'string' ? i['signed'] : null);
    if (this.signedKinds.size > DETAILS_CACHE) this.signedKinds.delete(this.signedKinds.keys().next().value!);
    if (Array.isArray(i['mergedTags']) && i['mergedTags'].length > 0) {
      this.mergedTagInfo.set(oid, i['mergedTags'] as never);
      if (this.mergedTagInfo.size > DETAILS_CACHE) this.mergedTagInfo.delete(this.mergedTagInfo.keys().next().value!);
    }
    const index = new Map<number, DiffLine>();
    const split = splitCommitDiff(diff, index);
    this.rememberOrder(oid, diff.json['files']);
    const missingFiles = this.rememberMissing(oid, diff);
    for (const [path, d] of split) {
      this.diffs.set(`${oid}:${path}`, d);
    }
    void this.loadSyntax(client, diff.json['view'], index, oid, split);
    while (this.diffs.size > DIFF_CACHE) {
      this.diffs.delete(this.diffs.keys().next().value!);
    }
    return {
      oid,
      parents: i['parents'] as string[],
      author: { name: String(i['author']), email: String(i['authorEmail']), date: Number(i['authorTime']) },
      committer: { name: String(i['committer']), email: String(i['committerEmail']), date: Number(i['committerTime']) },
      recordedAuthor: person(i['recordedAuthor']),
      recordedCommitter: person(i['recordedCommitter']),
      message: String(i['message']),
      files: commitFiles(diff),
      ...(missingFiles.length > 0 ? { missingFiles } : {}),
    };
  }

  /**
   * `commitDiff` の答えの、partial clone で中身が手元になく差分を取っていないファイルと、取る中身の ID を覚える。
   * そのファイルのパスを返す
   */
  private rememberMissing(key: string, diff: Reply): string[] {
    const ids = Array.isArray(diff.json['missing']) ? (diff.json['missing'] as string[]) : [];
    const paths = ((diff.json['files'] as { path: string; missing?: boolean }[] | undefined) ?? []).filter((f) => f.missing).map((f) => f.path);
    this.missing.delete(key);
    if (ids.length === 0 || paths.length === 0) return [];
    this.missing.set(key, { ids, paths: new Set(paths) });
    while (this.missing.size > DETAILS_CACHE) this.missing.delete(this.missing.keys().next().value!);
    return paths;
  }

  /** partial clone か（リポジトリの形を読んでいなければ読む） */
  private async isPartialClone(client: DaemonClient): Promise<boolean> {
    if (this.partialClone === undefined) {
      const reply = (await client.call({ op: 'run', command: { op: 'repositoryForm' } })).json;
      if (reply['ok'] !== true) return false;
      this.partialClone = !!(reply['info'] as { partial?: unknown } | undefined)?.partial;
    }
    return this.partialClone;
  }

  /** コミット `oid` のファイル `path` を見せるのに要る中身のうち、partial clone で手元にないもの（取りには行かない） */
  private async contentsMissing(client: DaemonClient, oid: string, path: string): Promise<string[]> {
    if (!(await this.isPartialClone(client))) return [];
    const reply = (await client.call({ op: 'run', command: { op: 'fileContentsMissing', oid, path } })).json;
    // 古いエンジンは知らない（そのときは今までどおり読む）
    if (reply['ok'] !== true) return [];
    const missing = (reply['info'] as { missing?: string[] } | undefined)?.missing;
    return Array.isArray(missing) ? missing : [];
  }

  /**
   * 「取ってくる」の帯が押された: 手元にない中身を（まとめて 1 回で）取り、差分なら読み直して送り直す。`path` があれば
   * 「すべてのファイル」のそのファイルの分（取れたら Webview が求め直す）
   */
  private async fetchContents(key: string, path?: string): Promise<void> {
    const reply = (error?: string) =>
      this.post({ type: 'contentsFetched', oid: key, ...(path !== undefined ? { path } : {}), ...(error !== undefined ? { error } : {}) });
    try {
      const client = await this.engine();
      if (!client) throw new Error('The Amon Hen engine isn’t running');
      const ids = path !== undefined ? await this.contentsMissing(client, key, path) : (this.missing.get(key)?.ids ?? []);
      if (ids.length > 0) {
        const fetched = (await client.call({ op: 'run', command: { op: 'fetchContents', ids } })).json;
        if (fetched['ok'] !== true) throw new Error(String(fetched['error'] ?? 'Can’t fetch the contents'));
      }
      if (path === undefined) await this.reloadDiff(key);
      // すべてのファイルで取った: 差分に中身のないファイルがあったなら、覚えた差分を捨てる（次に差分を出すときに
      // 読み直し、届いた中身のファイルの差分を出す。変更に戻ったときは Webview が `reloadDiff` で頼む）
      else if (this.missing.has(key)) this.forgetDiff(key);
      reply();
    } catch (err) {
      reply(errorMessage(err));
    }
  }

  /**
   * 中身を取った後で、コミット（か選択）の差分を読み直して送る（開いていたファイルは Webview が開いたままにする）。
   * コアは差分を求め直すだけで、取りには行かない
   */
  private async reloadDiff(key: string): Promise<void> {
    this.forgetDiff(key);
    const selection = this.selection;
    if (selection?.key === key) {
      await this.sendSelection(selection.token, selection.rows, selection.pair);
      return;
    }
    if (!selection && this.selectedOid === key) await this.sendDetails(key);
  }

  /** コミット（か選択の key）の、覚えた詳細・差分を捨てる（次に出すときに読み直す） */
  private forgetDiff(key: string): void {
    this.missing.delete(key);
    for (const k of [...this.sentDiffs]) if (k.startsWith(`${key}:`)) this.sentDiffs.delete(k);
    this.details.delete(key);
    for (const k of [...this.diffs.keys()]) if (k.startsWith(`${key}:`)) this.diffs.delete(k);
  }

  /**
   * コミットの差分の構文の色を後から頼み、行に付ける。もう送ったファイルは送り直す（ほかは送るときに色が付いている）。
   * 古いエンジンが断れば色なしのまま
   */
  private async loadSyntax(
    client: DaemonClient,
    view: unknown,
    index: ReadonlyMap<number, DiffLine>,
    key: string,
    diffs: ReadonlyMap<string, FileDiff>,
  ): Promise<void> {
    if (view === undefined || view === null) return;
    try {
      const touched = applySyntax(await client.call({ op: 'diffSyntax', view }), index);
      if (touched.size === 0) return;
      for (const [path, diff] of diffs) {
        if (this.sentDiffs.has(`${key}:${path}`) && diff.hunks.some((h) => h.lines.some((l) => touched.has(l)))) {
          this.post({ type: 'commitDiff', oid: key, path, diff });
        }
      }
    } catch {
      // 色なしのまま
    }
  }

  /**
   * コミットの中の画像の前（最初の親）と後を、data URL で送る。git から読むので、エンジンがなくても使える。大き
   * すぎるものは送らない（Remote-SSH では postMessage が SSH を通る）
   */
  private async sendCommitImage(oid: string, path: string): Promise<void> {
    // partial clone で中身が手元になければ、git で読む（取りに行く）前に断る（「取ってくる」の帯で取るまで）
    if (this.missing.get(oid)?.paths.has(path)) {
      this.post({ type: 'commitImage', oid, path, error: 'This file’s contents aren’t here yet.' });
      return;
    }
    const repo = this.repo;
    const details = this.details.get(oid);
    const file = details?.files.find((f) => f.path === path);
    if (!repo || !details || !file) return;
    const parent = details.parents[0];
    const read = async (rev: string): Promise<ImageBlob | undefined> => {
      const data = (await repo.backend.object(rev))?.data;
      if (!data) return undefined;
      if (data.length > MAX_IMAGE_BYTES) throw new Error(`The image is larger than ${MAX_IMAGE_BYTES / 1024 / 1024} MB`);
      return { url: `data:${imageType(path)};base64,${data.toString('base64')}`, bytes: data.length };
    };
    try {
      const before = file.status === 'A' || !parent ? undefined : await read(`${parent}:${file.origPath ?? file.path}`);
      const after = file.status === 'D' ? undefined : await read(`${oid}:${file.path}`);
      this.post({ type: 'commitImage', oid, path, ...(before ? { before } : {}), ...(after ? { after } : {}) });
    } catch (err) {
      this.post({ type: 'commitImage', oid, path, error: errorMessage(err) });
    }
  }

  /** コミットの木のフォルダの中身を送る（「すべてのファイル」。エンジンだけ） */
  private async sendTreeEntries(oid: string, path: string): Promise<void> {
    const client = this.daemon?.client;
    if (!client) return;
    const reply = await client.call({ op: 'run', command: { op: 'treeEntries', oid, path } });
    const info = reply.json['info'] as { entries?: TreeEntry[] } | undefined;
    if (reply.json['ok'] !== true || !info?.entries) {
      this.post({ type: 'treeEntries', oid, path, entries: [], error: String(reply.json['error'] ?? 'Can’t read the folder') });
      return;
    }
    this.post({ type: 'treeEntries', oid, path, entries: info.entries.map((e) => ({ name: e.name, kind: e.kind })) });
  }

  /**
   * コミットのファイルの中身を送る（「すべてのファイル」。エンジンだけ）。このコミットで変えたファイルは、ファイル
   * 全体の差分から、足した行・変えた行・消したところに印を付ける。画像は絵、Markdown は整形のブロック、SVG は絵と
   * モバイルの確認も。Markdown の図・数式、Android の VectorDrawable、SVG のモバイルの確認は、絵の手伝い
   * （`amonhen-render`）が `colors`（Webview の画面の色）で作る（手伝いがない・失敗したときは、コードとテキストのまま）。
   * 図の配色・gantt の時間帯・数式の大きさと時間の上限は設定から（時間帯が「OS の時間帯」なら Webview の `timeZone`）
   */
  private async sendTreeFile(oid: string, path: string, colors?: PictureColors, timeZone?: string): Promise<void> {
    const repo = this.repo;
    const client = this.daemon?.client;
    if (!repo || !client) return;
    // 絵の手伝いは、要るときに 1 度だけ起動する（同じコミットのほかのファイルは、求められたら読んで渡す）
    const at = this.details.get(oid)?.files.find((f) => f.path === path)?.status === 'D' ? this.details.get(oid)?.parents[0] : oid;
    let helper: Promise<RenderSession | undefined> | undefined;
    const render = async (op: string, fields: Record<string, unknown>): Promise<Record<string, unknown>> => {
      helper ??= this.startRender(client, repo, at ?? oid);
      const session = await helper;
      if (!session) throw new Error('No drawing helper');
      return session.request(op, fields);
    };
    try {
      const settings = await this.pictureSettings(repo.root, timeZone);
      await this.sendTreeFileWith(oid, path, render, colors, settings);
    } finally {
      void helper?.then((session) => session?.close());
    }
  }

  /** 図と数式の設定（アプリの Settings > Theme の Diagrams・Math と同じ） */
  private async pictureSettings(root: string, localZone: string | undefined): Promise<PictureSettings> {
    const config = vscode.workspace.getConfiguration('amonhen');
    return pictureSettings(
      {
        colors: config.get('diagrams.colors'),
        mermaidTheme: config.get('diagrams.mermaidTheme'),
        timeZone: config.get('diagrams.timeZone'),
        formulaSize: config.get('math.formulaSize'),
        timeLimit: config.get('math.timeLimit'),
      },
      { root, home: homedir(), read: (file) => readFile(file, 'utf8'), ...(localZone ? { localZone } : {}) },
    );
  }

  /** 読み込んだ Mermaid のテーマが使えないわけを知らせる（同じわけは 1 度だけ） */
  private warnTheme(error: string): void {
    if (this.themeWarned === error) return;
    this.themeWarned = error;
    void vscode.window.showWarningMessage(`${error}. Diagrams use the syntax colors.`);
  }

  private themeWarned: string | undefined;

  /** 絵の手伝いを起動する。同じコミット `oid` のファイル・フォルダの求めには、git とエンジンで読んで答える */
  private async startRender(client: DaemonClient, repo: Repository, oid: string): Promise<RenderSession | undefined> {
    const program = await renderPath().catch(() => undefined);
    if (!program) return undefined;
    return new RenderSession(
      program,
      {
        read: async (file) => {
          // partial clone で中身が手元にないファイルは、取りに行かずに読めないものとする
          if ((await this.contentsMissing(client, oid, file).catch(() => [])).length > 0) return undefined;
          const data = (await repo.backend.object(`${oid}:${file}`))?.data;
          return data && data.length <= MAX_IMAGE_BYTES ? data : undefined;
        },
        list: async (folder) => {
          const reply = await client.call({ op: 'run', command: { op: 'treeEntries', oid, path: folder } });
          const entries = (reply.json['info'] as { entries?: TreeEntry[] } | undefined)?.entries ?? [];
          return entries.map((e) => ({ name: e.name, folder: e.kind === 'folder' }));
        },
      },
      { language: daemonLanguage() },
    );
  }

  private async sendTreeFileWith(
    oid: string,
    path: string,
    render: (op: string, fields: Record<string, unknown>) => Promise<Record<string, unknown>>,
    colors: PictureColors | undefined,
    settings: PictureSettings,
  ): Promise<void> {
    const repo = this.repo;
    const client = this.daemon?.client;
    if (!repo || !client) return;
    const change = this.details.get(oid)?.files.find((f) => f.path === path);
    // このコミットで消したファイルは、前の版を出す（「このコミットで消した」と添える。アプリと同じ）
    const deleted = change?.status === 'D';
    const send = (view: FileView) => this.post({ type: 'treeFile', oid, path, view: deleted ? { ...view, deleted: true } : view });
    // partial clone で中身が手元になければ、git で読む（取りに行く）前に「取ってくる」の帯で聞く（押したときだけ取る）
    if ((await this.contentsMissing(client, oid, path)).length > 0) {
      send({ kind: 'missing' });
      return;
    }
    const data = (await repo.backend.object(`${oid}${deleted ? '^' : ''}:${path}`))?.data;
    if (!data) {
      send({ kind: 'error', message: `${path} isn’t in this commit` });
      return;
    }
    const svg = /\.svg$/i.test(path);
    if (/\.(png|jpe?g|gif|webp|bmp|ico|avif)$/i.test(path)) {
      if (data.length > MAX_IMAGE_BYTES) throw new Error(`The image is larger than ${MAX_IMAGE_BYTES / 1024 / 1024} MB`);
      send({ kind: 'image', image: { url: `data:${imageType(path)};base64,${data.toString('base64')}`, bytes: data.length } });
      return;
    }
    if (data.subarray(0, 8000).includes(0)) {
      send({ kind: 'binary', bytes: data.length });
      return;
    }
    const lines = change && !deleted ? await this.markedLines(client, oid, path) : undefined;
    const view: FileView = lines ?? { kind: 'text', lines: await this.plainLines(client, path, data.toString('utf8')) };
    if (view.kind !== 'text') {
      send(view);
      return;
    }
    // 消したファイルは前の版（最初の親のコミット）を読む
    const at = deleted ? this.details.get(oid)?.parents[0] : oid;
    if (/\.(md|markdown)$/i.test(path) && at) {
      const slug = repositorySlug(await repo.backend.remotes());
      const reply = await client.call({
        op: 'run',
        command: { op: 'markdownBlocks', oid: at, path, ...(slug ? { github: `https://github.com/${slug}` } : {}) },
      });
      const blocks = (reply.json['info'] as { blocks?: unknown[] } | undefined)?.blocks;
      if (blocks) {
        // 図と数式は絵の手伝いで描く（描けなかったものはコードのまま、構文の色を付ける）
        if (colors) {
          const drawn = await drawMarkdownPictures(blocks, path, colors, settings.options, render);
          const themeError = settings.themeError ?? drawn.themeError;
          if (themeError && hasDiagrams(blocks)) this.warnTheme(themeError);
        }
        view.markdown = await this.colorCode(client, await this.inlineImages(repo, client, at, path, blocks));
        view.mathScale = settings.mathScale;
      }
    }
    // Android の VectorDrawable は絵にする（引けない色は画面の文字の色で。描けなければテキストだけ。アプリと同じ）
    if (at && isVectorDrawable(path, data)) {
      const reply = await render('vectorDrawable', { path, data: data.toString('base64'), ...(colors ? { fallback: colors.text, dark: colors.dark } : {}) }).catch(
        () => undefined,
      );
      const info = reply as { svg?: string; notes?: string[] } | undefined;
      if (typeof info?.svg === 'string') {
        view.drawable = {
          image: { url: `data:image/svg+xml;base64,${Buffer.from(info.svg).toString('base64')}`, bytes: data.length },
          notes: info.notes ?? [],
        };
      }
    }
    if (svg && data.length <= MAX_IMAGE_BYTES) {
      const reply = await render('svgCheck', { data: data.toString('base64') }).catch(() => undefined);
      const check: SvgCheckView | undefined =
        reply && Array.isArray(reply['issues'])
          ? { issues: reply['issues'] as SvgCheckView['issues'], previews: (reply['previews'] ?? {}) as SvgCheckView['previews'] }
          : undefined;
      view.svg = { image: { url: `data:image/svg+xml;base64,${data.toString('base64')}`, bytes: data.length }, ...(check ? { check } : {}) };
    }
    send(view);
  }

  /**
   * Markdown のブロックの、コミットの中の絵（相対パス）を data URL に置き換える（外からは読まない。大きすぎるもの・
   * 読めないものはそのまま）
   */
  private async inlineImages(repo: Repository, client: DaemonClient, oid: string, path: string, blocks: unknown[]): Promise<unknown[]> {
    const dir = path.includes('/') ? path.slice(0, path.lastIndexOf('/') + 1) : '';
    let budget = MAX_IMAGE_BYTES;
    const visit = async (value: unknown): Promise<void> => {
      if (Array.isArray(value)) {
        for (const v of value) await visit(v);
        return;
      }
      if (!value || typeof value !== 'object') return;
      const run = value as Record<string, unknown>;
      // `<picture>` の暗い外観・明るい外観の画像も
      for (const key of ['image', 'image_dark', 'image_light']) {
        const image = run[key];
        if (typeof image !== 'string' || /^[a-z][a-z0-9+.-]*:|^\/\//i.test(image)) continue;
        const target = normalizePath(image.startsWith('/') ? image.slice(1) : `${dir}${image}`.split(/[?#]/)[0]!);
        // partial clone で中身が手元にない絵は、取りに行かずにそのまま（読めないものと同じ）
        const here = target ? (await this.contentsMissing(client, oid, target).catch(() => [])).length === 0 : false;
        const data = target && here ? (await repo.backend.object(`${oid}:${target}`).catch(() => undefined))?.data : undefined;
        if (data && data.length <= budget && /\.(png|jpe?g|gif|webp|bmp|ico|avif|svg)$/i.test(target!)) {
          budget -= data.length;
          run[key] = `data:${imageType(target!)};base64,${data.toString('base64')}`;
        }
      }
      for (const v of Object.values(run)) if (typeof v === 'object') await visit(v);
    };
    await visit(blocks);
    return blocks;
  }

  /** Markdown のコードブロックに、フェンスの言葉の構文の色を付ける（`syntax`: 行ごと。読めなければ色なし） */
  private async colorCode(client: DaemonClient, blocks: unknown[]): Promise<unknown[]> {
    const visit = async (value: unknown): Promise<void> => {
      if (Array.isArray(value)) {
        for (const v of value) await visit(v);
        return;
      }
      if (!value || typeof value !== 'object') return;
      const block = value as Record<string, unknown>;
      if (block['kind'] === 'code' && typeof block['language'] === 'string' && block['language'] && typeof block['text'] === 'string') {
        const texts = block['text'].replace(/\n$/, '').split('\n');
        const colored = await client
          .call({ op: 'textSyntax', path: `code.${block['language']}`, text: texts.join('\n') })
          .catch(() => undefined);
        if (colored) block['syntax'] = textSyntaxByLine(colored, texts);
        return;
      }
      for (const v of Object.values(block)) if (typeof v === 'object') await visit(v);
    };
    await visit(blocks);
    return blocks;
  }

  /** 変えたファイルの、ファイル全体の差分から印を付けた行（読めなければ undefined） */
  private async markedLines(client: DaemonClient, oid: string, path: string): Promise<FileView | undefined> {
    const index = this.fileOrder.get(oid)?.indexOf(path) ?? -1;
    if (index < 0) return undefined;
    const reply = await client.call({ op: 'commitDiff', id: oid, contexts: { [index]: 'all' }, ...this.whitespaceArg() });
    const byRow = new Map<number, DiffLine>();
    const diff = splitCommitDiff(reply, byRow).get(path);
    if (!diff || diff.binary) return undefined;
    const colored = await client.call({ op: 'diffSyntax', view: reply.json['view'] }).catch(() => undefined);
    if (colored) applySyntax(colored, byRow);
    const lines: ViewLine[] = [];
    let removed: string[] = [];
    for (const line of diff.hunks.flatMap((h) => h.lines)) {
      if (line.kind === '-') {
        removed.push(line.text);
        continue;
      }
      const view: ViewLine = { text: line.text, ...(line.syntax ? { syntax: line.syntax } : {}) };
      if (line.kind === '+') {
        const old = removed.shift();
        if (old !== undefined) {
          view.mark = 'changed';
          view.old = old;
        } else {
          view.mark = 'added';
        }
      } else if (removed.length > 0) {
        view.removedBefore = removed.length;
        removed = [];
      }
      lines.push(view);
    }
    return { kind: 'text', lines, ...(removed.length > 0 ? { removedAtEnd: removed.length } : {}) };
  }

  /** 変えていないファイルの行（構文の色つき。読めなければ色なし） */
  private async plainLines(client: DaemonClient, path: string, text: string): Promise<ViewLine[]> {
    const texts = text.replace(/\n$/, '').split('\n');
    const colored = await client.call({ op: 'textSyntax', path, text: texts.join('\n') }).catch(() => undefined);
    const syntax = colored ? textSyntaxByLine(colored, texts) : undefined;
    return texts.map((t, i) => (syntax?.[i]?.length ? { text: t, syntax: syntax[i] } : { text: t }));
  }

  /** 詳細の差分で選べること（エンジンで読んでいるときだけ） */
  private choices(): DiffChoices {
    return { whitespace: this.whitespace };
  }

  /** `commitDiff` に渡す、無視する空白（無視しなければなし） */
  private whitespaceArg(): { whitespace?: DiffWhitespace } {
    return this.whitespace === 'exact' ? {} : { whitespace: this.whitespace };
  }

  private rememberOrder(key: string, files: unknown): void {
    this.fileOrder.set(key, ((files as { path: string }[] | undefined) ?? []).map((f) => f.path));
    while (this.fileOrder.size > DETAILS_CACHE) this.fileOrder.delete(this.fileOrder.keys().next().value!);
  }

  /** 無視する空白を選び直した: 覚えた差分を捨て、出している詳細（か選んだ行の差分）を読み直す */
  private async setWhitespace(whitespace: DiffWhitespace): Promise<void> {
    if (whitespace === this.whitespace) return;
    this.whitespace = whitespace;
    this.details.clear();
    this.diffs.clear();
    this.sentDiffs.clear();
    this.missing.clear();
    const selection = this.selection;
    if (selection) {
      await this.sendSelection(selection.token, selection.rows, selection.pair);
    } else if (this.selectedOid) {
      await this.sendDetails(this.selectedOid);
    }
  }

  /** そのファイルの差分を、前後 `context` 行（all ならファイル全体）で読み直して送る（エンジンだけ） */
  private async sendFileContext(oid: string, path: string, context: number | 'all'): Promise<void> {
    const daemon = this.daemon;
    const index = this.fileOrder.get(oid)?.indexOf(path) ?? -1;
    if (!daemon || index < 0) return;
    const selection = this.selection?.key === oid ? this.selection : undefined;
    const target = selection ? { history: daemon.history, rows: selection.rows, pair: selection.pair } : { id: oid };
    const reply = await daemon.client.call({ op: 'commitDiff', ...target, contexts: { [index]: context }, ...this.whitespaceArg() });
    const lines = new Map<number, DiffLine>();
    const split = splitCommitDiff(reply, lines);
    const diff = split.get(path);
    if (!diff) return;
    if (selection) selection.diffs.set(path, diff);
    else this.diffs.set(`${oid}:${path}`, diff);
    this.sentDiffs.add(`${oid}:${path}`);
    this.post({ type: 'commitDiff', oid, path, diff });
    void this.loadSyntax(daemon.client, reply.json['view'], lines, oid, new Map([[path, diff]]));
  }

  private async sendCommitDiff(oid: string, path: string): Promise<void> {
    // 送ったものを覚える（構文の色が届いたら送り直す）。増えすぎたら古いものから忘れる
    this.sentDiffs.add(`${oid}:${path}`);
    while (this.sentDiffs.size > 4 * DIFF_CACHE) this.sentDiffs.delete(this.sentDiffs.values().next().value!);
    if (oid === this.selection?.key) {
      const diff = this.selection.diffs.get(path);
      if (diff) this.post({ type: 'commitDiff', oid, path, diff });
      return;
    }
    const repo = this.repo;
    const details = this.details.get(oid);
    const file = details?.files.find((f) => f.path === path);
    if (!repo || !details || !file) {
      return;
    }
    const key = `${oid}:${path}`;
    let diff = this.diffs.get(key);
    // partial clone で中身が手元にないファイルは、git で読む（取りに行く）代わりに空の差分（Webview が帯で聞く）
    if (!diff && this.missing.get(oid)?.paths.has(path)) diff = { path, version: 'missing', hunks: [] };
    if (!diff) {
      const parent = details.parents[0];
      const object = async (rev: string) => (await repo.backend.object(rev))?.data;
      const old = file.status === 'A' || !parent ? undefined : await object(`${parent}:${file.origPath ?? file.path}`);
      const now = file.status === 'D' ? undefined : await object(`${oid}:${file.path}`);
      diff = computeDiff(path, old, now).diff;
      this.diffs.set(key, diff);
      if (this.diffs.size > DIFF_CACHE) {
        this.diffs.delete(this.diffs.keys().next().value!);
      }
    }
    this.post({ type: 'commitDiff', oid, path, diff });
  }

  /**
   * oid のコミットを選んで見せる（Reflog の Show in History）。今の範囲になければ All Branches にして探す。
   * 共有の daemon の History だけ。見つかったか返す（一致だけの History で、一致しなければ見つからない）
   */
  async reveal(oid: string): Promise<boolean> {
    const rowOf = async (): Promise<number | undefined> => {
      const shown = this.daemon;
      if (!shown) return undefined;
      const row = (await shown.client.call({ op: 'historyRowOf', history: shown.history, id: oid })).json['row'];
      return typeof row === 'number' ? row : undefined;
    };
    await this.settled();
    let row = await rowOf();
    if (row === undefined && this.scope !== 'all') {
      this.scope = 'all';
      void this.reset();
      await this.settled();
      row = await rowOf();
    }
    if (row === undefined) return false;
    this.post({ type: 'reveal', row });
    return true;
  }

  /** 読み直しを待たせていれば今すぐ読み直し、すべて終わるまで待つ（結合テストが、試験の間で使う） */
  async idle(): Promise<void> {
    await this.settled();
  }

  /**
   * 読み直しが終わるまで待つ。HEAD やブランチが動いた直後で読み直しを待たせていれば、今すぐ読み直す。
   * 待つ間に次の読み直しが始まれば、それも待つ
   */
  private async settled(): Promise<void> {
    if (this.resetTimer !== undefined) {
      clearTimeout(this.resetTimer);
      this.resetTimer = undefined;
      void this.reset();
    }
    let generation: number;
    do {
      generation = this.generation;
      await this.resetting.catch(() => undefined);
    } while (generation !== this.generation);
  }

  /** 最初から読み直す */
  reset(): Promise<void> {
    this.resetting = this.reload();
    return this.resetting;
  }

  private async reload(): Promise<void> {
    const generation = ++this.generation;
    this.clearSelection();
    this.extras.clear();
    this.commits = [];
    this.known.clear();
    this.graph = new GraphBuilder();
    this.done = false;
    this.loading = false;
    const repo = this.repo;
    this.opening = repo !== undefined;
    this.post({ type: 'reset', title: repo ? basename(repo.root) : '', scope: this.scope });
    this.post({ type: 'historyDisplay', hidden: historyHidden(), oneLine: historyOneLine() });
    if (!repo) {
      this.post({ type: 'rows', start: 0, rows: [], done: true });
      return;
    }
    this.readingRefs++;
    let refsRead = false;
    const doneReadingRefs = () => {
      if (!refsRead) {
        refsRead = true;
        this.readingRefs--;
      }
    };
    try {
      await repo.want('tags');
      const head = await repo.backend.head();
      const state = repo.current;
      this.headOid = head.oid;
      doneReadingRefs();
      this.labels = refLabels(head, state.branches ?? [], state.remoteBranches ?? [], state.tags ?? []);
      this.chain = head.oid
        ? editableChain(await repo.backend.log(['--first-parent', 'HEAD'], 0, EDITABLE_LIMIT))
        : new Map();
    } catch (err) {
      showError(err);
    } finally {
      doneReadingRefs();
    }
    // 待つ間に新しい読み直しが始まっていれば、そちらが開いた History を閉じない
    if (generation !== this.generation) {
      return;
    }
    this.searchAbort?.abort();
    for (const previous of [this.daemon, this.base]) {
      void previous?.client.call({ op: 'historyRelease', history: previous.history }).catch(() => undefined);
    }
    this.daemon = undefined;
    this.base = undefined;
    try {
      const opened = this.headOid ? await this.openDaemonHistory(repo.root) : undefined;
      if (generation !== this.generation) {
        return;
      }
      this.daemon = opened;
    } catch (err) {
      // daemon を使えなければ、git で読む
      showError(err);
    }
    if (generation !== this.generation) {
      return;
    }
    this.post({ type: 'searchable', searchable: this.daemon !== undefined });
    if (this.daemon) void this.sendForm(this.daemon, generation).catch(() => undefined);
    // 札を付けてから行を送る。それまでは開いている途中のまま（Webview が求めても、札のない行を先に送らない）
    await this.addWorktreeLabels(repo, generation);
    // 札を待つ間に新しい読み直しが始まっていれば、その History の行を送らない
    if (generation !== this.generation) {
      return;
    }
    this.opening = false;
    await this.loadMore();
    // 探していたら（途中でも）、新しい History で探し直す
    const query = this.searching ?? this.search?.query;
    if (query && this.daemon && generation === this.generation) {
      await this.runSearch(query, this.matchingOnly, 0);
    }
  }

  /**
   * 探して、一致を Webview に送る。`matchingOnly` なら一致だけの History に切り替える。`selected` は Webview で
   * 選んでいる行で、History を切り替えるときは同じコミット（なければ近いもの）を選んだままにする。空なら検索をやめる
   */
  private async runSearch(query: string, matchingOnly: boolean, selected: number): Promise<void> {
    this.searchAbort?.abort();
    const abort = new AbortController();
    this.searchAbort = abort;
    this.searching = query || undefined;
    this.matchingOnly = matchingOnly;
    try {
      await this.searchOnce(query, matchingOnly, selected, abort);
    } finally {
      if (this.searchAbort === abort && !abort.signal.aborted) {
        this.searching = undefined;
      }
    }
  }

  private async searchOnce(
    query: string,
    matchingOnly: boolean,
    selected: number,
    abort: AbortController,
  ): Promise<void> {
    const shown = this.daemon;
    if (!shown) {
      return;
    }
    const base = this.base ?? shown;
    // もとの History での、選んでいる行
    const baseSelected = this.base ? (this.search?.rows[selected] ?? 0) : selected;
    const post = (state: Partial<SearchState>) =>
      this.post({
        type: 'search',
        state: { query, rows: [], searching: false, done: 0, matchingOnly: false, ...state },
      });
    if (!query) {
      this.search = undefined;
      await this.showMatching(base, undefined, baseSelected, abort.signal);
      post({});
      return;
    }
    let rows: number[];
    if (this.search?.query === query && this.search.history === base.history) {
      // 一致だけを出す切り替えだけなら、探し直さない
      rows = this.search.rows;
    } else {
      try {
        if (searchesContents(query)) {
          post({ searching: true });
          const need = await contentsToFetch(base.client, base.history, query, abort.signal);
          if (abort.signal.aborted) {
            return;
          }
          if (need.count > 0 && !(await confirmContentsSearch(need.count, need.more))) {
            if (this.searchAbort === abort) {
              await this.runSearch('', false, selected);
            }
            return;
          }
          if (abort.signal.aborted) {
            return;
          }
        }
        post({ searching: true });
        rows = await searchHistory(base.client, base.history, query, abort.signal, (partial, done) =>
          post({ rows: this.base ? [] : partial, searching: true, done, matchingOnly: this.base !== undefined }),
        );
      } catch (err) {
        if (!abort.signal.aborted) {
          post({ error: errorMessage(err) });
        }
        return;
      }
    }
    if (abort.signal.aborted) {
      return;
    }
    this.search = { query, history: base.history, rows };
    try {
      await this.showMatching(base, matchingOnly && rows.length > 0 ? rows : undefined, baseSelected, abort.signal);
    } catch (err) {
      post({ error: errorMessage(err) });
      return;
    }
    if (!abort.signal.aborted) {
      post({ rows, matchingOnly: this.base !== undefined });
    }
  }

  /**
   * `rows` があれば、`base` のその行だけの History（一致だけ）を、なければ `base` を出す。`selected` は `base` での
   * 選んでいる行
   */
  private async showMatching(
    base: DaemonHistory,
    rows: number[] | undefined,
    selected: number,
    signal: AbortSignal,
  ): Promise<void> {
    const filtered = this.base ? this.daemon : undefined;
    let next = base;
    let select = selected;
    if (rows) {
      const reply = await base.client.call({ op: 'historyMatching', history: base.history, rows });
      // 待つ間に読み直しや次の検索が始まっていれば、作った History は使わない（今の History を取り違える）
      if (signal.aborted) {
        void base.client.call({ op: 'historyRelease', history: reply.json['history'] }).catch(() => undefined);
        return;
      }
      next = { client: base.client, history: reply.json['history'], rows: Number(reply.json['rows']) };
      select = lowerBound(rows, selected);
    } else if (!filtered) {
      return;
    }
    if (filtered) {
      void filtered.client.call({ op: 'historyRelease', history: filtered.history }).catch(() => undefined);
    }
    this.base = rows ? base : undefined;
    this.daemon = next;
    const repo = this.repo;
    this.post({ type: 'reset', title: repo ? basename(repo.root) : '', scope: this.scope, select });
    const start = Math.floor(select / PAGE) * PAGE;
    await this.loadRange(next, start, PAGE);
  }

  /** 選んだコミットの祖先を送る */
  private async sendLineage(): Promise<void> {
    const request = ++this.lineageRequest;
    const shown = this.daemon;
    if (!this.highlighting || !shown || !this.selectedOid) {
      this.post({ type: 'lineage', rows: null });
      return;
    }
    let reply;
    try {
      const at = await shown.client.call({ op: 'historyRowOf', history: shown.history, id: this.selectedOid });
      const row = at.json['row'];
      if (typeof row !== 'number') return;
      reply = await shown.client.call({ op: 'historyLineage', history: shown.history, row });
    } catch (err) {
      // 読む間に読み直しが History を入れ替えて片付けたなら、新しい History で頼み直されるので捨てる
      if (shown !== this.daemon) return;
      throw err;
    }
    if (request === this.lineageRequest && shown === this.daemon) {
      this.post({
        type: 'lineage',
        rows: Buffer.from(reply.bytes('rows')).toString('base64'),
        edges: Buffer.from(reply.bytes('edgeFrom')).toString('base64'),
      });
    }
  }

  /** リポジトリの畳み方を読む（開き直したリポジトリなら。読めなければ何も畳まない） */
  private async loadFold(client: DaemonClient): Promise<void> {
    const root = this.repo?.root;
    if (!root || this.foldLoadedFor === root) return;
    this.foldLoadedFor = root;
    this.foldDefault = false;
    this.foldExcept.clear();
    try {
      const reply = (await client.call({ op: 'run', command: { op: 'foldState' } })).json;
      const info = reply['info'] as { folded?: boolean; except?: string[] } | undefined;
      this.foldDefault = info?.folded === true;
      for (const id of info?.except ?? []) this.foldExcept.add(id);
    } catch {
      // 古いエンジン。畳まない
    }
  }

  /** 畳み方を覚える（アプリ・TUI と同じファイル）。覚えられなくても、この窓では効く */
  private async saveFold(): Promise<void> {
    const client = await this.engine();
    if (!client) return;
    await client
      .call({ op: 'run', command: { op: 'setFoldState', folded: this.foldDefault, except: [...this.foldExcept] } })
      .catch(() => undefined);
  }

  /** merge を 1 つ開く・畳む。覚えて、読み直しても同じコミットを選んだままにする */
  async toggleFold(oid: string): Promise<void> {
    if (!this.foldExcept.delete(oid)) this.foldExcept.add(oid);
    await this.saveFold();
    await this.reset();
    // 読み直しの途中で次の読み直しが始まっていれば、それも待ってから選ぶ
    await this.settled();
    const shown = this.daemon;
    if (!shown) return;
    const row = (await shown.client.call({ op: 'historyRowOf', history: shown.history, id: oid })).json['row'];
    if (typeof row === 'number') this.post({ type: 'reveal', row });
  }

  /** 並び順と、行に出す項目を選ぶ（アプリの History Display と同じ） */
  private async displayMenu(): Promise<void> {
    // エンジンだけの項目は、エンジンを使えれば出す（読み直しの途中で、開いた History がまだなくても）
    const engine = this.daemon !== undefined || (await this.engine()) !== undefined;
    const order = historyOrder();
    const hidden = historyHidden();
    type Item = vscode.QuickPickItem & { order?: HistoryOrder; item?: HistoryItem };
    const mark = (on: boolean) => (on ? '$(check) ' : '$(blank) ');
    const separator = (label: string): Item => ({ label, kind: vscode.QuickPickItemKind.Separator });
    const orders: [HistoryOrder, string, string][] = [
      ['date', 'Date', 'Newest first'],
      ['topo', 'Topological', 'Keep a branch’s commits together under its merge'],
    ];
    const items: [HistoryItem, string][] = [
      ['refs', 'Branches and Tags'],
      ['author', 'Author'],
      ['date', 'Date'],
      ['oid', 'Commit ID'],
      ['signature', 'Signature'],
    ];
    const highlight: Item = {
      label: `${mark(this.highlighting)}Highlight Ancestors`,
      description: 'Full color only for the selected commit’s ancestors',
    };
    // merge の畳み方は、すべて畳む・すべて開く（1 つずつは右クリックか ← →。アプリと同じ）
    const foldAll: Item = { label: 'Fold All Merged Commits', description: 'Hide the commits each merge brought in' };
    const showAll: Item = { label: 'Show All Merged Commits' };
    const monospace: Item = { label: `${mark(monospacedMessage())}Monospaced Commit Message` };
    const tree: Item = { label: `${mark(detailsLayout().tree)}Changed Files as Tree` };
    const oneLine: Item = { label: `${mark(historyOneLine())}One Line per Commit` };
    const picked = await vscode.window.showQuickPick<Item>(
      [
        // 並び順は共有の daemon で変える（git で読むときは日付順のまま）
        ...(engine
          ? [
              separator('Order'),
              ...orders.map(([value, label, detail]): Item => ({
                label: `${mark(order === value)}${label}`,
                description: detail,
                order: value,
              })),
            ]
          : []),
        separator('Items'),
        ...items.map(([value, label]): Item => ({ label: `${mark(!hidden.includes(value))}${label}`, item: value })),
        separator('View'),
        oneLine,
        ...(engine ? [separator('Merged Commits'), foldAll, showAll, separator('Ancestors'), highlight] : []),
        // コミット情報（右の列）に効くもの（アプリの表示のメニューの Commit Details）
        separator('Commit Details'),
        monospace,
        tree,
      ],
      { placeHolder: 'History Display' },
    );
    if (picked === monospace) {
      await setMonospacedMessage(!monospacedMessage());
      this.post({ type: 'messageFont', monospace: monospacedMessage() });
    } else if (picked === tree) {
      const layout = { tree: !detailsLayout().tree };
      await setDetailsLayout(layout);
      this.post({ type: 'detailsLayout', layout });
    } else if (picked === oneLine) {
      await setHistoryOneLine(!historyOneLine());
      this.post({ type: 'historyDisplay', hidden, oneLine: historyOneLine() });
    } else if (picked === highlight) {
      this.highlightChoice = !this.highlighting;
      void setHistoryHighlight(this.highlightChoice);
      await this.sendLineage();
    } else if (picked === foldAll || picked === showAll) {
      // すべて畳む・すべて開く（1 つずつ開いた・畳んだものも忘れる。アプリと同じ）
      this.foldDefault = picked === foldAll;
      this.foldExcept.clear();
      await this.saveFold();
      await this.reset();
    } else if (picked?.order && picked.order !== order) {
      await setHistoryOrder(picked.order);
      await this.reset();
    } else if (picked?.item) {
      const next = hidden.includes(picked.item) ? hidden.filter((i) => i !== picked.item) : [...hidden, picked.item];
      await setHistoryHidden(next);
      this.post({ type: 'historyDisplay', hidden: next, oneLine: historyOneLine() });
    }
  }

  /** worktree がいくつもあれば、それぞれの HEAD の札を足す（読めなければ出さない） */
  private async addWorktreeLabels(repo: Repository, generation: number): Promise<void> {
    const client = this.daemon?.client;
    if (!client) return;
    try {
      const listed = (await client.call({ op: 'run', command: { op: 'worktrees' } })).json['info'] as
        | { worktrees?: Worktree[] }
        | undefined;
      const worktrees = (listed?.worktrees ?? []).filter((w) => !w.bare);
      if (worktrees.length < 2) return;
      const paths = worktrees.filter((w) => w.prunable === null).map((w) => w.path);
      const counted = (await client.call({ op: 'run', command: { op: 'worktreeChanges', paths } })).json['info'] as
        | { worktrees?: { path: string; count: number | null }[] }
        | undefined;
      if (generation !== this.generation) return;
      const changes = new Map((counted?.worktrees ?? []).map((w) => [w.path, w.count ?? 0]));
      const labels = worktreeLabels(worktrees, changes, (w) => samePath(w.path, repo.root));
      for (const [oid, added] of labels) this.labels.set(oid, [...added, ...(this.labels.get(oid) ?? [])]);
    } catch {
      // 札は出せなくてもよい
    }
  }

  /** 共有の daemon で、今の範囲の History を開く（daemon を使わない設定なら undefined） */
  private async openDaemonHistory(root: string): Promise<DaemonHistory | undefined> {
    const client = await daemonFor(root);
    if (!client) {
      return undefined;
    }
    let tips: string[] | undefined;
    if (this.scope === 'head') {
      tips = [this.headOid!];
    } else {
      const refs = scopeRefs(this.scope);
      if (refs) {
        // ブランチを 1 つ選んだら、その上流も一緒に辿る（手元と上流の分かれ方が見える。アプリと同じ）
        const upstream = refs.length === 1 ? this.repo?.current.branches?.find((b) => b.name === refs[0])?.upstream : undefined;
        const revs = upstream ? [...refs, upstream] : refs;
        const resolved = await Promise.all(
          revs.map((rev) => client.call({ op: 'run', command: { op: 'resolveCommit', rev } }).catch(() => undefined)),
        );
        tips = resolved.flatMap((r) => (r && r.json['ok'] !== false && r.json['text'] ? [String(r.json['text'])] : []));
      }
    }
    await client.call({ op: 'historyOrder', order: historyOrder() });
    await this.loadFold(client);
    const except = [...this.foldExcept];
    const history = await client.call({
      op: 'history',
      ...(tips ? { tips } : {}),
      ...(this.foldDefault ? { fold: true, expanded: except } : except.length > 0 ? { collapsed: except } : {}),
    });
    // 畳んだ数は History ごとに持つ（古い読み直しが新しい History の分を書き換えないように）
    const folded =
      history.json['folded'] === true
        ? (await client.call({ op: 'historyGraph', history: history.json['history'] })).u32('folded')
        : undefined;
    const boundary = history.json['shallow'] === true ? ((history.json['boundaryRows'] as number[] | undefined) ?? []) : undefined;
    return {
      client,
      history: history.json['history'],
      rows: Number(history.json['rows']),
      ...(boundary ? { boundary } : {}),
      ...(folded ? { folded } : {}),
    };
  }

  /**
   * 共有の daemon の History の、start から count 行を送る。行の数を知っているので、Webview は見えているところだけを
   * 飛び飛びに頼む（離れた一致へもすぐ移れる）
   */
  private async loadRange(daemon: DaemonHistory, start: number, count: number): Promise<void> {
    let rows: RowData[] = [];
    let toVerify: { oid: string; signed?: boolean }[] = [];
    const generation = this.generation;
    try {
      const reply = await daemon.client.call({ op: 'historyRows', history: daemon.history, start, count });
      // 読む間に新しい読み直しが始まっていれば、古い History の行は送らない（新しい reset の後に届いてしまう）
      if (daemon !== this.daemon || generation !== this.generation) {
        return;
      }
      const page = readRows(reply);
      for (const r of page) {
        this.known.set(r.oid, { oid: r.oid, parents: r.parentOids, author: r.author, email: '', date: r.date, subject: r.subject });
      }
      rows = toRowData(page, this.labels, this.chain);
      // 署名のある行だけ確かめる（アプリと同じ）。署名の欄を返さない古いエンジンなら全部の行
      toVerify = page.filter((r) => r.signed !== false).map((r) => ({ oid: r.oid, signed: r.signed }));
      rows.forEach((row, i) => {
        const hidden = daemon.folded?.[start + i];
        if (hidden) row.hidden = hidden;
        // 開いている merge は畳める（畳んで隠すものがあるかは、畳んでみるまで分からない）
        else if ((page[i]?.parentOids.length ?? 0) > 1) row.expanded = true;
      });
    } catch (err) {
      if (daemon !== this.daemon || generation !== this.generation) {
        return;
      }
      showError(err);
    }
    this.post({ type: 'rows', start, rows, done: true, total: daemon.rows });
    // 届けた行の署名を、裏で確かめて印を送る
    void this.verifyRows(daemon, toVerify).catch(() => undefined);
  }

  private async loadMore(): Promise<void> {
    const repo = this.repo;
    if (!repo || this.loading || this.done || this.opening) {
      return;
    }
    if (this.daemon) {
      this.done = true;
      return this.loadRange(this.daemon, 0, PAGE);
    }
    this.loading = true;
    const generation = this.generation;
    const revs =
      this.scope === 'all'
        ? ['HEAD', '--branches', '--remotes', '--tags']
        : (scopeRefs(this.scope) ?? ['HEAD']);
    let page: Commit[] = [];
    try {
      page = this.headOid ? await repo.backend.log(revs, this.commits.length, PAGE) : [];
    } catch (err) {
      showError(err);
    }
    if (generation !== this.generation) {
      return;
    }
    const start = this.commits.length;
    this.commits.push(...page);
    for (const c of page) {
      this.known.set(c.oid, c);
    }
    const rows: RowData[] = page.map((c) => {
      const chain = this.chain.get(c.oid);
      return {
        oid: c.oid,
        subject: c.subject,
        author: c.author,
        date: c.date,
        refs: this.labels.get(c.oid) ?? [],
        graph: this.graph.add(c.oid, c.parents),
        editable: chain !== undefined,
        chain,
        canSquash: chain !== undefined && this.chain.size > chain + 1,
      };
    });
    this.done = page.length < PAGE;
    this.loading = false;
    this.post({ type: 'rows', start, rows, done: this.done });
  }

  dispose(): void {
    this.signatureConfigListener.dispose();
    clearTimeout(this.resetTimer);
    this.searchAbort?.abort();
    this.repoListener?.dispose();
  }
}

/** 検証の結果を、行と詳細に出す形に。SSH の鍵は指紋が SHA256: で始まる（allowed signers に載っていれば信用している） */
/** 詳細（Full）の項目（アプリの CommitHeader の renderDetailed と同じもの） */
function fullExtras(
  info: {
    headRev?: string | null;
    branches?: string[];
    tags?: string[];
    tagCount?: number;
    previousTag?: string | null;
    originUrl?: string | null;
    remoteBranches?: string[];
    merged?: CommitSummaryView[];
    mergedCount?: number;
    references?: Record<string, CommitSummaryView>;
  },
  message: string,
): FullExtras {
  const short = (ref: string) => ref.replace(/^refs\/(heads|remotes|tags)\//, '');
  // メッセージに書かれたコミットを、書かれた順に、書かれ方と一緒に
  const related = Object.entries(info.references ?? {})
    .map(([token, commit]) => {
      const at = message.indexOf(token);
      if (at < 0) return undefined;
      const start = message.lastIndexOf('\n', at) + 1;
      const end = message.indexOf('\n', at);
      const line = message.slice(start, end < 0 ? undefined : end).toLowerCase();
      const kind = line.includes('cherry picked from')
        ? 'Cherry-picked from'
        : line.includes('reverts commit')
          ? 'Reverts'
          : line.startsWith('fixes:')
            ? 'Fixes'
            : 'Mentions';
      return { at, kind, commit };
    })
    .filter((r) => r !== undefined)
    .sort((a, b) => a.at - b.at)
    .map(({ kind, commit }) => ({ kind, commit }));
  return {
    ...(info.headRev ? { headRev: info.headRev } : {}),
    branches: (info.branches ?? []).map(short),
    tags: (info.tags ?? []).map(short),
    tagCount: info.tagCount ?? 0,
    ...(info.previousTag ? { previousTag: short(info.previousTag) } : {}),
    ...(info.originUrl ? { pushed: (info.remoteBranches ?? []).map(short) } : {}),
    merged: (info.merged ?? []).slice(0, 20),
    mergedCount: info.mergedCount ?? 0,
    related,
  };
}

function signatureView(check: SignatureCheck, committerEmail: string, signed?: string | null): SignatureView {
  // 形式はコミット・タグの署名の頭で分かる（なければ指紋の形から SSH を推す）
  const kind = signed ?? (check.fingerprint?.startsWith('SHA256:') ? 'ssh' : undefined);
  const format = kind === 'ssh' ? 'SSH' : kind === 'x509' ? 'X.509' : kind === 'gpg' ? 'GPG' : undefined;
  const status = signatureStatus(check, committerEmail, kind);
  return {
    kind: status.kind,
    ...(status.trust !== undefined ? { trust: status.trust } : {}),
    label: signatureLabel(status),
    explanation: signatureExplanation(status),
    ...(check.signer ? { signer: check.signer } : {}),
    ...(check.key ? { key: check.key } : {}),
    ...(format ? { format } : {}),
    ...(check.fingerprint ? { fingerprint: check.fingerprint } : {}),
  };
}
