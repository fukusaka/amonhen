import { randomBytes } from 'node:crypto';
import { basename } from 'node:path';
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import type { HostMessage, Sheet, SheetAnswer, ViewName, WebviewMessage } from '../view/protocol.ts';
import { BlameController } from './blameController.ts';
import { DaemonController } from './daemonController.ts';
import { FileHistoryController } from './fileHistoryController.ts';
import { HistoryController } from './historyController.ts';
import { detailsLayout, detailsStyle, monospacedMessage, setDetailsStyle } from './preferences.ts';
import type { DetailsStyle } from '../history/protocol.ts';
import { RangeDiffController } from './rangeDiffController.ts';
import { ReflogController } from './reflogController.ts';
import { SettingsController } from './settingsController.ts';
import { showError } from './stageCommands.ts';
import { filesToOpenTogether, WorkingCopyController } from './workingCopyController.ts';

/** シートの Check Connection: 欄の URL へつないでみて、結果の文とつながったかを返す */
export type SheetCheck = (url: string) => Promise<{ text: string; ok: boolean }>;

/**
 * メインビュー。エディタ領域の 1 つの Webview に、Working Copy・History・Reflog・Range Diff・Daemon などを出す。
 * どちらを出すかはサイドバーのナビで選ぶ。こちらは画面ごとのコントローラーにメッセージを振り分ける。
 */
/** 長い行を折り返すか（設定 `amonhen.wrapLongLines`） */
function wrapLongLines(): boolean {
  return vscode.workspace.getConfiguration('amonhen').get<boolean>('wrapLongLines', false);
}

/** 長い行の折り返しを切り替える（コマンド Wrap Long Lines、Alt+Z、差分の帯の ↩︎） */
export async function toggleWrapLongLines(): Promise<void> {
  await vscode.workspace.getConfiguration('amonhen').update('wrapLongLines', !wrapLongLines(), vscode.ConfigurationTarget.Global);
}

export class MainPanel implements vscode.Disposable {
  private static current: MainPanel | undefined;

  readonly history: HistoryController;
  readonly workingCopy: WorkingCopyController;
  readonly reflog: ReflogController;
  readonly rangeDiff: RangeDiffController;
  readonly fileHistory: FileHistoryController;
  readonly blame: BlameController;
  readonly settings: SettingsController;
  readonly daemon: DaemonController;
  private readonly panel: vscode.WebviewPanel;
  private readonly manager: RepositoryManager;
  private readonly disposables: vscode.Disposable[] = [];
  private repo: Repository | undefined;
  private ready = false;
  /** 準備ができたら行うこと（開いたばかりのときのコマンド） */
  private readonly whenReadyQueue: (() => void)[] = [];
  /** 準備ができる前に求められた画面 */
  private pendingView: ViewName | undefined;
  /** 答えを待っている確認のシート */
  private readonly sheets = new Map<
    number,
    { resolve: (answer: SheetAnswer | undefined) => void; open?: (item: number) => void; check?: SheetCheck }
  >();
  private nextSheet = 1;
  /** e2e が画面を押さずにシートに答える（あれば、シートを出さずにこれの答えを返す） */
  answerSheet:
    | ((sheet: Omit<Sheet, 'id'>, open?: (item: number) => void) => SheetAnswer | undefined | Promise<SheetAnswer | undefined>)
    | undefined;

  /** 開いて、view を出す。ref を渡すと History にその ref の履歴を出す */
  static show(extensionUri: vscode.Uri, manager: RepositoryManager, view?: ViewName, ref?: string): MainPanel {
    let current = MainPanel.current;
    if (current) {
      current.panel.reveal();
    } else {
      const panel = vscode.window.createWebviewPanel('amonhen.main', 'Amon Hen', vscode.ViewColumn.Active, {
        enableScripts: true,
        retainContextWhenHidden: true,
        // ⌘F で中を探す（History の詳細のメッセージや差分）
        enableFindWidget: true,
        localResourceRoots: [
          vscode.Uri.joinPath(extensionUri, 'dist', 'webview'),
          vscode.Uri.joinPath(extensionUri, 'media'),
        ],
      });
      current = new MainPanel(panel, extensionUri, manager);
      MainPanel.current = current;
    }
    if (view === 'history') {
      current.history.showScope(ref ? `ref:${ref}` : undefined);
    }
    if (view) {
      current.showView(view);
    }
    return current;
  }

