/** 今からどれだけ前か（アプリの RelativeDateTimeFormatter の短い形） */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { relativeTime } from '../src/history/relativeTime.ts';

test('counts in the largest unit, rounded down', () => {
  const now = Date.UTC(2026, 0, 10, 12);
  const at = (secondsAgo: number) => relativeTime(now / 1000 - secondsAgo, now);
  assert.equal(at(0), '0 sec. ago');
  assert.equal(at(42), '42 sec. ago');
  assert.equal(at(90), '1 min. ago');
  assert.equal(at(3 * 3600 + 1800), '3 hr. ago');
  assert.equal(at(36 * 3600), '1 day ago');
  assert.equal(at(10 * 86400), '1 wk. ago');
  assert.equal(at(45 * 86400), '1 mo. ago');
  assert.equal(at(800 * 86400), '2 yr. ago');
  assert.equal(at(-7200), 'in 2 hr.');
});
