/// <reference types="mocha" />
/**
 * VSCode を起動して、拡張を実際に動かす結合テスト（`npm run test:e2e`。e2e/run.mjs が一時ディレクトリにリポジトリと
 * エンジンの置き場所を用意し、そのリポジトリを開いた VSCode の拡張ホストで走る）。
 * メインビューの Webview が送るメッセージを拡張ホストに直接入れ、拡張ホストが Webview に送るメッセージと git の様子を
 * 確かめる。テストは上から順に、前のテストの結果を引き継いで進む
 */
import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import * as vscode from 'vscode';
import { DaemonClient } from '../src/daemon/client.ts';
import type { SelectedLine } from '../src/diff/fileDiff.ts';
import { settingsFile } from '../src/git/choice.ts';
import { samePath } from '../src/git/worktrees.ts';
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
/** 拡張が出したエラーの知らせ */
const errors: string[] = [];
let api: Api;

function git(...args: string[]): string {
  return childProcess.execFileSync('git', args, { cwd: root, encoding: 'utf8' });
}

/** fn が値を返すまで待つ。来なければ、seen（そこまでに来たもの）を添えて失敗する */
async function until<T>(
  what: string,
  fn: () => T | undefined | false | Promise<T | undefined | false>,
  ms = 20_000,
  seen?: () => string,
): Promise<T> {
  const start = Date.now();
  for (;;) {
    const value = await fn();
    if (value) {
      return value;
    }
    if (Date.now() - start > ms) {
      throw new Error(`待っても来ません: ${what}${seen ? `（来たもの: ${seen()}）` : ''}`);
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

/** 時間切れのときに添える、直近に出たエラー（操作が失敗した理由） */
const lastErrors = (): string => errors.slice(-3).map((e) => `エラー「${e}」`).join('、') || 'エラーなし';

/** since 番目より後に Webview に送られた type のメッセージで、match に合う最初のもの */
function waitPosted<T extends HostMessage['type']>(
  type: T,
  since: number,
  match: (m: Message<T>) => boolean = () => true,
): Promise<Message<T>> {
  return until(
    `${type} のメッセージ`,
    () => posted.slice(since).find((m): m is Message<T> => m.type === type && match(m as Message<T>)),
    undefined,
    () => {
      const kinds = posted.slice(since).map((m) => (m.type === 'rows' ? `rows[${m.start}+${m.rows.length}${m.total === undefined ? '' : `/${m.total}`}]` : m.type));
      return [kinds.join(', ') || 'なし', ...errors.slice(-3).map((e) => `エラー「${e}」`)].join('、');
    },
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

async function showView(command: 'amonhen.showWorkingCopy' | 'amonhen.showHistory' | 'amonhen.showReflog') {
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

/** 拡張が出すエラーの知らせを記録する（待っても来ないときに添える） */
function recordErrors(): void {
  const show = vscode.window.showErrorMessage.bind(vscode.window) as (message: string, ...rest: unknown[]) => Thenable<unknown>;
  (vscode.window as { showErrorMessage: unknown }).showErrorMessage = (message: string, ...rest: unknown[]) => {
    errors.push(message);
    return show(message, ...rest);
  };
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
  // ファイル一覧の見せ方は覚えて、次に送る一覧で返す
  await wc.handle({ type: 'wcLayout', layout: 'tree' });
  const mark = posted.length;
  await vscode.commands.executeCommand('amonhen.refresh');
  await waitPosted('wcFiles', mark, (m) => m.layout === 'tree' && m.files.some((f) => f.group === 'unstaged' && f.path === FILE));
  await wc.handle({ type: 'wcLayout', layout: 'flat' });

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

/**
 * サイドバーのタグに、リモートでの様子（fetch で読む）と署名の印が出て、右クリックで origin への push・置き換え・
 * 削除と手元の削除（Undo で戻る）ができる。リモートは追加・名前の変更・URL の変更・prune・削除ができる。
 * どれもコアの命令（daemon だけ）
 */
async function tagsAndRemotesFlow(): Promise<void> {
  const repo = api.manager.current!;
  const sidebar = api.sidebar;
  const origin = git('remote', 'get-url', 'origin').trim();
  const inOrigin = (...args: string[]) => childProcess.execFileSync('git', args, { cwd: origin, encoding: 'utf8' });
  const tagsSection = sidebar.getChildren().find((n) => n.kind === 'section' && n.section === 'tags')!;
  await repo.want('tags');
  const tagItem = (name: string) => {
    const node = sidebar.getChildren(tagsSection).find((n) => n.kind === 'tag' && n.ref.name === `refs/tags/${name}`);
    return node && { node, item: sidebar.getTreeItem(node) };
  };
  const tagIs = (name: string, what: string, fn: (item: vscode.TreeItem) => boolean) =>
    until(
      `${name} が ${what}`,
      () => {
        const found = tagItem(name);
        return found && fn(found.item) && found;
      },
      undefined,
      () => {
        const item = tagItem(name)?.item;
        return `${item?.contextValue}「${String(item?.description)}」${errors.slice(-2).join('、')}`;
      },
    );
  const fetch = () => vscode.commands.executeCommand('amonhen.fetch');
  const tag = 'e2e-tag';
  const remote = 'e2e-mirror';
  try {
    // まだ origin にないタグは「どのリモートにもない」。Push で origin に置く
    git('tag', tag);
    await fetch();
    let found = await tagIs(tag, 'どのリモートにもない', (i) => String(i.description).startsWith('not on any remote'));
    assert.match(String(found.item.contextValue), /;push/);
    assert.doesNotMatch(String(found.item.contextValue), /;onRemote/);
    await vscode.commands.executeCommand('amonhen.tag.push', found.node);
    assert.equal(inOrigin('rev-parse', tag).trim(), git('rev-parse', tag).trim());
    found = await tagIs(tag, 'origin にある', (i) => /;onRemote/.test(String(i.contextValue)));
    assert.doesNotMatch(String(found.item.contextValue), /;push/);
    assert.equal(found.item.description, git('log', '-1', '--format=%s', tag).trim());

    // origin で同じ名前の別のタグにすると、fetch の後に「違う」と出る。Replace で手元のものに置き換える
    inOrigin('tag', '-f', tag, `${tag}~1`);
    await fetch();
    found = await tagIs(tag, 'origin で違う', (i) => String(i.description).startsWith('different on origin'));
    assert.match(String(found.item.contextValue), /;replace/);
    await withWindow({ showWarningMessage: async () => 'Replace' }, async () =>
      vscode.commands.executeCommand('amonhen.tag.replace', found.node),
    );
    assert.equal(inOrigin('rev-parse', tag).trim(), git('rev-parse', tag).trim());

    // origin から消す。手元のものは残る
    found = await tagIs(tag, '置き換わる', (i) => !String(i.description).startsWith('different'));
    await withWindow({ showWarningMessage: async () => 'Delete' }, async () =>
      vscode.commands.executeCommand('amonhen.tag.deleteFromRemote', found.node),
    );
    assert.equal(inOrigin('tag', '--list', tag).trim(), '');
    found = await tagIs(tag, '消えたと分かる', (i) => String(i.description).startsWith('not on any remote'));

    // 手元のタグを消し、Undo で戻す
    const oid = git('rev-parse', tag).trim();
    await withWindow({ showWarningMessage: async () => 'Delete', showInformationMessage: async () => undefined }, async () =>
      vscode.commands.executeCommand('amonhen.tag.delete', found.node),
    );
    assert.equal(git('tag', '--list', tag).trim(), '');
    await vscode.commands.executeCommand('amonhen.undo');
    await until('タグの Undo', () => git('tag', '--list', tag).trim() === tag);
    assert.equal(git('rev-parse', tag).trim(), oid);

    // SSH の鍵で署名したタグ（git 2.34 から）は、allowed signers に載っていれば Verified
    const [major, minor] = git('--version').match(/(\d+)\.(\d+)/)!.slice(1).map(Number) as [number, number];
    if (major > 2 || (major === 2 && minor >= 34)) {
      const key = join(dirname(root), 'e2e-signing-key');
      childProcess.execFileSync('ssh-keygen', ['-q', '-t', 'ed25519', '-N', '', '-C', 'a@example.com', '-f', key]);
      const signers = join(dirname(root), 'e2e-allowed-signers');
      writeFileSync(signers, `a@example.com ${readFileSync(`${key}.pub`, 'utf8')}`);
      git('-c', 'gpg.format=ssh', '-c', `user.signingkey=${key}`, 'tag', '-s', '-m', 'signed', 'e2e-signed');
      git('config', 'gpg.ssh.allowedSignersFile', signers);
      await vscode.commands.executeCommand('amonhen.refresh');
      const signed = await tagIs('e2e-signed', 'Verified', (i) => (i.iconPath as vscode.ThemeIcon | undefined)?.id === 'verified-filled');
      assert.match((signed.item.tooltip as vscode.MarkdownString).value, /\*\*Verified\*\* \(ssh\)/);
      assert.equal((tagItem(tag)!.item.iconPath as vscode.ThemeIcon).id, 'tag');
    }

    // リモートを足し、名前と URL を変え、prune して、外す
    await withWindow({ showInputBox: inputs(remote, origin) }, async () => vscode.commands.executeCommand('amonhen.remote.add'));
    assert.equal(git('remote', 'get-url', remote).trim(), origin);
    const remoteNode = (name: string) =>
      until(`リモート ${name}`, async () => {
        await repo.refresh();
        const section = sidebar.getChildren().find((n) => n.kind === 'section' && n.section === 'remotes')!;
        return sidebar.getChildren(section).find((n) => n.kind === 'remote' && n.remote.name === name);
      });
    await withWindow({ showInputBox: inputs(`${remote}-2`) }, async () =>
      vscode.commands.executeCommand('amonhen.remote.rename', await remoteNode(remote)),
    );
    assert.equal(git('remote', 'get-url', `${remote}-2`).trim(), origin);
    const url = join(origin, '.git');
    await withWindow({ showInputBox: inputs(url) }, async () =>
      vscode.commands.executeCommand('amonhen.remote.editUrl', await remoteNode(`${remote}-2`)),
    );
    assert.equal(git('remote', 'get-url', `${remote}-2`).trim(), url);
    // prune で、リモートで消えたブランチの追跡が消える
    git('update-ref', `refs/remotes/${remote}-2/gone`, 'HEAD');
    await vscode.commands.executeCommand('amonhen.remote.prune', await remoteNode(`${remote}-2`));
    assert.equal(git('for-each-ref', `refs/remotes/${remote}-2/gone`).trim(), '');
    await withWindow({ showWarningMessage: async () => 'Remove' }, async () =>
      vscode.commands.executeCommand('amonhen.remote.remove', await remoteNode(`${remote}-2`)),
    );
    assert.equal(git('remote').split('\n').includes(`${remote}-2`), false);
  } finally {
    for (const t of [tag, 'e2e-signed']) if (git('tag', '--list', t).trim()) git('tag', '-d', t);
    if (inOrigin('tag', '--list', tag).trim()) inOrigin('tag', '-d', tag);
    for (const r of [remote, `${remote}-2`]) if (git('remote').split('\n').includes(r)) git('remote', 'remove', r);
    try {
      git('config', '--unset', 'gpg.ssh.allowedSignersFile');
    } catch {
      // 署名を試していない
    }
    await repo.refresh();
  }
}

/**
 * worktree を作ると Working Copy の下に並び（書き込む先は ✎）、書き込む先がそれになる。変更の数が出て、
 * 別の worktree が checkout しているブランチに印が付く。メインに戻し、消し、フォルダの消えた worktree を片付ける。
 * どれもコアの命令（daemon だけ）
 */
async function worktreeFlow(): Promise<void> {
  const sidebar = api.sidebar;
  const manager = api.manager;
  const main = manager.repositories.find((r) => samePath(r.root, root))!;
  const branch = git('symbolic-ref', '--short', 'HEAD').trim();
  const folder = join(dirname(root), 'repo-e2e-wt');
  const gone = join(dirname(root), 'repo-e2e-gone');
  const late = join(dirname(root), 'repo-e2e-late');
  const rows = () => {
    const wc = sidebar.getChildren().find((n) => n.kind === 'view' && n.view === 'workingCopy')!;
    return sidebar.getChildren(wc).flatMap((n) => (n.kind === 'cleanWorktrees' ? sidebar.getChildren(n) : [n]));
  };
  const row = (path: string) => {
    const node = rows().find((n) => n.kind === 'worktree' && samePath(n.worktree.path, path));
    return node && { node, item: sidebar.getTreeItem(node) };
  };
  const icon = (item: vscode.TreeItem) => (item.iconPath as vscode.ThemeIcon | undefined)?.id;
  try {
    // HEAD から e2e-wt を作り、それを checkout した worktree を書き込む先にする
    await withWindow({ showInputBox: inputs('e2e-wt', folder) }, async () => vscode.commands.executeCommand('amonhen.worktree.new'));
    assert.equal(git('-C', folder, 'symbolic-ref', '--short', 'HEAD').trim(), 'e2e-wt');
    await until('新しい worktree が書き込む先', () => manager.current && samePath(manager.current.root, folder));
    const editing = await until('✎ の行', () => {
      const r = row(folder);
      return r && icon(r.item) === 'edit' && r;
    });
    assert.match(String(editing.item.description), /^e2e-wt · no changes/);
    assert.equal(icon(row(root)!.item), 'home');
    // メインの作業ツリーが checkout しているブランチに ⌂ が付く
    const branches = sidebar.getChildren(sidebar.getChildren().find((n) => n.kind === 'section' && n.section === 'branches')!);
    const mainBranch = branches.find((n) => n.kind === 'branch' && n.ref.name === `refs/heads/${branch}`)!;
    assert.match(String(sidebar.getTreeItem(mainBranch).description), new RegExp(`⌂ ${basename(root)}`));

    // worktree の変更の数が出る
    writeFileSync(join(folder, 'wt-change.txt'), 'change\n');
    await vscode.commands.executeCommand('amonhen.refresh');
    await until('変更の数', () => /1 changed/.test(String(row(folder)?.item.description)));

    // メインに戻す
    await vscode.commands.executeCommand('amonhen.worktree.edit', row(root)!.node);
    assert.ok(samePath(manager.current!.root, root));
    await until('✎ が戻る', () => icon(row(root)!.item) === 'edit');

    // 変更があっても、確かめてから消す（ブランチは残る）
    await withWindow({ showWarningMessage: async () => 'Remove Worktree' }, async () =>
      vscode.commands.executeCommand('amonhen.worktree.remove', row(folder)!.node),
    );
    await until('フォルダーが消える', () => !existsSync(folder) && !git('worktree', 'list').includes('repo-e2e-wt'), 10_000, lastErrors);
    assert.equal(git('branch', '--list', 'e2e-wt').trim(), 'e2e-wt');

    // メインの側の変更の数は古いことがある（変更のない worktree は、HEAD か index が変わるまで数え直さない）。消す前に
    // 数え直し、後から足したファイルがあれば、それも消すと確かめる
    git('worktree', 'add', '-q', '-b', 'e2e-late', late);
    git('-C', late, 'commit', '-q', '--allow-empty', '-m', 'e2e-late');
    await new Promise((r) => setTimeout(r, 1000));
    await sidebar.worktrees.reload();
    await until('後から足す worktree', () => row(late));
    writeFileSync(join(late, 'late.txt'), 'late\n');
    let detail = '';
    await withWindow(
      {
        showWarningMessage: async (_message: string, options: vscode.MessageOptions) => {
          detail = String(options.detail);
          return 'Remove Worktree';
        },
      },
      async () => vscode.commands.executeCommand('amonhen.worktree.remove', row(late)!.node),
    );
    assert.match(detail, /uncommitted changes in 1 files/);
    await until('後から足したファイルごと消える', () => !existsSync(late) && !git('worktree', 'list').includes('repo-e2e-late'), 10_000, lastErrors);

    // フォルダの消えた worktree は「folder is gone」と出て、Prune で片付く。git が prunable と言うのは 2.31 から
    git('worktree', 'add', '-q', '-b', 'e2e-gone', gone);
    rmSync(gone, { recursive: true, force: true });
    const [major, minor] = git('--version').match(/(\d+)\.(\d+)/)!.slice(1).map(Number) as [number, number];
    if (major > 2 || (major === 2 && minor >= 31)) {
      await sidebar.worktrees.reload();
      await until('folder is gone', () => /folder is gone/.test(String(row(gone)?.item.description)));
    }
    await vscode.commands.executeCommand('amonhen.worktree.prune');
    await until('片付く', () => !row(gone) && !git('worktree', 'list').includes('repo-e2e-gone'));
  } finally {
    manager.select(main);
    await manager.forget(folder);
    for (const path of [folder, gone, late]) {
      if (git('worktree', 'list').includes(basename(path))) git('worktree', 'remove', '--force', path);
    }
    git('worktree', 'prune');
    for (const b of ['e2e-wt', 'e2e-gone', 'e2e-late']) if (git('branch', '--list', b).trim()) git('branch', '-D', b);
    await sidebar.worktrees.reload();
  }
}

/**
 * ファイルの右クリックの File History で、名前の変更を追ってファイルを変えたコミットが出る。マージも出す・出さないを
 * 切り替え、行のコミットを History で選ぶ。コアの命令（daemon だけ）
 */
async function fileHistoryFlow(): Promise<void> {
  const panel = await showView('amonhen.showHistory');
  const start = git('rev-parse', 'HEAD').trim();
  const commit = (message: string) => git('commit', '-q', '-m', message);
  try {
    writeFileSync(join(root, 'fh-a.txt'), 'one\n');
    git('add', 'fh-a.txt');
    commit('add fh-a');
    writeFileSync(join(root, 'fh-a.txt'), 'one\ntwo\n');
    git('commit', '-q', '-a', '-m', 'change fh-a');
    git('mv', 'fh-a.txt', 'fh-b.txt');
    commit('rename fh-a to fh-b');
    git('commit', '-q', '--allow-empty', '-m', 'not about fh');
    await vscode.commands.executeCommand('amonhen.refresh');

    // エクスプローラーの右クリック（Uri）から
    let mark = posted.length;
    await vscode.commands.executeCommand('amonhen.fileHistory', vscode.Uri.file(join(root, 'fh-b.txt')));
    const shown = await waitPosted('fileHistory', mark, (m) => m.path === 'fh-b.txt' && m.complete);
    assert.deepEqual(
      shown.entries.map((e) => [e.subject, e.path]),
      [
        ['rename fh-a to fh-b', 'fh-b.txt'],
        ['change fh-a', 'fh-a.txt'],
        ['add fh-a', 'fh-a.txt'],
      ],
    );
    assert.equal(shown.merges, false);

    // 行を選ぶと、そのコミットの詳細（History と同じ）が届く
    const oldest = shown.entries[2]!;
    mark = posted.length;
    await panel.history.handle({ type: 'select', oid: oldest.oid });
    assert.deepEqual((await waitPosted('details', mark, (m) => m.details.oid === oldest.oid)).details.files.map((f) => f.path), ['fh-a.txt']);

    // マージも出すに切り替えると読み直す
    mark = posted.length;
    await panel.fileHistory.handle({ type: 'fileHistoryLoad', merges: true });
    // 前の読み込みの答えが後から届くことがある。切り替えた答えを待つ
    await waitPosted('fileHistory', mark, (m) => m.complete && m.merges);

    // ダブルクリックで、そのコミットを History で選ぶ
    mark = posted.length;
    await panel.fileHistory.handle({ type: 'fileHistoryReveal', oid: oldest.oid });
    await waitPosted('reveal', mark);

    // メインビューのファイルの右クリック（リポジトリの中のパス）から
    mark = posted.length;
    await vscode.commands.executeCommand('amonhen.fileHistory', { path: 'fh-b.txt' });
    await waitPosted('fileHistory', mark, (m) => m.path === 'fh-b.txt' && m.entries.length === 3);
  } finally {
    git('reset', '-q', '--hard', start);
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

/**
 * ファイルの右クリックの Blame で、各行を最後に変えたコミットが出る（まだコミットしていない行も）。作業中の変更で
 * 読み直し、コミットの行からはそのコミットの内容を出し、行のコミットを History で選ぶ。コアの命令（daemon だけ）
 */
async function blameFlow(): Promise<void> {
  const panel = await showView('amonhen.showHistory');
  const start = git('rev-parse', 'HEAD').trim();
  const file = join(root, 'bl.txt');
  try {
    writeFileSync(file, 'one\ntwo\n');
    git('add', 'bl.txt');
    git('commit', '-q', '-m', 'add bl');
    const first = git('rev-parse', 'HEAD').trim();
    writeFileSync(file, 'one\nTWO\nthree\n');
    git('commit', '-q', '-a', '-m', 'change bl');
    writeFileSync(file, 'one\nTWO\nthree\nfour\n');
    await vscode.commands.executeCommand('amonhen.refresh');

    // エクスプローラーの右クリック（Uri）から: 作業ツリーの内容
    let mark = posted.length;
    await vscode.commands.executeCommand('amonhen.blame', vscode.Uri.file(file));
    const shown = await waitPosted('blame', mark, (m) => m.path === 'bl.txt' && !m.loading);
    const by = (m: typeof shown) => m.lines.map((l) => [l.t, /^0+$/.test(m.commits[l.c]!.oid) ? '' : m.commits[l.c]!.summary]);
    assert.equal(shown.rev, undefined);
    assert.deepEqual(by(shown), [
      ['one', 'add bl'],
      ['TWO', 'change bl'],
      ['three', 'change bl'],
      ['four', ''],
    ]);

    // 作業中の変更で読み直す
    mark = posted.length;
    writeFileSync(file, 'one\nTWO\nthree\nfour\nfive\n');
    await vscode.commands.executeCommand('amonhen.refresh');
    await waitPosted('blame', mark, (m) => m.lines.length === 5);

    // コミットの行（History の詳細など）から: そのコミットの内容
    mark = posted.length;
    await vscode.commands.executeCommand('amonhen.blame', { path: 'bl.txt', oid: first });
    const old = await waitPosted('blame', mark, (m) => m.rev === first && !m.loading);
    assert.deepEqual(by(old), [
      ['one', 'add bl'],
      ['two', 'add bl'],
    ]);

    // ダブルクリックで、そのコミットを History で選ぶ
    mark = posted.length;
    await panel.blame.handle({ type: 'blameReveal', oid: first });
    await waitPosted('reveal', mark);
  } finally {
    rmSync(file, { force: true });
    git('reset', '-q', '--hard', start);
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

/**
 * エンジンがあれば Branches がアプリと同じ並びになる: 90 日コミットのないブランチは Inactive、アーカイブしたものは
 * Archived に畳まれ、お気に入りは ★ で上に並ぶ（順を変えられる）。どれも git の設定に書く（daemon だけ）
 */
async function branchArrangeFlow(): Promise<void> {
  // 絞り込み（Branches・Remotes・Tags）も、ここで確かめる
  const sidebar = api.sidebar;
  const section = sidebar.getChildren().find((n) => n.kind === 'section' && n.section === 'branches')!;
  /** Branches の中のブランチの名前を、入れ子・グループごとに */
  const shape = (nodes = sidebar.getChildren(section)): unknown[] =>
    nodes.map((n) =>
      n.kind === 'branch'
        ? n.stack?.length
          ? [n.label, shape(sidebar.getChildren(n))]
          : n.label
        : n.kind === 'branchGroup'
          ? { [n.key]: shape(sidebar.getChildren(n)) }
          : n.kind === 'branchFolder'
            ? { [`${n.label}/`]: shape(sidebar.getChildren(n)) }
            : n.kind,
    );
  const node = (name: string) => {
    const find = (nodes: ReturnType<typeof sidebar.getChildren>): ReturnType<typeof sidebar.getChildren>[number] | undefined => {
      for (const n of nodes) {
        if (n.kind === 'branch' && n.ref.name === `refs/heads/${name}`) return n;
        const inner = n.kind === 'branch' || n.kind === 'branchGroup' || n.kind === 'branchFolder' ? find(sidebar.getChildren(n)) : undefined;
        if (inner) return inner;
      }
      return undefined;
    };
    return find(sidebar.getChildren(section))!;
  };
  const config = (key: string) => {
    try {
      return git('config', key).trim();
    } catch {
      return undefined;
    }
  };
  const settle = async () => {
    await vscode.commands.executeCommand('amonhen.refresh');
    await sidebar.branchFacts.reload();
  };
  const names = ['arr-old', 'arr-a', 'arr-b', 'arr-arch'];
  const head = git('rev-parse', 'HEAD').trim();
  const current = git('symbolic-ref', '--short', 'HEAD').trim();
  try {
    // 200 日前のコミットだけのブランチ
    const old = new Date(Date.now() - 200 * 86_400_000).toISOString();
    const tree = git('rev-parse', 'HEAD^{tree}').trim();
    const commit = childProcess
      .execFileSync('git', ['commit-tree', tree, '-p', head, '-m', 'old work'], {
        cwd: root,
        encoding: 'utf8',
        env: { ...process.env, GIT_AUTHOR_DATE: old, GIT_COMMITTER_DATE: old },
      })
      .trim();
    git('branch', 'arr-old', commit);
    for (const b of ['arr-a', 'arr-b', 'arr-arch']) git('branch', b, head);
    await settle();
    await until('Inactive', () => JSON.stringify(shape()).includes('{"inactive":["arr-old"'));

    // 絞り込むと、名前にどの言葉も含むものだけが平らに出る（大文字小文字は区別しない）
    await withWindow({ showInputBox: inputs('ARR- ch') }, async () => vscode.commands.executeCommand('amonhen.filterRefs'));
    assert.deepEqual(shape(), ['arr-arch']);
    const remotes = sidebar.getChildren().find((n) => n.kind === 'section' && n.section === 'remotes')!;
    assert.deepEqual(sidebar.getChildren(remotes), []);
    assert.match(String(sidebar.getTreeItem(section).id), /:filtered$/);
    await vscode.commands.executeCommand('amonhen.clearRefsFilter');
    assert.equal(sidebar.filter, '');
    assert.ok(JSON.stringify(shape()).includes('{"inactive":["arr-old"'));

    // アーカイブすると Archived に移り、戻すと元の場所に戻る
    await vscode.commands.executeCommand('amonhen.branch.archive', node('arr-arch'));
    assert.equal(config('branch.arr-arch.amonhenArchived'), 'true');
    await until('Archived', () => JSON.stringify(shape()).includes('{"archived":["arr-arch"]}'));
    assert.match(String(sidebar.getTreeItem(node('arr-arch')).contextValue), /;archived/);
    await vscode.commands.executeCommand('amonhen.branch.unarchive', node('arr-arch'));
    assert.equal(config('branch.arr-arch.amonhenArchived'), undefined);
    await until('Archived が消える', () => !JSON.stringify(shape()).includes('archived'));

    // お気に入りは ★ で一番上に、足した順に並び、順を変えられる。いくつも選んでもよい
    await vscode.commands.executeCommand('amonhen.branch.addToFavorites', node('arr-b'), [node('arr-b'), node('arr-a')]);
    assert.deepEqual([config('branch.arr-b.amonhenPinned'), config('branch.arr-a.amonhenPinned')], ['1', '2']);
    await until('お気に入り', () => {
      const top = shape().slice(0, 2);
      return JSON.stringify(top) === JSON.stringify(['arr-b', 'arr-a']);
    });
    assert.equal((sidebar.getTreeItem(node('arr-a')).iconPath as vscode.ThemeIcon).id, 'star-full');
    await vscode.commands.executeCommand('amonhen.branch.favoriteUp', node('arr-a'));
    await until('並べ替え', () => JSON.stringify(shape().slice(0, 2)) === JSON.stringify(['arr-a', 'arr-b']));
    await vscode.commands.executeCommand('amonhen.branch.removeFromFavorites', node('arr-a'), [node('arr-a'), node('arr-b')]);
    assert.deepEqual([config('branch.arr-a.amonhenPinned'), config('branch.arr-b.amonhenPinned')], [undefined, undefined]);
    await until('お気に入りが消える', () => (sidebar.getTreeItem(node('arr-a')).iconPath as vscode.ThemeIcon).id === 'git-branch');

    // いくつも選んで merge すると 1 つのコミットになり（Undo 1 回で戻る）、まとめて消せる
    for (const b of ['arr-a', 'arr-b']) {
      git('switch', '-q', b);
      writeFileSync(join(root, `${b}.txt`), `${b}\n`);
      git('add', `${b}.txt`);
      git('commit', '-q', '-m', b);
    }
    git('switch', '-q', current);
    await settle();
    const before = git('rev-parse', 'HEAD').trim();
    await withWindow(
      {
        showQuickPick: async (items: readonly vscode.QuickPickItem[]) => items[0],
        showInformationMessage: async () => undefined,
      },
      async () => vscode.commands.executeCommand('amonhen.nav.merge', node('arr-a'), [node('arr-a'), node('arr-b')]),
    );
    // HEAD はどちらの祖先でもあるので、git は片方へ fast-forward してから、もう片方を merge する
    const parents = git('rev-list', '--parents', '-n', '1', 'HEAD').trim().split(' ').slice(1);
    assert.deepEqual(parents.sort(), [git('rev-parse', 'arr-a').trim(), git('rev-parse', 'arr-b').trim()].sort());
    await vscode.commands.executeCommand('amonhen.undo');
    await until('merge の Undo', () => git('rev-parse', 'HEAD').trim() === before);
    await settle();
    // 取り込んでいないので、もう一度確かめてから消す
    const answer = async (message: string) => (/merge されていない/.test(message) ? 'Delete Anyway' : 'Delete');
    await withWindow({ showWarningMessage: answer }, async () =>
      vscode.commands.executeCommand('amonhen.nav.delete', node('arr-a'), [node('arr-a'), node('arr-b')]),
    );
    assert.equal(git('branch', '--list', 'arr-a', 'arr-b').trim(), '');
  } finally {
    if (git('symbolic-ref', '--short', 'HEAD').trim() !== current) git('switch', '-q', current);
    for (const b of names) if (git('branch', '--list', b).trim()) git('branch', '-D', b);
    await settle();
  }
}

/**
 * History の行に署名の印が出て（SSH の鍵で署名したコミット。git 2.34 から）、詳細に署名と子のコミットが出る。
 * HEAD・親・子のコミットに移れる。どれもコアの命令（daemon だけ）
 */
async function historySignatureFlow(): Promise<void> {
  const panel = await showView('amonhen.showHistory');
  const start = git('rev-parse', 'HEAD').trim();
  const key = join(dirname(root), 'e2e-commit-key');
  const signers = join(dirname(root), 'e2e-commit-signers');
  const [major, minor] = git('--version').match(/(\d+)\.(\d+)/)!.slice(1).map(Number) as [number, number];
  const ssh = major > 2 || (major === 2 && minor >= 34);
  try {
    if (ssh) {
      childProcess.execFileSync('ssh-keygen', ['-q', '-t', 'ed25519', '-N', '', '-C', 'a@example.com', '-f', key]);
      writeFileSync(signers, `a@example.com ${readFileSync(`${key}.pub`, 'utf8')}`);
      git('config', 'gpg.ssh.allowedSignersFile', signers);
      git('-c', 'gpg.format=ssh', '-c', `user.signingkey=${key}`, 'commit', '-q', '-S', '--allow-empty', '-m', 'signed commit');
    } else {
      git('commit', '-q', '--allow-empty', '-m', 'signed commit');
    }
    const signed = git('rev-parse', 'HEAD').trim();
    git('commit', '-q', '--allow-empty', '-m', 'after signed');
    const after = git('rev-parse', 'HEAD').trim();
    let mark = posted.length;
    await vscode.commands.executeCommand('amonhen.refresh');
    await panel.history.reset();
    await waitPosted('rows', mark, (m) => m.rows.some((r) => r.oid === signed));
    if (ssh) {
      const marks = await waitPosted('signatures', mark, (m) => signed in m.marks);
      assert.equal(marks.marks[signed]!.kind, 'verified');
      assert.equal(marks.marks[after], undefined, '署名のないコミットに印はない');
    }

    // 詳細に署名と子のコミット
    mark = posted.length;
    await panel.history.handle({ type: 'select', oid: signed });
    const extras = (await waitPosted('detailsExtras', mark, (m) => m.extras.oid === signed)).extras;
    assert.deepEqual(extras.children, [after]);
    if (ssh) assert.equal(extras.signature?.label, 'Verified');

    // 子・親・HEAD へ移る
    for (const [to, oid] of [
      ['child', signed],
      ['parent', after],
      ['head', undefined],
    ] as const) {
      mark = posted.length;
      await panel.history.handle({ type: 'goTo', to, ...(oid ? { oid } : {}) });
      await waitPosted('reveal', mark);
    }
  } finally {
    git('reset', '-q', '--hard', start);
    try {
      git('config', '--unset', 'gpg.ssh.allowedSignersFile');
    } catch {
      // 署名を試していない
    }
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

/**
 * shallow で partial な clone を開くと、History に Shallow・Partial の札と境目の印、最後に取り足す帯が出る。
 * Fetch More History で取り足すと、shallow でなくなる。コアの命令（daemon だけ）
 */
async function shallowFlow(): Promise<void> {
  const manager = api.manager;
  const main = manager.current!;
  const origin = git('remote', 'get-url', 'origin').trim();
  const clone = join(dirname(root), 'shallow-e2e');
  await showView('amonhen.showHistory');
  try {
    // partial clone の中身をすべて取るのは git 2.36 から（古い git では shallow だけを確かめる）
    const [major, minor] = git('--version').match(/(\d+)\.(\d+)/)!.slice(1).map(Number) as [number, number];
    const partial = major > 2 || (major === 2 && minor >= 36);
    childProcess.execFileSync('git', ['-C', origin, 'config', 'uploadpack.allowFilter', 'true']);
    const url = `file://${origin.replaceAll('\\', '/')}`;
    childProcess.execFileSync('git', ['clone', '-q', '--depth', '1', ...(partial ? ['--filter=blob:none'] : []), url, clone]);
    const repo = (await manager.openPath(clone))!;
    let mark = posted.length;
    manager.select(repo);
    const form = await waitPosted('form', mark, (m) => m.shallow);
    assert.equal(form.boundary.length, 1, 'the one commit fetched is the boundary');
    if (partial) assert.equal(form.partial?.filter, 'blob:none');

    mark = posted.length;
    await withWindow({ showQuickPick: async (items: readonly vscode.QuickPickItem[]) => items[0] }, async () =>
      vscode.commands.executeCommand('amonhen.fetchMoreHistory'),
    );
    assert.equal(childProcess.execFileSync('git', ['-C', clone, 'rev-parse', '--is-shallow-repository'], { encoding: 'utf8' }).trim(), 'false');
    await waitPosted('form', mark, (m) => !m.shallow && m.boundary.length === 0);
  } finally {
    manager.select(main);
    await manager.forget(clone);
    // Windows では daemon がまだ見張っていて消せないことがある（e2e の一時ディレクトリごと、終わりに消える）
    try {
      rmSync(clone, { recursive: true, force: true });
    } catch {
      // 後で消える
    }
    childProcess.execFileSync('git', ['-C', origin, 'config', '--unset', 'uploadpack.allowFilter']);
  }
}

/**
 * Submodules の行に印と URL・追うブランチが出る。Add Submodule… で足し、追うブランチと URL を変え、Update… で追う
 * ブランチの最新にし（違うコミットの印）、Remove… で外す（.git/modules は残す）。コアの命令（daemon だけ）
 */
async function submoduleFlow(): Promise<void> {
  const sidebar = api.sidebar;
  const repo = api.manager.current!;
  const start = git('rev-parse', 'HEAD').trim();
  const lib = join(dirname(root), 'sm-lib');
  const moved = join(dirname(root), 'sm-lib-moved');
  const path = 'vendor/sm-lib';
  const gitDir = git('rev-parse', '--absolute-git-dir').trim();
  const libGit = (...args: string[]) => childProcess.execFileSync('git', ['-C', lib, ...args], { encoding: 'utf8' });
  const url = (p: string) => `file://${p.replaceAll('\\', '/')}`;
  const section = () => sidebar.getChildren().find((n) => n.kind === 'section' && n.section === 'submodules')!;
  const row = () => sidebar.getChildren(section()).find((n) => n.kind === 'submodule' && n.submodule.path === path);
  const gitmodules = (key: string) => {
    try {
      return git('config', '--file', '.gitmodules', `submodule.${path}.${key}`).trim();
    } catch {
      return undefined;
    }
  };
  // git 2.38.1 から、submodule の clone は file の URL を断る（試験の HOME の設定で許す）
  git('config', '--global', 'protocol.file.allow', 'always');
  childProcess.execFileSync('git', ['init', '-q', lib]);
  libGit('symbolic-ref', 'HEAD', 'refs/heads/main');
  libGit('-c', 'user.name=a', '-c', 'user.email=a@example.com', 'commit', '-q', '--allow-empty', '-m', 'lib one');
  try {
    // Submodules を開く（開くまでは読まない）
    sidebar.onDidExpand(section());
    await withWindow({ showInputBox: inputs(url(lib), path, '') }, async () => vscode.commands.executeCommand('amonhen.submodules.add'));
    assert.equal(gitmodules('url'), url(lib));
    await until('submodule の行', () => row());
    const fact = await until('取り出してある', () => sidebar.submoduleFacts.get(path)?.initialized && sidebar.submoduleFacts.get(path));
    assert.equal(fact.url, url(lib));
    assert.equal(sidebar.getTreeItem(row()!).description, undefined);
    assert.match(String(sidebar.getTreeItem(row()!).tooltip), /Follows the recorded commit/);

    // 追うブランチと URL を変える
    await withWindow({ showInputBox: inputs('main') }, async () => vscode.commands.executeCommand('amonhen.submodule.setBranch', row()));
    assert.equal(gitmodules('branch'), 'main');
    cpSync(lib, moved, { recursive: true });
    await withWindow({ showInputBox: inputs(url(moved)) }, async () => vscode.commands.executeCommand('amonhen.submodule.editUrl', row()));
    assert.equal(gitmodules('url'), url(moved));
    assert.equal(git('config', `submodule.${path}.url`).trim(), url(moved));

    // 追うブランチの最新にすると、記録と違うコミットになる
    childProcess.execFileSync('git', ['-C', moved, '-c', 'user.name=a', '-c', 'user.email=a@example.com', 'commit', '-q', '--allow-empty', '-m', 'lib two']);
    await withWindow(
      { showQuickPick: async (items: readonly (vscode.QuickPickItem & { key: string })[]) => items.filter((i) => i.key === 'remote') },
      async () => vscode.commands.executeCommand('amonhen.submodule.updateWith', row()),
    );
    assert.equal(git('-C', path, 'log', '-1', '--format=%s').trim(), 'lib two');
    await until('違うコミットの印', () => /another commit/.test(String(sidebar.getTreeItem(row()!).description)));

    // 外す。.git/modules は残す（git の既定）。.gitmodules の変更が stage されていないと、git rm が断る
    git('add', '.gitmodules');
    await withWindow({ showWarningMessage: async () => 'Remove' }, async () => vscode.commands.executeCommand('amonhen.submodule.remove', row()));
    assert.equal(gitmodules('url'), undefined, lastErrors());
    assert.ok(!existsSync(join(root, path)));
    assert.ok(existsSync(join(gitDir, 'modules', path)));
    await until('行が消える', () => !row());
  } finally {
    git('reset', '-q', '--hard', start);
    rmSync(join(root, 'vendor'), { recursive: true, force: true });
    rmSync(join(gitDir, 'modules'), { recursive: true, force: true });
    try {
      git('config', '--remove-section', `submodule.${path}`);
    } catch {
      // ない
    }
    git('config', '--global', '--unset', 'protocol.file.allow');
    for (const p of [lib, moved]) rmSync(p, { recursive: true, force: true });
    await repo.refresh();
  }
}

/**
 * git に git subtree があれば Subtrees が出る。Add Subtree… で取り込み、相手を覚え、Pull… で新しいコミットを取り込み、
 * Split… でフォルダの履歴だけのブランチを作り、Push… で相手に送る。コアの命令（daemon だけ）
 */
async function subtreeFlow(): Promise<void> {
  const sidebar = api.sidebar;
  const repo = api.manager.current!;
  const facts = sidebar.subtreeFacts;
  await facts.reload();
  if (!facts.available) return;
  const start = git('rev-parse', 'HEAD').trim();
  const lib = join(dirname(root), 'st-lib');
  const dir = 'vendor/st-lib';
  const libGit = (...args: string[]) =>
    childProcess.execFileSync('git', ['-C', lib, '-c', 'user.name=a', '-c', 'user.email=a@example.com', ...args], { encoding: 'utf8' });
  const url = `file://${lib.replaceAll('\\', '/')}`;
  const quiet = { showInformationMessage: async () => undefined };
  const keepHistory = { showQuickPick: async (items: readonly (vscode.QuickPickItem & { squash: boolean })[]) => items.find((i) => !i.squash) };
  childProcess.execFileSync('git', ['init', '-q', lib]);
  libGit('symbolic-ref', 'HEAD', 'refs/heads/main');
  writeFileSync(join(lib, 'one.txt'), 'one\n');
  libGit('add', 'one.txt');
  libGit('commit', '-q', '-m', 'lib one');
  try {
    const section = () => sidebar.getChildren().find((n) => n.kind === 'section' && n.section === 'subtrees');
    assert.ok(section(), 'git subtree があれば Subtrees が出る');
    await withWindow({ ...quiet, ...keepHistory, showInputBox: inputs(url, 'main', dir) }, async () =>
      vscode.commands.executeCommand('amonhen.subtrees.add'),
    );
    assert.ok(existsSync(join(root, dir, 'one.txt')));
    const added = await until('覚えた相手', () => facts.get(dir)?.url === url && facts.get(dir));
    assert.equal(added.ref, 'main');
    const row = () => sidebar.getChildren(section()!).find((n) => n.kind === 'subtree' && n.subtree.dir === dir)!;
    assert.match(String(sidebar.getTreeItem(row()).tooltip), /From .*st-lib \(main\)/);

    // 相手の新しいコミットを取り込む
    writeFileSync(join(lib, 'two.txt'), 'two\n');
    libGit('add', 'two.txt');
    libGit('commit', '-q', '-m', 'lib two');
    await withWindow({ ...quiet, ...keepHistory, showInputBox: inputs(url, 'main') }, async () =>
      vscode.commands.executeCommand('amonhen.subtree.pull', row()),
    );
    assert.ok(existsSync(join(root, dir, 'two.txt')));

    // フォルダの履歴だけのブランチ
    await withWindow({ ...quiet, showInputBox: inputs('st-split') }, async () => vscode.commands.executeCommand('amonhen.subtree.split', row()));
    assert.equal(git('show', 'st-split:two.txt').replaceAll('\r\n', '\n'), 'two\n');

    // 相手の別のブランチに送る
    await withWindow({ ...quiet, showInputBox: inputs(url, 'from-repo') }, async () => vscode.commands.executeCommand('amonhen.subtree.push', row()));
    assert.equal(libGit('rev-parse', 'from-repo').trim(), git('rev-parse', 'st-split').trim());
    await until('Push の先を覚える', () => facts.get(dir)?.pushRef === 'from-repo');
  } finally {
    git('reset', '-q', '--hard', start);
    if (git('branch', '--list', 'st-split').trim()) git('branch', '-q', '-D', 'st-split');
    try {
      git('config', '--remove-section', `amonhen.subtree.${dir}`);
    } catch {
      // 覚えていない
    }
    rmSync(join(root, 'vendor'), { recursive: true, force: true });
    rmSync(lib, { recursive: true, force: true });
    await repo.refresh();
    await facts.reload();
  }
}

/**
 * History のファイルの Restore the Version Before… で、作業ツリーのファイルを 1 つ前の版にし、Undo で戻す。
 * Restore and Stage は index にも入れる。コアの命令（daemon だけ）
 */
async function restoreFlow(): Promise<void> {
  const repo = api.manager.current!;
  const start = git('rev-parse', 'HEAD').trim();
  const path = join(root, 'restore-e2e.txt');
  const read = () => readFileSync(path, 'utf8').replaceAll('\r\n', '\n');
  writeFileSync(path, 'one\n');
  git('add', 'restore-e2e.txt');
  git('commit', '-q', '-m', 'restore one');
  writeFileSync(path, 'two\n');
  git('commit', '-q', '-am', 'restore two');
  const head = git('rev-parse', 'HEAD').trim();
  try {
    await repo.refresh();
    await withWindow({ showWarningMessage: async () => 'Restore' }, async () =>
      vscode.commands.executeCommand('amonhen.restoreFile.versionBefore', { oid: head, path: 'restore-e2e.txt' }),
    );
    assert.equal(read(), 'one\n', lastErrors());
    assert.equal(git('diff', '--cached', '--name-only').trim(), '', 'Restore は index に入れない');
    await withWindow({ showWarningMessage: async () => 'Undo' }, async () => vscode.commands.executeCommand('amonhen.undo'));
    assert.equal(read(), 'two\n');

    // この版（HEAD）に戻すのは何も変えない。1 つ前の版を stage まで
    await withWindow({ showWarningMessage: async () => 'Restore and Stage' }, async () =>
      vscode.commands.executeCommand('amonhen.restoreFile.versionBefore', { oid: head, path: 'restore-e2e.txt' }),
    );
    assert.equal(git('diff', '--cached', '--name-only').trim(), 'restore-e2e.txt');
  } finally {
    git('reset', '-q', '--hard', start);
    rmSync(path, { force: true });
    await repo.refresh();
  }
}

/**
 * Stash… で stage したものだけ（git 2.35 から）・stage した変更を残して stash し、Working Copy のファイルの Stash… で
 * そのファイルだけを stash する。コアの命令（daemon だけ）
 */
async function stashOptionsFlow(): Promise<void> {
  const repo = api.manager.current!;
  const [major, minor] = git('--version').match(/(\d+)\.(\d+)/)!.slice(1).map(Number) as [number, number];
  const a = join(root, 'so-a.txt');
  const b = join(root, 'so-b.txt');
  const start = git('rev-parse', 'HEAD').trim();
  writeFileSync(a, 'a\n');
  writeFileSync(b, 'b\n');
  git('add', 'so-a.txt', 'so-b.txt');
  git('commit', '-q', '-m', 'stash options');
  const pick = (label: string) => ({
    showQuickPick: async (items: readonly vscode.QuickPickItem[]) => items.find((i) => i.label === label),
    showInputBox: inputs(''),
  });
  try {
    // stage した変更を残す: a（stage）も b（stage していない）も stash に入り、a の stage は残る
    writeFileSync(a, 'a2\n');
    writeFileSync(b, 'b2\n');
    git('add', 'so-a.txt');
    await repo.refresh();
    await withWindow(pick('Stash, Keeping the Staged Changes'), async () => vscode.commands.executeCommand('amonhen.stash'));
    assert.equal(git('diff', '--cached', '--name-only').trim(), 'so-a.txt', lastErrors());
    assert.equal(git('diff', '--name-only').trim(), '');
    assert.equal(git('stash', 'list').trim().split('\n').length, 1);
    git('reset', '-q', '--hard');
    git('stash', 'drop', '-q');

    if (major > 2 || (major === 2 && minor >= 35)) {
      // stage したものだけ: b の変更は作業ツリーに残る
      writeFileSync(a, 'a3\n');
      writeFileSync(b, 'b3\n');
      git('add', 'so-a.txt');
      await repo.refresh();
      await withWindow(pick('Stash Only the Staged Changes'), async () => vscode.commands.executeCommand('amonhen.stash'));
      assert.equal(git('diff', '--cached', '--name-only').trim(), '', lastErrors());
      assert.equal(git('diff', '--name-only').trim(), 'so-b.txt');
      assert.equal(git('stash', 'show', '--name-only').trim(), 'so-a.txt');
      git('reset', '-q', '--hard');
      git('stash', 'drop', '-q');
    }

    // ファイルを選んで: b だけを stash する
    writeFileSync(a, 'a4\n');
    writeFileSync(b, 'b4\n');
    await repo.refresh();
    await withWindow({ showInputBox: inputs('only b') }, async () => vscode.commands.executeCommand('amonhen.stash.files', { path: 'so-b.txt' }));
    assert.equal(git('diff', '--name-only').trim(), 'so-a.txt', lastErrors());
    assert.equal(git('stash', 'show', '--name-only').trim(), 'so-b.txt');
  } finally {
    git('reset', '-q', '--hard', start);
    while (git('stash', 'list').trim()) git('stash', 'drop', '-q');
    for (const p of [a, b]) rmSync(p, { force: true });
    await repo.refresh();
  }
}

/**
 * worktree をロックし（理由つき）、外し、別のフォルダに移し、手で移したものの今の場所を教える（repair は git 2.29
 * から）。コアの命令（daemon だけ）
 */
async function worktreeMoreFlow(): Promise<void> {
  const sidebar = api.sidebar;
  const repo = api.manager.current!;
  const folder = join(dirname(root), 'wt-more-a');
  const moved = join(dirname(root), 'wt-more-b');
  const byHand = join(dirname(root), 'wt-more-c');
  const [major, minor] = git('--version').match(/(\d+)\.(\d+)/)!.slice(1).map(Number) as [number, number];
  const node = async (path: string) => {
    await sidebar.worktrees.reload();
    const worktree = await until(`worktree ${path}`, () => sidebar.worktrees.worktrees.find((w) => samePath(w.path, path)));
    return { kind: 'worktree', worktree };
  };
  const porcelain = () => git('worktree', 'list', '--porcelain');
  git('worktree', 'add', '-q', '-b', 'wt-more', folder);
  try {
    // ロックは git dir の locked に理由を書く（`worktree list` が locked を出すのは git 2.31 から）
    const locked = join(git('-C', folder, 'rev-parse', '--absolute-git-dir').trim(), 'locked');
    await withWindow({ showInputBox: inputs('keep it') }, async () => vscode.commands.executeCommand('amonhen.worktree.lock', await node(folder)));
    assert.equal(existsSync(locked) && readFileSync(locked, 'utf8').trim(), 'keep it', lastErrors());
    await vscode.commands.executeCommand('amonhen.worktree.unlock', await node(folder));
    assert.ok(!existsSync(locked), lastErrors());

    await withWindow({ showInputBox: inputs(moved) }, async () => vscode.commands.executeCommand('amonhen.worktree.move', await node(folder)));
    assert.ok(existsSync(moved) && !existsSync(folder), lastErrors());
    assert.match(porcelain().replaceAll('\\', '/'), new RegExp(basename(moved)));

    if (major > 2 || (major === 2 && minor >= 29)) {
      // 手で移すと、git は場所を見失う。今の場所を教えると直る
      renameSync(moved, byHand);
      await withWindow({ showOpenDialog: async () => [vscode.Uri.file(byHand)] }, async () =>
        vscode.commands.executeCommand('amonhen.worktree.repair', await node(moved)),
      );
      assert.match(porcelain().replaceAll('\\', '/'), new RegExp(basename(byHand)), lastErrors());
      assert.equal(git('-C', byHand, 'symbolic-ref', '--short', 'HEAD').trim(), 'wt-more');
    }
  } finally {
    for (const p of [folder, moved, byHand]) {
      if (existsSync(p)) {
        try {
          // 途中で落ちたときはロックが残っている
          git('worktree', 'unlock', p);
        } catch {
          // ロックしていなければ git が断る
        }
        try {
          git('worktree', 'remove', '--force', p);
        } catch {
          rmSync(p, { recursive: true, force: true });
        }
      }
    }
    git('worktree', 'prune');
    if (git('branch', '--list', 'wt-more').trim()) git('branch', '-q', '-D', 'wt-more');
    await sidebar.worktrees.reload();
    await repo.refresh();
  }
}

/**
 * 手で clone してから足した submodule は、データを自分のフォルダに持つ。Move Repository Data into .git/modules で
 * 移し、Run in Each Submodule… でそれぞれの中で命令を動かす。コアの命令（daemon だけ）
 */
async function submoduleMoreFlow(): Promise<void> {
  const sidebar = api.sidebar;
  const repo = api.manager.current!;
  const start = git('rev-parse', 'HEAD').trim();
  const lib = join(dirname(root), 'smm-lib');
  const path = 'vendor/smm-lib';
  const gitDir = git('rev-parse', '--absolute-git-dir').trim();
  git('config', '--global', 'protocol.file.allow', 'always');
  childProcess.execFileSync('git', ['init', '-q', lib]);
  childProcess.execFileSync('git', ['-C', lib, '-c', 'user.name=a', '-c', 'user.email=a@example.com', 'commit', '-q', '--allow-empty', '-m', 'lib']);
  try {
    // 手で clone してから足す（データはその中に残る）
    git('clone', '-q', `file://${lib.replaceAll('\\', '/')}`, path);
    git('submodule', 'add', '-q', `file://${lib.replaceAll('\\', '/')}`, path);
    const section = sidebar.getChildren().find((n) => n.kind === 'section' && n.section === 'submodules')!;
    sidebar.onDidExpand(section);
    await repo.refresh();
    await sidebar.submoduleFacts.reload();
    await until('自分のフォルダにデータ', () => sidebar.submoduleFacts.get(path)?.ownGitDir);
    const row = await until('submodule の行', () => sidebar.getChildren(section).find((n) => n.kind === 'submodule' && n.submodule.path === path));
    assert.match(String(sidebar.getTreeItem(row).contextValue), /;ownGitDir/);
    await vscode.commands.executeCommand('amonhen.submodule.absorb', row);
    assert.ok(existsSync(join(gitDir, 'modules', path)), lastErrors());
    assert.ok(statSync(join(root, path, '.git')).isFile(), '.git はファイル（gitdir: の指し先）になる');

    await withWindow(
      { showInputBox: inputs('git tag foreach-e2e'), showQuickPick: async (items: readonly vscode.QuickPickItem[]) => items[0] },
      async () => vscode.commands.executeCommand('amonhen.submodules.foreach'),
    );
    assert.equal(git('-C', path, 'tag', '--list', 'foreach-e2e').trim(), 'foreach-e2e', lastErrors());
  } finally {
    git('reset', '-q', '--hard', start);
    rmSync(join(root, 'vendor'), { recursive: true, force: true });
    rmSync(join(gitDir, 'modules'), { recursive: true, force: true });
    try {
      git('config', '--remove-section', `submodule.${path}`);
    } catch {
      // ない
    }
    git('config', '--global', '--unset', 'protocol.file.allow');
    rmSync(lib, { recursive: true, force: true });
    await repo.refresh();
  }
}

/**
 * rerere が当てた解決を Forget Resolution で忘れると、コンフリクトのマーカーが戻る。Repository Settings の
 * Forget All… で、覚えた解決をすべて消す。コアの命令（daemon だけ）
 */
async function rerereForgetFlow(): Promise<void> {
  const panel = await showView('amonhen.showWorkingCopy');
  const branch = git('symbolic-ref', '--short', 'HEAD').trim();
  const gitDir = git('rev-parse', '--absolute-git-dir').trim();
  const file = join(root, 'rf.txt');
  const side = (name: string) => {
    writeFileSync(file, `1\n${name}\n3\n`);
    git('add', 'rf.txt');
    git('commit', '-q', '-m', `rf ${name}`);
  };
  const merge = () => assert.throws(() => childProcess.execFileSync('git', ['merge', '-q', 'rf-theirs'], { cwd: root, stdio: 'ignore' }));
  git('config', 'rerere.enabled', 'true');
  git('switch', '-q', '-c', 'rf-ours');
  side('ours');
  git('switch', '-q', '-c', 'rf-theirs', 'HEAD~');
  side('theirs');
  git('switch', '-q', 'rf-ours');
  try {
    merge();
    writeFileSync(file, '1\nboth\n3\n');
    git('rerere');
    git('merge', '--abort');
    merge();
    assert.equal(readFileSync(file, 'utf8'), '1\nboth\n3\n', 'rerere が当てる');
    await vscode.commands.executeCommand('amonhen.forgetResolution', { path: 'rf.txt' });
    assert.match(readFileSync(file, 'utf8'), /^<<<<<<< /m, lastErrors());
    git('merge', '--abort');

    // 覚えた解決をすべて忘れる（もう一度覚えさせてから）
    merge();
    writeFileSync(file, '1\nboth\n3\n');
    git('rerere');
    git('merge', '--abort');
    assert.ok(readdirSync(join(gitDir, 'rr-cache')).length > 0);
    await withWindow({ showWarningMessage: async () => 'Forget All' }, async () =>
      panel.settings.handle({ type: 'settingsRun', action: 'forgetAllResolutions' }),
    );
    assert.equal(existsSync(join(gitDir, 'rr-cache')) ? readdirSync(join(gitDir, 'rr-cache')).length : 0, 0, lastErrors());
  } finally {
    try {
      git('merge', '--abort');
    } catch {
      // もうやめている
    }
    git('switch', '-q', branch);
    git('branch', '-q', '-D', 'rf-ours', 'rf-theirs');
    git('config', '--unset', 'rerere.enabled');
    rmSync(join(gitDir, 'rr-cache'), { recursive: true, force: true });
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

/**
 * New Orphan Branch… で履歴のないブランチに切り替える。Keep the Files なら今のファイルが stage されて残り、Start Empty
 * なら何もない。コアの命令（daemon だけ）
 */
async function orphanFlow(): Promise<void> {
  const repo = api.manager.current!;
  const branch = git('symbolic-ref', '--short', 'HEAD').trim();
  const pick = (name: string, label: string) => ({
    showInputBox: inputs(name),
    showQuickPick: async (items: readonly vscode.QuickPickItem[]) => items.find((i) => i.label === label),
  });
  const unborn = () => assert.throws(() => git('rev-parse', '-q', '--verify', 'HEAD'), 'まだコミットがない');
  try {
    await withWindow(pick('orphan-keep', 'Keep the Files'), async () => vscode.commands.executeCommand('amonhen.branch.newOrphan'));
    assert.equal(git('symbolic-ref', '--short', 'HEAD').trim(), 'orphan-keep', lastErrors());
    unborn();
    assert.ok(git('diff', '--cached', '--name-only').includes(FILE), '今のファイルは stage されて残る');
    git('reset', '-q');
    git('switch', '-q', '-f', branch);

    await withWindow(pick('orphan-empty', 'Start Empty'), async () => vscode.commands.executeCommand('amonhen.branch.newOrphan'));
    assert.equal(git('symbolic-ref', '--short', 'HEAD').trim(), 'orphan-empty', lastErrors());
    unborn();
    assert.equal(git('ls-files').trim(), '', '空から始まる');
  } finally {
    git('switch', '-q', '-f', branch);
    await repo.refresh();
  }
}

/**
 * Delete Ignored Files… は git clean -X が消すもの（フォルダは 1 つ）を先に見せ、選んだものだけを消す。ignore されて
 * いないファイルは残る。コアの命令（daemon だけ）
 */
async function deleteIgnoredFlow(): Promise<void> {
  const repo = api.manager.current!;
  const exclude = join(git('rev-parse', '--absolute-git-dir').trim(), 'info', 'exclude');
  const before = existsSync(exclude) ? readFileSync(exclude, 'utf8') : '';
  const files = ['di-a.log', 'di-b.log', 'di-build/out.bin', 'di-keep.txt'];
  writeFileSync(exclude, `${before}\n*.log\ndi-build/\n`);
  mkdirSync(join(root, 'di-build'), { recursive: true });
  for (const f of files) writeFileSync(join(root, f), 'x\n');
  let listed: string[] = [];
  try {
    await withWindow(
      {
        showQuickPick: async (items: readonly vscode.QuickPickItem[]) => {
          listed = items.map((i) => i.label);
          return items.filter((i) => i.label !== 'di-b.log');
        },
        showWarningMessage: async () => 'Delete',
      },
      async () => vscode.commands.executeCommand('amonhen.deleteIgnored'),
    );
    assert.deepEqual(listed.sort(), ['di-a.log', 'di-b.log', 'di-build/'], lastErrors());
    assert.ok(!existsSync(join(root, 'di-a.log')) && !existsSync(join(root, 'di-build')), lastErrors());
    assert.ok(existsSync(join(root, 'di-b.log')), '選ばなかったものは残る');
    assert.ok(existsSync(join(root, 'di-keep.txt')), 'ignore されていないものは残る');
  } finally {
    writeFileSync(exclude, before);
    for (const f of files) rmSync(join(root, f), { force: true });
    rmSync(join(root, 'di-build'), { recursive: true, force: true });
    await repo.refresh();
  }
}

/**
 * Clean Up Branches… は、既定のブランチに取り込んだもの（チェック済み）と上流が消えたもの（チェックなし）を理由ごとに
 * 並べ、選んだものをまとめて消す。Undo 1 回で、設定（上流）ごと戻る。コアの命令（daemon だけ）
 */
async function branchCleanupFlow(): Promise<void> {
  const repo = api.manager.current!;
  const branch = git('symbolic-ref', '--short', 'HEAD').trim();
  const start = git('rev-parse', 'HEAD').trim();
  const commit = (name: string) => {
    writeFileSync(join(root, `${name}.txt`), `${name}\n`);
    git('add', `${name}.txt`);
    git('commit', '-q', '-m', name);
  };
  let listed: vscode.QuickPickItem[] = [];
  try {
    // 取り込んだもの
    git('switch', '-q', '-c', 'cu-merged');
    commit('cu-merged');
    git('switch', '-q', branch);
    git('merge', '-q', '--no-ff', '--no-edit', 'cu-merged');
    // 上流が消えたもの（取り込んでいない。自分のコミットごと消える）
    git('remote', 'add', 'cu-remote', root);
    git('switch', '-q', '-c', 'cu-gone');
    commit('cu-gone');
    git('config', 'branch.cu-gone.remote', 'cu-remote');
    git('config', 'branch.cu-gone.merge', 'refs/heads/cu-gone');
    git('switch', '-q', branch);
    await repo.refresh();

    await withWindow(
      {
        showQuickPick: async (items: readonly vscode.QuickPickItem[]) => {
          listed = [...items];
          return items.filter((i) => i.kind !== vscode.QuickPickItemKind.Separator);
        },
      },
      async () => vscode.commands.executeCommand('amonhen.branches.cleanUp'),
    );
    const item = (label: string) => listed.find((i) => i.label === label && i.kind !== vscode.QuickPickItemKind.Separator);
    assert.equal(item('cu-merged')?.picked, true, lastErrors());
    assert.equal(item('cu-gone')?.picked, false, '上流が消えたものはチェックしない');
    assert.ok(!item(branch), '今のブランチは並ばない');
    assert.deepEqual(
      listed.filter((i) => i.kind === vscode.QuickPickItemKind.Separator).map((i) => i.label),
      [`Merged into ${branch}`, 'Upstream gone, not merged (their own commits go with them)'],
    );
    assert.equal(git('branch', '--list', 'cu-merged', 'cu-gone').trim(), '', lastErrors());

    // Undo 1 回で両方が、上流の設定ごと戻る
    await vscode.commands.executeCommand('amonhen.undo');
    await until('Undo で戻る', () => git('branch', '--list', 'cu-merged', 'cu-gone').split('\n').filter(Boolean).length === 2);
    assert.equal(git('config', 'branch.cu-gone.remote').trim(), 'cu-remote');
  } finally {
    for (const b of ['cu-merged', 'cu-gone']) if (git('branch', '--list', b).trim()) git('branch', '-q', '-D', b);
    if (git('remote').split('\n').includes('cu-remote')) git('remote', 'remove', 'cu-remote');
    git('reset', '-q', '--keep', start);
    await repo.refresh();
  }
}

/**
 * GitHub のプルリクエストで（squash・rebase で）取り込んだブランチに、取り込み済みの印が出る。消すときはプルリクエストを
 * 添えた確認 1 回で消え、Undo で戻る。先端が違うプルリクエストでは印を出さない（gh の答えは差し替える）
 */
async function pullRequestMarkFlow(): Promise<void> {
  const sidebar = api.sidebar;
  const repo = api.manager.current!;
  const branch = git('symbolic-ref', '--short', 'HEAD').trim();
  const facts = sidebar.branchFacts;
  const readMerged = facts.readMerged;
  const section = () => sidebar.getChildren().find((n) => n.kind === 'section' && n.section === 'branches');
  const row = (name = 'pm-squashed') => {
    const find = (nodes: ReturnType<typeof sidebar.getChildren>): ReturnType<typeof sidebar.getChildren>[number] | undefined => {
      for (const n of nodes) {
        if (n.kind === 'branch' && n.ref.name === `refs/heads/${name}`) return n;
        const inner = n.kind === 'branch' || n.kind === 'branchGroup' || n.kind === 'branchFolder' ? find(sidebar.getChildren(n)) : undefined;
        if (inner) return inner;
      }
      return undefined;
    };
    return find(sidebar.getChildren(section()!));
  };
  const line = () => {
    const n = row();
    return n && sidebar.getTreeItem(n);
  };
  const setMerged = async (oid: string) => {
    facts.readMerged = async () => [{ number: 77, branch: 'pm-squashed', oid }];
    await facts.readPullRequests(true);
  };
  try {
    git('switch', '-q', '-c', 'pm-squashed');
    writeFileSync(join(root, 'pm.txt'), 'pm\n');
    git('add', 'pm.txt');
    git('commit', '-q', '-m', 'pm');
    git('switch', '-q', branch);
    await vscode.commands.executeCommand('amonhen.refresh');
    await facts.reload();
    const before = await until('印のない行', () => line());
    assert.ok(!/merged/.test(String(before.description)), String(before.description));
    const tip = git('rev-parse', 'pm-squashed').trim();

    // 先端が違うプルリクエストでは印を出さない
    await setMerged(git('rev-parse', branch).trim());
    assert.ok(!/merged/.test(String(line()!.description)));

    await setMerged(tip);
    const marked = await until('取り込み済みの印', () => {
      const item = line();
      return item && /merged/.test(String(item.description)) && item;
    });
    assert.match(String(marked.tooltip), /Merged by pull request #77/);

    // 確認は 1 回だけで、プルリクエストの番号が出る
    const asked: { message: string; detail?: string | undefined }[] = [];
    await withWindow(
      {
        showWarningMessage: async (message: string, options: vscode.MessageOptions) => {
          asked.push({ message, detail: options.detail });
          return 'Delete';
        },
      },
      async () => vscode.commands.executeCommand('amonhen.nav.delete', row()),
    );
    assert.equal(asked.length, 1, JSON.stringify(asked));
    assert.match(asked[0]!.detail ?? '', /#77/);
    assert.equal(git('branch', '--list', 'pm-squashed').trim(), '', lastErrors());
    await vscode.commands.executeCommand('amonhen.undo');
    await until('Undo で戻る', () => git('branch', '--list', 'pm-squashed').trim() !== '');
    assert.equal(git('rev-parse', 'pm-squashed').trim(), tip);

    // 取り込んでいないものと一緒に選ぶと、確認にプルリクエストが添えられ、もう一度聞くのは取り込んでいないものだけ
    git('switch', '-q', '-c', 'pm-plain');
    writeFileSync(join(root, 'pm-plain.txt'), 'pm\n');
    git('add', 'pm-plain.txt');
    git('commit', '-q', '-m', 'pm-plain');
    git('switch', '-q', branch);
    await vscode.commands.executeCommand('amonhen.refresh');
    await facts.reload();
    const squashed = await until('取り込み済みの行', () => /merged/.test(String(line()?.description)) && row());
    const plain = await until('取り込んでいない行', () => row('pm-plain'));
    asked.length = 0;
    await withWindow(
      {
        showWarningMessage: async (message: string, options: vscode.MessageOptions) => {
          asked.push({ message, detail: options.detail });
          return /merge されていない/.test(message) ? 'Delete Anyway' : 'Delete';
        },
      },
      async () => vscode.commands.executeCommand('amonhen.nav.delete', squashed, [squashed, plain]),
    );
    assert.equal(asked.length, 2, JSON.stringify(asked));
    assert.match(asked[0]!.detail ?? '', /pm-squashed（プルリクエスト #77 でマージ済み）/);
    assert.match(asked[1]!.message, /^pm-plain は/);
    assert.equal(git('branch', '--list', 'pm-squashed', 'pm-plain').trim(), '', lastErrors());
  } finally {
    facts.readMerged = readMerged;
    await facts.readPullRequests(true);
    for (const b of ['pm-squashed', 'pm-plain']) if (git('branch', '--list', b).trim()) git('branch', '-q', '-D', b);
    await repo.refresh();
  }
}

/**
 * Stashes は stash があるときだけ出て、行に作ったときのブランチ・土台と HEAD の離れ具合・untracked が出る。
 * stash からブランチを作り（git stash branch）、新しい stash を確かめてから pop する（daemon だけ）
 */
async function historySelectionFlow(): Promise<void> {
  const repo = api.manager.current!;
  const branch = git('symbolic-ref', '--short', 'HEAD').trim();
  const panel = await showView('amonhen.showHistory');
  const warning = vscode.window.showWarningMessage;
  const information = vscode.window.showInformationMessage;
  const subjects = (n: number) => git('log', `-${n}`, '--format=%s').trim().split('\n');
  // ref や HEAD の変化を知ると History を読み直し、選んだ行は消える。読み直しが落ち着くまで待つ
  const settled = async () => {
    for (;;) {
      const mark = posted.length;
      await new Promise((r) => setTimeout(r, 800));
      if (!posted.slice(mark).some((m) => m.type === 'reset' || m.type === 'rows' || m.type === 'details')) return;
    }
  };
  const read = async (names: string[]) => {
    // status の HEAD が古いままだと、追いついたときに History を読み直す（選んだ行が消える）。追いつくまで待つ
    const head = git('rev-parse', 'HEAD').trim();
    await until('status の HEAD', () => repo.current.status?.branch.oid === head || undefined);
    await settled();
    const mark = posted.length;
    await panel.history.reset();
    await waitPosted('searchable', mark, (m) => m.searchable);
    const { rows } = await waitPosted('rows', mark, (m) => m.total !== undefined);
    // Webview が最初の行を選び終えるまで待つ（1 行を選ぶと、いくつも選んだ状態は消える）
    await waitPosted('details', mark);
    await settled();
    return names.map((name) => {
      const row = rows.findIndex((r) => r.subject === name);
      assert.ok(row >= 0, `${name} in ${rows.map((r) => r.subject).join(', ')}`);
      return row;
    });
  };
  const select = async (token: number, rows: number[], pair: 'forward' | 'reverse' | 'mergeBase' = 'forward') => {
    const mark = posted.length;
    await panel.history.handle({ type: 'selectRows', token, rows, pair });
    const answer = await waitPosted('selection', mark, (m) => m.token === token);
    assert.ok(answer.view, answer.error);
    return answer.view;
  };
  try {
    const base = git('rev-parse', 'HEAD').trim();
    git('switch', '-q', '-c', 'sel-base');
    for (const name of ['s1', 's2', 's3', 's4']) {
      writeFileSync(join(root, `${name}.txt`), `${name}\n`);
      git('add', `${name}.txt`);
      git('commit', '-q', '-m', name);
    }
    await vscode.commands.executeCommand('amonhen.refresh');
    const [s1, s2, s3, s4] = await read(['s1', 's2', 's3', 's4']) as [number, number, number, number];

    // 続いた行は 1 つにまとめた差分
    const range = await select(1, [s4, s3]);
    assert.equal(range.kind, 'range');
    assert.deepEqual(range.files.map((f) => f.path).sort(), ['s3.txt', 's4.txt']);
    assert.deepEqual(range.commits.map((c) => c.subject), ['s4', 's3']);
    const mark = posted.length;
    await panel.history.handle({ type: 'commitDiff', oid: range.key, path: 's3.txt' });
    const { diff } = await waitPosted('commitDiff', mark, (m) => m.oid === range.key);
    assert.ok(diff.hunks.some((h) => h.lines.some((l) => l.kind === '+' && l.text === 's3')), JSON.stringify(diff));

    // 離れた 2 つは取り方を選べる。逆にすると足した行が消した行になる
    const pair = await select(2, [s4, s1]);
    assert.equal(pair.kind, 'pair');
    assert.ok(pair.files.some((f) => f.path === 's4.txt' && f.added === 1), JSON.stringify(pair.files));
    const reverse = await select(3, [s4, s1], 'reverse');
    assert.ok(reverse.files.some((f) => f.path === 's4.txt' && f.removed === 1), JSON.stringify(reverse.files));
    assert.equal((await select(4, [s4, s1], 'mergeBase')).kind, 'pair');

    // 離れた 3 つ以上はコミットごと
    const each = await select(5, [s4, s2, s1]);
    assert.equal(each.kind, 'each');
    assert.equal(each.files.length, 3);

    // ID はまとめてコピーできる（新しい順）
    await select(6, [s4, s2]);
    await vscode.commands.executeCommand('amonhen.history.selection.copyIds');
    assert.equal(await vscode.env.clipboard.readText(), [git('rev-parse', 'sel-base').trim(), git('rev-parse', 'sel-base~2').trim()].join('\n'));

    // 2 つは範囲の差分と同じ窓口で比べられる
    await select(7, [s4, s2]);
    await vscode.commands.executeCommand('amonhen.history.selection.rangeDiff');
    await until('Range Diff のタブ', () => /Range Diff/.test(vscode.window.tabGroups.activeTabGroup.activeTab?.label ?? '') || undefined);
    await vscode.commands.executeCommand('workbench.action.closeActiveEditor');

    // 選んだコミットを、古い順に 1 回で cherry-pick する
    const prompts: string[] = [];
    // 終わりの知らせ（Undo のボタンつき）は、閉じるまで待たされる
    (vscode.window as { showInformationMessage: unknown }).showInformationMessage = async () => undefined;
    (vscode.window as { showWarningMessage: unknown }).showWarningMessage = async (title: string, _options: unknown, ...items: unknown[]) => {
      prompts.push(title);
      return items[0];
    };
    git('switch', '-q', '-c', 'sel-target', base);
    await vscode.commands.executeCommand('amonhen.refresh');
    await panel.history.handle({ type: 'scope', scope: 'all' });
    const again = await read(['s1', 's2', 's3', 's4']) as [number, number, number, number];
    await select(8, [again[3], again[1]]);
    await vscode.commands.executeCommand('amonhen.history.selection.cherryPick');
    assert.ok(prompts.some((p) => p === 'Cherry-pick 2 commits onto sel-target?'), prompts.join(' / '));
    assert.deepEqual(subjects(2), ['s4', 's2']);
    // 履歴には 1 回の操作として残り、Undo で戻る
    await repo.refresh();
    await vscode.commands.executeCommand('amonhen.undo');
    await until('Undo した HEAD', () => git('rev-parse', 'HEAD').trim() === base || undefined);

    // 新しい順に Revert する
    git('switch', '-q', 'sel-base');
    await vscode.commands.executeCommand('amonhen.refresh');
    const onBase = await read(['s3', 's4']) as [number, number];
    await select(9, onBase);
    await vscode.commands.executeCommand('amonhen.history.selection.revert');
    assert.deepEqual(subjects(2), ['Revert "s3"', 'Revert "s4"']);
    git('reset', '-q', '--hard', 'HEAD~2');
    await vscode.commands.executeCommand('amonhen.refresh');

    // 続いたコミットを、メッセージを編集して 1 つにする
    const combine = await read(['s3', 's4']) as [number, number];
    await select(10, combine);
    const before = git('rev-parse', 'HEAD~2').trim();
    const edit = posted.length;
    await vscode.commands.executeCommand('amonhen.history.selection.combine');
    const opened = await waitPosted('editMessage', edit, (m) => m.kind === 'combine');
    assert.equal(opened.message, 's3\n\ns4');
    assert.equal(opened.oid, git('rev-parse', 'HEAD~1').trim());
    assert.equal(opened.last, git('rev-parse', 'HEAD').trim());
    await panel.history.handle({ type: 'saveMessage', oid: opened.oid, last: opened.last, kind: 'combine', message: 's3 and s4' });
    await until('まとめたコミット', () => git('log', '-1', '--format=%s').trim() === 's3 and s4' || undefined);
    assert.equal(git('rev-parse', 'HEAD~1').trim(), before);

  } finally {
    (vscode.window as { showWarningMessage: unknown }).showWarningMessage = warning;
    (vscode.window as { showInformationMessage: unknown }).showInformationMessage = information;
    await panel.history.handle({ type: 'scope', scope: 'head' });
    git('switch', '-q', branch);
    for (const name of ['sel-base', 'sel-target']) {
      if (git('branch', '--list', name).trim()) git('branch', '-q', '-D', name);
    }
    await vscode.commands.executeCommand('amonhen.refresh');
    await repo.refresh();
  }
}

async function stashFlow(): Promise<void> {
  const sidebar = api.sidebar;
  const repo = api.manager.current!;
  const current = git('symbolic-ref', '--short', 'HEAD').trim();
  const start = git('rev-parse', 'HEAD').trim();
  const stashesSection = () => sidebar.getChildren().find((n) => n.kind === 'section' && n.section === 'stashes');
  const file = join(root, 'st.txt');
  try {
    await vscode.commands.executeCommand('amonhen.refresh');
    await until('Stashes がない', () => git('stash', 'list').trim() === '' && !stashesSection());
    writeFileSync(file, 'untracked\n');
    git('stash', 'push', '-q', '-u', '-m', 'e2e stash');
    git('commit', '-q', '--allow-empty', '-m', 'after stash');
    await vscode.commands.executeCommand('amonhen.refresh');
    const section = await until('Stashes が出る', () => stashesSection());
    await repo.want('stashes');
    const row = await until('stash の行の様子', () => {
      const node = sidebar.getChildren(section).find((n) => n.kind === 'stash');
      const item = node && sidebar.getTreeItem(node);
      return item && /untracked/.test(String(item.description)) && /HEAD has 1 commit since/.test(String(item.tooltip)) && { node, item };
    });
    assert.match(String(row.item.tooltip), new RegExp(`Made on ${current}`));

    // stash からブランチを作ると、そこへ checkout して stash を当てて消す
    await withWindow({ showInputBox: inputs('e2e-stash-br') }, async () =>
      vscode.commands.executeCommand('amonhen.stash.branch', row.node),
    );
    assert.equal(git('symbolic-ref', '--short', 'HEAD').trim(), 'e2e-stash-br');
    assert.equal(git('rev-parse', 'HEAD').trim(), start);
    assert.equal(readFileSync(file, 'utf8').replaceAll('\r\n', '\n'), 'untracked\n');
    assert.equal(git('stash', 'list').trim(), '');

    // 新しい stash を確かめてから pop する
    git('stash', 'push', '-q', '-u', '-m', 'again');
    await vscode.commands.executeCommand('amonhen.refresh');
    await withWindow({ showWarningMessage: async () => 'Pop' }, async () => vscode.commands.executeCommand('amonhen.stash.popLatest'));
    assert.equal(git('stash', 'list').trim(), '');
    assert.equal(readFileSync(file, 'utf8').replaceAll('\r\n', '\n'), 'untracked\n');
  } finally {
    rmSync(file, { force: true });
    git('switch', '-q', current);
    git('reset', '-q', '--hard', start);
    if (git('branch', '--list', 'e2e-stash-br').trim()) git('branch', '-D', 'e2e-stash-br');
    while (git('stash', 'list').trim()) git('stash', 'drop', '-q');
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

/**
 * サイドバーの上の Undo は、何を戻すかを見せて確かめてから戻す（やめれば戻さない）。Undo History… は記録を新しい
 * 順に並べ、選んだところまでまとめて戻す
 */
async function undoFlow(): Promise<void> {
  const sidebar = api.sidebar;
  const node = (name: string) => {
    const section = sidebar.getChildren().find((n) => n.kind === 'section' && n.section === 'branches')!;
    const find = (nodes: ReturnType<typeof sidebar.getChildren>): ReturnType<typeof sidebar.getChildren>[number] | undefined => {
      for (const n of nodes) {
        if (n.kind === 'branch' && n.ref.name === `refs/heads/${name}`) return n;
        const inner = n.kind === 'branch' || n.kind === 'branchGroup' || n.kind === 'branchFolder' ? find(sidebar.getChildren(n)) : undefined;
        if (inner) return inner;
      }
      return undefined;
    };
    return find(sidebar.getChildren(section));
  };
  const names = ['undo-a', 'undo-b'];
  try {
    for (const b of names) git('branch', b);
    await vscode.commands.executeCommand('amonhen.refresh');
    for (const b of names) {
      const n = await until(`${b} の行`, () => node(b));
      await withWindow({ showWarningMessage: async () => 'Delete' }, async () => vscode.commands.executeCommand('amonhen.nav.delete', n));
    }
    assert.equal(git('branch', '--list', ...names).trim(), '');

    // やめれば戻さない。尋ねる文に、戻す操作の名前と何を戻すかが出る
    let asked: { message: string; detail?: string } | undefined;
    await withWindow(
      {
        showWarningMessage: async (message: string, options: vscode.MessageOptions) => {
          asked = { message, ...(options.detail ? { detail: options.detail } : {}) };
          return undefined;
        },
      },
      async () => vscode.commands.executeCommand('amonhen.undoWithConfirmation'),
    );
    assert.match(String(asked?.message), /^Undo “Delete (branch )?undo-b”\?$/);
    assert.match(String(asked?.detail), /HEAD and the branches/);
    assert.equal(git('branch', '--list', ...names).trim(), '');

    // Undo History… で 2 つ目まで戻すと、2 つとも戻る
    await withWindow(
      {
        showQuickPick: async (items: readonly (vscode.QuickPickItem & { count: number })[]) => items.find((i) => i.count === 2),
        showWarningMessage: async () => 'Undo',
      },
      async () => vscode.commands.executeCommand('amonhen.undoHistory'),
    );
    assert.deepEqual(git('branch', '--list', '--format=%(refname:short)', ...names).trim().split('\n'), names);
  } finally {
    for (const b of names) if (git('branch', '--list', b).trim()) git('branch', '-D', b);
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

/**
 * Repository Settings に、このリポジトリの git の設定（どこで決まっているか付き）と git の版が出て、行の値を
 * このリポジトリの設定に書き、↩︎ で上書きをやめる（daemon だけ）
 */
async function settingsFlow(): Promise<void> {
  await showView('amonhen.showHistory');
  const panel = api.panel()!;
  const config = (key: string) => {
    try {
      return git('config', '--local', key).trim();
    } catch {
      return undefined;
    }
  };
  const keys = ['fetch.prune', 'pull.rebase', 'pull.ff'];
  const excludeFile = join(git('rev-parse', '--absolute-git-dir').trim(), 'info', 'exclude');
  const exclude = existsSync(excludeFile) ? readFileSync(excludeFile, 'utf8') : '';
  const hadGitignore = existsSync(join(root, '.gitignore'));
  let sparseCommit = false;
  let hookFile: string | undefined;
  const gitattributes = existsSync(join(root, '.gitattributes')) ? readFileSync(join(root, '.gitattributes'), 'utf8') : undefined;
  // git lfs track はフック（post-checkout など）を入れ、lfs.repositoryformatversion を書く。残すと、この後の
  // checkout や rebase のたびに git-lfs が動き、Windows では .git/config を同時に読んだ git が失敗する
  const hooksDir = join(git('rev-parse', '--git-common-dir').trim().replace(/^(?![\\/]|[A-Za-z]:)/, `${root}/`), 'hooks');
  const hooksBefore = new Set(existsSync(hooksDir) ? readdirSync(hooksDir) : []);
  const lfsVersion = config('lfs.repositoryformatversion');
  try {
    let mark = posted.length;
    await vscode.commands.executeCommand('amonhen.showRepositorySettings');
    await panel.settings.handle({ type: 'settingsLoad' });
    const first = await waitPosted('settings', mark, (m) => m.entries.length > 0);
    assert.match(String(first.facts.gitVersion), /^\d+\.\d+/);
    assert.ok(first.entries.some((e) => e.key === 'user.email' && e.scope === 'local'));

    mark = posted.length;
    await panel.settings.handle({ type: 'settingsSet', writes: [['fetch.prune', 'true'], ['pull.rebase', 'false'], ['pull.ff', 'only']] });
    const written = await waitPosted('settings', mark);
    assert.deepEqual(keys.map(config), ['true', 'false', 'only']);
    assert.ok(written.entries.some((e) => e.key === 'pull.ff' && e.value === 'only' && e.scope === 'local'));

    // ↩︎（上書きをやめる）
    mark = posted.length;
    await panel.settings.handle({ type: 'settingsSet', writes: keys.map((k): [string, null] => [k, null]) });
    await waitPosted('settings', mark);
    assert.deepEqual(keys.map(config), [undefined, undefined, undefined]);

    // Branches: 既定のブランチを選ぶ。Working Copy: .git/info/exclude を書く（.gitignore はそのまま）
    const branch = git('symbolic-ref', '--short', 'HEAD').trim();
    assert.ok(first.facts.branches.includes(branch));
    mark = posted.length;
    await panel.settings.handle({ type: 'settingsSet', writes: [['amonhen.defaultBranch', branch]] });
    assert.equal((await waitPosted('settings', mark)).facts.base, branch);
    assert.equal(config('amonhen.defaultBranch'), branch);
    const gitignore = existsSync(join(root, '.gitignore')) ? readFileSync(join(root, '.gitignore'), 'utf8') : undefined;
    mark = posted.length;
    await panel.settings.handle({ type: 'settingsSaveExclude', exclude: `${exclude}e2e-ignored\n` });
    assert.match((await waitPosted('settings', mark)).facts.exclude, /e2e-ignored/);
    assert.match(readFileSync(excludeFile, 'utf8'), /e2e-ignored/);
    assert.equal(existsSync(join(root, '.gitignore')) ? readFileSync(join(root, '.gitignore'), 'utf8') : undefined, gitignore ?? '');

    // Remotes: origin が表に出る。push の URL を別にして戻し、つながるか確かめる
    const origin = first.facts.remotes.find((r) => r.name === 'origin');
    assert.ok(origin, 'origin が Remotes にある');
    mark = posted.length;
    await withWindow({ showInputBox: inputs('../push-e2e') }, async () =>
      panel.settings.handle({ type: 'settingsRun', action: 'remote', op: 'editPushUrl', name: 'origin' }),
    );
    assert.equal((await waitPosted('settings', mark)).facts.remotes.find((r) => r.name === 'origin')?.pushUrl, '../push-e2e');
    assert.equal(config('remote.origin.pushurl'), '../push-e2e');
    await withWindow({ showInputBox: inputs('') }, async () =>
      panel.settings.handle({ type: 'settingsRun', action: 'remote', op: 'editPushUrl', name: 'origin' }),
    );
    assert.equal(config('remote.origin.pushurl'), undefined);
    let reached = '';
    await withWindow({ showInformationMessage: async (m: string) => void (reached = m) }, async () =>
      panel.settings.handle({ type: 'settingsRun', action: 'remote', op: 'check', name: 'origin' }),
    );
    assert.match(reached, /^origin: reachable, \d+ branches/);

    // Keys and Values: キーを足し、値を変え、消す。git が知っている名前も届く
    assert.ok(first.facts.knownKeys.includes('core.autocrlf'));
    mark = posted.length;
    await withWindow({ showInputBox: inputs('e2e.added', 'one') }, async () =>
      panel.settings.handle({ type: 'settingsRun', action: 'configEdit' }),
    );
    await waitPosted('settings', mark, (m) => m.entries.some((e) => e.key === 'e2e.added' && e.value === 'one'));
    await withWindow({ showInputBox: inputs('two') }, async () =>
      panel.settings.handle({ type: 'settingsRun', action: 'configEdit', key: 'e2e.added' }),
    );
    assert.equal(config('e2e.added'), 'two');
    await panel.settings.handle({ type: 'settingsSet', writes: [['e2e.added', null]] });
    assert.equal(config('e2e.added'), undefined);

    // Author and Signing: 名前を上書きし、SSH で署名するにすると、その形式の鍵を集める（e2e の HOME には鍵がない）
    mark = posted.length;
    await panel.settings.handle({ type: 'settingsSet', writes: [['user.name', 'E2E Author'], ['commit.gpgsign', 'true'], ['gpg.format', 'ssh']] });
    const signing = await waitPosted('settings', mark);
    assert.deepEqual(['user.name', 'commit.gpgsign', 'gpg.format'].map(config), ['E2E Author', 'true', 'ssh']);
    assert.ok(Array.isArray(signing.facts.signingKeys));
    await panel.settings.handle({ type: 'settingsSet', writes: [['user.name', 'a'], ['commit.gpgsign', null], ['gpg.format', null]] });
    assert.equal(config('user.name'), 'a');

    // Commits: このリポジトリ専用のテンプレートを書くと commit.template がそれを指し、None で外れる
    mark = posted.length;
    await panel.settings.handle({ type: 'settingsRun', action: 'templateSave', text: 'e2e: \n' });
    const templated = await waitPosted('settings', mark, (m) => m.facts.commitTemplate?.text === 'e2e: \n');
    assert.equal(config('commit.template'), templated.facts.commitTemplate?.path);
    await panel.settings.handle({ type: 'settingsSet', writes: [['commit.template', null]] });
    assert.equal(config('commit.template'), undefined);

    // Hooks and LFS: フックを作り（エディタで開く）、実行の許可を外す（Windows では外せない）。LFS は入っていれば
    const hook = join(git('rev-parse', '--git-common-dir').trim().replace(/^(?![\\/]|[A-Za-z]:)/, `${root}/`), 'hooks', 'pre-commit');
    hookFile = hook;
    mark = posted.length;
    await withWindow(
      { showQuickPick: async (items: readonly vscode.QuickPickItem[]) => items.find((i) => i.label === 'pre-commit') },
      async () => panel.settings.handle({ type: 'settingsRun', action: 'hookCreate' }),
    );
    const created = await waitPosted('settings', mark, (m) => !!m.facts.hooksAndLfs?.hooks.some((h) => h.name === 'pre-commit'));
    assert.ok(existsSync(hook));
    assert.equal(vscode.window.activeTextEditor?.document.uri.fsPath.toLowerCase(), hook.toLowerCase());
    await vscode.commands.executeCommand('workbench.action.closeActiveEditor');
    if (process.platform !== 'win32') {
      mark = posted.length;
      await panel.settings.handle({ type: 'settingsRun', action: 'hookEnabled', name: 'pre-commit', enabled: false });
      const off = await waitPosted('settings', mark);
      assert.equal(off.facts.hooksAndLfs?.hooks.find((h) => h.name === 'pre-commit')?.enabled, false);
    }
    if (created.facts.hooksAndLfs?.lfs) {
      mark = posted.length;
      await panel.settings.handle({ type: 'settingsRun', action: 'lfsTrack', pattern: '*.e2e-bin', track: true });
      assert.ok((await waitPosted('settings', mark)).facts.hooksAndLfs?.lfsPatterns.includes('*.e2e-bin'));
      mark = posted.length;
      await panel.settings.handle({ type: 'settingsRun', action: 'lfsTrack', pattern: '*.e2e-bin', track: false });
      assert.ok(!(await waitPosted('settings', mark)).facts.hooksAndLfs?.lfsPatterns.includes('*.e2e-bin'));
    }

    // Fetching and Folders: HEAD の木からフォルダを選んで sparse checkout にし、すべてに戻す（git 2.25 から）。
    // maintenance は OS の予定に登録するので、ここでは試さない
    if (first.facts.repositoryForm?.canSparseCheckout) {
      mkdirSync(join(root, 'sp-dir'), { recursive: true });
      writeFileSync(join(root, 'sp-dir', 'a.txt'), 'a\n');
      writeFileSync(join(root, 'sp-top.txt'), 'top\n');
      git('add', 'sp-dir', 'sp-top.txt');
      git('commit', '-q', '-m', 'sparse e2e');
      sparseCommit = true;
      const picks = [/sp-dir/, /Check out sp-dir/];
      mark = posted.length;
      await withWindow(
        { showQuickPick: async (items: readonly vscode.QuickPickItem[]) => items.find((i) => picks[0]!.test(i.label) && picks.shift()) },
        async () => panel.settings.handle({ type: 'settingsRun', action: 'addFolder' }),
      );
      const sparse = await waitPosted('settings', mark, (m) => !!m.facts.repositoryForm?.sparse);
      assert.deepEqual(sparse.facts.repositoryForm?.sparse, { cone: true, folders: ['sp-dir'] });
      assert.ok(existsSync(join(root, 'sp-dir', 'a.txt')));
      mark = posted.length;
      await withWindow({ showWarningMessage: async () => 'Check Out All' }, async () =>
        panel.settings.handle({ type: 'settingsRun', action: 'checkOutAll' }),
      );
      assert.equal((await waitPosted('settings', mark)).facts.repositoryForm?.sparse, null);
    }
  } finally {
    if (sparseCommit) {
      try {
        git('sparse-checkout', 'disable');
      } catch {
        // もうやめている
      }
      git('reset', '-q', '--hard', 'HEAD~1');
    }
    for (const k of [...keys, 'amonhen.defaultBranch', 'e2e.added', 'commit.template', 'commit.gpgsign', 'gpg.format']) {
      if (config(k) !== undefined) git('config', '--local', '--unset', k);
    }
    git('config', '--local', 'user.name', 'a');
    writeFileSync(excludeFile, exclude);
    if (hookFile) rmSync(hookFile, { force: true });
    if (existsSync(hooksDir)) for (const f of readdirSync(hooksDir)) if (!hooksBefore.has(f)) rmSync(join(hooksDir, f), { force: true });
    if (lfsVersion === undefined && config('lfs.repositoryformatversion') !== undefined) {
      git('config', '--local', '--unset', 'lfs.repositoryformatversion');
    }
    if (gitattributes === undefined) rmSync(join(root, '.gitattributes'), { force: true });
    else writeFileSync(join(root, '.gitattributes'), gitattributes);
    if (!hadGitignore) rmSync(join(root, '.gitignore'), { force: true });
  }
}

/** showInputBox の代わり。呼ばれるたびに values を順に返す */
function inputs(...values: string[]): () => Promise<string | undefined> {
  let i = 0;
  return async () => values[i++];
}

/**
 * Reflog に HEAD の動きが出る。今のブランチを reset（Mixed・Hard）すると、外れたコミットが「どのブランチにもない」と
 * 出て、Undo で戻る。Hard は作業中の変更を先に stash に残す。Reflog の行のコミットを checkout（detached）し、
 * History で見せる。どれもコアの命令（daemon だけ）
 */
async function reflogFlow(): Promise<void> {
  const branch = git('symbolic-ref', '--short', 'HEAD').trim();
  const rev = (r: string) => git('rev-parse', r).trim();
  const file = join(root, 'reflog-e2e.txt');
  git('switch', '-q', '-c', 'reflog-e2e');
  writeFileSync(file, 'reflog\n');
  git('add', 'reflog-e2e.txt');
  git('commit', '-q', '-m', 'reflog e2e');
  const lost = rev('HEAD');
  const parent = rev('HEAD^');
  await vscode.commands.executeCommand('amonhen.refresh');
  const reflog = (since: number, match: (m: Message<'reflog'>) => boolean) => waitPosted('reflog', since, match);
  let answer = 'Mixed';
  const asked: { message: string; buttons: string[] }[] = [];
  const stubs = {
    showWarningMessage: async (message: string, ...rest: unknown[]) => {
      const options = rest[0] as { modal?: boolean } | undefined;
      if (!options?.modal) return undefined;
      asked.push({ message, buttons: rest.slice(1) as string[] });
      return answer;
    },
    showInformationMessage: async () => undefined,
  };
  try {
    let mark = posted.length;
    const panel = await showView('amonhen.showReflog');
    await panel.reflog.handle({ type: 'reflogLoad' });
    const first = await reflog(mark, (m) => m.entries[0]?.oid === lost);
    assert.equal(first.entries[0]!.selector, 'HEAD@{0}');
    assert.equal(first.entries[0]!.message, 'commit: reflog e2e');
    assert.ok(first.entries[0]!.reachable);

    // Mixed: 外れたコミットの変更は作業ツリーに残り、そのコミットはどのブランチにもなくなる
    mark = posted.length;
    await withWindow(stubs, async () => vscode.commands.executeCommand('amonhen.resetToCommit', { oid: parent }));
    assert.equal(rev('HEAD'), parent, lastErrors());
    assert.match(asked.at(-1)!.message, new RegExp(`^Reset reflog-e2e to ${parent.slice(0, 8)}( “.*”)?\\?$`));
    assert.deepEqual(asked.at(-1)!.buttons, ['Mixed', 'Soft', 'Hard']);
    assert.equal(rev('HEAD'), parent);
    assert.equal(git('status', '--porcelain', '--', 'reflog-e2e.txt'), '?? reflog-e2e.txt\n');
    const after = await reflog(mark, (m) => m.entries[0]?.oid === parent);
    assert.equal(after.entries.find((e) => e.oid === lost)?.reachable, false);
    await vscode.commands.executeCommand('amonhen.undo');
    assert.equal(rev('HEAD'), lost);
    assert.equal(git('status', '--porcelain'), '');

    // Hard: 作業中の変更を stash に残してから、作業ツリーもそのコミットに合わせる
    writeFileSync(file, 'changed\n');
    answer = 'Hard';
    await withWindow(stubs, async () => vscode.commands.executeCommand('amonhen.resetToCommit', { oid: parent }));
    assert.equal(rev('HEAD'), parent, lastErrors());
    assert.equal(existsSync(file), false);
    assert.match(git('stash', 'list', '-n', '1', '--format=%s'), /Before reset --hard to [0-9a-f]{8}/);
    await vscode.commands.executeCommand('amonhen.undo');
    assert.equal(rev('HEAD'), lost);
    assert.equal(readFileSync(file, 'utf8').replaceAll('\r\n', '\n'), 'reflog\n');
    git('stash', 'drop', '-q');

    // 一覧の上のボタン: どのブランチからも辿れなくなったコミットから、ブランチを作って取り戻す
    git('reset', '-q', '--hard', parent);
    mark = posted.length;
    await panel.reflog.handle({ type: 'reflogLoad' });
    const unreachable = await reflog(mark, (m) => m.entries.some((e) => e.oid === lost && !e.reachable));
    const entry = unreachable.entries.find((e) => e.oid === lost)!;
    await withWindow({ showInputBox: async () => 'reflog-rescue' }, async () =>
      panel.reflog.handle({ type: 'reflogAction', action: 'newBranch', oid: entry.oid, selector: entry.selector }),
    );
    assert.equal(git('symbolic-ref', '--short', 'HEAD').trim(), 'reflog-rescue');
    assert.equal(rev('HEAD'), lost);

    // Checkout（detached）と、History で見せる
    await panel.reflog.handle({ type: 'reflogAction', action: 'checkout', oid: parent, selector: 'HEAD@{1}' });
    assert.equal(rev('HEAD'), parent, lastErrors());
    assert.throws(() => git('symbolic-ref', '-q', 'HEAD'));
    git('switch', '-q', 'reflog-e2e');
    git('reset', '-q', '--hard', lost);
    await vscode.commands.executeCommand('amonhen.refresh');
    mark = posted.length;
    await vscode.commands.executeCommand('amonhen.reflog.showInHistory', { oid: parent });
    await waitPosted('reveal', mark);
  } finally {
    git('switch', '-q', branch);
    git('branch', '-D', 'reflog-e2e');
    if (git('branch', '--list', 'reflog-rescue')) git('branch', '-D', 'reflog-rescue');
    await vscode.commands.executeCommand('amonhen.refresh');
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
 * History の編集（Reword・Squash・Fixup・並べ替え・Edit・Split）を、確かめてから行い、Undo で戻す。daemon ならコアの
 * `rewrite` で、git 実行層なら拡張の todo で行う。確認でやめれば何も変わらない。Edit と Split は止まったところから
 * サイドバーの Continue で続け、Undo 1 回で編集の前に戻る
 */
async function editFlow(label: string): Promise<void> {
  const branch = git('symbolic-ref', '--short', 'HEAD').trim();
  const work = `ed-${label}`;
  const rev = (r: string) => git('rev-parse', r).trim();
  const subjects = (n: number) => git('log', `-${n}`, '--format=%s', work).trim().split('\n');
  const name = (c: string) => `ed-${c}-${label}`;
  git('switch', '-q', '-c', work, branch);
  for (const c of ['a', 'b', 'c', 'd']) {
    writeFileSync(join(root, `${name(c)}.txt`), `${c}\n`);
    git('add', `${name(c)}.txt`);
    git('commit', '-q', '-m', name(c));
  }
  await vscode.commands.executeCommand('amonhen.refresh');
  const panel = await showView('amonhen.showHistory');
  const top = rev(work);
  const [d, c, b] = [rev(`${work}`), rev(`${work}~1`), rev(`${work}~2`)];
  const asked: string[] = [];
  let answer: string | undefined;
  const stubs = {
    showWarningMessage: async (message: string, ...rest: unknown[]) => {
      if (!(rest[0] as { modal?: boolean } | undefined)?.modal) return undefined;
      asked.push(message);
      return (rest.slice(1) as vscode.MessageItem[]).find((i) => i.title === answer);
    },
    showInformationMessage: async () => undefined,
  };
  // Webview からの編集は、受け取ったところで返る。終わり（busy が解ける）まで待つ
  const fromWebview = async (message: Parameters<typeof panel.history.handle>[0]) => {
    const mark = posted.length;
    await panel.history.handle(message);
    await waitPosted('busy', mark, (m) => !m.busy);
  };
  const undo = async (what: string) => {
    await vscode.commands.executeCommand('amonhen.undo');
    await until(`${what} の Undo`, () => rev(work) === top);
  };
  const mark = spawned.length;
  try {
    await withWindow(stubs, async () => {
      // やめれば何も変わらない。書いたメッセージのまま、編集欄を開き直す
      answer = undefined;
      const count = asked.length;
      const reopen = posted.length;
      await panel.history.handle({ type: 'saveMessage', oid: c, kind: 'reword', message: 'never' });
      await until('確認', () => asked.length > count);
      const again = await waitPosted('editMessage', reopen);
      assert.deepEqual([again.oid, again.kind, again.title, again.message], [c, 'reword', 'Reword', 'never']);
      assert.equal(rev(work), top);

      answer = 'Reword';
      await fromWebview({ type: 'saveMessage', oid: c, kind: 'reword', message: 'C2\n\nbody' });
      await until('reword', () => git('log', '-1', '--format=%B', `${work}~1`).trim() === 'C2\n\nbody');
      assert.deepEqual(subjects(3), [name('d'), 'C2', name('b')]);
      await undo('reword');

      answer = 'Squash';
      await fromWebview({ type: 'saveMessage', oid: c, kind: 'squash', message: 'B and C' });
      await until('squash', () => subjects(2).join() === [name('d'), 'B and C'].join());
      assert.equal(rev(`${work}~2`), rev(`${b}~1`));
      await undo('squash');

      answer = 'Fixup';
      await vscode.commands.executeCommand('amonhen.history.fixup', { oid: d });
      await until('fixup', () => subjects(1)[0] === name('c') && rev(work) !== top);
      assert.ok(existsSync(join(root, `${name('d')}.txt`)), 'd の変更は c に入る');
      await undo('fixup');

      // Webview での並べ替え: d を 2 つ古い側へ
      answer = 'Move';
      await fromWebview({ type: 'move', oid: d, to: 2 });
      await until('move', () => subjects(3).join() === [name('c'), name('b'), name('d')].join());
      await undo('move');

      // Edit: b で止まり、変えて amend されるように stage してから Continue
      answer = 'Edit';
      await vscode.commands.executeCommand('amonhen.history.edit', { oid: b });
      await until('edit で止まる', async () => (await api.manager.current!.backend.sequenceInProgress()) === 'rebase');
      writeFileSync(join(root, `${name('b')}.txt`), 'b edited\n');
      git('add', `${name('b')}.txt`);
      await vscode.commands.executeCommand('amonhen.refresh');
      await vscode.commands.executeCommand('amonhen.continueSequence');
      await until('edit の続き', async () => (await api.manager.current!.backend.sequenceInProgress()) === undefined && rev(work) !== top);
      assert.equal(git('show', `${work}~2:${name('b')}.txt`), 'b edited\n');
      assert.deepEqual(subjects(3), [name('d'), name('c'), name('b')]);
      await undo('edit');

      // Split: c を取り消して変更を作業ツリーに戻したところで止まる。2 つに分けてコミットしてから Continue
      answer = 'Split';
      await vscode.commands.executeCommand('amonhen.history.split', { oid: c });
      await until('split で止まる', async () => (await api.manager.current!.backend.sequenceInProgress()) === 'rebase');
      assert.equal(rev('HEAD'), b, 'c を取り消した');
      writeFileSync(join(root, `${name('c')}-2.txt`), 'more\n');
      git('add', `${name('c')}.txt`);
      git('commit', '-q', '-m', 'C part 1');
      git('add', `${name('c')}-2.txt`);
      git('commit', '-q', '-m', 'C part 2');
      await vscode.commands.executeCommand('amonhen.refresh');
      await vscode.commands.executeCommand('amonhen.continueSequence');
      await until('split の続き', async () => (await api.manager.current!.backend.sequenceInProgress()) === undefined && subjects(1)[0] === name('d'));
      assert.deepEqual(subjects(4), [name('d'), 'C part 2', 'C part 1', name('b')]);
      await undo('split');
      assert.ok(!existsSync(join(root, `${name('c')}-2.txt`)));
    });
    assert.deepEqual(
      asked.map((m) => m.replace(/^.* を /, '')),
      ['Reword しますか？', 'Reword しますか？', 'Squash しますか？', 'Fixup しますか？', 'Move しますか？', 'Edit しますか？', 'Split しますか？'],
    );
    // daemon ならコアが rebase する（拡張は git の rebase を起動しない）
    assert.equal(
      spawned.slice(mark).some((p) => p.args.includes('rebase')),
      label === 'git',
      label === 'git' ? '拡張が git の rebase をする' : 'コアが rebase する',
    );
  } finally {
    if (git('status', '--porcelain', '--untracked-files=no').trim() || existsSync(join(root, '.git', 'rebase-merge'))) {
      try { git('rebase', '--abort'); } catch { /* 止まっていない */ }
    }
    git('switch', '-q', '-f', branch);
    git('branch', '-q', '-D', work);
    rmSync(join(root, `${name('c')}-2.txt`), { force: true });
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

/**
 * Rebase と History の編集の前の確認。daemon なら見通し（積み重ねたブランチ・コンフリクトの見込み）を出し、今回だけ
 * 積み重ねたブランチも動かすボタンを選べば、そのブランチも一緒に動く（git 2.38 から）。Undo で全部戻る。
 * git 実行層では見通しなしで確かめる
 */
async function rewriteFlow(label: string): Promise<void> {
  const branch = git('symbolic-ref', '--short', 'HEAD').trim();
  const name = (b: string) => `rw-${b}-${label}`;
  const rev = (r: string) => git('rev-parse', r).trim();
  const commit = (file: string, subject: string) => {
    writeFileSync(join(root, file), `${subject}\n`);
    git('add', file);
    git('commit', '-q', '-m', subject);
  };
  git('switch', '-q', '-c', name('onto'), branch);
  commit(`rw-onto-${label}.txt`, name('onto'));
  git('switch', '-q', '-c', name('top'), branch);
  commit(`rw-one-${label}.txt`, name('one'));
  // 積み重ねたブランチ（top の書き換えるコミットに先頭がある）
  git('branch', name('mid'));
  commit(`rw-two-${label}.txt`, name('two'));
  await vscode.commands.executeCommand('amonhen.refresh');
  const before = { top: rev(name('top')), mid: rev(name('mid')) };

  const asked: { message: string; detail: string; buttons: string[] }[] = [];
  let answer: (buttons: string[]) => string | undefined = () => undefined;
  const stubs = {
    showWarningMessage: async (message: string, ...rest: unknown[]) => {
      const options = rest[0] as { modal?: boolean; detail?: string } | undefined;
      if (!options?.modal) return undefined;
      const items = rest.slice(1) as vscode.MessageItem[];
      asked.push({ message, detail: options.detail ?? '', buttons: items.map((i) => i.title) });
      const pick = answer(items.map((i) => i.title));
      return items.find((i) => i.title === pick);
    },
    showInformationMessage: async () => undefined,
  };
  // 今回だけ積み重ねたブランチも動かす。git が古くてできなければ、そのまま
  const moveStacked = (ok: string) => (buttons: string[]) => buttons.find((b) => b.includes('も動かす')) ?? ok;
  // mid は「設定がオフ」か「git が古い」で動かない、と見通しに出る
  const stays = new RegExp(`${name('mid')} は動きません（(rebase\\.updateRefs がオフ|動かすには git 2\\.38 が要ります)）`);
  try {
    await withWindow(stubs, async () => {
      // 右クリックの Rebase。やめれば何も変わらない
      const node = { kind: 'branch', ref: { name: `refs/heads/${name('onto')}` } };
      await vscode.commands.executeCommand('amonhen.nav.rebase', node);
      const first = asked.at(-1)!;
      assert.equal(first.message, `${name('top')} を ${name('onto')} の上に Rebase しますか？`);
      assert.equal(rev(name('top')), before.top);
      if (label === 'daemon') {
        assert.match(first.detail, stays);
        // git 2.38 からは、今回だけ動かすこともできる
        if (first.detail.includes('がオフ')) {
          assert.deepEqual(first.buttons, ['Rebase', 'Rebase（積み重ねたブランチも動かす）']);
        }
        // コンフリクトの見込みは git 2.40 から
        assert.doesNotMatch(first.detail, /コンフリクトする見込み/);
      } else {
        assert.equal(first.detail, 'Undo で戻せます。');
        assert.deepEqual(first.buttons, ['Rebase']);
      }

      // 今回だけ積み重ねたブランチも動かして rebase する
      answer = moveStacked('Rebase');
      await vscode.commands.executeCommand('amonhen.nav.rebase', node);
      await until('rebase', () => rev(`${name('top')}~2`) === rev(name('onto')) && rev(name('top')), undefined, lastErrors);
      const moved = asked.at(-1)!.buttons.length > 1;
      assert.equal(moved, label === 'daemon' && asked.at(-1)!.buttons.includes('Rebase（積み重ねたブランチも動かす）'));
      assert.equal(rev(name('mid')) === rev(`${name('top')}~1`), moved, moved ? 'mid も動く' : 'mid は残る');
      await vscode.commands.executeCommand('amonhen.undo');
      await until('rebase の Undo', () => rev(name('top')) === before.top && rev(name('mid')) === before.mid, undefined, lastErrors);

      // History の編集（Drop）。mid の先頭のコミットを取り除く
      await showView('amonhen.showHistory');
      const one = rev(`${name('top')}~1`);
      answer = moveStacked('Drop');
      await vscode.commands.executeCommand('amonhen.history.drop', { oid: one });
      const drop = asked.at(-1)!;
      assert.match(drop.message, /を Drop しますか？$/);
      assert.ok(drop.detail.startsWith('履歴から取り除きます。'), drop.detail);
      if (label === 'daemon') {
        assert.match(drop.detail, stays);
      } else {
        assert.deepEqual(drop.buttons, ['Drop']);
      }
      await until('drop', () => git('log', '-1', '--format=%s', `${name('top')}~1`).trim() === branchSubject(branch) && rev(name('top')), undefined, lastErrors);
      const dropMoved = drop.buttons.length > 1;
      assert.equal(rev(name('mid')) !== before.mid, dropMoved, dropMoved ? 'mid も動く' : 'mid は残る');
      await vscode.commands.executeCommand('amonhen.undo');
      await until('drop の Undo', () => rev(name('top')) === before.top && rev(name('mid')) === before.mid, undefined, lastErrors);
    });
  } finally {
    git('switch', '-q', branch);
    git('branch', '-q', '-D', name('onto'), name('top'), name('mid'));
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

/** ブランチの先頭のコミットの件名 */
function branchSubject(branch: string): string {
  return git('log', '-1', '--format=%s', branch).trim();
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
  const warned: string[] = [];
  const failed: string[] = [];
  const stubs = {
    showQuickPick: async (items: unknown, options?: { canPickMany?: boolean; placeHolder?: string }) => {
      const list = (await items) as ({ label: string; ref?: string } & Record<string, unknown>)[];
      if (options?.canPickMany) {
        return order.map((b) => list.find((i) => i.ref === `refs/heads/${b}`)!);
      }
      placeHolders.push(options?.placeHolder ?? '');
      return list.find((i) => choose(i.label));
    },
    showInformationMessage: async () => undefined,
    // git が断ったら 1 つずつを選び、コンフリクトしたら中止する
    showWarningMessage: async (message: string, ...rest: unknown[]) => {
      if ((rest[0] as { modal?: boolean } | undefined)?.modal) return 'Merge One by One';
      warned.push(message);
      return 'Abort';
    },
    showErrorMessage: async (message: string) => void failed.push(message),
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
        assert.match(placeHolders.at(-1)!, new RegExp(`${name('y')} がコンフリクトするので`));
      }
      // 1 つずつなら x は merge され、y で止まって中止する。残りの a を知らせる
      await until('1 つずつ', () => warned.some((m) => m.includes(`まだ merge していないもの: ${name('a')}`)));
      await until('中止', async () => (await repo.backend.sequenceInProgress()) === undefined);
      // x は今のブランチの先にあるので、git の既定どおり fast-forward になる
      assert.equal(head(), git('rev-parse', name('x')).trim());
      await vscode.commands.executeCommand('amonhen.undo');
      await until('1 つずつの Undo', () => head() === start);

      // merge が止まっている間に始めた merge は、コンフリクトではなく、何の途中かを言って失敗する
      git('merge', '-q', name('x'));
      assert.throws(() => childProcess.execFileSync('git', ['merge', '-q', name('y')], { cwd: root, stdio: 'ignore' }));
      await vscode.commands.executeCommand('amonhen.refresh');
      const warnings = warned.length;
      order = [name('a')];
      await vscode.commands.executeCommand('amonhen.mergeBranches');
      // 文は daemon ならコアの訳（VSCode の表示言語）、git 実行層なら拡張の日本語
      const busy = /merge (in progress|の途中です)/;
      await until('途中なので断る', () => failed.some((m) => busy.test(m)));
      assert.equal(warned.length, warnings, 'コンフリクトとは知らせない');
      assert.equal(await repo.backend.sequenceInProgress(), 'merge');
      git('merge', '--abort');
      git('reset', '-q', '--hard', start);
    });
    assert.deepEqual(
      failed.filter((m) => !/merge (in progress|の途中です)/.test(m)),
      [],
    );
  } finally {
    git('reset', '-q', '--hard', start);
    git('branch', '-q', '-D', name('a'), name('x'), name('y'));
    await vscode.commands.executeCommand('amonhen.refresh');
  }
}

suite('Amon Hen in VSCode', () => {
  suiteSetup(async () => {
    recordSpawns();
    recordErrors();
    api = await ext.activate();
  });

  // 前の試験の git 操作を知って遅れて始まる History の読み直しが、次の試験に入り込まないように、落ち着くまで待つ
  setup(async () => {
    const history = api.panel()?.history;
    if (!history) return;
    for (let i = 0; i < 10; i++) {
      await history.idle();
      const quiet = posted.length;
      await new Promise((r) => setTimeout(r, 700));
      if (!posted.slice(quiet).some((m) => m.type === 'reset' || m.type === 'rows')) return;
    }
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
    // エンジンの用意（置いたエンジンを一度起動する。macOS では OS の確認で数十秒かかることがある）が済むと、
    // サイドバーに Reflog が出る。後のテストがこの待ちを負わないよう、ここで待つ
    await until('サイドバーの Reflog', () => api.sidebar.getChildren().some((n) => n.kind === 'view' && n.view === 'reflog'), 50_000);
  });

  test('shows History, commit details and diffs through the daemon', async () => {
    const mark = posted.length;
    const panel = await showView('amonhen.showHistory');
    // 開いたときの読み直しは、記録を始める前に送り終えていることがある
    await panel.history.reset();
    // Webview は開いた直後に最初の行を頼む。読み直しが HEAD と読み方を決める前に届いても、History は空にならない
    await until('リポジトリ', () => panel.history.repository);
    void panel.history.handle({ type: 'more' });
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

  test('selects several commits in History: their diff and the actions on them, through the daemon', async () => {
    await historySelectionFlow();
  });

  test('searches History through the daemon, and shows only the matches keeping the selection', async () => {
    const mark = posted.length;
    const panel = await showView('amonhen.showHistory');
    await panel.history.reset();
    await waitPosted('searchable', mark, (m) => m.searchable);
    const total = (await waitPosted('rows', mark, (m) => m.total !== undefined)).total!;
    const query = 'subject:crlf';
    await panel.history.handle({ type: 'search', query, matchingOnly: false, selected: 0 });
    const { state } = await waitPosted('search', mark, (m) => m.state.query === query && !m.state.searching);
    assert.ok(state.rows.length > 0 && !state.error, JSON.stringify(state));
    // 行は飛び飛びに頼める（離れた一致へ移るとき）
    const last = state.rows[state.rows.length - 1]!;
    let at = posted.length;
    await panel.history.handle({ type: 'range', start: last, count: 1 });
    assert.match((await waitPosted('rows', at, (m) => m.start === last)).rows[0]!.subject, /crlf/i);
    // 一致だけの History にしても、選んでいたコミットを選んだまま
    at = posted.length;
    await panel.history.handle({ type: 'search', query, matchingOnly: true, selected: last });
    const switched = await waitPosted('reset', at, (m) => m.select !== undefined);
    assert.equal(switched.select, state.rows.length - 1);
    // 一致だけの行は、切り替えの reset の後から（History の読み直しが間に入っても取り違えない）
    const only = await waitPosted('rows', posted.indexOf(switched), (m) => m.total !== undefined);
    assert.equal(only.total, state.rows.length);
    assert.ok(only.rows.every((r) => /crlf/i.test(r.subject)));
    assert.ok((await waitPosted('search', at, (m) => !m.state.searching)).state.matchingOnly);
    // 検索をやめると、もとの History の同じコミットに戻る
    at = posted.length;
    await panel.history.handle({ type: 'search', query: '', matchingOnly: true, selected: state.rows.length - 1 });
    assert.equal((await waitPosted('reset', at, (m) => m.select !== undefined)).select, last);
    assert.equal((await waitPosted('rows', at, (m) => m.total !== undefined)).total, total);
    // 構文の誤りは件数の欄に出す
    at = posted.length;
    await panel.history.handle({ type: 'search', query: 'after:yesterday-ish', matchingOnly: false, selected: 0 });
    assert.ok((await waitPosted('search', at, (m) => !m.state.searching)).state.error);
    await panel.history.handle({ type: 'search', query: '', matchingOnly: false, selected: 0 });
  });

  test('keeps the History open when an older reload finishes after a newer one', async () => {
    const panel = await showView('amonhen.showHistory');
    const repo = api.manager.current!;
    const want = repo.want;
    let release = () => {};
    const gate = new Promise<void>((r) => (release = r));
    // 古い読み直しだけ、新しい読み直しが終わるまで止める
    repo.want = async function (this: typeof repo, ...args: Parameters<typeof want>) {
      repo.want = want;
      await gate;
      return want.apply(this, args);
    };
    try {
      const older = panel.history.reset();
      await panel.history.reset();
      release();
      await older;
    } finally {
      repo.want = want;
      release();
    }
    // ほかの読み直し（前の試験の git の変化を知って始まるもの）が途中だと、その History の行は送られない。落ち着くのを待つ
    for (;;) {
      const quiet = posted.length;
      await new Promise((r) => setTimeout(r, 800));
      if (!posted.slice(quiet).some((m) => m.type === 'reset' || m.type === 'rows')) break;
    }
    const at = posted.length;
    // 読み直しが頼むのは最初の行なので、2 行目の答えは、この頼みへの答えだけ
    await panel.history.handle({ type: 'range', start: 1, count: 1 });
    assert.equal((await waitPosted('rows', at, (m) => m.start === 1)).rows.length, 1);
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

  test('previews a rebase and a History edit through the daemon', async () => {
    await rewriteFlow('daemon');
  });

  test('edits History through the daemon', async () => {
    await editFlow('daemon');
  });

  test('shows the reflog and resets to a commit through the daemon', async () => {
    await reflogFlow();
  });

  test('marks tags by remote and signature, and pushes, replaces and deletes tags and changes remotes through the daemon', async () => {
    await tagsAndRemotesFlow();
  });

  test('lists worktrees under Working Copy, edits, creates, removes and prunes them through the daemon', async () => {
    await worktreeFlow();
  });

  test('shows a file’s history across a rename through the daemon', async () => {
    await fileHistoryFlow();
  });

  test('blames a file in the working tree and at a commit through the daemon', async () => {
    await blameFlow();
  });

  test('arranges branches with favorites, Inactive and Archived through the daemon', async () => {
    await branchArrangeFlow();
  });

  test('marks signed commits in History and goes to HEAD, a parent and a child through the daemon', async () => {
    await historySignatureFlow();
  });

  test('shows a shallow partial clone in History and fetches more of it through the daemon', async () => {
    await shallowFlow();
  });

  test('adds, changes, updates and removes a submodule through the daemon', async () => {
    await submoduleFlow();
  });

  test('adds, pulls, splits and pushes a subtree through the daemon', async () => {
    await subtreeFlow();
  });

  test('restores a file to the version before a commit through the daemon', async () => {
    await restoreFlow();
  });

  test('stashes only the staged changes, keeping them, or one file through the daemon', async () => {
    await stashOptionsFlow();
  });

  test('locks, unlocks, moves and locates a moved worktree through the daemon', async () => {
    await worktreeMoreFlow();
  });

  test('moves a submodule’s repository data into .git/modules and runs a command in each through the daemon', async () => {
    await submoduleMoreFlow();
  });

  test('forgets one rerere resolution and all of them through the daemon', async () => {
    await rerereForgetFlow();
  });

  test('creates an orphan branch, empty or keeping the files, through the daemon', async () => {
    await orphanFlow();
  });

  test('lists ignored files and deletes the ones chosen through the daemon', async () => {
    await deleteIgnoredFlow();
  });

  test('cleans up merged branches and ones whose upstream is gone, with one Undo, through the daemon', async () => {
    await branchCleanupFlow();
  });

  test('marks a branch merged by a pull request and deletes it with one confirmation through the daemon', async () => {
    await pullRequestMarkFlow();
  });

  test('shows stash details, branches from a stash and pops the latest through the daemon', async () => {
    await stashFlow();
  });

  test('asks before Undo and undoes several operations from Undo History', async () => {
    await undoFlow();
  });

  test('shows and writes this repository’s settings through the daemon', async () => {
    await settingsFlow();
  });

  test('keeps the commit message as a draft for each repository, and starts from the template', async () => {
    const panel = await showView('amonhen.showWorkingCopy');
    const wc = panel.workingCopy;
    const reopen = async () => {
      const mark = posted.length;
      wc.setRepository(panel.repository);
      return waitPosted('wcMessage', mark, () => true);
    };
    const template = join(root, '..', `amonhen-template-${process.pid}.txt`);
    writeFileSync(template, '# say why\nfeat: from the template\n\nDetails\n');
    git('config', 'commit.template', template);
    try {
      const fromTemplate = await reopen();
      assert.deepEqual([fromTemplate.summary, fromTemplate.body], ['feat: from the template', 'Details']);
      await wc.handle({ type: 'wcDraft', summary: 'my draft', body: 'my body' });
      const fromDraft = await reopen();
      assert.deepEqual([fromDraft.summary, fromDraft.body], ['my draft', 'my body']);
      // 空にすると下書きを忘れ、テンプレートに戻る
      await wc.handle({ type: 'wcDraft', summary: ' ', body: '' });
      assert.equal((await reopen()).summary, 'feat: from the template');
    } finally {
      git('config', '--unset', 'commit.template');
      rmSync(template, { force: true });
    }
    assert.equal((await reopen()).summary, '');
  });

  test('sends the messages of recent commits to the commit box', async () => {
    const panel = await showView('amonhen.showWorkingCopy');
    const mark = posted.length;
    panel.workingCopy.setRepository(panel.repository);
    const { messages } = await waitPosted('wcRecent', mark, (m) => m.messages.length > 0);
    assert.ok(messages.length <= 30);
    assert.equal(messages[0], git('log', '-1', '--format=%B').trim());
  });

  test('commits and pushes from the commit box', async () => {
    const panel = await showView('amonhen.showWorkingCopy');
    const branch = git('symbolic-ref', '--short', 'HEAD').trim();
    const bare = join(root, '..', `amonhen-push-${process.pid}.git`);
    git('init', '-q', '--bare', bare);
    git('remote', 'add', 'wc-push', bare);
    git('switch', '-q', '-c', 'wc-push-branch');
    git('push', '-q', '-u', 'wc-push', 'wc-push-branch');
    try {
      const file = join(root, 'wc-push.txt');
      writeFileSync(file, 'pushed\n');
      git('add', 'wc-push.txt');
      const mark = posted.length;
      await vscode.commands.executeCommand('amonhen.refresh');
      await waitPosted('wcFiles', mark, (m) => m.files.some((f) => f.group === 'staged' && f.path === 'wc-push.txt'));
      // 終わりの知らせが閉じられるまで待つことがあるので、ここでは待たない
      void panel.workingCopy.handle({ type: 'wcCommit', message: 'pushed from the commit box\n', amend: false, push: true });
      await until('push したコミット', () => {
        const remote = git('ls-remote', bare, 'refs/heads/wc-push-branch').split('\t')[0];
        return remote === git('rev-parse', 'HEAD').trim() && git('log', '-1', '--format=%s').trim() === 'pushed from the commit box';
      });
    } finally {
      git('switch', '-q', branch);
      git('branch', '-q', '-D', 'wc-push-branch');
      git('remote', 'remove', 'wc-push');
      rmSync(bare, { recursive: true, force: true });
      await vscode.commands.executeCommand('amonhen.refresh');
    }
  });

  test('shows what the hooks wrote when committing, and why a commit failed', async () => {
    const panel = await showView('amonhen.showWorkingCopy');
    const hooks = git('rev-parse', '--git-path', 'hooks').trim();
    const hook = resolve(root, hooks, 'pre-commit');
    mkdirSync(dirname(hook), { recursive: true });
    const start = git('rev-parse', 'HEAD').trim();
    const stage = async (name: string) => {
      writeFileSync(join(root, name), `${name}\n`);
      git('add', name);
      const mark = posted.length;
      await vscode.commands.executeCommand('amonhen.refresh');
      await waitPosted('wcFiles', mark, (m) => m.files.some((f) => f.group === 'staged' && f.path === name));
    };
    try {
      await stage('hook-ok.txt');
      // 前の試験のファイルが残っていれば、書くだけでは実行できるようにならない
      writeFileSync(hook, '#!/bin/sh\necho said by the hook\n');
      chmodSync(hook, 0o755);
      let at = posted.length;
      await panel.workingCopy.handle({ type: 'wcCommit', message: 'with a hook\n', amend: false });
      const said = await waitPosted('wcCommitOutput', at);
      assert.equal(said.title, 'Committed (hooks output)');
      assert.match(said.text, /said by the hook/);
      await stage('hook-no.txt');
      writeFileSync(hook, '#!/bin/sh\necho refused by the hook\nexit 1\n');
      at = posted.length;
      await panel.workingCopy.handle({ type: 'wcCommit', message: 'stopped by a hook\n', amend: false });
      const failed = await waitPosted('wcCommitOutput', at);
      assert.equal(failed.title, 'Couldn’t Commit');
      assert.match(failed.text, /refused by the hook/);
      assert.ok(!posted.slice(at).some((m) => m.type === 'wcCommitted'));
    } finally {
      rmSync(hook, { force: true });
      git('reset', '-q', '--hard', start);
      await vscode.commands.executeCommand('amonhen.refresh');
    }
  });

  test('counts the changed lines of each file through the daemon', async () => {
    const panel = await showView('amonhen.showWorkingCopy');
    const file = join(root, 'wc-stat.txt');
    writeFileSync(file, 'one\ntwo\n');
    git('add', 'wc-stat.txt');
    git('commit', '-q', '-m', 'wc-stat');
    try {
      writeFileSync(file, 'one\nchanged\nthree\n');
      const mark = posted.length;
      panel.workingCopy.setRepository(panel.repository);
      const { stats } = await waitPosted('wcStats', mark, (m) => m.stats.some((s) => s.path === 'wc-stat.txt'));
      const stat = stats.find((s) => s.path === 'wc-stat.txt')!;
      assert.deepEqual([stat.added, stat.removed], [2, 1]);
    } finally {
      git('reset', '-q', '--hard', 'HEAD~1');
      await vscode.commands.executeCommand('amonhen.refresh');
    }
  });

  test('adds a file to .gitignore, and notes a sparse checkout, through the daemon', async () => {
    const panel = await showView('amonhen.showWorkingCopy');
    const gitignore = join(root, '.gitignore');
    const had = existsSync(gitignore) ? readFileSync(gitignore) : undefined;
    const quickPick = vscode.window.showQuickPick;
    (vscode.window as { showQuickPick: unknown }).showQuickPick = async (items: string[]) => items[1];
    try {
      await vscode.commands.executeCommand('amonhen.addToGitignore', { path: 'dir/new.ignored-e2e' });
      assert.match(readFileSync(gitignore, 'utf8'), /^\*\.ignored-e2e$/m);
      await vscode.commands.executeCommand('amonhen.addToGitignore', { path: 'other.ignored-e2e' });
      assert.equal(readFileSync(gitignore, 'utf8').match(/\*\.ignored-e2e/g)?.length, 1);
      let mark = posted.length;
      panel.workingCopy.setRepository(panel.repository);
      assert.equal((await waitPosted('wcSparse', mark)).sparse, null);
      git('sparse-checkout', 'set', '--cone', 'e2e-only-folder');
      mark = posted.length;
      panel.workingCopy.setRepository(panel.repository);
      assert.deepEqual((await waitPosted('wcSparse', mark, (m) => m.sparse !== null)).sparse?.folders, ['e2e-only-folder']);
    } finally {
      (vscode.window as { showQuickPick: unknown }).showQuickPick = quickPick;
      git('sparse-checkout', 'disable');
      if (had) writeFileSync(gitignore, had);
      else rmSync(gitignore, { force: true });
      await vscode.commands.executeCommand('amonhen.refresh');
    }
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

  test('confirms a rebase and a History edit on the git execution layer', async () => {
    await rewriteFlow('git');
  });

  test('edits History on the git execution layer', async () => {
    await editFlow('git');
  });

  test('has no History search on the git execution layer', async () => {
    const mark = posted.length;
    const panel = await showView('amonhen.showHistory');
    await panel.history.reset();
    await waitPosted('searchable', mark, (m) => !m.searchable);
    const rows = await waitPosted('rows', mark);
    assert.equal(rows.total, undefined);
  });

  test('has no Reflog on the git execution layer', async () => {
    const reflogRow = () => api.sidebar.getChildren().some((n) => n.kind === 'view' && n.view === 'reflog');
    await until('Reflog の行が消える', () => !reflogRow());
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
