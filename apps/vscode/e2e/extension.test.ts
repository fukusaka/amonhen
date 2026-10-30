/// <reference types="mocha" />
/**
 * VSCode を起動して、拡張を実際に動かす結合テスト（`npm run test:e2e`。e2e/run.mjs が一時ディレクトリにリポジトリと
 * エンジンの置き場所を用意し、そのリポジトリを開いた VSCode の拡張ホストで走る）。
 * メインビューの Webview が送るメッセージを拡張ホストに直接入れ、拡張ホストが Webview に送るメッセージと git の様子を
 * 確かめる。テストは上から順に、前のテストの結果を引き継いで進む
 */
import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import * as vscode from 'vscode';
import { DaemonClient } from '../src/daemon/client.ts';
import type { SelectedLine } from '../src/diff/fileDiff.ts';
import { settingsFile } from '../src/git/choice.ts';
import type { activate } from '../src/extension.ts';
import type { HostMessage } from '../src/view/protocol.ts';

type Api = Awaited<ReturnType<typeof activate>>;
type Message<T extends HostMessage['type']> = Extract<HostMessage, { type: T }>;

/** 拡張が起動した子プロセス */
interface Spawned {
  args: readonly string[];
  options: childProcess.SpawnOptions;
  child: childProcess.ChildProcess;
}

const FILE = 'crlf.txt';
const ext = vscode.extensions.getExtension<Api>('fukusaka.amonhen')!;
const root = vscode.workspace.workspaceFolders![0]!.uri.fsPath;
const spawned: Spawned[] = [];
const posted: HostMessage[] = [];
let api: Api;

function git(...args: string[]): string {
  return childProcess.execFileSync('git', args, { cwd: root, encoding: 'utf8' });
}

