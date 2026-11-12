//! History の永続キャッシュ。次の起動では、前回の History（コミットの表）をすぐ読む。レイアウトは持たない
//! （開いてから計算する。`docs/design.md` の「History persistent cache」）。
//!
//! 形式: リポジトリごとに 1 つのフォルダに、コミットの表と、並び順ごとの表を置く。
//! - コミットの表は列ごとのファイル（`ids`・`times`・`ends`（行ごとの親の終わりの位置）・`parents`（親の行番号））。
//!   行は足すだけで動かさない（どの並び順の表からも同じ行番号で指す）。長さは `table` に書き、丸ごと置き換える。
//! - 並び順の表（`date`・`topo`）は、その順に古い方から並べたコミットの表の行番号。書くときは前回と違う所から
//!   後ろだけを切って足す。`state-date`・`state-topo` に、その History の起点・HEAD・行の数などを書き、丸ごと置き換える。
//!   `state-*` を書いたときに書き込みが済んだことになる（表が長ければ、余りは読まずに次に書くとき切る）。
//!
//! 並び順を替えても、コミットの表は書かない（並び順の表を書くだけ）。どの並び順の表からも指されない行（amend や
//! rebase で消えたコミット）が増えたら、今の並び順で作り直す（ほかの並び順の表は捨てる）。
//!
//! ほかのプロセス（共有の daemon とアプリ、CLI）と同時に読み書きするので、`lock` のファイルで書くときは
//! 1 つだけ、読むときは書いていない間だけにする。読むときは写したファイルから、新しい方から並べてメモリにコピー
//! する（写したままにしておくと、あとでほかのプロセスが切ったとき、そこに触れて落ちる）。

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use amonhen_layout::Pod;
use amonhen_model::{CommitGraph, HistoryOrder, Oid, Result};
use memmap2::Mmap;

const MAGIC: [u8; 8] = *b"AMONHIS3";
/// 形式の指紋（キャッシュに書くものを決めるソースのハッシュ。`build.rs`）。フォルダの名前と `state` に入れ、違う
/// 形式のものは読まない（作り直す）。並んで動く別の形式のビルドとは、別のフォルダを使う。
const FORMAT: &str = env!("AMONHEN_CACHE_FORMAT");

/// ほかの形式の同じリポジトリのキャッシュを、使われなくなってから消すまでの日数（並んで動く別のビルドが、まだ
/// 使っているかもしれない）。
const KEEP_OTHER_FORMATS_DAYS: u64 = 7;

/// 変化のあと、この間ほかの変化がなければ書く（続けて起きる変化を 1 回の書き込みにまとめる）。
pub const WRITE_DELAY: Duration = Duration::from_secs(2);

/// どの並び順の表からも指されない行が、コミットの表のこの割合（分の 1）を超えたら作り直す。
const GARBAGE_DIVISOR: usize = 5;

const ORDERS: [HistoryOrder; 2] = [HistoryOrder::Date, HistoryOrder::Topo];

/// `state` の `version`（形式の指紋の頭 8 文字）。
fn version() -> u32 {
    u32::from_str_radix(&FORMAT[..8], 16).unwrap()
}

/// キャッシュに入れる History 一式。
#[derive(Clone, Debug, PartialEq)]
pub struct CachedHistory {
    pub tips: Vec<Oid>,
    /// shallow clone の境界（`GitBackend::shallow`）。変わっていれば History を作り直す。
    pub shallow: Vec<Oid>,
    pub head: Option<Oid>,
    /// `graph` の並び順。
    pub order: HistoryOrder,
    /// 新しい方から並べた表（メモリの上と同じ）。
    pub graph: CommitGraph,
}

/// 書く History（`CachedHistory` の借りたもの）。
#[derive(Clone, Copy)]
pub struct Entry<'a> {
    pub tips: &'a [Oid],
    pub shallow: &'a [Oid],
    pub head: Option<Oid>,
    pub order: HistoryOrder,
    pub graph: &'a CommitGraph,
}

impl CachedHistory {
    pub fn entry(&self) -> Entry<'_> {
        Entry {
            tips: &self.tips,
            shallow: &self.shallow,
            head: self.head,
            order: self.order,
            graph: &self.graph,
        }
    }
}

/// `state-*` の中身（並び順の表のほかの部分）。
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub tips: Vec<Oid>,
    pub shallow: Vec<Oid>,
    pub head: Option<Oid>,
    pub order: HistoryOrder,
    /// 並び順の表の行の数。
    pub rows: u64,
    /// 最後の（最も新しい）行の ID。表がこれと合わなければ使わない。
    last: Option<Oid>,
}

/// `table` の中身（コミットの表の長さ）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Table {
    rows: u64,
    /// 親の数（`parents` の長さ）。
    parents: u64,
}

fn order_code(order: HistoryOrder) -> u32 {
    match order {
        HistoryOrder::Date => 0,
        HistoryOrder::Topo => 1,
    }
}

fn order_from_code(code: u32) -> Option<HistoryOrder> {
    match code {
        0 => Some(HistoryOrder::Date),
        1 => Some(HistoryOrder::Topo),
        _ => None,
    }
}

/// `dir` の中の、`key`（リポジトリのパスなど）ごとのキャッシュのフォルダ。
pub fn path_for(dir: &Path, key: &str) -> PathBuf {
    // FNV-1a。Rust のバージョンが変わっても同じ名前になるよう、標準のハッシュは使わない。
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    dir.join(format!("history-{hash:016x}-{}", &FORMAT[..8]))
}

