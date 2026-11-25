import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

type Entry = { command?: string; title?: unknown; submenu?: string; when?: string; group?: string };
const manifest = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as {
  contributes: { commands: Entry[]; menus: Record<string, Entry[]> };
};
const { commands, menus } = manifest.contributes;

// VSCode は題のないコマンドを起動のたびにエラーで知らせる（メニューの項目が紛れ込んだとき）
test('every contributed command has a title', () => {
  const bad = commands.filter((c) => typeof c.title !== 'string' || c.title === '').map((c) => c.command);
  assert.deepEqual(bad, []);
});

test('commands are declared once', () => {
  const seen = new Set<string>();
  const twice = commands.map((c) => c.command ?? '').filter((id) => seen.size === seen.add(id).size);
  assert.deepEqual(twice, []);
});

test('menu items name a declared command', () => {
  const declared = new Set(commands.map((c) => c.command));
  const unknown = Object.entries(menus).flatMap(([menu, items]) =>
    items.filter((i) => i.command !== undefined && !declared.has(i.command)).map((i) => `${menu}: ${i.command}`),
  );
  assert.deepEqual(unknown, []);
});
