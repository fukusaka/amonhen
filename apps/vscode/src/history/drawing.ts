/**
 * 確認のシートに添える前と後の絵（docs/design.md の「Previewing graph changes」。アプリの GraphDrawingView と
 * 同じ描き方）。コアの見込み（movePreview・mergePreview・pickPreview・octopusPreview・rewritePreview）から、行・
 * 省いたところ・分かれたところを並べた列（Drawing）を作る。描くのは Webview（`webview/drawing.ts`）。vscode に
 * 依存しない
 */
import type { DrawnCommit, EditStep, RewritePreview } from './rewritePreview.ts';

/** 札。kind は `local`・`remote`・`moved`（一緒に動いた）。filled は動かす ref・今のブランチ（塗った札） */
export interface DrawnLabel {
  name: string;
  kind: string;
  filled?: boolean;
}

/** 1 行。lane 0 が左の線、1 から先が右の線（いちばん下で分かれたところへ合流する） */
export interface DrawnRow {
  subject: string;
  lane: number;
  labels: DrawnLabel[];
  dim?: boolean;
  /** 強調（rebase の後の A′） */
  highlight?: boolean;
  /** 中を抜いた点（外れる・消えるコミット） */
  hollow?: boolean;
  /** これから作るコミット（点線の丸） */
  isNew?: boolean;
  /** 件名の後ろに添える文と、その色（`red` は消えるもの） */
  note?: string;
  tone?: 'red';
}

/** 省いたところ（その線を点線にして「and n more」）か、描かなかった線（「and n more branches」） */
export type DrawnItem = DrawnRow | { lane: number; more: number } | { lane: number; moreBranches: number };

export interface DrawnColumn {
  heading: string;
  items: DrawnItem[];
  /** 分かれたところ（一番下。右の線はここへ合流する） */
  base?: DrawnRow;
  /** merge コミットの行（items の何番目か）。そこから右の線それぞれの先端へ下りる */
  joinsAt?: number;
  /** 絵の代わりに出す文（取り込めない取り込み方） */
  message?: string;
}

/** 前と後（または今だけ）の列 */
export interface Drawing {
  columns: DrawnColumn[];
}

export interface DrawnRef {
  oid: string;
  name: string;
  kind: string;
}

/** コアの `movePreview`・`deletePreview` の答え */
export interface MovePreview {
  gainedCount: number;
  lostCount: number;
  files: number;
  upstream?: string | null;
  upstreamHas: number;
  drawing?: {
    gained: (DrawnCommit & { hidden?: boolean | null })[];
    gainedMore: number;
    gainedMoreAt?: number | null;
    lost: DrawnCommit[];
    lostMore: number;
    lostMoreAt?: number | null;
    base?: DrawnCommit | null;
    labels: DrawnRef[];
  } | null;
}

/** コアの `mergePreview` の答え */
export interface MergePreview {
  fastForward?: boolean | null;
  upToDate?: boolean | null;
  move?: MovePreview | null;
  drawing?: {
    ours: DrawnCommit[];
    oursMore: number;
    oursMoreAt?: number | null;
    theirs: DrawnCommit[];
    theirsMore: number;
    theirsMoreAt?: number | null;
    base?: DrawnCommit | null;
    labels: DrawnRef[];
    subject: string;
  } | null;
}

/** コアの `pickPreview` の答え */
export interface PickPreview {
  drawing?: {
    head: DrawnCommit[];
    picks: DrawnCommit[];
    picksMore: number;
    picksMoreAt?: number | null;
    labels: DrawnRef[];
  } | null;
}

/** コアの `octopusPreview` の答え */
export interface OctopusPreview {
  upToDate?: boolean | null;
  drawing?: {
    ours: DrawnCommit[];
    oursMore: number;
    oursMoreAt?: number | null;
    sides: { rev: string; tip?: DrawnCommit | null; more: number }[];
    base?: DrawnCommit | null;
    labels: DrawnRef[];
  } | null;
}

