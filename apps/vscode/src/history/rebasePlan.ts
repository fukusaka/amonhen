/**
 * コミットの編集を、非対話の rebase の todo に組み立てる。vscode に依存しない。
 * todo はこちらで丸ごと書き、GIT_SEQUENCE_EDITOR で git の todo に上書きする。
 * メッセージの書き換えは、対象のコミットの直後に `exec git commit --amend -F <file>` を挟んで行う。
 */
import { shellQuote } from '../git/exec.ts';
import type { Commit } from '../git/log.ts';

export type HistoryEdit =
  | { kind: 'reword'; oid: string; message: string }
  /** oid を親にまとめ、まとめたコミットのメッセージを message にする */
  | { kind: 'squash'; oid: string; message: string }
  /** oid を親にまとめ、親のメッセージを残す */
  | { kind: 'fixup'; oid: string }
  | { kind: 'drop'; oid: string }
  /** oid を当てたところで止まる */
  | { kind: 'edit'; oid: string }
  /** oid を取り消して変更を作業ツリーに戻したところで止まる。分けてコミットしてから続ける */
  | { kind: 'split'; oid: string }
  /** oid を、新しい順の並びで to 番目に動かす */
  | { kind: 'move'; oid: string; to: number };

export interface RebasePlan {
  /** rebase の起点。null なら `--root` */
  base: string | null;
  /** todo の内容 */
  todo: string;
  /** 書き出しておくメッセージ。キーはファイル名 */
  messages: Map<string, string>;
}

/**
 * chain は HEAD から first-parent で辿ったコミット（新しい順）。間に merge が無いこと。
 * messagePath はメッセージのファイル名から、exec で渡す絶対パスを作る。
 */
export function planRebase(chain: readonly Commit[], edit: HistoryEdit, messagePath: (name: string) => string): RebasePlan {
  // 古い順にする
  let seq = [...chain].reverse();
  const index = seq.findIndex((c) => c.oid === edit.oid);
  if (index < 0) {
    throw new Error('このコミットは編集できません（HEAD から辿れないか、merge より前にあります）');
  }

  let start = index;
  if (edit.kind === 'squash' || edit.kind === 'fixup') {
    if (index === 0) {
      throw new Error('親が編集できる範囲に無いので、まとめられません');
    }
    start = index - 1;
  }
  if (edit.kind === 'split' && seq[index]!.parents.length === 0) {
    throw new Error('最初のコミットは split できません');
  }
  if (edit.kind === 'move') {
    const target = seq.length - 1 - edit.to;
    if (target < 0 || target >= seq.length) {
      throw new Error('移動先が編集できる範囲の外です');
    }
    const [moved] = seq.splice(index, 1);
    seq.splice(target, 0, moved!);
    start = Math.min(index, target);
  }

  // 起点は、元の並びで start より前にあるコミット。先頭まで含むなら、そのさらに親（無ければ --root）
  const original = [...chain].reverse();
  const base = start > 0 ? original[start - 1]!.oid : (original[0]!.parents[0] ?? null);
  seq = seq.slice(start);

  const messages = new Map<string, string>();
  const lines: string[] = [];
  const amend = (oid: string, message: string) => {
    const name = `message-${oid}.txt`;
    messages.set(name, message);
    lines.push(`exec git commit --amend --allow-empty --no-verify --file=${shellQuote(messagePath(name))}`);
  };

  for (const c of seq) {
    if (c.oid !== edit.oid) {
      lines.push(`pick ${c.oid}`);
      continue;
    }
    switch (edit.kind) {
      case 'reword':
        lines.push(`pick ${c.oid}`);
        amend(c.oid, edit.message);
        break;
      case 'squash':
        lines.push(`fixup ${c.oid}`);
        amend(c.oid, edit.message);
        break;
      case 'fixup':
        lines.push(`fixup ${c.oid}`);
        break;
      case 'drop':
        lines.push(`drop ${c.oid}`);
        break;
      case 'edit':
        lines.push(`pick ${c.oid}`, 'break');
        break;
      case 'split':
        lines.push(`pick ${c.oid}`, 'exec git reset --quiet HEAD^', 'break');
        break;
      case 'move':
        lines.push(`pick ${c.oid}`);
        break;
    }
  }
  return { base, todo: `${lines.join('\n')}\n`, messages };
}
