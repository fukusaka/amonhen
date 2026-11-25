import assert from 'node:assert/strict';
import { test } from 'node:test';
import { isVectorDrawable, rootElement } from '../src/history/vectorDrawable.ts';

const bytes = (text: string) => new TextEncoder().encode(text);

test('a VectorDrawable is an .xml file whose root is <vector> or <animated-vector>', () => {
  const vector = bytes('<?xml version="1.0" encoding="utf-8"?>\n<!-- icon -->\n<vector xmlns:android="x" android:width="24dp"/>');
  assert.equal(isVectorDrawable('res/drawable/icon.xml', vector), true);
  assert.equal(isVectorDrawable('res/drawable/ICON.XML', vector), true);
  assert.equal(isVectorDrawable('res/drawable/icon.svg', vector), false);
  assert.equal(isVectorDrawable('a.xml', bytes('<animated-vector xmlns:aapt="y">')), true);
  assert.equal(isVectorDrawable('a.xml', bytes('﻿<!DOCTYPE x>\n<android:vector/>')), true);
  assert.equal(isVectorDrawable('res/layout/main.xml', bytes('<LinearLayout/>')), false);
  assert.equal(isVectorDrawable('a.xml', bytes('<vectors/>')), false);
  assert.equal(isVectorDrawable('a.xml', bytes('not xml')), false);
});

test('the root element skips declarations, comments and DOCTYPE', () => {
  assert.equal(rootElement(bytes('<?xml?><!-- a --><!DOCTYPE b><resources>')), 'resources');
  assert.equal(rootElement(bytes('<!-- never closed')), undefined);
  assert.equal(rootElement(bytes('')), undefined);
});