  /** 開いていれば返す（Webview の右クリックメニューのコマンドから使う） */
  static get active(): MainPanel | undefined {
    return MainPanel.current;
  }

  private constructor(panel: vscode.WebviewPanel, extensionUri: vscode.Uri, manager: RepositoryManager) {
    this.panel = panel;
    this.manager = manager;
    this.history = new HistoryController((m) => this.post(m));
    this.workingCopy = new WorkingCopyController((m) => this.post(m));
    this.reflog = new ReflogController((m) => this.post(m));
    this.rangeDiff = new RangeDiffController(
      (m) => this.post(m),
      () => this.showView('history'),
    );
    this.fileHistory = new FileHistoryController((m) => this.post(m));
    this.blame = new BlameController((m) => this.post(m));
    this.settings = new SettingsController((m) => this.post(m));
    this.daemon = new DaemonController(
      (m) => this.post(m),
      () => this.watch(this.repo),
    );
    // タブのアイコンは画像として描かれ、currentColor が黒になるので、テーマごとに色を書いた写しを渡す
    // （色はサイドバーのアイコンと同じ、VSCode の既定のアイコンの色）。
    panel.iconPath = {
      light: vscode.Uri.joinPath(extensionUri, 'media', 'amonhen-light.svg'),
      dark: vscode.Uri.joinPath(extensionUri, 'media', 'amonhen-dark.svg'),
    };
    panel.webview.html = html(panel.webview, extensionUri);
    this.disposables.push(
      this.history,
      this.workingCopy,
      this.reflog,
      this.rangeDiff,
      this.fileHistory,
      this.blame,
      this.settings,
      this.daemon,
      panel.onDidDispose(() => this.dispose()),
      panel.webview.onDidReceiveMessage((m: WebviewMessage) => void this.onMessage(m).catch(showError)),
      manager.onDidChangeCurrent((r) => this.watch(r)),
      vscode.workspace.onDidChangeConfiguration((e) => {
        if (e.affectsConfiguration('amonhen.wrapLongLines')) this.post({ type: 'wrapLines', wrap: wrapLongLines() });
        if (e.affectsConfiguration('amonhen.diff.lineStaging')) this.workingCopy.lineStagingChanged();
        if (e.affectsConfiguration('amonhen.diff.filesToOpenTogether')) {
          this.post({ type: 'filesToOpenTogether', count: filesToOpenTogether() });
          this.workingCopy.filesToOpenTogetherChanged();
        }
        // 図・数式の設定を変えたら、「すべてのファイル」の絵を描き直す（テーマを変えたときと同じ）
        if (e.affectsConfiguration('amonhen.diagrams') || e.affectsConfiguration('amonhen.math')) this.post({ type: 'pictureSettings' });
      }),
    );
    this.watch(manager.current);
  }

  get repository(): Repository | undefined {
    return this.repo;
  }

  post(message: HostMessage): void {
    void this.panel.webview.postMessage(message);
  }

