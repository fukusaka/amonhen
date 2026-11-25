import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CheckerTone } from '../src/webview/checkerTone.ts';

/** 一色の絵（灰 `gray`、不透明度 `a`。不透明度を掛けていない RGBA） */
function solid(gray: number, a: number, n: number): number[] {
  return Array.from({ length: n * n }, () => [gray, gray, gray, a]).flat();
}

function tone(images: number[][], n: number): CheckerTone {
  const t = new CheckerTone();
  for (const i of images) t.add(i, n, n);
  return t;
}

test('the dark checkerboard is the default for light pictures', () => {
  // 白いアイコン（半分透けた白も）は暗い市松模様、黒いアイコン・中間の灰は明るい市松模様（コアと同じ）
  assert.ok(tone([solid(255, 255, 8)], 8).dark());
  assert.ok(tone([solid(255, 128, 8)], 8).dark());
  assert.ok(!tone([solid(0, 255, 8)], 8).dark());
  assert.ok(!tone([solid(128, 255, 8)], 8).dark());
  assert.ok(tone([solid(140, 255, 8)], 8).dark());
  // 透けた所は数えない
  const icon = [...solid(255, 255, 8), ...solid(0, 0, 8), ...solid(0, 0, 8), ...solid(0, 0, 8)];
  const t = new CheckerTone();
  t.add(icon, 8, 32);
  assert.ok(t.dark() && t.transparent());
  // 何もなければ明るい方
  assert.ok(!tone([solid(0, 0, 8)], 8).dark());
  assert.ok(!tone([], 8).dark());
});

test('both sides of a comparison decide once', () => {
  // 白と黒の平均の相対輝度は 0.5
  let t = tone([solid(255, 255, 8), solid(0, 255, 8)], 8);
  assert.ok(t.dark());
  assert.ok(!t.transparent());
  t = tone([solid(0, 255, 8), solid(0, 255, 8), solid(255, 255, 8)], 8);
  assert.ok(t.dark());
  t = tone([solid(0, 255, 8), solid(255, 0, 8)], 8);
  assert.ok(!t.dark() && t.transparent());
});

test('colors use relative luminance, and large pictures are sampled', () => {
  // 青だけ（相対輝度 0.07）と赤だけ（0.21）は明るい方、緑（0.72）は暗い方
  for (const [rgba, dark] of [
    [[0, 0, 255, 255], false],
    [[255, 0, 0, 255], false],
    [[0, 255, 0, 255], true],
  ] as const) {
    const t = new CheckerTone();
    t.add(rgba, 1, 1);
    assert.equal(t.dark(), dark);
  }
  // 大きな絵は間引くが、決め方は同じ
  const n = 1024;
  const t = new CheckerTone();
  t.add(new Uint8ClampedArray(n * n * 4).fill(255), n, n);
  assert.ok(t.dark());
  // 短すぎるバッファは数えない
  const short = new CheckerTone();
  short.add([255, 255, 255], 1, 1);
  assert.ok(!short.dark() && !short.transparent());
});