/** 分かれたところに添える文（「split off 3 mo. ago」）。日時がなければ undefined */
export function splitNote(base: DrawnCommit | null | undefined, now = Date.now()): string | undefined {
  if (!base?.time) return undefined;
  const days = (base.time * 1000 - now) / 86_400_000;
  const f = new Intl.RelativeTimeFormat('en', { numeric: 'auto', style: 'short' });
  const text =
    Math.abs(days) >= 365
      ? f.format(Math.round(days / 365), 'year')
      : Math.abs(days) >= 30
        ? f.format(Math.round(days / 30), 'month')
        : Math.abs(days) >= 1
          ? f.format(Math.round(days), 'day')
          : f.format(Math.round(days * 24), 'hour');
  return `split off ${text}`;
}

/** 長い列（新しい順）を、省いたところ（at の前、more 個）を挟んで並べる */
function run<T>(list: readonly T[], more: number, at: number | null | undefined, lane: number, row: (c: T, i: number) => DrawnRow): DrawnItem[] {
  const out: DrawnItem[] = [];
  const gap = more > 0 ? (at ?? list.length) : -1;
  list.forEach((c, i) => {
    if (i === gap) out.push({ lane, more });
    out.push(row(c, i));
  });
  if (gap === list.length) out.push({ lane, more });
  return out;
}

const refsOf = (labels: readonly DrawnRef[]) => (oid: string) => labels.filter((l) => l.oid === oid);
const plain = (refs: readonly DrawnRef[], except?: string): DrawnLabel[] =>
  refs.filter((l) => l.name !== except).map((l) => ({ name: l.name, kind: l.kind }));

/** rebase の前と後（アプリの RewriteDrawingView）。描く材料がなければ undefined */
export function rebaseDrawing(p: RewritePreview): Drawing | undefined {
  const d = p.drawing;
  if (!d || d.picks.length === 0) return undefined;
  const moving = new Set(p.moving.map((b) => b.name));
  const refs = refsOf(d.labels);
  const picks = [...d.picks].reverse();
  const layout = (pick: (c: DrawnCommit, i: number) => DrawnRow, onto: (c: DrawnCommit) => DrawnRow, pickLane: number) => [
    ...run(picks, d.picksMore ?? 0, d.picksMoreAt, pickLane, pick),
    ...run(d.onto, d.ontoMore, d.ontoMoreAt, 0, onto),
  ];
  // 左: 今。札はすべて今のところに
  const nowLabels = (oid: string): DrawnLabel[] => refs(oid).map((l) => ({ name: l.name, kind: l.kind, filled: l.name === p.current }));
  // 右: 今のブランチは先頭の A′ へ、一緒に動くブランチは元の先頭に対応する A′ へ。載せ先の札はそのまま
  const stays = (oid: string): DrawnLabel[] => plain(refs(oid).filter((l) => l.name !== p.current && !moving.has(l.name)));
  return {
    columns: [
      {
        heading: 'Now',
        items: layout(
          (c) => ({ subject: c.subject, lane: 1, dim: true, labels: nowLabels(c.oid) }),
          (c) => ({ subject: c.subject, lane: 0, labels: nowLabels(c.oid) }),
          1,
        ),
        base: d.base ? { subject: d.base.subject, lane: 0, dim: true, labels: nowLabels(d.base.oid), note: splitNote(d.base) } : undefined,
      },
      {
        heading: 'After the rebase',
        items: layout(
          (c, i) => ({
            subject: `${c.subject}′`,
            lane: 0,
            highlight: true,
            labels: [
              ...(i === 0 && p.current ? [{ name: p.current, kind: 'local', filled: true }] : []),
              ...refs(c.oid)
                .filter((l) => l.kind === 'local' && moving.has(l.name))
                .map((l) => ({ name: l.name, kind: 'moved' })),
            ],
          }),
          (c) => ({ subject: c.subject, lane: 0, dim: true, labels: stays(c.oid) }),
          0,
        ),
        base: d.base ? { subject: d.base.subject, lane: 0, dim: true, labels: stays(d.base.oid) } : undefined,
      },
    ],
  };
}

/**
 * History の編集の前と後（アプリの EditDrawingView）。左は書き換わるコミットで、drop するものは中を抜いた点、まとめる・
 * メッセージを変える・止まるものにはそうと添える。右はできるコミット（′ 付き）。下に書き換えないコミット
 */
