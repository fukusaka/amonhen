import assert from 'node:assert/strict';
import { join } from 'node:path';
import { test } from 'node:test';
import { pictureSettings } from '../src/history/pictureSettings.ts';

const home = join('/', 'home', 'me');
const root = join('/', 'repo');

function context(files: Record<string, string> = {}, localZone?: string) {
  const read = async (path: string) => {
    const text = files[path];
    if (text === undefined) throw new Error('not found');
    return text;
  };
  return { root, home, read, ...(localZone ? { localZone } : {}) };
}

test('the defaults are the app’s', async () => {
  const settings = await pictureSettings({}, context({}, 'Europe/Paris'));
  assert.deepEqual(settings, { options: { scheme: 'mermaid', timeZone: 'Europe/Paris', timeLimit: 2 }, mathScale: 1.21 });
  // Webview の時間帯がわからなければ、手伝いの OS の時間帯
  assert.equal((await pictureSettings({}, context())).options.timeZone, '');
});

test('the chosen colors, time zone, size and limit are passed on', async () => {
  const settings = await pictureSettings(
    { colors: 'mermaid-forest', timeZone: ' Asia/Tokyo ', formulaSize: 1.5, timeLimit: 0 },
    context({}, 'Europe/Paris'),
  );
  assert.deepEqual(settings, { options: { scheme: 'mermaid-forest', timeZone: 'Asia/Tokyo', timeLimit: 0 }, mathScale: 1.5 });
  assert.equal((await pictureSettings({ timeZone: 'UTC' }, context({}, 'Europe/Paris'))).options.timeZone, 'UTC');
});

test('values the app doesn’t offer fall back to the defaults', async () => {
  const settings = await pictureSettings({ colors: 'rainbow', formulaSize: 1.25, timeLimit: 3 }, context());
  assert.equal(settings.options.scheme, 'mermaid');
  assert.equal(settings.mathScale, 1.21);
  assert.equal(settings.options.timeLimit, 2);
});

test('an imported theme is read from its file', async () => {
  const theme = '{"theme": "base"}';
  const relative = await pictureSettings({ colors: 'imported', mermaidTheme: 'docs/theme.json' }, context({ [join(root, 'docs/theme.json')]: theme }));
  assert.deepEqual(relative.options, { scheme: 'mermaid-import:theme.json', importedTheme: theme, timeZone: '', timeLimit: 2 });
  assert.equal(relative.themeError, undefined);
  const inHome = await pictureSettings({ colors: 'imported', mermaidTheme: '~/mine.json' }, context({ [join(home, 'mine.json')]: theme }));
  assert.equal(inHome.options.importedTheme, theme);
  const missing = await pictureSettings({ colors: 'imported', mermaidTheme: 'gone.json' }, context());
  assert.equal(missing.options.importedTheme, undefined);
  assert.match(missing.themeError ?? '', /gone\.json/);
  const unset = await pictureSettings({ colors: 'imported' }, context());
  assert.equal(unset.options.scheme, 'mermaid-import:');
  assert.match(unset.themeError ?? '', /amonhen\.diagrams\.mermaidTheme/);
});
