/**
 * History の編集モード（docs/ux.md の「History's edit mode」）の決まり。選んだコミットから HEAD までの行（新しい順）に
 * 何をするかを決め、コアの `rewrite` の `plan`（古い順の並び）にする。アプリ（HistoryEdit.swift）と同じ。vscode に依存しない
 */

/** 行ですること。git の対話的な rebase の言葉 */
export type EditAction = 'pick' | 'reword' | 'squash' | 'fixup' | 'drop' | 'edit';

export const EDIT_ACTIONS: readonly EditAction[] = ['pick', 'reword', 'squash', 'fixup', 'drop', 'edit'];

/** 選ぶときの名前（git の言葉はツールチップだけに出す） */
export const ACTION_TITLES: Record<EditAction, string> = {
  pick: 'Keep',
  reword: 'Edit Message',
  squash: 'Combine with Older',
  fixup: 'Combine with Older, Keep Its Message',
  drop: 'Drop',
  edit: 'Stop to Change Files',
};

export const ACTION_TOOLTIPS: Record<EditAction, string> = {
  pick: 'Leave the commit as it is (git: pick).',
  reword: 'Give the commit a new message (git: reword).',
  squash: 'Make this and the older commit below one commit, with a message you can edit (git: squash).',
  fixup: 'Make this and the older commit below one commit, keeping the older one’s message (git: fixup).',
  drop: 'Remove the commit and its changes (git: drop).',
  edit: 'Stop after this commit to change its files and amend it, then Continue (git: edit).',
};

/** 1 行（1 コミット） */
export interface EditRow {
  oid: string;
  /** メッセージ全体 */
  message: string;
  author: string;
  /** merge のコミット（作り直すだけで、することは選べない） */
  merge?: boolean;
  action: EditAction;
  /** 直したメッセージ（reword）か、まとめたコミットのメッセージ（squash） */
  newMessage?: string;
}

/** コアに渡す 1 手（古い順） */
export interface PlanStep {
  oid: string;
  action: EditAction;
  message?: string;
}

/** 開くときの最初の変更（右クリックの近道・History のドラッグから） */
export type EditPreset =
  | { kind: 'reword'; oid: string }
  | { kind: 'combine'; oids: string[] }
  | { kind: 'fixup'; oid: string }
  | { kind: 'drop'; oid: string }
  | { kind: 'edit'; oid: string }
  /** `oid` を、新しい順の並びで `to` 番目へ */
  | { kind: 'move'; oid: string; to: number };

export function subjectOf(row: EditRow): string {
  return (row.newMessage ?? row.message).split('\n', 1)[0] ?? '';
}

/** 行で選べること。merge の行は作り直すだけ。いちばん古い行と merge のすぐ上の行は、まとめる相手がない */
export function choices(rows: readonly EditRow[], i: number): EditAction[] {
  if (rows[i]?.merge) return ['pick'];
  const noOlder = i === rows.length - 1 || rows[i + 1]?.merge === true;
  return EDIT_ACTIONS.filter((a) => !(noOlder && (a === 'squash' || a === 'fixup')));
}

/** merge をまたいで並べ替えはできない（何をどこへ動かすかが一意でない） */
export function canReorder(rows: readonly EditRow[]): boolean {
  return !rows.some((r) => r.merge);
}

/** i の行のすることを変える。メッセージを持たないことにしたら、直したメッセージは捨てる */
export function withAction(rows: readonly EditRow[], i: number, action: EditAction): EditRow[] {
  return rows.map((r, j) => {
    if (j !== i) return r;
    const { newMessage, ...rest } = r;
    return action === 'reword' || action === 'squash' ? { ...r, action } : { ...rest, action };
  });
}

/** i の行（squash）から下（古い方）へ、まとめる先の行までのメッセージを、古い順につないだもの（git の squash の既定） */
export function combinedMessage(rows: readonly EditRow[], i: number): string {
  let j = i;
  while (j + 1 < rows.length && (rows[j]!.action === 'squash' || rows[j]!.action === 'fixup')) j++;
  return rows
    .slice(i, j + 1)
    .reverse()
    .filter((r) => r.action !== 'fixup')
    .map((r) => r.newMessage ?? r.message)
    .join('\n\n');
}