/// `path` と同じリポジトリの、ほかの形式のキャッシュ（フォルダと、前の形式のファイル
/// `history-<ハッシュ>-<形式>.bin`・`history-<ハッシュ>.bin`）のうち、`KEEP_OTHER_FORMATS_DAYS` 日書かれていない
/// ものを消す。
fn prune_other_formats(path: &Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return;
    };
    // `history-<リポジトリのハッシュ>-<形式>` の `history-<リポジトリのハッシュ>`。
    let Some(repo) = name.rsplit_once('-').map(|(repo, _)| repo) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let keep = Duration::from_secs(KEEP_OTHER_FORMATS_DAYS * 24 * 60 * 60);
    for entry in entries.flatten() {
        let other = entry.file_name();
        let Some(other) = other.to_str() else {
            continue;
        };
        let format = |rest: &str| !rest.is_empty() && !rest.contains('-') && !rest.contains('.');
        let same_repo = other == format!("{repo}.bin")
            || other
                .strip_prefix(repo)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(|rest| format(rest.strip_suffix(".bin").unwrap_or(rest)));
        let unused = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|age| age > keep));
        if same_repo && other != name && unused {
            let other = entry.path();
            let _ = if other.is_dir() {
                std::fs::remove_dir_all(other)
            } else {
                std::fs::remove_file(other)
            };
        }
    }
}

fn bytes_of<T: Pod>(values: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), std::mem::size_of_val(values)) }
}

/// 読むために写したファイル。写すのは共有か排他のロックの間だけ（書く側は排他のロックを取ってから切るので、
/// 写している間に短くならない）。
struct Mapped(Option<Mmap>);

impl Mapped {
    fn open(path: &Path) -> Option<Self> {
        let file = match File::open(path) {
            Ok(file) => file,
            // まだ書いていない（行のない表）。
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(Self(None)),
            Err(_) => return None,
        };
        if file.metadata().ok()?.len() == 0 {
            return Some(Self(None));
        }
        // SAFETY: 上のとおり、写している間はほかのプロセスも書き換えない。
        Some(Self(Some(unsafe { Mmap::map(&file) }.ok()?)))
    }

    /// 先頭の `count` 個の `T`。足りなければ `None`。
    fn array<T: Pod>(&self, count: usize) -> Option<&[T]> {
        if count == 0 {
            return Some(&[]);
        }
        let bytes: &[u8] = self.0.as_deref().unwrap_or_default();
        let need = count.checked_mul(size_of::<T>())?;
        if bytes.len() < need || !(bytes.as_ptr() as usize).is_multiple_of(align_of::<T>()) {
            return None;
        }
        // SAFETY: `T` は `Pod`。長さと揃え方は上で確かめた。
        Some(unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast(), count) })
    }
}

/// キャッシュのフォルダの中のファイル。
struct Files {
    dir: PathBuf,
    ids: PathBuf,
    times: PathBuf,
    ends: PathBuf,
    parents: PathBuf,
    table: PathBuf,
    lock: PathBuf,
}

impl Files {
    fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            ids: dir.join("ids"),
            times: dir.join("times"),
            ends: dir.join("ends"),
            parents: dir.join("parents"),
            table: dir.join("table"),
            lock: dir.join("lock"),
        }
    }

    /// 並び順の表。
    fn order(&self, order: HistoryOrder) -> PathBuf {
        self.dir.join(order.name())
    }

    fn state(&self, order: HistoryOrder) -> PathBuf {
        self.dir.join(format!("state-{}", order.name()))
    }

    /// 書くときは `exclusive`、読むときは共有でロックする（手放すと外れる）。
    fn lock(&self, exclusive: bool) -> Option<File> {
        let file = OpenOptions::new()
            .create(exclusive)
            .truncate(false)
            .write(true)
            .open(&self.lock)
            .ok()?;
        if exclusive {
            file.lock().ok()?;
        } else {
            file.lock_shared().ok()?;
        }
        Some(file)
    }

    fn read_table(&self) -> Option<Table> {
        let bytes = std::fs::read(&self.table).ok()?;
        let (head, rest) = header(&bytes)?;
        if head != b"table" || rest.len() != 16 {
            return None;
        }
        Some(Table {
            rows: u64::from_le_bytes(rest[..8].try_into().unwrap()),
            parents: u64::from_le_bytes(rest[8..].try_into().unwrap()),
        })
    }

    fn read_state(&self, order: HistoryOrder) -> Option<State> {
        decode_state(&std::fs::read(self.state(order)).ok()?).filter(|s| s.order == order)
    }

    /// 写したコミットの表（`table` の長さまで）。
    fn map_table(&self, table: Table) -> Option<MappedTable> {
        Some(MappedTable {
            table,
            ids: Mapped::open(&self.ids)?,
            times: Mapped::open(&self.times)?,
            ends: Mapped::open(&self.ends)?,
            parents: Mapped::open(&self.parents)?,
        })
    }

    /// キャッシュを空にする（`lock` は残す）。
    fn clear(&self) -> Result<()> {
        // 先に `state-*` を消す（途中で落ちても、残った表を読まない）。
        for order in ORDERS {
            remove_file(&self.state(order))?;
        }
        for file in [
            &self.table,
            &self.ids,
            &self.times,
            &self.ends,
            &self.parents,
        ] {
            remove_file(file)?;
        }
        for order in ORDERS {
            remove_file(&self.order(order))?;
        }
        Ok(())
    }
}

fn remove_file(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

struct MappedTable {
    table: Table,
    ids: Mapped,
    times: Mapped,
    ends: Mapped,
    parents: Mapped,
}

/// コミットの表の列。
struct Columns<'a> {
    ids: &'a [Oid],
    times: &'a [i64],
    ends: &'a [u32],
    parents: &'a [u32],
}

impl MappedTable {
    fn columns(&self) -> Option<Columns<'_>> {
        let n = usize::try_from(self.table.rows).ok()?;
        let c = Columns {
            ids: self.ids.array(n)?,
            times: self.times.array(n)?,
            ends: self.ends.array(n)?,
            parents: self
                .parents
                .array(usize::try_from(self.table.parents).ok()?)?,
        };
        (c.ends.last().map_or(0, |&e| u64::from(e)) == self.table.parents).then_some(c)
    }
}

impl Columns<'_> {
    /// 行 `s` の親の行番号。
    fn parents_of(&self, s: usize) -> Option<&[u32]> {
        let start = if s == 0 { 0 } else { self.ends[s - 1] } as usize;
        self.parents.get(start..self.ends[s] as usize)
    }
}

