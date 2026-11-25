import assert from 'node:assert/strict';
import { test } from 'node:test';
import { hexColor } from '../src/history/cssColor.ts';

test('a computed CSS color becomes #rrggbb for the core to draw with', () => {
  assert.equal(hexColor('rgb(204, 204, 204)'), '#cccccc');
  assert.equal(hexColor('rgba(30, 30, 30, 0.5)'), '#1e1e1e');
  assert.equal(hexColor('rgb(0 128 255 / 80%)'), '#0080ff');
  assert.equal(hexColor('rgba(0, 0, 0, 0)'), undefined);
  assert.equal(hexColor('transparent'), undefined);
});