  /**
   * 詳細の帯の表示の設定（アプリの View Options と同じ並び）: コミット情報の表示項目（簡潔・標準・詳細）・高さを自動で
   * 決める・コミット情報だけを表示、コードの長い行を折り返す
   */
  private async detailsViewOptions(kept: boolean, expanded: boolean): Promise<void> {
    type Item = vscode.QuickPickItem & { run?: () => unknown };
    const mark = (on: boolean) => (on ? '$(check) ' : '$(blank) ');
    const style = detailsStyle();
    const styles: [DetailsStyle, string, string][] = [
      ['brief', 'Brief', 'The subject and one line of author, date, refs and parents'],
      ['standard', 'Standard', 'The author, IDs, signature, tags and the message'],
      ['full', 'Full', 'Also the children, branches, tags, pushed state, merged and related commits'],
    ];
    const items: Item[] = [
      { label: 'Commit Details', kind: vscode.QuickPickItemKind.Separator },
      ...styles.map(([value, label, detail]): Item => ({
        label: `${mark(style === value)}${label}`,
        description: detail,
        run: async () => {
          await setDetailsStyle(value);
          this.post({ type: 'detailsStyle', style: value });
        },
      })),
      {
        label: `${mark(!kept)}Fit Height Automatically`,
        description: 'Size the commit details to each message. Dragging the divider keeps its position instead.',
        run: () => this.post({ type: 'detailsView', action: 'fit' }),
      },
      {
        label: `${mark(expanded)}Show Only Commit Details`,
        description: 'Expand the commit details to fill the right column (choose again to restore)',
        run: () => this.post({ type: 'detailsView', action: 'expand' }),
      },
      { label: 'Code', kind: vscode.QuickPickItemKind.Separator },
      { label: `${mark(wrapLongLines())}Wrap Long Lines`, description: 'Alt+Z', run: () => toggleWrapLongLines() },
    ];
    const picked = await vscode.window.showQuickPick(items, { placeHolder: 'View Options' });
    await picked?.run?.();
  }

  /** Webview の準備ができてから fn を行う（できていればすぐ） */
  whenReady(fn: () => void): void {
    if (this.ready) fn();
    else this.whenReadyQueue.push(fn);
  }

  showView(view: ViewName): void {
    if (this.ready) {
      this.post({ type: 'show', view });
    } else {
      this.pendingView = view;
    }
  }

  /**
   * 確認のシートを重ねて、押したボタンの番号（buttons の順）と選んでいたもの（choices の順）を返す。やめたら
   * undefined。open は一覧の行（lists を通して数えた番号）を押したとき。準備ができているかは canAsk で確かめる
   * （できていなければ、呼ぶ側はモーダルで確かめる）
   */
  async ask(sheet: Omit<Sheet, 'id'>, open?: (item: number) => void, check?: SheetCheck): Promise<SheetAnswer | undefined> {
    if (this.answerSheet) return this.answerSheet(sheet, open);
    const id = this.nextSheet++;
    this.panel.reveal(undefined, false);
    return new Promise((resolve) => {
      this.sheets.set(id, { resolve, open, check });
      this.post({ type: 'sheet', id, ...sheet });
    });
  }

  /** シートを出せるか（Webview の準備ができている） */
  get canAsk(): boolean {
    return this.ready;
  }

  private watch(repo: Repository | undefined): void {
    this.repo = repo;
    this.panel.title = repo ? `Amon Hen: ${basename(repo.root)}` : 'Amon Hen';
    // Range Diff は開く前から比べ始める（結果は出したときに送り直す）
    this.rangeDiff.setRepository(repo);
    if (this.ready) {
      this.history.setRepository(repo);
      this.workingCopy.setRepository(repo);
      this.reflog.setRepository(repo);
      this.fileHistory.setRepository(repo);
      this.blame.setRepository(repo);
      this.settings.setRepository(repo);
    }
  }