/// 頭（`MAGIC`・形式・種類）を確かめて、残りを返す。種類は `table` か `state`。
fn header(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = bytes.strip_prefix(&MAGIC)?;
    let (v, rest) = rest.split_first_chunk::<4>()?;
    if u32::from_le_bytes(*v) != version() {
        return None;
    }
    let (kind, rest) = rest.split_first_chunk::<5>()?;
    Some((kind, rest))
}

fn encode_header(kind: &[u8; 5]) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&version().to_le_bytes());
    out.extend_from_slice(kind);
    out
}

fn encode_table(t: Table) -> Vec<u8> {
    let mut out = encode_header(b"table");
    out.extend_from_slice(&t.rows.to_le_bytes());
    out.extend_from_slice(&t.parents.to_le_bytes());
    out
}

fn encode_state(s: &State) -> Vec<u8> {
    let mut out = encode_header(b"state");
    out.extend_from_slice(&order_code(s.order).to_le_bytes());
    out.push(u8::from(s.head.is_some()));
    out.extend_from_slice(&s.head.unwrap_or_default().0);
    out.push(u8::from(s.last.is_some()));
    out.extend_from_slice(&s.last.unwrap_or_default().0);
    out.extend_from_slice(&s.rows.to_le_bytes());
    out.extend_from_slice(&(s.tips.len() as u32).to_le_bytes());
    out.extend_from_slice(&(s.shallow.len() as u32).to_le_bytes());
    for id in s.tips.iter().chain(&s.shallow) {
        out.extend_from_slice(&id.0);
    }
    out
}

fn decode_state(bytes: &[u8]) -> Option<State> {
    fn take<const N: usize>(bytes: &mut &[u8]) -> Option<[u8; N]> {
        let (head, rest) = bytes.split_first_chunk::<N>()?;
        *bytes = rest;
        Some(*head)
    }
    fn oid(bytes: &mut &[u8], present: bool) -> Option<Option<Oid>> {
        let id = Oid(take::<20>(bytes)?);
        Some(present.then_some(id))
    }
    let (kind, mut bytes) = header(bytes)?;
    if kind != b"state" {
        return None;
    }
    let order = order_from_code(u32::from_le_bytes(take(&mut bytes)?))?;
    let has_head = take::<1>(&mut bytes)?[0] != 0;
    let head = oid(&mut bytes, has_head)?;
    let has_last = take::<1>(&mut bytes)?[0] != 0;
    let last = oid(&mut bytes, has_last)?;
    let rows = u64::from_le_bytes(take(&mut bytes)?);
    let tip_count = u32::from_le_bytes(take(&mut bytes)?) as usize;
    let shallow_count = u32::from_le_bytes(take(&mut bytes)?) as usize;
    if bytes.len() != (tip_count + shallow_count) * 20 || has_last != (rows > 0) {
        return None;
    }
    let mut ids = bytes
        .as_chunks::<20>()
        .0
        .iter()
        .map(|&c| Oid(c))
        .collect::<Vec<_>>();
    let shallow = ids.split_off(tip_count);
    Some(State {
        tips: ids,
        shallow,
        head,
        order,
        rows,
        last,
    })
}

/// 一時ファイルに書いてから置き換える（書きかけを読ませない）。書いたバイト数を返す。
fn replace(path: &Path, bytes: &[u8]) -> Result<u64> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let result = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    Ok(bytes.len() as u64)
}

/// `order` の `state` だけを読む（表は読まない。daemon が書いたものが今の History と合うかを確かめるのに使う）。
pub fn read_state(path: &Path, order: HistoryOrder) -> Option<State> {
    let files = Files::new(path);
    let _lock = files.lock(false)?;
    files.read_state(order)
}

/// `order` の History を読む。読めなければ（ない・壊れている・形式が古い）`None`。
pub fn read(path: &Path, order: HistoryOrder) -> Option<CachedHistory> {
    let files = Files::new(path);
    let _lock = files.lock(false)?;
    let state = files.read_state(order)?;
    let table = files.map_table(files.read_table()?)?;
    let columns = table.columns()?;
    let mapped = Mapped::open(&files.order(order))?;
    let rows: &[u32] = mapped.array(usize::try_from(state.rows).ok()?)?;
    let last = match rows.last() {
        Some(&s) => Some(*columns.ids.get(s as usize)?),
        None => None,
    };
    if last != state.last {
        return None;
    }
    let graph = newest_first(&columns, rows)?;
    Some(CachedHistory {
        tips: state.tips,
        shallow: state.shallow,
        head: state.head,
        order: state.order,
        graph,
    })
}

/// 並び順の表（古い方から並べた、コミットの表の行番号）の順に、新しい方から並べた表を作る。行番号が表の外、
/// 同じ行が 2 度、表にない親、親が自分より新しい、のどれかがあれば `None`。
fn newest_first(c: &Columns<'_>, rows: &[u32]) -> Option<CommitGraph> {
    let m = rows.len();
    // 列ごとに別のスレッドでコピーする（linux の 150 万行で、1 つのスレッドなら 10 ms）。
    let (ids, times, links) = std::thread::scope(|s| {
        let ids = s.spawn(|| {
            rows.iter()
                .rev()
                .map(|&r| c.ids.get(r as usize).copied())
                .collect::<Option<Vec<_>>>()
        });
        let times = s.spawn(|| {
            rows.iter()
                .rev()
                .map(|&r| c.times.get(r as usize).copied())
                .collect::<Option<Vec<_>>>()
        });
        let links = (|| {
            // コミットの表の行から、新しい方から数えた行へ。
            let mut row_of = vec![u32::MAX; c.ids.len()];
            for (k, &r) in rows.iter().enumerate() {
                let slot = row_of.get_mut(r as usize)?;
                if *slot != u32::MAX {
                    return None;
                }
                *slot = (m - 1 - k) as u32;
            }
            let mut offsets = Vec::with_capacity(m + 1);
            let mut parents = Vec::with_capacity(m + m / 4);
            offsets.push(0u32);
            for (i, &r) in rows.iter().rev().enumerate() {
                for &p in c.parents_of(r as usize)? {
                    let q = *row_of.get(p as usize)?;
                    if q == u32::MAX || q as usize <= i {
                        return None;
                    }
                    parents.push(q);
                }
                offsets.push(parents.len() as u32);
            }
            Some((offsets, parents))
        })();
        (ids.join().unwrap(), times.join().unwrap(), links)
    });
    let (offsets, parents) = links?;
    CommitGraph::from_parts(ids?.into(), times?.into(), offsets.into(), parents.into())
}