export function editDrawing(p: RewritePreview): Drawing | undefined {
  const d = p.editDrawing;
  if (!d) return undefined;
  const refs = refsOf(d.labels);
  const notes: Record<string, Pick<DrawnRow, 'hollow' | 'note' | 'tone'>> = {
    drop: { hollow: true, note: 'dropped', tone: 'red' },
    squash: { note: 'combined with the one below' },
    fixup: { note: 'combined with the one below' },
    reword: { note: 'new message' },
    edit: { note: 'stops here' },
  };
  const column = (after: boolean): DrawnColumn => {
    const steps = after ? d.after : d.before;
    const top = steps[0]?.oid;
    // 今のブランチの札は左右とも一番上に。ほかの札は左（書き換える前）と、下の書き換えないコミットに
    const labels = (oid: string, base = false): DrawnLabel[] => [
      ...(oid === top && p.current ? [{ name: p.current, kind: 'local', filled: true }] : []),
      ...(!after || base ? plain(refs(oid), p.current ?? undefined) : []),
    ];
    const row = (s: EditStep): DrawnRow =>
      after
        ? { subject: `${s.subject}′`, lane: 0, highlight: true, labels: labels(s.oid), note: s.combined ? `${s.combined + 1} commits in one` : undefined }
        : { subject: s.subject, lane: 0, dim: true, labels: labels(s.oid), ...notes[s.action] };
    return {
      heading: after ? 'After the edit' : 'Now',
      items: run(steps, after ? d.afterMore : d.beforeMore, after ? d.afterMoreAt : d.beforeMoreAt, 0, row),
      base: d.base ? { subject: d.base.subject, lane: 0, dim: true, labels: labels(d.base.oid, true) } : undefined,
    };
  };
  return { columns: [column(false), column(true)] };
}

export interface MoveOptions {
  /** 動かす ref（`main`、`origin/main`）と、その札の種類 */
  moving: string;
  kind: string;
  afterTitle: string;
  /** 外れるコミットに添える文と色（Force Push は赤、Reset は灰） */
  lostNote: string;
  tone?: 'red';
  /** ref が消える（ブランチの削除）。右には札を描かない */
  removes?: boolean;
}

/**
 * ref を動かす前と後（アプリの MoveDrawingView。Force Push・Push・Reset・Fast-Forward・Undo・ブランチの削除）。
 * 行き先だけにあるコミットは左の線、外れるコミットは右の線。動かす ref の札は、右では行き先へ移る
 */
export function moveDrawing(p: MovePreview | undefined, o: MoveOptions): Drawing | undefined {
  const d = p?.drawing;
  if (!d) return undefined;
  const refs = refsOf(d.labels);
  // 両方にあれば 2 本（行き先の側が左）、片方だけなら 1 本
  const lostLane = d.gained.length === 0 ? 0 : 1;
  const destination = d.gained[0]?.oid ?? d.base?.oid;
  const column = (after: boolean): DrawnColumn => {
    const labels = (oid: string): DrawnLabel[] => {
      const out = plain(refs(oid), o.moving);
      const here = after ? oid === destination && !o.removes : refs(oid).some((l) => l.name === o.moving);
      return here ? [{ name: o.moving, kind: o.kind, filled: true }, ...out] : out;
    };
    const items = [
      ...run(d.gained, d.gainedMore, d.gainedMoreAt, 0, (c) =>
        // どこからも辿れないコミットは、今は History にない（Reflog にだけある）
        !after && c.hidden
          ? { subject: c.subject, lane: 0, dim: true, hollow: true, note: 'only in Reflog', labels: labels(c.oid) }
          : { subject: c.subject, lane: 0, highlight: after, labels: labels(c.oid) },
      ),
      ...run(d.lost, d.lostMore, d.lostMoreAt, lostLane, (c) =>
        after
          ? { subject: c.subject, lane: lostLane, dim: true, hollow: true, note: o.lostNote || undefined, tone: o.tone, labels: labels(c.oid) }
          : { subject: c.subject, lane: lostLane, labels: labels(c.oid) },
      ),
    ];
    // 両方の側があれば、左の分かれたところに、いつ分かれたか
    const split = !after && d.gained.length > 0 && d.lost.length > 0 ? splitNote(d.base) : undefined;
    return {
      heading: after ? o.afterTitle : 'Now',
      items,
      base: d.base ? { subject: d.base.subject, lane: 0, dim: true, labels: labels(d.base.oid), note: split } : undefined,
    };
  };
  return { columns: [column(false), column(true)] };
}

