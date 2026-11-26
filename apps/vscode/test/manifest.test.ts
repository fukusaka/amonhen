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

/** サイドバーの右クリックで、viewItem の行に出るコマンド（when の viewItem の条件だけを見る） */
function sidebarItems(viewItem: string): string[] {
  return (menus['view/item/context'] ?? [])
    .filter((i) => {
      const when = i.when ?? '';
      if (!when.includes('view == amonhen.sidebar')) return false;
      const regex = when.match(/viewItem =~ \/(.+?)\/(?= |$)/)?.[1];
      const equal = when.match(/viewItem == ([\w.;-]+)/)?.[1];
      return regex !== undefined ? new RegExp(regex).test(viewItem) : equal === viewItem;
    })
    .map((i) => i.command ?? '');
}

// アプリのサイドバーの右クリックと同じ項目（SidebarViewController の menu(for:)）
test('the sidebar offers the app’s items on branches, remote branches and tags', () => {
  for (const tag of ['tag', 'tag;push']) {
    const items = sidebarItems(tag);
    assert.ok(items.includes('amonhen.nav.merge'), tag);
    for (const id of ['amonhen.nav.rebase', 'amonhen.newTag', 'amonhen.worktree.new']) assert.ok(!items.includes(id), `${tag}: ${id}`);
  }
  const remote = sidebarItems('remoteBranch');
  assert.ok(remote.includes('amonhen.nav.rebase'));
  for (const id of ['amonhen.newTag', 'amonhen.worktree.new']) assert.ok(!remote.includes(id), id);
  for (const branch of ['branch', 'branch.current', 'branch;ahead']) {
    const items = sidebarItems(branch);
    assert.ok(items.includes('amonhen.newTag'), branch);
    assert.ok(!items.includes('amonhen.worktree.new'), branch);
  }
});

const when = (menu: string, command: string) => (menus[menu] ?? []).find((i) => i.command === command)?.when ?? '';

// リモートがなければ Publish の先がない（アプリは押せなくし、ブランチの行には出さない）
test('Publish is offered only when there is a remote', () => {
  assert.match(when('view/item/context', 'amonhen.nav.publish'), /amonhen\.hasRemote/);
  assert.match(when('commandPalette', 'amonhen.publish'), /amonhen\.hasRemote/);
});

// アプリと同じく、Continue・Abort は途中の操作があるときだけ、Skip は rebase・cherry-pick・am だけ、Pull・Merge…・
// Rebase…・Apply Patch… は途中の操作がないときだけ
test('commands follow the operation in progress', () => {
  for (const id of ['amonhen.continueSequence', 'amonhen.abortSequence']) {
    assert.match(when('commandPalette', id), /&& amonhen\.sequence(?! =~)/, id);
  }
  assert.match(when('commandPalette', 'amonhen.skipSequence'), /amonhen\.sequence =~ \/\^\(rebase\|cherry-pick\|am\)\$\//);
  for (const id of ['amonhen.pull', 'amonhen.mergeBranches', 'amonhen.rebase', 'amonhen.applyPatch']) {
    assert.match(when('commandPalette', id), /!amonhen\.sequence/, id);
  }
  assert.match(when('explorer/context', 'amonhen.applyPatch'), /!amonhen\.sequence/);
});

// 先に確かめるものは、共有のコマンドの表（crates/commands）と同じく「…」で終わる
test('commands that ask first end with an ellipsis', () => {
  const titles = new Map(commands.map((c) => [c.command, c.title]));
  for (const [id, title] of [
    ['amonhen.fetch', 'Fetch…'],
    ['amonhen.pull', 'Pull…'],
    ['amonhen.push', 'Push…'],
    ['amonhen.forcePush', 'Force Push…'],
    ['amonhen.publish', 'Publish Branch…'],
  ]) {
    assert.equal(titles.get(id), title);
  }
});