/// 書いたもの。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Written {
    /// 書いたバイト数（表に足した分と `table`・`state`）。
    pub bytes: u64,
    /// 並び順の表に足した行の数。
    pub rows: u64,
    /// 並び順の表から切った行の数。
    pub cut: u64,
}

/// キャッシュの `h.order` の History を `h` にする。並び順の表は前回と同じ所（古い方から一致する行）は書かず、
/// 違う所から後ろを切って足す。コミットの表には、まだないコミットだけ足す。
pub fn write(path: &Path, h: Entry<'_>) -> Result<Written> {
    std::fs::create_dir_all(path)?;
    let files = Files::new(path);
    let _lock = files
        .lock(true)
        .ok_or_else(|| std::io::Error::other("can’t lock the History cache"))?;
    let mut written = Written::default();
    // 合わない所があったり、使わない行が増えたりしたら、空にしてもう一度書く。
    if !write_locked(&files, h, &mut written)? {
        files.clear()?;
        write_locked(&files, h, &mut written)?;
    }
    prune_other_formats(path);
    Ok(written)
}

/// 書けたら `true`。キャッシュを作り直すべきなら（コミットの表が壊れている、表のコミットの親が違う、使わない行が
/// 多い）`false`。
fn write_locked(files: &Files, h: Entry<'_>, written: &mut Written) -> Result<bool> {
    let g = h.graph;
    let n = g.len();
    let table = match files.read_table() {
        Some(table) => table,
        None => {
            // 初めて書く（か、作り直す途中で落ちた）。
            files.clear()?;
            Table::default()
        }
    };
    let Some(mapped) = files.map_table(table) else {
        return Ok(false);
    };
    let Some(c) = mapped.columns() else {
        return Ok(false);
    };
    let state = files.read_state(h.order);
    let order = Mapped::open(&files.order(h.order)).unwrap_or(Mapped(None));
    let old: &[u32] = state
        .as_ref()
        .and_then(|s| order.array(usize::try_from(s.rows).ok()?))
        .unwrap_or_default();
    // 並び順の表の、古い方から一致する行の数。
    let common = old
        .iter()
        .zip(g.ids().iter().rev())
        .take_while(|&(&s, id)| c.ids.get(s as usize) == Some(id))
        .count();
    if common < old.len() {
        // 切る前に、残す行の数の `state` を書く（切ってから落ちても、`state` が表より長くならない）。
        written.bytes += replace(
            &files.state(h.order),
            &encode_state(&State {
                rows: common as u64,
                last: common.checked_sub(1).map(|k| g.ids()[n - 1 - k]),
                ..state.clone().unwrap()
            }),
        )?;
        written.cut = (old.len() - common) as u64;
    }
    // 古い方から数えた行ごとの、コミットの表の行。
    let mut rows = old[..common].to_vec();
    // 表にあるのに、並び順の表の残す所から指されていない行（切った行、ほかの並び順だけの行、使わない行）。
    let mut known = HashMap::new();
    if common < n {
        let mut used = vec![false; c.ids.len()];
        for &s in &rows {
            used[s as usize] = true;
        }
        known.extend(
            used.iter()
                .enumerate()
                .filter(|&(_, &u)| !u)
                .map(|(s, _)| (c.ids[s], s as u32)),
        );
    }
    let (mut ids, mut times, mut ends, mut parents) = (vec![], vec![], vec![], vec![]);
    let mut next = table.rows as u32;
    let mut end = table.parents as u32;
    for k in common..n {
        let r = n - 1 - k;
        let ours = g.parents(r).iter().map(|&p| rows[n - 1 - p as usize]);
        if let Some(&s) = known.get(&g.ids()[r]) {
            // 表にあるコミットは、親も同じでなければならない（shallow の境界が変わったなど。作り直す）。
            if !c
                .parents_of(s as usize)
                .is_some_and(|stored| stored.iter().copied().eq(ours))
            {
                return Ok(false);
            }
            rows.push(s);
            continue;
        }
        parents.extend(ours);
        ids.push(g.ids()[r]);
        times.push(g.commit_times()[r]);
        end += g.parents(r).len() as u32;
        ends.push(end);
        rows.push(next);
        next += 1;
    }
    drop(mapped);
    // コミットの表に足す（`table` より後ろに書きかけが残っていれば切る）。
    for (file, keep, bytes) in [
        (&files.ids, table.rows * 20, bytes_of(&ids)),
        (&files.times, table.rows * 8, bytes_of(&times)),
        (&files.ends, table.rows * 4, bytes_of(&ends)),
        (&files.parents, table.parents * 4, bytes_of(&parents)),
    ] {
        append(file, keep, bytes)?;
        written.bytes += bytes.len() as u64;
    }
    if !ids.is_empty() {
        written.bytes += replace(
            &files.table,
            &encode_table(Table {
                rows: u64::from(next),
                parents: u64::from(end),
            }),
        )?;
    }
    append(
        &files.order(h.order),
        common as u64 * 4,
        bytes_of(&rows[common..]),
    )?;
    written.bytes += (n - common) as u64 * 4;
    written.rows = (n - common) as u64;
    written.bytes += replace(
        &files.state(h.order),
        &encode_state(&State {
            tips: h.tips.to_vec(),
            shallow: h.shallow.to_vec(),
            head: h.head,
            order: h.order,
            rows: n as u64,
            last: g.ids().first().copied(),
        }),
    )?;
    Ok(!too_much_garbage(files, h.order)?)
}

