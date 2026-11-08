import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  archiveTopFolder,
  cloneBranch,
  defaultArchiveFormat,
  folderForBundle,
  readBundleHeader,
} from '../src/bundle/archiveBundle.ts';

const bundle = (lines: string[]) => Buffer.concat([Buffer.from(`${lines.join('\n')}\n\n`), Buffer.from([0x50, 0x41, 0x43, 0x4b])]);
const a = 'a'.repeat(40);
const b = 'b'.repeat(40);

test('the archive format starts from the last one, else zip on macOS and Windows and tar.gz on Linux', () => {
  assert.equal(defaultArchiveFormat('darwin', undefined), 'zip');
  assert.equal(defaultArchiveFormat('win32', undefined), 'zip');
  assert.equal(defaultArchiveFormat('linux', undefined), 'tar.gz');
  assert.equal(defaultArchiveFormat('darwin', 'tar'), 'tar');
});

test('the top folder joins the repository and the name, without characters files can’t have', () => {
  assert.equal(archiveTopFolder('amonhen', 'v1.0'), 'amonhen-v1.0');
  assert.equal(archiveTopFolder('amonhen', 'feature/x'), 'amonhen-feature-x');
});

test('a bundle header lists its refs and HEAD, and counts what it builds on', () => {
  assert.deepEqual(readBundleHeader(bundle(['# v2 git bundle', `${a} refs/heads/main`, `${a} HEAD`])), {
    refs: ['refs/heads/main'],
    head: true,
    prerequisites: 0,
  });
  assert.deepEqual(
    readBundleHeader(bundle(['# v3 git bundle', '@object-format=sha1', `-${b} base`, `${a} refs/heads/topic`, `${a} refs/tags/v1`])),
    { refs: ['refs/heads/topic', 'refs/tags/v1'], head: false, prerequisites: 1 },
  );
  assert.equal(readBundleHeader(Buffer.from('not a bundle\n\n')), undefined);
});

test('a bundle without HEAD clones main, master or its first branch', () => {
  assert.equal(cloneBranch({ refs: ['refs/heads/x', 'refs/heads/master'], head: false, prerequisites: 0 }), 'master');
  assert.equal(cloneBranch({ refs: ['refs/tags/v1', 'refs/heads/x'], head: false, prerequisites: 0 }), 'x');
  assert.equal(cloneBranch({ refs: ['refs/tags/v1'], head: false, prerequisites: 0 }), undefined);
  assert.equal(folderForBundle('/tmp/repo.bundle'), 'repo');
  assert.equal(folderForBundle('C:\\x\\Repo.BUNDLE'), 'Repo');
});
