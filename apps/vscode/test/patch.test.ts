/** パッチの読み方と、git 実行層の format-patch・am・apply を一時ディレクトリの実リポジトリで確かめる */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { cleanSubject, decodeWords, parsePatch, patchPaths } from '../src/git/patch.ts';
import { UndoLog } from '../src/state/undo.ts';
import { initMain } from './git.ts';

Object.assign(process.env, {
  GIT_CONFIG_GLOBAL: '/dev/null',
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'a',
  GIT_AUTHOR_EMAIL: 'a@example.com',
  GIT_COMMITTER_NAME: 'a',
  GIT_COMMITTER_EMAIL: 'a@example.com',
});

const SERIES = `From 7e7483545ca94fc356725e6d9d00ab04180d8218 Mon Sep 17 00:00:00 2001
From: =?UTF-8?q?Ren=C3=A9=20Example?= <rene@example.com>
Date: Thu, 29 Oct 2026 03:19:33 +0900
Subject: [PATCH 1/2] =?UTF-8?q?change=201=20=C3=A9?=
 and more

---
diff --git a/f b/f
index 83db48f..bf5f2b1 100644
--- a/f
+++ b/f
@@ -1,3 +1,3 @@
-1
+x1
 2
 3
--
2.56.0

From 1111111111111111111111111111111111111111 Mon Sep 17 00:00:00 2001
From: Sender <s@example.com>
Subject: [PATCH v2 2/2] Re: add a file

From: Author Name <author@example.com>

---
diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+a
+b
diff --git a/old name.txt b/new name.txt
similarity index 100%
rename from old name.txt
rename to new name.txt
`;

test('reads a series of mails', () => {
  const summary = parsePatch(SERIES);
  assert.equal(summary.kind, 'mailbox');
  assert.equal(summary.patches.length, 2);
  assert.equal(summary.patches[0]!.subject, 'change 1 é and more');
  assert.equal(summary.patches[0]!.author, 'René Example');
  assert.deepEqual(summary.patches[0]!.files, [{ path: 'f', oldPath: undefined, added: 1, removed: 1, binary: false, status: 'modified' }]);
  // 本文の先頭の From: が作者
  assert.equal(summary.patches[1]!.subject, 'add a file');
  assert.equal(summary.patches[1]!.author, 'Author Name');
  assert.equal(summary.patches[1]!.files[0]!.status, 'added');
  assert.equal(summary.patches[1]!.files[1]!.status, 'renamed');
  assert.deepEqual(patchPaths(summary), ['f', 'new.txt', 'new name.txt', 'old name.txt']);
});

test('reads plain diffs, with and without git headers', () => {
  const git = parsePatch('diff --git "a/caf\\303\\251" "b/caf\\303\\251"\nindex 1..2 100644\nBinary files a/x and b/x differ\n');
  assert.equal(git.kind, 'diff');
  assert.equal(git.patches[0]!.files[0]!.path, 'café');
  assert.equal(git.patches[0]!.files[0]!.binary, true);
  const unified = parsePatch('--- a/x.c\t2026-10-29\n+++ b/x.c\t2026-10-29\n@@ -1 +1,2 @@\n-a\n+b\n+c\n');
  assert.deepEqual(unified.patches[0]!.files, [{ path: 'x.c', oldPath: undefined, added: 2, removed: 1, binary: false, status: 'modified' }]);
});

test('decodes encoded words and cleans subjects like git mailinfo', () => {
  assert.equal(decodeWords('=?UTF-8?B?44OR44OD44OB?= =?UTF-8?q?=E3=81=A7?='), 'パッチで');
  assert.equal(decodeWords('a =?iso-8859-1?q?caf=E9?= b'), 'a café b');
  assert.equal(cleanSubject('Re: [PATCH 2/3] [RFC] fix it'), 'fix it');
});

let tmp: string;
let dir: string;
let backend: CliBackend;

function git(...args: string[]): string {
  return execFileSync('git', args, { cwd: dir, encoding: 'utf8' }).trim();
}

function commitFile(name: string, content: string, message: string): string {
  writeFileSync(join(dir, name), content);
  git('add', name);
  git('commit', '-q', '-m', message);
  return git('rev-parse', 'HEAD');
}