/**
 * merge の前と後（アプリの MergeDrawingView）。右の一番上に、これから作る merge コミット（点線の丸）が両方の
 * 先端を親に持ち、今のブランチの札がそこへ移る。fast-forward で済むなら ref が動く絵。nowOnly なら左の今だけ
 * （右には message）
 */
export function mergeDrawing(
  p: MergePreview | undefined,
  current: string,
  afterTitle = 'After the merge',
  nowOnly?: string,
): Drawing | undefined {
  if (!p) return undefined;
  if (p.fastForward && p.move?.drawing && nowOnly === undefined) {
    return moveDrawing(p.move, { moving: current, kind: 'local', afterTitle, lostNote: '' });
  }
  const d = p.drawing;
  if (!d) return undefined;
  const refs = refsOf(d.labels);
  const oursTip = d.ours[0]?.oid ?? d.base?.oid;
  const column = (after: boolean): DrawnColumn => {
    const labels = (oid: string): DrawnLabel[] => {
      const out = plain(refs(oid), current);
      return !after && oid === oursTip ? [{ name: current, kind: 'local', filled: true }, ...out] : out;
    };
    const items: DrawnItem[] = [
      ...(after ? [{ subject: d.subject, lane: 0, isNew: true, labels: [{ name: current, kind: 'local', filled: true }] }] : []),
      ...run(d.ours, d.oursMore, d.oursMoreAt, 0, (c) => ({ subject: c.subject, lane: 0, labels: labels(c.oid) })),
      ...run(d.theirs, d.theirsMore, d.theirsMoreAt, 1, (c) => ({ subject: c.subject, lane: 1, labels: labels(c.oid) })),
    ];
    return {
      heading: after ? afterTitle : 'Now',
      items,
      base: d.base
        ? { subject: d.base.subject, lane: 0, dim: true, labels: labels(d.base.oid), note: after ? undefined : splitNote(d.base) }
        : undefined,
      joinsAt: after ? 0 : undefined,
    };
  };
  return {
    columns: nowOnly === undefined ? [column(false), column(true)] : [column(false), { heading: afterTitle, items: [], message: nowOnly }],
  };
}

export type PickKind = 'cherryPick' | 'revert' | 'patches';

/**
 * cherry-pick・revert・パッチの取り込みの前と後（アプリの PickDrawingView）。右では今の先端の上に、これから作る
 * コミット（点線の丸。cherry-pick は件名に ′）が並び、今のブランチの札が一番上へ移る
 */
export function pickDrawing(p: PickPreview | undefined, current: string, kind: PickKind): Drawing | undefined {
  const d = p?.drawing;
  if (!d) return undefined;
  const refs = refsOf(d.labels);
  const tip = d.head[0]?.oid;
  const column = (after: boolean): DrawnColumn => {
    const labels = (oid: string): DrawnLabel[] => {
      const out = plain(refs(oid), current);
      return !after && oid === tip ? [{ name: current, kind: 'local', filled: true }, ...out] : out;
    };
    const picks = after
      ? run(d.picks, d.picksMore, d.picksMoreAt, 0, (c, i) => ({
          subject: kind === 'cherryPick' ? `${c.subject}′` : c.subject,
          lane: 0,
          isNew: true,
          labels: i === 0 ? [{ name: current, kind: 'local', filled: true }] : [],
        }))
      : [];
    const head = d.head.map((c, i): DrawnRow => ({ subject: c.subject, lane: 0, dim: after || i > 0, labels: labels(c.oid) }));
    return { heading: after ? pickTitles[kind] : 'Now', items: [...picks, ...head] };
  };
  return { columns: [column(false), column(true)] };
}

