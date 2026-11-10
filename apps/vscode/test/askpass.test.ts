/** git と ssh が資格情報を尋ねる先（askpass） */
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname } from 'node:path';
import { test } from 'node:test';
import { Askpass, askpassEnv, hiddenAnswer } from '../src/git/askpass.ts';

/** `git credential fill` で、git に askpass でユーザー名とパスワードを尋ねさせる */
function fill(askpass: string): Promise<{ code: number; out: string }> {
  return new Promise((resolve) => {
    const env = { ...process.env, ...askpassEnv({ AMONHEN_ASKPASS: askpass }), GIT_TERMINAL_PROMPT: '0' };
    const child = execFile('git', ['-c', 'credential.helper=', 'credential', 'fill'], { env }, (err, out) =>
      resolve({ code: err ? ((err as { code?: number }).code ?? 1) : 0, out }),
    );
    child.stdin!.end('protocol=https\nhost=example.com\n\n');
  });
}

test('hides passwords and passphrases but not user names or host keys', () => {
  assert.equal(hiddenAnswer("Password for 'https://a@example.com': "), true);
  assert.equal(hiddenAnswer("Enter passphrase for key '/home/a/.ssh/id_ed25519': "), true);
  assert.equal(hiddenAnswer("Username for 'https://example.com': "), false);
  assert.equal(hiddenAnswer('Are you sure you want to continue connecting (yes/no/[fingerprint])? '), false);
});

test('gives git the script, preferring AMONHEN_ASKPASS', () => {
  assert.deepEqual(askpassEnv({ AMONHEN_ASKPASS: '/a/askpass' }), {
    GIT_ASKPASS: '/a/askpass',
    SSH_ASKPASS: '/a/askpass',
    SSH_ASKPASS_REQUIRE: 'prefer',
  });
});

test('answers git’s prompts with what is asked, and fails when cancelled', async () => {
  const asked: string[] = [];
  const helper = await Askpass.start(async (prompt) => {
    asked.push(prompt);
    return prompt.startsWith('Username') ? 'alice' : 'it’s secret';
  });
  try {
    const answered = await fill(helper.path);
    assert.equal(answered.code, 0);
    assert.match(answered.out, /^username=alice$/m);
    assert.match(answered.out, /^password=it’s secret$/m);
    assert.deepEqual(asked, ["Username for 'https://example.com': ", "Password for 'https://alice@example.com': "]);
  } finally {
    helper.dispose();
  }
  assert.equal(existsSync(dirname(helper.path)), false);

  const cancelled = await Askpass.start(async () => undefined);
  try {
    assert.notEqual((await fill(cancelled.path)).code, 0);
  } finally {
    cancelled.dispose();
  }
});
