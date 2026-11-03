/** 署名の見せ方（アプリの SignatureStatus と同じ決まり） */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { committerMatch, signatureExplanation, signatureIcon, signatureLabel, signatureStatus } from '../src/git/signature.ts';

test('signatureStatus follows git’s result, the key’s trust and who signed', () => {
  assert.deepEqual(signatureStatus(undefined), { kind: 'pending' });
  assert.deepEqual(signatureStatus({ status: 'G', trust: 'ultimate' }), { kind: 'verified', trust: 3 });
  assert.deepEqual(signatureStatus({ status: 'G', trust: 'marginal' }), { kind: 'verified', trust: 2 });
  // 信用度が決まっていなければ、正しい署名でも Untrusted
  assert.deepEqual(signatureStatus({ status: 'G', trust: 'undefined' }), { kind: 'untrusted' });
  assert.deepEqual(signatureStatus({ status: 'U', trust: 'full' }), { kind: 'untrusted' });
  // SSH の署名は allowed signers に載っていれば信用している
  assert.deepEqual(signatureStatus({ status: 'G', trust: '' }, '', 'ssh'), { kind: 'verified', trust: 3 });
  const emails = [{ email: 'Ann@example.com', validity: 'u' }];
  assert.deepEqual(signatureStatus({ status: 'G', trust: 'full', emails }, 'ann@example.com'), { kind: 'verified', trust: 3 });
  assert.deepEqual(signatureStatus({ status: 'G', trust: 'full', emails }, 'bob@example.com'), { kind: 'signerMismatch' });
  for (const [status, kind] of [
    ['X', 'expired'],
    ['Y', 'expired'],
    ['R', 'revoked'],
    ['B', 'bad'],
    ['V', 'unsupported'],
    ['E', 'noKey'],
  ] as const) {
    assert.equal(signatureStatus({ status }).kind, kind, status);
  }
});

test('committerMatch counts only user IDs that may be valid', () => {
  assert.equal(committerMatch([], 'a@example.com'), 'unknown');
  assert.equal(committerMatch([{ email: 'a@example.com', validity: 'u' }], ''), 'unknown');
  assert.equal(committerMatch([{ email: 'a@example.com', validity: 'f' }], 'A@example.com'), 'matched');
  assert.equal(committerMatch([{ email: 'a@example.com', validity: 'q' }], 'a@example.com'), 'unconfirmed');
  // 失効・期限切れのユーザー ID とだけ一致するなら、一致しないのと同じ
  assert.equal(committerMatch([{ email: 'a@example.com', validity: 'r' }], 'a@example.com'), 'mismatched');
  assert.equal(committerMatch([{ email: 'b@example.com', validity: 'u' }], 'a@example.com'), 'mismatched');
});

test('words and icons say whether it is a tag or a commit', () => {
  assert.equal(signatureLabel({ kind: 'verified', trust: 3 }), 'Verified');
  assert.match(signatureExplanation({ kind: 'bad' }, true), /The tag was changed/);
  assert.match(signatureExplanation({ kind: 'bad' }), /The commit was changed/);
  assert.match(signatureExplanation({ kind: 'signerMismatch' }, true), /tagger’s email/);
  assert.match(signatureExplanation({ kind: 'verified', trust: 2 }), /marginal/);
  assert.deepEqual(signatureIcon({ kind: 'verified', trust: 3 }), { icon: 'verified-filled', color: 'charts.green' });
  assert.deepEqual(signatureIcon({ kind: 'verified', trust: 2 }), { icon: 'verified', color: 'charts.green' });
  assert.equal(signatureIcon({ kind: 'bad' }).color, 'charts.red');
  assert.equal(signatureIcon({ kind: 'noKey' }).color, undefined);
});