const pickTitles: Record<PickKind, string> = {
  cherryPick: 'After the cherry-pick',
  revert: 'After the revert',
  patches: 'After applying the patches',
};

/**
 * git が octopus merge のコミットに付ける件名（アプリの mergeMessage と同じ）。revs は完全な ref 名かコミット、into は
 * 取り込む先のブランチの短い名前
 */
export function octopusSubject(revs: readonly string[], into: string | undefined): string {
  const joined = (names: string[]) => {
    const quoted = names.map((n) => `'${n}'`);
    return quoted.length > 1 ? `${quoted.slice(0, -1).join(', ')} and ${quoted.at(-1)}` : (quoted[0] ?? '');
  };
  const kinds: [string, string, string][] = [
    ['refs/heads/', 'branch', 'branches'],
    ['refs/remotes/', 'remote-tracking branch', 'remote-tracking branches'],
    ['refs/tags/', 'tag', 'tags'],
  ];
  const groups = kinds.flatMap(([prefix, one, many]) => {
    const names = revs.filter((r) => r.startsWith(prefix)).map((r) => r.slice(prefix.length));
    return names.length === 0 ? [] : [`${names.length === 1 ? one : many} ${joined(names)}`];
  });
  let text = `Merge ${groups.join(', ')}`;
  const commits = revs.filter((r) => !kinds.some(([prefix]) => r.startsWith(prefix)));
  if (commits.length > 0) text += `${groups.length === 0 ? '' : '; '}${commits.length === 1 ? 'commit' : 'commits'} ${joined(commits)}`;
  if (into !== 'main' && into !== 'master') text += ` into ${into ?? 'HEAD'}`;
  return text;
}

/** octopus merge で描く線の数（残りは「and n more branches」） */
const MAX_LANES = 4;

/**
 * octopus merge の前と後（アプリの OctopusDrawingView）。取り込むものは並べた順（order）に右の線。右の一番上の
 * merge コミット（点線の丸。件名 subject）がすべての先端を親に持つ
 */
export function octopusDrawing(p: OctopusPreview | undefined, current: string, order: readonly string[], subject: string): Drawing | undefined {
  const d = p?.drawing;
  if (!d) return undefined;
  const refs = refsOf(d.labels);
  const oursTip = d.ours[0]?.oid ?? d.base?.oid;
  const all = order.flatMap((rev) => d.sides.filter((s) => s.rev === rev && s.tip).slice(0, 1));
  const shown = all.slice(0, MAX_LANES);
  const rest = all.length - shown.length;
  const tips = new Map(shown.map((s) => [s.tip!.oid, s.rev]));
  const column = (after: boolean): DrawnColumn => {
    const labels = (oid: string): DrawnLabel[] => {
      const out = plain(refs(oid), current);
      // タグ・コミットを取り込むときは、その名前を札に
      const rev = tips.get(oid);
      if (rev && !out.some((l) => l.name === rev)) out.push({ name: rev, kind: 'local' });
      return !after && oid === oursTip ? [{ name: current, kind: 'local', filled: true }, ...out] : out;
    };
    const items: DrawnItem[] = [
      ...(after ? [{ subject, lane: 0, isNew: true, labels: [{ name: current, kind: 'local', filled: true }] }] : []),
      ...run(d.ours, d.oursMore, d.oursMoreAt, 0, (c) => ({ subject: c.subject, lane: 0, labels: labels(c.oid) })),
      ...shown.flatMap((s, i): DrawnItem[] => [
        { subject: s.tip!.subject, lane: i + 1, labels: labels(s.tip!.oid) },
        ...(s.more > 0 ? [{ lane: i + 1, more: s.more }] : []),
      ]),
      ...(rest > 0 ? [{ lane: shown.length, moreBranches: rest }] : []),
    ];
    return {
      heading: after ? 'After the merge' : 'Now',
      items,
      base: d.base
        ? { subject: d.base.subject, lane: 0, dim: true, labels: labels(d.base.oid), note: after ? undefined : splitNote(d.base) }
        : undefined,
      joinsAt: after ? 0 : undefined,
    };
  };
  return { columns: [column(false), column(true)] };
}