/** fn が値を返すまで待つ */
async function until<T>(what: string, fn: () => T | undefined | false | Promise<T | undefined | false>, ms = 20_000): Promise<T> {
  const start = Date.now();
  for (;;) {
    const value = await fn();
    if (value) {
      return value;
    }
    if (Date.now() - start > ms) {
      throw new Error(`待っても来ません: ${what}`);
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

/** since 番目より後に Webview に送られた type のメッセージで、match に合う最初のもの */
function waitPosted<T extends HostMessage['type']>(
  type: T,
  since: number,
  match: (m: Message<T>) => boolean = () => true,
): Promise<Message<T>> {
  return until(`${type} のメッセージ`, () =>
    posted.slice(since).find((m): m is Message<T> => m.type === type && match(m as Message<T>)),
  );
}

/**
 * 拡張が spawn・execFile で起動した子プロセスを記録する（呼び出し元が拡張の dist/extension.js のものだけ）。
 * 拡張は child_process の関数を呼ぶたびに引くので、モジュールの関数を差し替えれば拡張の呼び出しも通る
 */
function recordSpawns(): void {
  // Windows ではドライブ文字の大文字小文字が extensionPath とスタックで違うことがあるので、そろえて比べる
  const normalize = (path: string) => path.replaceAll('\\', '/').toLowerCase();
  const own = normalize(join(ext.extensionPath, 'dist', 'extension.js'));
  const cp = childProcess as unknown as Record<string, (...a: unknown[]) => childProcess.ChildProcess>;
  for (const name of ['spawn', 'execFile']) {
    const original = cp[name]!;
    cp[name] = function (this: unknown, ...a: unknown[]) {
      const child = original.apply(this, a);
      if (normalize(new Error().stack ?? '').includes(own)) {
        const args = Array.isArray(a[1]) ? (a[1] as string[]) : [];
        const options = (a.slice(1).find((x) => x && typeof x === 'object' && !Array.isArray(x)) ?? {}) as childProcess.SpawnOptions;
        spawned.push({ args, options, child });
      }
      return child;
    };
  }
}

async function showView(command: 'amonhen.showWorkingCopy' | 'amonhen.showHistory') {
  const start = Date.now();
  await vscode.commands.executeCommand(command);
  const panel = await until('メインビュー', () => api.panel());
  // Webview に送るメッセージを記録する（コントローラーは panel.post を呼ぶたびに引く）
  if (!Object.hasOwn(panel, 'post')) {
    const post = panel.post.bind(panel);
    panel.post = (m) => {
      if (posted.length === 0) {
        console.log(`      Webview の準備: ${((Date.now() - start) / 1000).toFixed(1)}s`);
      }
      posted.push(m);
      post(m);
    };
  }
  return panel;
}

/** daemon の様子（`amonhen daemon status --json`）。動いていなければ undefined */
function daemonStatus(): { pid: number; connections: number; repos: { path: string }[] } | undefined {
  const engine = process.env['AMONHEN_E2E_ENGINE']!;
  try {
    return JSON.parse(childProcess.execFileSync(engine, ['daemon', 'status', '--json'], { encoding: 'utf8' }));
  } catch {
    return undefined;
  }
}

/**
 * CRLF のファイルの 1 行を変え、Working Copy で行単位の stage・unstage、行の破棄と Undo、コミットと Undo をする。
 * どれも、変えた 1 行だけが変わったものとして扱われることを確かめる
 */
async function workingCopyFlow(label: string): Promise<void> {
  const panel = await showView('amonhen.showWorkingCopy');
  const wc = panel.workingCopy;
  const path = join(root, FILE);
  const before = readFileSync(path, 'latin1');
  // 3 行目（前の流れでコミットした内容）を書き換える
  const old = before.split('\r\n')[2]!;
  assert.ok(old.startsWith('three') && !old.includes('\n'), '作業中のファイルは CRLF');
  const changed = before.replace(`${old}\r\n`, `three ${label}\r\n`);
  writeFileSync(path, changed, 'latin1');
  const mark = posted.length;
  await vscode.commands.executeCommand('amonhen.refresh');
  await waitPosted('wcFiles', mark, (m) => m.files.some((f) => f.group === 'unstaged' && f.path === FILE));

  /** 差分を選び、変わった行が 1 行の書き換えだけであることを確かめて、その行を返す */
  const select = async (group: 'unstaged' | 'staged') => {
    const at = posted.length;
    await wc.handle({ type: 'wcSelect', group, path: FILE });
    const { diff } = await waitPosted('wcDiff', at, (m) => m.group === group && m.diff?.path === FILE);
    const lines = diff!.hunks.flatMap((h) => h.lines).filter((l) => l.kind !== ' ');
    assert.deepEqual(
      lines.map((l) => [l.kind, l.text]),
      [
        ['-', old],
        ['+', `three ${label}`],
      ],
      '変えた 1 行だけが差分になる（改行の違いで全体が書き換えに見えない）',
    );
    const selected = lines.map((l): SelectedLine => (l.kind === '-' ? { kind: '-', old: l.old! } : { kind: '+', new: l.new! }));
    return { group, path: FILE, version: diff!.version, lines: selected };
  };
  const numstat = (...args: string[]) => git('diff', '--numstat', ...args).trim();
  const one = `1\t1\t${FILE}`;

  // 行単位の stage と unstage
  await wc.handle({ type: 'wcStageLines', ...(await select('unstaged')) });
  await until('stage', () => numstat('--cached') === one && numstat() === '');
  await wc.handle({ type: 'wcStageLines', ...(await select('staged')) });
  await until('unstage', () => numstat('--cached') === '' && numstat() === one);

  // 行の破棄（確かめのダイアログには Discard と答える）と、その Undo
  const { version, lines } = await select('unstaged');
  const warning = vscode.window.showWarningMessage;
  (vscode.window as { showWarningMessage: unknown }).showWarningMessage = async () => 'Discard';
  try {
    await wc.handle({ type: 'wcDiscardLines', path: FILE, version, lines });
  } finally {
    (vscode.window as { showWarningMessage: unknown }).showWarningMessage = warning;
  }
  await until('行の破棄', () => git('status', '--porcelain').trim() === '');
  assert.equal(readFileSync(path, 'latin1'), before, '破棄したら元の内容（CRLF のまま）');
  await vscode.commands.executeCommand('amonhen.undo');
  await until('破棄の Undo', () => readFileSync(path, 'latin1') === changed);

  // stage してコミットし、Undo で stage した状態に戻してから、もう一度コミットする
  await wc.handle({ type: 'wcStageLines', ...(await select('unstaged')) });
  await until('stage', () => numstat('--cached') === one);
  const head = git('rev-parse', 'HEAD').trim();
  const commit = async () => {
    const at = posted.length;
    await wc.handle({ type: 'wcCommit', message: `change three (${label})`, amend: false });
    await waitPosted('wcCommitted', at);
    assert.equal(git('log', '-1', '--format=%s').trim(), `change three (${label})`);
    assert.equal(git('rev-parse', 'HEAD~').trim(), head);
    assert.equal(git('status', '--porcelain').trim(), '');
  };
  await commit();
  await vscode.commands.executeCommand('amonhen.undo');
  await until('コミットの Undo', () => git('rev-parse', 'HEAD').trim() === head && numstat('--cached') === one);
  await commit();
  assert.ok(!git('show', `HEAD:${FILE}`).includes('\r'), 'コミットした内容は LF');
  assert.equal(readFileSync(path, 'latin1'), changed, '作業中のファイルは CRLF のまま');
}

/**
 * `.mailmap` を置くと、History の行と詳細が mailmap の名前になり（詳細には記録されたものも付く）、`log.mailmap` を
 * false にすると記録のままに戻る。`mailmap.file` も、その中身を書き換えたときも同じ。どれも開いたまま、知らせ
 * （ここでは amonhen.refresh）で読み直す
 */
async function mailmapFlow(): Promise<void> {
  const panel = await showView('amonhen.showHistory');
  const mailmap = join(root, '.mailmap');
  const shared = join(dirname(root), 'shared-mailmap');
  const show = async (author: string) => {
    const mark = posted.length;
    await vscode.commands.executeCommand('amonhen.refresh');
    const rows = await waitPosted('rows', mark, (m) => m.rows.some((r) => r.subject === 'first' && r.author === author));
    const first = rows.rows.find((r) => r.subject === 'first')!;
    await panel.history.handle({ type: 'select', oid: first.oid });
    return (await waitPosted('details', mark, (m) => m.details.oid === first.oid)).details;
  };
  writeFileSync(mailmap, 'Ann Example <ann@example.com> <a@example.com>\n');
  try {
    const mapped = await show('Ann Example');
    assert.deepEqual([mapped.author.name, mapped.author.email], ['Ann Example', 'ann@example.com']);
    assert.deepEqual(mapped.recordedAuthor, { name: 'a', email: 'a@example.com' });
    git('config', 'log.mailmap', 'false');
    const recorded = await show('a');
    assert.deepEqual([recorded.author.name, recorded.author.email], ['a', 'a@example.com']);
    assert.equal(recorded.recordedAuthor, undefined);
    git('config', '--unset', 'log.mailmap');
    rmSync(mailmap);
    writeFileSync(shared, 'Carol Example <a@example.com>\n');
    git('config', 'mailmap.file', shared);
    assert.equal((await show('Carol Example')).author.name, 'Carol Example');
    // 設定はそのままで、ファイルの中身だけを変える
    writeFileSync(shared, 'Dave Example <a@example.com>\n');
    assert.equal((await show('Dave Example')).author.name, 'Dave Example');
  } finally {
    // 途中で消したものもあるので、あれば消す（ない設定を消すと git は失敗する）
    for (const key of ['log.mailmap', 'mailmap.file']) {
      try {
        git('config', '--unset', key);
      } catch {
        // なかった
      }
    }
    rmSync(mailmap, { force: true });
    rmSync(shared, { force: true });
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

/**
 * ブランチを書き換えると（ここでは update-ref で）、そのブランチの右クリックの Compare with Previous Version が、
 * 書き換える前の版と比べた文書を開く。range-diff を動かすのは、daemon を使うならコア、使わなければ拡張の git
 */
async function rangeDiffFlow(path: 'daemon' | 'git'): Promise<void> {
  const branch = `range-diff-${path}`;
  const tree = git('rev-parse', 'HEAD^{tree}').trim();
  const make = (parent: string, message: string) => git('commit-tree', tree, '-p', parent, '-m', message).trim();
  const head = git('rev-parse', 'HEAD').trim();
  const one = make(head, 'one');
  git('branch', branch, make(one, 'two'));
  git('update-ref', '-m', 'rewrite', `refs/heads/${branch}`, make(head, 'one, reworded'));
  const mark = spawned.length;
  try {
    const target = { kind: 'branch', remote: false, ref: { name: `refs/heads/${branch}` } };
    await vscode.commands.executeCommand('amonhen.compareWithPrevious', target);
    const doc = await until('Range Diff の文書', () =>
      vscode.workspace.textDocuments.find((d) => d.uri.scheme === 'amonhen' && d.uri.path.endsWith(`${branch}.range-diff.diff`)),
    );
    const text = doc.getText();
    assert.match(text, new RegExp(`^# ${branch}@\\{1\\} → ${branch}: rewrite\n# git range-diff `), text);
    assert.match(text, /^\s*\d+:\s+[0-9a-f]{7} < +-:\s+-+ two$/m, text);
    assert.equal(
      spawned.slice(mark).some((s) => s.args.includes('range-diff')),
      path === 'git',
      path === 'git' ? '拡張が git range-diff を動かす' : 'range-diff はコアがする',
    );
  } finally {
    await vscode.commands.executeCommand('workbench.action.closeActiveEditor');
    git('branch', '-D', branch);
  }
}

/** vscode.window の関数を fn のあいだだけ差し替える（ダイアログに答える） */
async function withWindow<T>(stubs: Record<string, unknown>, fn: () => Promise<T>): Promise<T> {
  const window = vscode.window as unknown as Record<string, unknown>;
  const originals = Object.fromEntries(Object.keys(stubs).map((k) => [k, window[k]]));
  Object.assign(window, stubs);
  try {
    return await fn();
  } finally {
    Object.assign(window, originals);
  }
}

/**
 * コミットをパッチに保存して git am で当て、Undo で戻す。当たらずに止まった am を 3-way merge で当て直し、
 * 解決して Continue し、Undo で戻す。ただの diff を git apply で作業ツリーに当て、Undo で戻す
 */
async function patchFlow(label: string): Promise<void> {
  await showView('amonhen.showHistory');
  const repo = api.manager.current!;
  const name = `patch-${label}.txt`;
  const path = join(root, name);
  const out = join(dirname(root), `patches-${label}`);
  const diff = join(dirname(root), `change-${label}.diff`);
  // 作業中のファイルは CRLF になる（core.autocrlf=true）
  const text = () => readFileSync(path, 'utf8').replaceAll('\r\n', '\n');
  const head = () => git('rev-parse', 'HEAD').trim();
  const subject = () => git('log', '-1', '--format=%s').trim();
  const commitFile = (content: string, message: string) => {
    writeFileSync(path, content);
    git('add', name);
    git('commit', '-q', '-m', message);
    return head();
  };
  // 当てる前の確かめ（modal）には answer と答え、ほかの知らせは閉じる
  let answer = 'Apply';
  const stubs = {
    showQuickPick: async (items: unknown) => ((await items) as unknown[])[0],
    showOpenDialog: async () => [vscode.Uri.file(out)],
    showInformationMessage: async (_message: string, ...rest: unknown[]) =>
      (rest[0] as { modal?: boolean } | undefined)?.modal ? answer : undefined,
    showWarningMessage: async () => undefined,
    showErrorMessage: async (message: string) => void errors.push(message),
  };
  const errors: string[] = [];
  await withWindow(stubs, async () => {
    try {
      // 保存して、そのコミットを消してから am で当て直す
      commitFile('1\n2\n3\n', `base ${label}`);
      const change = commitFile('1\nx\n3\n', `change ${label}`);
      await vscode.commands.executeCommand('amonhen.history.savePatch', { oid: change });
      const patch = join(out, `0001-change-${label}.patch`);
      assert.ok(existsSync(patch), `${patch} がある`);
      git('reset', '-q', '--hard', 'HEAD~1');
      const base = head();
      await vscode.commands.executeCommand('amonhen.applyPatch', vscode.Uri.file(patch));
      await until('am', () => subject() === `change ${label}` && git('rev-parse', 'HEAD~').trim() === base);
      await vscode.commands.executeCommand('amonhen.undo');
      await until('am の Undo', () => head() === base);

      // 当たらずに止まる → 3-way merge で当て直す → 解決して Continue → Undo で全体が戻る
      const other = commitFile('1\ny\n3\n', `other ${label}`);
      await vscode.commands.executeCommand('amonhen.applyPatch', vscode.Uri.file(patch));
      await until('am で止まる', () => repo.current.sequence === 'am' && repo.current.am?.retried === false);
      assert.equal(head(), other);
      await vscode.commands.executeCommand('amonhen.amRetryThreeWay');
      await until('3-way merge で当て直す', () => text().includes('<<<<<<<') && repo.current.am?.retried === true);
      writeFileSync(path, '1\nx\n3\n');
      git('add', name);
      await vscode.commands.executeCommand('amonhen.continueSequence');
      await until('Continue', () => !repo.current.sequence && subject() === `change ${label}`);
      assert.equal(git('rev-parse', 'HEAD~').trim(), other);
      await vscode.commands.executeCommand('amonhen.undo');
      await until('止まった am の Undo', () => head() === other && text() === '1\ny\n3\n');

      // ただの diff は作業ツリーだけに当てる（git apply の既定）
      writeFileSync(diff, `diff --git a/${name} b/${name}\n--- a/${name}\n+++ b/${name}\n@@ -1,3 +1,3 @@\n 1\n y\n-3\n+z\n`);
      await vscode.commands.executeCommand('amonhen.applyPatch', vscode.Uri.file(diff));
      await until('apply', () => text() === '1\ny\nz\n');
      assert.equal(git('diff', '--cached', '--name-only').trim(), '', 'index には触らない');
      assert.equal(head(), other);
      await vscode.commands.executeCommand('amonhen.undo');
      await until('apply の Undo', () => text() === '1\ny\n3\n' && git('status', '--porcelain').trim() === '');
      assert.deepEqual(errors, [], 'エラーは出ない');
    } finally {
      if (repo.current.sequence === 'am') {
        git('am', '--abort');
      }
      rmSync(out, { recursive: true, force: true });
      rmSync(diff, { force: true });
      await vscode.commands.executeCommand('amonhen.refresh');
    }
  });
}

/**
 * rerere が前の解決を当てたファイルと、ほかのエディタで直したファイルは、マーカーが残っていないコンフリクトとして
 * Webview に送られる（マーカーが残っているファイルとは分ける）。そうしたファイルで Accept を選ぶと、解決した内容が
 * 失われると確かめ、まとめて解決済みにできる
 */
async function conflictsWithoutMarkersFlow(): Promise<void> {
  const panel = await showView('amonhen.showWorkingCopy');
  const wc = panel.workingCopy;
  const branch = git('symbolic-ref', '--short', 'HEAD').trim();
  const write = (file: string, text: string) => writeFileSync(join(root, file), text);
  const files = ['rerere.txt', 'hand.txt', 'open.txt'];
  const commitSide = (side: string) => {
    for (const f of files) write(f, `1\n${f} ${side}\n3\n`);
    git('add', ...files);
    git('commit', '-q', '-m', `conflict ${side}`);
  };
  git('config', 'rerere.enabled', 'true');
  git('switch', '-q', '-c', 'conflict-ours');
  commitSide('ours');
  git('switch', '-q', '-c', 'conflict-theirs', 'HEAD~');
  commitSide('theirs');
  git('switch', '-q', 'conflict-ours');
  const merge = () => assert.throws(() => childProcess.execFileSync('git', ['merge', '-q', 'conflict-theirs'], { cwd: root, stdio: 'ignore' }));
  try {
    // 1 回目: rerere.txt だけ解決し、rerere に覚えさせてやめる。2 回目は rerere が rerere.txt を解く
    merge();
    write('rerere.txt', '1\nrerere.txt both\n3\n');
    git('rerere');
    git('merge', '--abort');
    merge();
    assert.equal(readFileSync(join(root, 'rerere.txt'), 'utf8'), '1\nrerere.txt both\n3\n');
    write('hand.txt', '1\nhand.txt both\n3\n');
    const mark = posted.length;
    await vscode.commands.executeCommand('amonhen.refresh');
    const sent = await waitPosted('wcFiles', mark, (m) => m.files.filter((f) => f.group === 'conflicts').length === 3);
    const state = (path: string) => {
      const f = sent.files.find((x) => x.group === 'conflicts' && x.path === path)!;
      return { markers: f.markers, rerere: f.rerere };
    };
    assert.deepEqual(state('rerere.txt'), { markers: 0, rerere: true });
    assert.deepEqual(state('hand.txt'), { markers: 0, rerere: false });
    assert.deepEqual(state('open.txt'), { markers: 1, rerere: false });

    // Accept は、解決した内容が失われると確かめる（ここでは断る）
    let detail: string | undefined;
    const showWarningMessage = async (_: string, options: { detail?: string }) => {
      detail = options.detail;
      return undefined;
    };
    await withWindow({ showWarningMessage }, () => wc.handle({ type: 'wcResolve', path: 'hand.txt', action: 'current' }));
    assert.match(detail ?? '', /もう解決済みかもしれません/);
    assert.equal(readFileSync(join(root, 'hand.txt'), 'utf8'), '1\nhand.txt both\n3\n', '断れば変わらない');

    // Conflicts の見出しの Mark N as Resolved と同じく、マーカーのないものだけを解決済みにする
    await wc.handle({
      type: 'wcStageFiles',
      op: 'stage',
      files: ['rerere.txt', 'hand.txt'].map((path) => ({ group: 'conflicts' as const, path })),
    });
    await until('解決済み', () => git('diff', '--name-only', '--diff-filter=U').trim() === 'open.txt');
  } finally {
    try {
      git('merge', '--abort');
    } catch {
      // もうやめている
    }
    git('switch', '-q', branch);
    git('branch', '-q', '-D', 'conflict-ours', 'conflict-theirs');
    git('config', '--unset', 'rerere.enabled');
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

/**
 * Merge Branches… でいくつものブランチを 1 つのコミットにまとめて merge し、Undo で戻す。最後より前のものが
 * コンフリクトして git が断ったら、勧めに従って 1 つずつ merge し、止まったところで中止して Undo で戻す
 */
async function mergeBranchesFlow(label: string): Promise<void> {
  const repo = api.manager.current!;
  const branch = git('symbolic-ref', '--short', 'HEAD').trim();
  const head = () => git('rev-parse', 'HEAD').trim();
  const name = (b: string) => `mb-${b}-${label}`;
  // a は自分のファイル、x と y は同じファイルの同じ行を変える（後のほうがぶつかる）
  for (const [b, file] of [['a', `mb-a-${label}.txt`], ['x', `mb-${label}.txt`], ['y', `mb-${label}.txt`]] as const) {
    git('switch', '-q', '-c', name(b), branch);
    writeFileSync(join(root, file), `${b}\n`);
    git('add', file);
    git('commit', '-q', '-m', name(b));
  }
  git('switch', '-q', branch);
  await vscode.commands.executeCommand('amonhen.refresh');
  const start = head();
  // 1 つ目の QuickPick（いくつも選ぶ）では order の順に選び、2 つ目（どう merge するか）では choose で選ぶ
  let order: string[] = [];
  let choose: (label: string) => boolean = () => false;
  const placeHolders: string[] = [];
  const informed: string[] = [];
  const stubs = {
    showQuickPick: async (items: unknown, options?: { canPickMany?: boolean; placeHolder?: string }) => {
      const list = (await items) as ({ label: string; ref?: string } & Record<string, unknown>)[];
      if (options?.canPickMany) {
        return order.map((b) => list.find((i) => i.ref === `refs/heads/${b}`)!);
      }
      placeHolders.push(options?.placeHolder ?? '');
      return list.find((i) => choose(i.label));
    },
    showInformationMessage: async (message: string) => void informed.push(message),
    // git が断ったら 1 つずつを選び、コンフリクトしたら中止する
    showWarningMessage: async (_: string, ...rest: unknown[]) =>
      (rest[0] as { modal?: boolean } | undefined)?.modal ? 'Merge One by One' : 'Abort',
    showErrorMessage: async (message: string) => assert.fail(message),
  };
  try {
    await withWindow(stubs, async () => {
      order = [name('a'), name('x')];
      choose = (l) => l.startsWith('$(git-merge)');
      await vscode.commands.executeCommand('amonhen.mergeBranches');
      await until('merge', () => git('log', '-1', '--format=%s').startsWith(`Merge branches '${name('a')}' and '${name('x')}'`));
      assert.equal(placeHolders.at(-1), `2 個を ${branch} に 1 つのコミットで merge する`);
      await vscode.commands.executeCommand('amonhen.undo');
      await until('merge の Undo', () => head() === start);

      // x の後の y はぶつかる。見込みが分かれば（git 2.38 以降）そう伝える。それでも 1 つにまとめると git は断る
      order = [name('x'), name('y'), name('a')];
      await vscode.commands.executeCommand('amonhen.mergeBranches');
      if (!placeHolders.at(-1)!.includes('個を')) {
        assert.match(placeHolders.at(-1)!, new RegExp(`${name('y')} がコンフリクトします`));
      }
      // 1 つずつなら x は merge され、y で止まって中止する。残りの a を知らせる
      await until('1 つずつ', () => informed.some((m) => m.startsWith('まだ merge していないもの') && m.includes(name('a'))));
      await until('中止', async () => (await repo.backend.sequenceInProgress()) === undefined);
      // x は今のブランチの先にあるので、git の既定どおり fast-forward になる
      assert.equal(head(), git('rev-parse', name('x')).trim());
      await vscode.commands.executeCommand('amonhen.undo');
      await until('1 つずつの Undo', () => head() === start);
    });
  } finally {
    git('reset', '-q', '--hard', start);
    git('branch', '-q', '-D', name('a'), name('x'), name('y'));
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

suite('Amon Hen in VSCode', () => {
  suiteSetup(async () => {
    recordSpawns();
    api = await ext.activate();
  });

  test('activates with the bundled engine and the shared daemon', async () => {
    const repo = await until('リポジトリ', () => api.manager.current);
    assert.equal(repo.root.toLowerCase(), root.toLowerCase());
    // 同梱のエンジンが置き場所に置かれ、current がそれを指す
    const data =
      process.platform === 'win32'
        ? join(process.env['LOCALAPPDATA']!, 'amonhen')
        : join(process.env['XDG_DATA_HOME']!, 'amonhen');
    await until('エンジンの置き場所', () => existsSync(join(data, 'current')));
    const link =
      process.platform === 'win32' ? join(data, 'bin', 'amonhen.exe') : join(process.env['HOME']!, '.local', 'bin', 'amonhen');
    // install は current を書いてから CLI を写す（Windows では写すのに時間がかかる）
    await until(`${link} がある`, () => existsSync(link));
  });

  test('shows History, commit details and diffs through the daemon', async () => {
    const mark = posted.length;
    const panel = await showView('amonhen.showHistory');
    const rows = await waitPosted('rows', mark, (m) => m.rows.some((r) => r.subject === 'add crlf.txt'));
    const commit = rows.rows.find((r) => r.subject === 'add crlf.txt')!;
    await panel.history.handle({ type: 'select', oid: commit.oid });
    const { details } = await waitPosted('details', mark, (m) => m.details.oid === commit.oid);
    assert.deepEqual(
      details.files.map((f) => f.path),
      [FILE],
    );
    await panel.history.handle({ type: 'commitDiff', oid: commit.oid, path: FILE });
    const { diff } = await waitPosted('commitDiff', mark, (m) => m.oid === commit.oid);
    assert.deepEqual(
      diff.hunks.flatMap((h) => h.lines).map((l) => [l.kind, l.text]),
      ['one', 'two', 'three', 'four', 'five'].map((t) => ['+', t]),
    );
    // History は共有の daemon が読んでいる
    const status = await until('daemon', () => daemonStatus());
    assert.ok(
      status.repos.some((r) => r.path.toLowerCase() === root.toLowerCase()),
      `daemon が ${root} を開いている: ${JSON.stringify(status.repos.map((r) => r.path))}`,
    );
  });

  test('shows names from the mailmap through the daemon and reloads them when it changes', async () => {
    await mailmapFlow();
  });

  test('compares a branch with its previous version through the daemon', async () => {
    await rangeDiffFlow('daemon');
  });

  test('stages, undoes, discards lines and commits on a CRLF file through the daemon', async () => {
    const mark = spawned.length;
    await workingCopyFlow('daemon');
    // 行の stage は daemon がする（拡張は git の update-index を起動しない）
    assert.deepEqual(
      spawned.slice(mark).filter((s) => s.args.includes('update-index')),
      [],
    );
  });

  test('saves commits as patches and applies them through the daemon', async () => {
    await patchFlow('daemon');
  });

  test('merges several branches in one commit through the daemon', async () => {
    await mergeBranchesFlow('daemon');
  });

  test('drops the selection of a file whose changes went away before its diff is read', async () => {
    const panel = await showView('amonhen.showWorkingCopy');
    const path = join(root, FILE);
    const before = readFileSync(path);
    writeFileSync(path, Buffer.concat([before, Buffer.from('more\r\n')]));
    let mark = posted.length;
    await vscode.commands.executeCommand('amonhen.refresh');
    await waitPosted('wcFiles', mark, (m) => m.files.some((f) => f.group === 'unstaged' && f.path === FILE));
    // 拡張が知る前に元に戻す。daemon は新しい status を読むので、このファイルの差分を断る
    writeFileSync(path, before);
    mark = posted.length;
    await panel.workingCopy.handle({ type: 'wcSelect', group: 'unstaged', path: FILE });
    await waitPosted('wcDiff', mark, (m) => m.diff === undefined);
    await waitPosted('wcFiles', mark, (m) => !m.files.some((f) => f.path === FILE));
  });

  test('shares one daemon with another window and the CLI', async () => {
    const engine = process.env['AMONHEN_E2E_ENGINE']!;
    const before = await until('daemon', () => daemonStatus());
    // もう 1 つの窓の拡張と同じつなぎ方で、同じリポジトリを開く
    const other = await DaemonClient.open(engine, root);
    try {
      const during = daemonStatus()!;
      assert.equal(during.pid, before.pid, '新しい daemon を起動しない');
      assert.equal(during.connections, before.connections + 1);
      // CLI も同じ daemon を通して読む
      const log = JSON.parse(
        childProcess.execFileSync(engine, ['log', '-C', `daemon:${root}`, '-n', '1', '--no-graph', '--json'], {
          encoding: 'utf8',
        }),
      ) as { id: string }[];
      assert.equal(log[0]!.id, git('rev-parse', 'HEAD').trim());
      assert.equal(daemonStatus()!.pid, before.pid, 'CLI も同じ daemon を使う');
    } finally {
      other.dispose();
    }
  });

  test('does the same on the git execution layer with the daemon off', async () => {
    await vscode.workspace.getConfiguration('amonhen').update('daemon.enabled', false, vscode.ConfigurationTarget.Global);
    const mark = spawned.length;
    await workingCopyFlow('git');
    assert.ok(
      spawned.slice(mark).some((s) => s.args.includes('update-index')),
      '行の stage は拡張が git の update-index でする',
    );
  });

  test('saves commits as patches and applies them on the git execution layer', async () => {
    await patchFlow('git');
  });

  test('merges several branches in one commit on the git execution layer', async () => {
    await mergeBranchesFlow('git');
  });

  test('shows names from the mailmap on the git execution layer', async () => {
    await mailmapFlow();
  });

  test('compares a branch with its previous version on the git execution layer', async () => {
    await rangeDiffFlow('git');
  });

  test('runs the git chosen in Amon Hen’s settings, and stops when it isn’t there', async () => {
    const repo = api.manager.current!;
    const settings = settingsFile();
    const choose = (git: string) => {
      mkdirSync(dirname(settings), { recursive: true });
      writeFileSync(settings, JSON.stringify({ git }));
    };
    const pathGit =
      process.platform === 'win32'
        ? childProcess.execFileSync('where', ['git'], { encoding: 'utf8' }).split(/\r?\n/)[0]!
        : childProcess.execFileSync('sh', ['-c', 'command -v git'], { encoding: 'utf8' }).trim();
    const missing = join(dirname(settings), 'no-such-git');
    try {
      for (const daemon of [false, true]) {
        await vscode.workspace.getConfiguration('amonhen').update('daemon.enabled', daemon, vscode.ConfigurationTarget.Global);
        const via = daemon ? 'daemon' : 'git';
        // 選んだ git がなければ、ほかの git で動かさずに止まる
        choose(missing);
        await assert.rejects(repo.backend.createBranch(`missing-${via}`, 'HEAD', false), (e: Error) => e.message.includes(missing));
        assert.equal(git('branch', '--list', `missing-${via}`), '');
        // 選び直せば、再起動もつなぎ直しもなく、その git で動く
        choose(pathGit);
        await repo.backend.createBranch(`chosen-${via}`, 'HEAD', false);
        assert.notEqual(git('branch', '--list', `chosen-${via}`), '');
      }
    } finally {
      rmSync(settings, { force: true });
      await vscode.workspace.getConfiguration('amonhen').update('daemon.enabled', false, vscode.ConfigurationTarget.Global);
    }
  });

  test('follows a change to git.path at once, through the daemon too', async () => {
    const repo = api.manager.current!;
    const gitConfig = vscode.workspace.getConfiguration('git');
    const pathGit =
      process.platform === 'win32'
        ? childProcess.execFileSync('where', ['git'], { encoding: 'utf8' }).split(/\r?\n/)[0]!
        : childProcess.execFileSync('sh', ['-c', 'command -v git'], { encoding: 'utf8' }).trim();
    const missing = join(dirname(settingsFile()), 'no-such-git');
    // 設定を変えたあと、拡張が読み直して daemon に送るまでを待つ
    const settle = () => new Promise((resolve) => setTimeout(resolve, 300));
    try {
      for (const daemon of [false, true]) {
        await vscode.workspace.getConfiguration('amonhen').update('daemon.enabled', daemon, vscode.ConfigurationTarget.Global);
        const via = daemon ? 'daemon' : 'git';
        // Amon Hen の設定で選んでいなければ git.path の git。変えれば、開き直さずに次の操作から使う
        await gitConfig.update('path', missing, vscode.ConfigurationTarget.Global);
        await settle();
        await assert.rejects(repo.backend.createBranch(`path-missing-${via}`, 'HEAD', false), (e: Error) => e.message.includes(missing));
        await gitConfig.update('path', pathGit, vscode.ConfigurationTarget.Global);
        await settle();
        await repo.backend.createBranch(`path-chosen-${via}`, 'HEAD', false);
        assert.notEqual(git('branch', '--list', `path-chosen-${via}`), '');
      }
    } finally {
      await gitConfig.update('path', undefined, vscode.ConfigurationTarget.Global);
      await vscode.workspace.getConfiguration('amonhen').update('daemon.enabled', false, vscode.ConfigurationTarget.Global);
    }
  });

  test('tells conflicts without markers and marks them resolved', async () => {
    await conflictsWithoutMarkersFlow();
  });

  test('starts every process without a console window', () => {
    assert.ok(spawned.length > 0);
    const shown = spawned.filter((s) => s.options.windowsHide !== true);
    assert.deepEqual(
      shown.map((s) => s.args.join(' ')),
      [],
      'Windows でコンソールの窓が出る',
    );
  });

  test('stops its git processes when the repositories are closed', async () => {
    api.manager.dispose();
    // daemon は拡張と切り離して起動するので除く。常駐の cat-file が残ると、Windows ではリポジトリを消せない（EPERM）
    const own = spawned.filter((s) => !s.options.detached);
    await until(
      'git の終了',
      () => own.every((s) => s.child.exitCode !== null || s.child.signalCode !== null),
      5_000,
    ).catch(() => {
      const alive = own.filter((s) => s.child.exitCode === null && s.child.signalCode === null);
      assert.fail(`終わっていない: ${alive.map((s) => s.args.join(' ')).join(', ')}`);
    });
  });
});
