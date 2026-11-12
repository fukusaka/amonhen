import assert from 'node:assert/strict';
import { test } from 'node:test';
import { bytes, clientsCell, duration, indexCell, sshCell, statusLine, type DaemonRepo } from '../src/daemonStatus/protocol.ts';

const repo = (r: Partial<DaemonRepo>): DaemonRepo => ({ path: '/r', clients: 1, idleMs: 0, history: true, watching: true, ...r });

test('duration and bytes read as the app writes them', () => {
  assert.equal(duration(5_400), '5 s');
  assert.equal(duration(180_000), '3 min');
  assert.equal(duration(185_000), '3 min 5 s');
  assert.equal(duration(7_800_000), '2 h 10 min');
  assert.equal(bytes(0), 'Zero KB');
  assert.equal(bytes(512), '512 bytes');
  assert.equal(bytes(52_300), '52 KB');
  assert.equal(bytes(3_240_000), '3.2 MB');
  assert.equal(bytes(2_000_000_000), '2 GB');
});

test('the Daemon page writes the status and each repository as the app does', () => {
  assert.equal(
    statusLine({ pid: 12, version: '0.2.10', build: 'ab12', socket: '/s', uptimeMs: 300_000, connections: 2, idleExitMs: 600_000, repos: [] }),
    'pid 12 · version 0.2.10 (build ab12) · up 5 min · connections 2',
  );
  assert.equal(sshCell(repo({})), '—');
  assert.equal(sshCell(repo({ ssh: true, sent: 2_000, received: 3_000_000 })), '2 KB · 3 MB');
  assert.equal(sshCell(repo({ ssh: true, closed: 'Lost the connection' })), 'Disconnected');
  assert.equal(clientsCell(repo({ clients: 0, idleMs: 65_000 })), 'None (for 1 min 5 s)');
  assert.equal(indexCell(repo({ indexRows: 52_000, indexBytes: 3_100_000 })), '52,000 rows · 3.1 MB');
  assert.equal(indexCell(repo({})), '—');
});