  private async onMessage(message: WebviewMessage): Promise<void> {
    switch (message.type) {
      case 'ready':
        this.ready = true;
        this.post({ type: 'detailsLayout', layout: detailsLayout() });
        this.post({ type: 'wrapLines', wrap: wrapLongLines() });
        this.post({ type: 'filesToOpenTogether', count: filesToOpenTogether() });
        this.post({ type: 'messageFont', monospace: monospacedMessage() });
        this.post({ type: 'detailsStyle', style: detailsStyle() });
        if (this.pendingView) {
          this.post({ type: 'show', view: this.pendingView });
          this.pendingView = undefined;
        }
        this.history.setRepository(this.repo);
        this.workingCopy.setRepository(this.repo);
        this.reflog.setRepository(this.repo);
        this.fileHistory.setRepository(this.repo);
        this.blame.setRepository(this.repo);
        this.settings.setRepository(this.repo);
        for (const fn of this.whenReadyQueue.splice(0)) fn();
        break;
      case 'toggleWrapLines':
        await toggleWrapLongLines();
        break;
      case 'detailsViewOptions':
        await this.detailsViewOptions(message.kept, message.expanded);
        break;
      case 'view':
        // Daemon は出している間だけ読み直す
        this.daemon.setVisible(message.view === 'daemon');
        break;
      case 'sheetAnswer':
        this.sheets
          .get(message.id)
          ?.resolve(
            message.index === undefined
              ? undefined
              : { index: message.index, choice: message.choice ?? 0, ...(message.values ? { values: message.values } : {}) },
          );
        this.sheets.delete(message.id);
        break;
      case 'sheetOpen':
        this.sheets.get(message.id)?.open?.(message.item);
        break;
      case 'sheetCheck': {
        const check = this.sheets.get(message.id)?.check;
        if (!check) break;
        const result = await check(message.url).catch((err: unknown) => ({
          text: err instanceof Error ? err.message : String(err),
          ok: false,
        }));
        this.post({ type: 'sheetChecked', id: message.id, ...result });
        break;
      }
      case 'wcSelect':
      case 'wcSelectFiles':
      case 'wcSide':
      case 'wcLook':
      case 'wcLayout':
      case 'wcDraft':
      case 'wcPickRecent':
      case 'wcShowFolders':
      case 'wcPickInsert':
      case 'wcCommitMenu':
      case 'wcStageFiles':
      case 'wcStageLines':
      case 'wcDiscardFiles':
      case 'wcDiscardLines':
      case 'wcCommit':
      case 'wcOpenFile':
      case 'wcCommitRefused':
      case 'wcWhitespace':
      case 'wcGaps':
      case 'wcResolve':
        await this.workingCopy.handle(message);
        break;
      case 'reflogLoad':
      case 'reflogAction':
        await this.reflog.handle(message);
        break;
      case 'rangeDiffLoad':
      case 'rangeDiffCompare':
      case 'rangeDiffClose':
        await this.rangeDiff.handle(message);
        break;
      case 'fileHistoryLoad':
      case 'fileHistoryReveal':
        await this.fileHistory.handle(message);
        break;
      case 'blameLoad':
      case 'blameReveal':
        await this.blame.handle(message);
        break;
      case 'settingsLoad':
      case 'settingsSet':
      case 'settingsSaveExclude':
      case 'settingsRun':
        await this.settings.handle(message);
        break;
      case 'daemonStart':
      case 'daemonStop':
        await this.daemon.handle(message);
        break;
      default:
        await this.history.handle(message);
    }
  }

  dispose(): void {
    MainPanel.current = undefined;
    for (const { resolve } of this.sheets.values()) resolve(undefined);
    this.sheets.clear();
    vscode.Disposable.from(...this.disposables).dispose();
    this.panel.dispose();
  }
}

function html(webview: vscode.Webview, extensionUri: vscode.Uri): string {
  const nonce = randomBytes(16).toString('base64');
  const script = webview.asWebviewUri(vscode.Uri.joinPath(extensionUri, 'dist', 'webview', 'main.js'));
  const style = webview.asWebviewUri(vscode.Uri.joinPath(extensionUri, 'media', 'main.css'));
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src ${webview.cspSource}; font-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<link rel="stylesheet" href="${style}">
</head>
<body>
<script nonce="${nonce}" src="${script}"></script>
</body>
</html>`;
}