/** from の行を to（抜いた後の並びでの位置ではなく、落とした位置）へ動かす。いちばん古い行はまとめられないので戻す */
export function moveRow(rows: readonly EditRow[], from: number, to: number): EditRow[] {
  const next = [...rows];
  const [moved] = next.splice(from, 1);
  next.splice(from < to ? to - 1 : to, 0, moved!);
  const last = next.length - 1;
  return next[last]!.action === 'squash' || next[last]!.action === 'fixup' ? withAction(next, last, 'pick') : next;
}

/** 何か変えたか（original は開いたときの並び） */
export function hasChanges(rows: readonly EditRow[], original: readonly string[]): boolean {
  return rows.some((r, i) => r.action !== 'pick' || r.oid !== original[i]);
}

/** 下に出す要約 */
export function summary(rows: readonly EditRow[], original: readonly string[]): string {
  const count = (...actions: EditAction[]) => rows.filter((r) => actions.includes(r.action)).length;
  const parts: string[] = [];
  if (rows.some((r, i) => r.oid !== original[i])) parts.push('reordered');
  const n = (k: number, what: string) => `${k} ${what}`;
  if (count('reword') > 0) parts.push(n(count('reword'), count('reword') === 1 ? 'message' : 'messages'));
  if (count('squash', 'fixup') > 0) parts.push(n(count('squash', 'fixup'), 'combined'));
  if (count('drop') > 0) parts.push(n(count('drop'), 'dropped'));
  if (count('edit') > 0) parts.push(n(count('edit'), 'to change files'));
  return parts.length === 0 ? 'No changes yet.' : `Changes: ${parts.join(', ')}. You can undo this.`;
}

/** コアに渡す並び（古い順）。まとまり（squash を含む）の最後の squash に、まとめたメッセージを付ける */
export function planSteps(rows: readonly EditRow[]): PlanStep[] {
  const ordered = [...rows].reverse();
  return ordered.map((row, i) => {
    const step: PlanStep = { oid: row.oid, action: row.action };
    if (row.action === 'reword') {
      step.message = row.newMessage ?? row.message;
    } else if (row.action === 'squash') {
      const rest = ordered.slice(i + 1);
      const group = rest.slice(0, prefixLength(rest, (r) => r.action === 'squash' || r.action === 'fixup'));
      if (!group.some((r) => r.action === 'squash')) {
        step.message = row.newMessage ?? combinedMessage(rows, rows.length - 1 - i);
      }
    }
    return step;
  });
}

function prefixLength<T>(items: readonly T[], test: (item: T) => boolean): number {
  const index = items.findIndex((item) => !test(item));
  return index < 0 ? items.length : index;
}

/** 最初の変更を入れる。メッセージを直すときは、その行の番号を返す（そのまま編集欄を開く） */
export function applyPreset(rows: readonly EditRow[], preset: EditPreset): { rows: EditRow[]; editMessageAt?: number } {
  const at = (oid: string) => rows.findIndex((r) => r.oid === oid);
  switch (preset.kind) {
    case 'reword': {
      const i = at(preset.oid);
      return i < 0 ? { rows: [...rows] } : { rows: withAction(rows, i, 'reword'), editMessageAt: i };
    }
    case 'fixup':
    case 'drop':
    case 'edit': {
      const i = at(preset.oid);
      return { rows: i >= 0 && choices(rows, i).includes(preset.kind) ? withAction(rows, i, preset.kind) : [...rows] };
    }
    case 'combine': {
      // 選んだもののうちいちばん古いものに、ほかをまとめる。1 つだけなら、1 つ古いコミット（読んである）にまとめる
      const indices = preset.oids.map(at).filter((i) => i >= 0).sort((a, b) => a - b);
      let next = [...rows];
      for (const i of indices.length === 1 ? indices : indices.slice(0, -1)) {
        if (choices(next, i).includes('squash')) next = withAction(next, i, 'squash');
      }
      return { rows: next };
    }
    case 'move': {
      const from = at(preset.oid);
      if (from < 0 || preset.to < 0 || preset.to >= rows.length) return { rows: [...rows] };
      // 落とす位置で数えると、下へ動かすときは抜けた分だけ先になる
      return { rows: moveRow(rows, from, preset.to > from ? preset.to + 1 : preset.to) };
    }
  }
}