/// `file` を `keep` バイトにしてから `bytes` を足す。
fn append(file: &Path, keep: u64, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(file)?;
    if f.metadata()?.len() != keep {
        f.set_len(keep)?;
    }
    f.seek(SeekFrom::Start(keep))?;
    f.write_all(bytes)?;
    Ok(())
}

/// どの並び順の表からも指されない行が、コミットの表の `GARBAGE_DIVISOR` 分の 1 を超えたか。
fn too_much_garbage(files: &Files, active: HistoryOrder) -> Result<bool> {
    let Some(table) = files.read_table() else {
        return Ok(false);
    };
    let total = table.rows as usize;
    let Some(active_rows) = files.read_state(active).map(|s| s.rows as usize) else {
        return Ok(false);
    };
    // 今の並び順だけで十分に使っていれば、ほかの表を読むまでもない。
    if total - total.min(active_rows) <= total / GARBAGE_DIVISOR {
        return Ok(false);
    }
    let mut used = vec![false; total];
    for order in ORDERS {
        let Some(state) = files.read_state(order) else {
            continue;
        };
        let Some(mapped) = Mapped::open(&files.order(order)) else {
            continue;
        };
        for &s in mapped.array::<u32>(state.rows as usize).unwrap_or_default() {
            if let Some(u) = used.get_mut(s as usize) {
                *u = true;
            }
        }
    }
    let unused = used.iter().filter(|&&u| !u).count();
    Ok(unused > total / GARBAGE_DIVISOR)
}

/// キャッシュのファイルの大きさの合計（バイト）。
pub fn size(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// `dir`（`path_for` に渡すフォルダ）の、History のキャッシュすべて（どのリポジトリ・形式のものも）。
fn all_in(dir: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("history-") || n == "history.bin")
        })
        .map(|e| e.path())
}

/// `dir` の History のキャッシュの大きさの合計（Settings > Performance）。
pub fn total_size(dir: &Path) -> u64 {
    all_in(dir)
        .map(|p| {
            if p.is_dir() {
                size(&p)
            } else {
                p.metadata().map_or(0, |m| m.len())
            }
        })
        .sum()
}

/// `dir` の History のキャッシュをすべて消す（Settings > Performance）。開いているリポジトリは、次に History が
/// 変わったときに書き直す。
pub fn remove_all(dir: &Path) -> std::io::Result<()> {
    for p in all_in(dir) {
        if p.is_dir() {
            remove(&p)?;
        } else {
            remove_file(&p)?;
        }
    }
    Ok(())
}

/// キャッシュを消す（History を作り直すとき、Settings で消すとき）。
pub fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// 書き終えたときに呼ぶもの。
pub type OnWrite = Box<dyn Fn(&Written) + Send + Sync>;

/// 書き込みをまとめる。`schedule` で渡した History を、`WRITE_DELAY` の間ほかに渡されなければ裏のスレッドで書く。
/// `flush` と、手放したときは待たずに書く。
pub struct Writer {
    inner: Arc<WriterInner>,
}

struct WriterInner {
    path: PathBuf,
    delay: Duration,
    pending: Mutex<Pending>,
    wake: Condvar,
    /// 書き終えたときに呼ぶ。
    on_write: Option<OnWrite>,
    /// 起動してから書いたもの（Activity に出す）。
    stats: Mutex<WriteStats>,
}

/// `Writer` が起動してから書いたもの。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteStats {
    /// 書いた回数。
    pub count: u64,
    /// 書いたバイト数の合計。
    pub bytes: u64,
    /// 足した行・切った行の合計。
    pub rows: u64,
    pub cut: u64,
    /// 最後に書いたバイト数と、その時刻（UNIX 時刻のミリ秒）。
    pub last_bytes: u64,
    pub last_at: Option<u64>,
}

#[derive(Default)]
struct Pending {
    /// 次に書く History と、渡された時刻。
    next: Option<(Arc<dyn AsEntry>, Instant)>,
    /// 裏のスレッドが動いている。
    running: bool,
    /// 書いている最中（`flush` はこれが終わるのを待つ）。
    writing: bool,
    /// 捨てた（History を作り直す）。書きかけのものは書かない。
    epoch: u64,
}

/// `Writer` に渡せる History（`HistorySnapshot` など。書くまで持っておく）。
pub trait AsEntry: Send + Sync {
    fn entry(&self) -> Entry<'_>;
}

impl AsEntry for CachedHistory {
    fn entry(&self) -> Entry<'_> {
        CachedHistory::entry(self)
    }
}

impl Writer {
    pub fn new(path: PathBuf) -> Self {
        Self::with_delay(path, WRITE_DELAY, None)
    }