beforeEach(async () => {
  tmp = realpathSync.native(mkdtempSync(join(tmpdir(), 'amonhen-patch-')));
  dir = join(tmp, 'work');
  initMain(dir);
  commitFile('a.txt', '1\n2\n3\n', 'first');
  backend = await CliBackend.open(new GitExecutor('git'), dir);
});

afterEach(async () => {
  await backend.dispose();
  rmSync(tmp, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

test('saves commits as patches and applies them with am, undoably', async () => {
  const second = commitFile('b.txt', 'b\n', 'second');
  const third = commitFile('c.txt', 'c\n', 'third');
  assert.deepEqual(await backend.firstParentsToHead(second), [second, third]);
  const files = await backend.formatPatch([second, third], {}, { dir: join(tmp, 'out') });
  assert.deepEqual(files.map((f) => basename(f)), ['0001-second.patch', '0002-third.patch']);
  const mbox = join(tmp, 'all.mbox');
  await backend.formatPatch([third], { coverLetter: false }, { file: mbox });
  assert.match(readFileSync(mbox, 'utf8'), /^Subject: \[PATCH\] third$/m);

  git('reset', '-q', '--hard', 'HEAD~2');
  assert.equal(await backend.checkPatch(files, true), undefined);
  const undo = new UndoLog(backend);
  const result = await undo.record('Apply 2 patches', ['refs/heads/main'], 'keep', () => backend.am(files, false), {
    pending: (r) => r !== 'done',
  });
  assert.equal(result, 'done');
  assert.equal(git('log', '--format=%s'), 'third\nsecond\nfirst');
  await undo.undo();
  assert.equal(git('log', '--format=%s'), 'first');
});

test('stops am where a patch does not apply and retries it with a 3-way merge', async () => {
  commitFile('a.txt', '1\nx\n3\n', 'change');
  const [patch] = await backend.formatPatch([git('rev-parse', 'HEAD')], {}, { dir: join(tmp, 'out') });
  git('reset', '-q', '--hard', 'HEAD~1');
  commitFile('a.txt', '1\ny\n3\n', 'other');
  assert.match((await backend.checkPatch([patch!], true)) ?? '', /patch does not apply/);

  const undo = new UndoLog(backend);
  const result = await undo.record('Apply 1 patch', ['refs/heads/main'], 'keep', () => backend.am([patch!], false), {
    pending: (r) => r !== 'done',
  });
  assert.equal(result, 'conflict');
  assert.equal(await backend.sequenceInProgress(), 'am');
  assert.deepEqual(await backend.rebaseProgress(), { current: 1, total: 1 });
  assert.deepEqual(await backend.amState(), { subject: 'change', threeWay: false, empty: false, retried: false });

  assert.equal(await backend.amThreeWay(), 'conflict');
  assert.equal((await backend.amState())?.retried, true);
  assert.match(readFileSync(join(dir, 'a.txt'), 'utf8'), /<<<<<<</);
  writeFileSync(join(dir, 'a.txt'), '1\nx\n3\n');
  git('add', 'a.txt');
  assert.equal(await backend.continueSequence('am'), 'done');
  assert.equal(await undo.settlePending(), true);
  assert.equal(git('log', '--format=%s'), 'change\nother\nfirst');
  await undo.undo();
  assert.equal(git('log', '--format=%s'), 'other\nfirst');
  // 当て直した印は、am が終わると rebase-apply ごと消え、次の am に残らない
  assert.equal(await backend.am([patch!], false), 'conflict');
  assert.equal((await backend.amState())?.retried, false);
});

test('applies a diff to the working tree, undoably', async () => {
  const diff = join(tmp, 'change.diff');
  writeFileSync(diff, 'diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n 1\n-2\n+two\n 3\n');
  const undo = new UndoLog(backend);
  const summary = parsePatch(readFileSync(diff, 'utf8'));
  assert.equal(await undo.recordFiles('Apply change.diff', patchPaths(summary), () => backend.applyPatch([diff], {})), 'done');
  assert.equal(readFileSync(join(dir, 'a.txt'), 'utf8'), '1\ntwo\n3\n');
  // 既定では index に触らない（git apply と同じ）
  assert.equal(git('diff', '--cached', '--name-only'), '');
  await undo.undo();
  assert.equal(readFileSync(join(dir, 'a.txt'), 'utf8'), '1\n2\n3\n');
  assert.ok(!existsSync(join(dir, '.git', 'rebase-apply')));
});