    pub fn with_delay(path: PathBuf, delay: Duration, on_write: Option<OnWrite>) -> Self {
        let inner = Arc::new(WriterInner {
            path,
            delay,
            pending: Mutex::default(),
            wake: Condvar::new(),
            on_write,
            stats: Mutex::default(),
        });
        let mut writers = WRITERS.lock().unwrap();
        writers.retain(|w| w.strong_count() > 0);
        writers.push(Arc::downgrade(&inner));
        drop(writers);
        static AT_EXIT: std::sync::Once = std::sync::Once::new();
        AT_EXIT.call_once(|| {
            unsafe extern "C" {
                fn atexit(f: extern "C" fn()) -> std::ffi::c_int;
            }
            extern "C" fn flush_at_exit() {
                flush_all();
            }
            // SAFETY: 引数のない関数を渡すだけ。
            unsafe { atexit(flush_at_exit) };
        });
        Self { inner }
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// 起動してから書いたもの。
    pub fn stats(&self) -> WriteStats {
        *self.inner.stats.lock().unwrap()
    }

    /// `history` を少し後に書く。
    pub fn schedule(&self, history: Arc<dyn AsEntry>) {
        let mut pending = self.inner.pending.lock().unwrap();
        pending.next = Some((history, Instant::now()));
        if !pending.running {
            pending.running = true;
            let inner = self.inner.clone();
            std::thread::Builder::new()
                .name("amonhen-history-cache".into())
                .spawn(move || inner.run())
                .expect("can’t start the History cache writer");
        } else {
            self.inner.wake.notify_all();
        }
    }

    /// 待っている書き込みがあれば、すぐ書く。書いている最中なら終わるまで待つ。
    pub fn flush(&self) -> Result<()> {
        self.inner.flush()
    }

    /// 待っている書き込みを捨て、キャッシュを消す。
    pub fn discard(&self) -> std::io::Result<()> {
        let mut pending = self.inner.pending.lock().unwrap();
        while pending.writing {
            pending = self.inner.wake.wait(pending).unwrap();
        }
        pending.next = None;
        pending.epoch += 1;
        remove(&self.inner.path)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// 動いている `Writer`（プロセスが終わるときに、待っている書き込みを書く）。
static WRITERS: Mutex<Vec<std::sync::Weak<WriterInner>>> = Mutex::new(Vec::new());

/// どの `Writer` も、待っている書き込みをすぐ書く。プロセスが `exit` で終わるとき（アプリを終える、`--snapshot` など）に
/// 呼ぶ（`Writer` は手放されないので、まとめて待っていた書き込みが失われる）。最初の `Writer` を作ったときに `atexit` に
/// 登録する。
pub fn flush_all() {
    let writers: Vec<_> = WRITERS
        .lock()
        .unwrap()
        .iter()
        .filter_map(std::sync::Weak::upgrade)
        .collect();
    for w in writers {
        let _ = w.flush();
    }
}

impl WriterInner {
    fn flush(&self) -> Result<()> {
        let mut pending = self.pending.lock().unwrap();
        while pending.writing {
            pending = self.wake.wait(pending).unwrap();
        }
        let Some((history, _)) = pending.next.take() else {
            return Ok(());
        };
        pending.writing = true;
        drop(pending);
        let result = self.write(history.as_ref());
        self.pending.lock().unwrap().writing = false;
        self.wake.notify_all();
        result
    }

    fn write(&self, history: &dyn AsEntry) -> Result<()> {
        let written = write(&self.path, history.entry())?;
        {
            let mut s = self.stats.lock().unwrap();
            s.count += 1;
            s.bytes += written.bytes;
            s.rows += written.rows;
            s.cut += written.cut;
            s.last_bytes = written.bytes;
            s.last_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis() as u64);
        }
        if let Some(f) = &self.on_write {
            f(&written);
        }
        Ok(())
    }

    fn run(self: Arc<Self>) {
        let mut pending = self.pending.lock().unwrap();
        loop {
            let Some((_, at)) = &pending.next else {
                pending.running = false;
                return;
            };
            let due = *at + self.delay;
            let now = Instant::now();
            if pending.writing {
                pending = self.wake.wait(pending).unwrap();
                continue;
            }
            if now < due {
                pending = self.wake.wait_timeout(pending, due - now).unwrap().0;
                continue;
            }
            let (history, _) = pending.next.take().unwrap();
            pending.writing = true;
            let epoch = pending.epoch;
            drop(pending);
            // 書けなくても History は使える。次の起動が遅くなるだけ。
            let _ = self.write(history.as_ref());
            pending = self.pending.lock().unwrap();
            pending.writing = false;
            if pending.epoch != epoch {
                // 書いている間に捨てられた。
                let _ = remove(&self.path);
            }
            self.wake.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::synthetic;

    fn history(graph: CommitGraph) -> CachedHistory {
        CachedHistory {
            tips: graph.ids().first().copied().into_iter().collect(),
            shallow: vec![],
            head: graph.ids().first().copied(),
            order: HistoryOrder::Date,
            graph,
        }
    }

    /// `g` の先頭のコミットの上に、新しいコミットを `n` 個積んだ表。
    fn on_top(g: &CommitGraph, n: u8) -> CommitGraph {
        on_top_tagged(g, n, 0)
    }

    fn new_id(tag: u8, i: u8) -> Oid {
        let mut id = [0; 20];
        (id[0], id[1], id[2]) = (0xee, tag, i);
        Oid(id)
    }

    /// `on_top` の、`tag` ごとに別のコミットのもの。
    fn on_top_tagged(g: &CommitGraph, n: u8, tag: u8) -> CommitGraph {
        let mut b = amonhen_model::CommitGraphBuilder::default();
        for i in 0..n {
            let parent = if i + 1 < n {
                new_id(tag, i + 1)
            } else {
                g.ids()[0]
            };
            b.push(new_id(tag, i), 0, [parent]);
        }
        for r in 0..g.len() {
            b.push(
                g.ids()[r],
                g.commit_times()[r],
                g.parents(r).iter().map(|&p| g.ids()[p as usize]),
            );
        }
        b.build()
    }

    /// `g` の行 `at` から出る `n` 個のコミットの枝を、行 `at` のすぐ上に並べた表。
    fn with_branch_at(g: &CommitGraph, at: usize, n: u8) -> CommitGraph {
        let mut b = amonhen_model::CommitGraphBuilder::default();
        let old = |b: &mut amonhen_model::CommitGraphBuilder, r: usize| {
            b.push(
                g.ids()[r],
                g.commit_times()[r],
                g.parents(r).iter().map(|&p| g.ids()[p as usize]),
            );
        };
        for r in 0..at {
            old(&mut b, r);
        }
        for i in 0..n {
            let parent = if i + 1 < n {
                new_id(0xbb, i + 1)
            } else {
                g.ids()[at]
            };
            b.push(new_id(0xbb, i), g.commit_times()[at], [parent]);
        }
        for r in at..g.len() {
            old(&mut b, r);
        }
        b.build()
    }

    /// 親のない行だけ（親のファイルが空）でも読める。
    #[test]
    fn round_trips_a_history_without_parents() {
        let mut b = amonhen_model::CommitGraphBuilder::default();
        b.push(Oid([1; 20]), 0, []);
        let h = history(b.build());
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/r");
        write(&path, h.entry()).unwrap();
        assert_eq!(read(&path, HistoryOrder::Date).unwrap(), h);
    }

    #[test]
    fn round_trips() {
        let (graph, _) = synthetic::graph(10_000, 9);
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/some/repo");
        let h = CachedHistory {
            tips: vec![graph.ids()[0], graph.ids()[1]],
            shallow: vec![graph.ids()[9_999]],
            head: Some(graph.ids()[3]),
            order: HistoryOrder::Topo,
            graph,
        };
        let w = write(&path, h.entry()).unwrap();
        assert_eq!((w.rows, w.cut), (10_000, 0));
        assert_eq!(read(&path, HistoryOrder::Topo).unwrap(), h);
        let state = read_state(&path, HistoryOrder::Topo).unwrap();
        assert_eq!((state.tips, state.head), (h.tips.clone(), h.head));
    }

    /// 新しいコミットは足すだけ、消えたコミットは切ってから足す。同じものを書いても `state` だけ。
    #[test]
    fn writes_only_what_changed() {
        let (old, _) = synthetic::graph(20_000, 5);
        let (grown, _) = synthetic::extended(20_000, 30, 5);
        assert_eq!(&grown.ids()[30..], old.ids());
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/r");
        write(&path, history(old.clone()).entry()).unwrap();

        let w = write(&path, history(old.clone()).entry()).unwrap();
        assert_eq!((w.rows, w.cut), (0, 0));
        assert!(w.bytes < 200, "{w:?}");

        let w = write(&path, history(grown.clone()).entry()).unwrap();
        assert_eq!((w.rows, w.cut), (30, 0));
        assert!(w.bytes < 30 * 40 + 200, "{w:?}");
        assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, grown);

        // 上の 30 行が消え、別の 5 行になった（reset して別のコミット）。
        let other = on_top(&old, 5);
        assert_eq!(&other.ids()[5..], old.ids());
        let w = write(&path, history(other.clone()).entry()).unwrap();
        assert_eq!((w.rows, w.cut), (5, 30));
        assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, other);

        // ずっと下の枝（古い方から 5,000 行目のあたり）が消えた（ブランチを消した）。並び順の表はそこから上を
        // 書き直すが、コミットはどれも表にあるので、コミットの表には足さない。
        let branched = with_branch_at(&old, 15_000, 3);
        write(&path, history(branched.clone()).entry()).unwrap();
        assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, branched);
        let w = write(&path, history(old.clone()).entry()).unwrap();
        assert_eq!((w.cut, w.rows), (20_003 - 5_000, 20_000 - 5_000));
        assert!(w.bytes < 15_000 * 4 + 200, "{w:?}");
        assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, old);
    }

    /// 並び順を替えても、コミットの表は書かない。どちらの並び順も読める。
    #[test]
    fn switching_the_order_writes_only_the_order_table() {
        let (date, _) = synthetic::graph(20_000, 6);
        let topo = amonhen_model::topo::reorder(&date, HistoryOrder::Topo);
        assert_ne!(topo.ids(), date.ids());
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/r");
        write(&path, history(date.clone()).entry()).unwrap();
        let topo_history = CachedHistory {
            order: HistoryOrder::Topo,
            ..history(topo.clone())
        };
        let w = write(&path, topo_history.entry()).unwrap();
        assert_eq!(w.rows, 20_000);
        assert!(w.bytes < 20_000 * 4 + 200, "{w:?}");
        assert_eq!(read(&path, HistoryOrder::Topo).unwrap(), topo_history);
        assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, date);
        // 戻しても同じ。
        let w = write(&path, history(date.clone()).entry()).unwrap();
        assert_eq!((w.rows, w.cut), (0, 0));
    }

    /// どの並び順からも指されない行が増えたら、今の並び順で作り直す。親が前と違うコミットがあっても作り直す。
    #[test]
    fn rebuilds_when_unused_rows_pile_up_or_parents_differ() {
        let (old, _) = synthetic::graph(1000, 7);
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/r");
        write(&path, history(old.clone()).entry()).unwrap();
        // amend を繰り返す（上の 5 行が毎回別のコミットになる）。
        let mut rebuilt = false;
        for i in 0..60u8 {
            let g = on_top_tagged(&old, 5, i);
            let w = write(&path, history(g.clone()).entry()).unwrap();
            rebuilt |= w.rows == 1005;
            assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, g);
        }
        assert!(rebuilt);
        assert!(size(&path) < 1005 * 40 * 2, "{}", size(&path));

        // 親が違う（shallow の境界が変わったなど）。
        let cut = old.retain(&(0..old.len()).map(|r| r != 500).collect::<Vec<_>>());
        let w = write(&path, history(cut.clone()).entry()).unwrap();
        assert_eq!(w.rows, 999);
        assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, cut);
    }

    /// `state` より長い表（足してから `state` を書くまでに落ちた）は、`state` の所まで読み、次に書くとき切る。
    /// 短い表や、最後の行が合わない表は使わない。
    #[test]
    fn reads_up_to_the_state_and_rejects_short_or_mismatched_tables() {
        let (graph, _) = synthetic::graph(1000, 1);
        let (grown, _) = synthetic::extended(1000, 10, 1);
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/r");
        write(&path, history(grown.clone()).entry()).unwrap();
        let files = Files::new(&path);
        let state = std::fs::read(files.state(HistoryOrder::Date)).unwrap();
        write(&path, history(graph.clone()).entry()).unwrap();
        let short_state = std::fs::read(files.state(HistoryOrder::Date)).unwrap();

        // 長い表と短い state。
        write(&path, history(grown.clone()).entry()).unwrap();
        std::fs::write(files.state(HistoryOrder::Date), &short_state).unwrap();
        assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, graph);
        let w = write(&path, history(grown.clone()).entry()).unwrap();
        assert_eq!((w.rows, w.cut), (10, 0));
        assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, grown);

        // 短い表と長い state。
        let ids = std::fs::read(&files.ids).unwrap();
        std::fs::write(&files.ids, &ids[..ids.len() - 20]).unwrap();
        assert!(read(&path, HistoryOrder::Date).is_none());
        std::fs::write(&files.ids, &ids).unwrap();

        // 最後の行が合わない。
        let mut wrong = ids.clone();
        let len = wrong.len();
        wrong[len - 1] ^= 0xff;
        std::fs::write(&files.ids, &wrong).unwrap();
        assert!(read(&path, HistoryOrder::Date).is_none());

        // 形式が違う。
        let mut old = state.clone();
        old[8] ^= 0xff;
        std::fs::write(files.state(HistoryOrder::Date), &old).unwrap();
        assert!(read(&path, HistoryOrder::Date).is_none());
    }

    /// 同じキャッシュを同時に書いても、読み書きしても壊れない（共有の daemon とアプリ、1 つのプロセスの中の
    /// いくつものスレッド）。
    #[test]
    fn writes_and_reads_the_same_cache_from_several_threads_at_once() {
        let (small, _) = synthetic::graph(20_000, 4);
        let (big, _) = synthetic::extended(20_000, 500, 4);
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/r");
        std::thread::scope(|s| {
            for i in 0..8 {
                let (small, big, path) = (&small, &big, &path);
                s.spawn(move || {
                    for j in 0..5 {
                        let g = if (i + j) % 2 == 0 { small } else { big };
                        write(path, history(g.clone()).entry()).unwrap();
                        let read = read(path, HistoryOrder::Date).unwrap().graph;
                        assert!(read == *small || read == *big);
                    }
                });
            }
        });
    }

    /// 続けて渡した History は、最後のものを 1 回だけ書く。手放すときは待たずに書く。
    #[test]
    fn the_writer_gathers_changes_and_writes_on_drop() {
        let (old, _) = synthetic::graph(1000, 2);
        let (grown, _) = synthetic::extended(1000, 3, 2);
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/r");
        let writes = Arc::new(Mutex::new(Vec::new()));
        let log = writes.clone();
        let writer = Writer::with_delay(
            path.clone(),
            Duration::from_millis(200),
            Some(Box::new(move |w: &Written| log.lock().unwrap().push(*w))),
        );
        writer.schedule(Arc::new(history(old.clone())));
        writer.schedule(Arc::new(history(grown.clone())));
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(writes.lock().unwrap().len(), 1);
        assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, grown);

        let stats = writer.stats();
        assert_eq!((stats.count, stats.rows, stats.cut), (1, 1003, 0));
        assert_eq!(stats.bytes, stats.last_bytes);
        assert!(stats.last_at.is_some());
        assert_eq!(
            size(&path),
            std::fs::read_dir(&path)
                .unwrap()
                .flatten()
                .map(|e| e.metadata().unwrap().len())
                .sum::<u64>()
        );

        writer.schedule(Arc::new(history(old.clone())));
        drop(writer);
        assert_eq!(writes.lock().unwrap().len(), 2);
        assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, old);
    }

    /// Settings の大きさと消去は、どのリポジトリの History のキャッシュも数えて消し、ほかのものは残す。
    #[test]
    fn totals_and_removes_every_history_cache() {
        let dir = tempfile::tempdir().unwrap();
        let (g, _) = synthetic::graph(100, 1);
        for key in ["/a", "/b"] {
            write(&path_for(dir.path(), key), history(g.clone()).entry()).unwrap();
        }
        std::fs::write(dir.path().join("history-0123-old.bin"), [0; 10]).unwrap();
        std::fs::create_dir(dir.path().join("assist")).unwrap();
        let each = size(&path_for(dir.path(), "/a"));
        assert_eq!(total_size(dir.path()), each * 2 + 10);
        remove_all(dir.path()).unwrap();
        assert_eq!(total_size(dir.path()), 0);
        assert!(dir.path().join("assist").exists());
    }

    /// `flush_all`（プロセスが終わるとき）は、手放されていない `Writer` の待っている書き込みを書く。
    #[test]
    fn flush_all_writes_what_is_waiting() {
        let (g, _) = synthetic::graph(100, 3);
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/r");
        let writer = Writer::with_delay(path.clone(), Duration::from_secs(3600), None);
        writer.schedule(Arc::new(history(g.clone())));
        assert!(read(&path, HistoryOrder::Date).is_none());
        flush_all();
        assert_eq!(read(&path, HistoryOrder::Date).unwrap().graph, g);
        std::mem::forget(writer);
    }

    /// 同じリポジトリのほかの形式（と前の形式のファイル）は、7 日書かれていなければ消す。最近のもの（並んで動く
    /// 別のビルドが使っている）・ほかのリポジトリのものは残す。
    #[test]
    fn prunes_unused_caches_of_other_formats() {
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/repo");
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        let repo = name.rsplit_once('-').unwrap().0.to_owned();
        let old_format = dir.path().join(format!("{repo}-00000000"));
        let old_file = dir.path().join(format!("{repo}-22222222.bin"));
        let recent_format = dir.path().join(format!("{repo}-11111111"));
        let legacy = dir.path().join(format!("{repo}.bin"));
        let other_repo = dir.path().join("history-0000000000000000-00000000");
        let week_ago = std::time::SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60);
        for (entry, is_dir, old) in [
            (&old_format, true, true),
            (&old_file, false, true),
            (&recent_format, true, false),
            (&legacy, false, true),
            (&other_repo, true, true),
        ] {
            let file = if is_dir {
                std::fs::create_dir(entry).unwrap();
                entry.join("state")
            } else {
                entry.clone()
            };
            std::fs::write(&file, b"").unwrap();
            if old {
                for f in [&file, entry] {
                    File::open(f).unwrap().set_modified(week_ago).unwrap();
                }
            }
        }
        write(&path, history(CommitGraph::default()).entry()).unwrap();
        assert!(path.exists() && recent_format.exists() && other_repo.exists());
        assert!(!old_format.exists() && !old_file.exists() && !legacy.exists());
    }
}
