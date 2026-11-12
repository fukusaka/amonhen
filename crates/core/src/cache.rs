//! History の永続キャッシュ。次の起動では、前回の History（コミットの表）をすぐ読む。レイアウトは持たない
//! （開いてから計算する。`docs/design.md` の「History persistent cache」）。
//!
//! 形式: リポジトリごとに 1 つのフォルダに、コミットの表を列ごとのファイルにして古い方から並べる（`ids`・`times`・
//! `ends`（行ごとの親の終わりの位置）・`parents`（親の行番号。古い方から数える））。古い行は動かないので、
//! 書くときは前回と違う所から後ろだけを切って足す。`state` に起点・HEAD・行の数などを書き、丸ごと置き換える。
//! `state` を書いたときに書き込みが済んだことになる（表が `state` より長ければ、余りは読まずに次に書くとき切る）。
//!
//! ほかのプロセス（共有の daemon とアプリ、CLI）と同時に読み書きするので、`lock` のファイルで書くときは
//! 1 つだけ、読むときは書いていない間だけにする。読むときは写したファイルから、並びを逆にしてメモリにコピー
//! する（写したままにしておくと、あとでほかのプロセスが切ったとき、そこに触れて落ちる）。

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use amonhen_layout::Pod;
use amonhen_model::{CommitGraph, HistoryOrder, Oid, Result};
use memmap2::Mmap;

const MAGIC: [u8; 8] = *b"AMONHIS2";
/// 形式の指紋（キャッシュに書くものを決めるソースのハッシュ。`build.rs`）。フォルダの名前と `state` に入れ、違う
/// 形式のものは読まない（作り直す）。並んで動く別の形式のビルドとは、別のフォルダを使う。
const FORMAT: &str = env!("AMONHEN_CACHE_FORMAT");

/// ほかの形式の同じリポジトリのキャッシュを、使われなくなってから消すまでの日数（並んで動く別のビルドが、まだ
/// 使っているかもしれない）。
const KEEP_OTHER_FORMATS_DAYS: u64 = 7;

/// 変化のあと、この間ほかの変化がなければ書く（続けて起きる変化を 1 回の書き込みにまとめる）。
pub const WRITE_DELAY: Duration = Duration::from_secs(2);

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

/// `state` の中身（表のほかの部分）。
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub tips: Vec<Oid>,
    pub shallow: Vec<Oid>,
    pub head: Option<Oid>,
    pub order: HistoryOrder,
    /// 表の行の数。
    pub rows: u64,
    /// 親の数（`parents` の長さ）。
    parents: u64,
    /// 最後の（最も新しい）行の ID。表がこれと合わなければ使わない。
    last: Option<Oid>,
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

/// `file` の先頭から `count` 個の `T` を読む。足りなければ `None`。
fn read_array<T: Pod>(file: &mut File, count: usize) -> Option<Vec<T>> {
    let bytes = count.checked_mul(size_of::<T>())?;
    let mut values: Vec<T> = Vec::with_capacity(count);
    // SAFETY: `T` は `Pod`（どのビット列も正しい値）。0 で埋めてから長さを決め、その領域に読み込む。
    unsafe {
        std::ptr::write_bytes(values.as_mut_ptr().cast::<u8>(), 0, bytes);
        values.set_len(count);
        let buf = std::slice::from_raw_parts_mut(values.as_mut_ptr().cast::<u8>(), bytes);
        file.read_exact(buf).ok()?;
    }
    Some(values)
}

/// 読むために写したファイル。写すのは読むときの共有のロックの間だけ（書く側は排他のロックを取ってから切るので、
/// 写している間に短くならない）。
struct Mapped(Option<Mmap>);

impl Mapped {
    fn open(path: &Path) -> Option<Self> {
        let file = File::open(path).ok()?;
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
    ids: PathBuf,
    times: PathBuf,
    ends: PathBuf,
    parents: PathBuf,
    state: PathBuf,
    lock: PathBuf,
}

impl Files {
    fn new(dir: &Path) -> Self {
        Self {
            ids: dir.join("ids"),
            times: dir.join("times"),
            ends: dir.join("ends"),
            parents: dir.join("parents"),
            state: dir.join("state"),
            lock: dir.join("lock"),
        }
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
}

fn encode_state(s: &State) -> Vec<u8> {
    let mut out = Vec::with_capacity(96 + 20 * (s.tips.len() + s.shallow.len()));
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&version().to_le_bytes());
    out.extend_from_slice(&order_code(s.order).to_le_bytes());
    out.push(u8::from(s.head.is_some()));
    out.extend_from_slice(&s.head.unwrap_or_default().0);
    out.push(u8::from(s.last.is_some()));
    out.extend_from_slice(&s.last.unwrap_or_default().0);
    out.extend_from_slice(&s.rows.to_le_bytes());
    out.extend_from_slice(&s.parents.to_le_bytes());
    out.extend_from_slice(&(s.tips.len() as u32).to_le_bytes());
    out.extend_from_slice(&(s.shallow.len() as u32).to_le_bytes());
    for id in s.tips.iter().chain(&s.shallow) {
        out.extend_from_slice(&id.0);
    }
    out
}

fn decode_state(mut bytes: &[u8]) -> Option<State> {
    fn take<const N: usize>(bytes: &mut &[u8]) -> Option<[u8; N]> {
        let (head, rest) = bytes.split_first_chunk::<N>()?;
        *bytes = rest;
        Some(*head)
    }
    fn oid(bytes: &mut &[u8], present: bool) -> Option<Option<Oid>> {
        let id = Oid(take::<20>(bytes)?);
        Some(present.then_some(id))
    }
    if take::<8>(&mut bytes)? != MAGIC || u32::from_le_bytes(take(&mut bytes)?) != version() {
        return None;
    }
    let order = order_from_code(u32::from_le_bytes(take(&mut bytes)?))?;
    let has_head = take::<1>(&mut bytes)?[0] != 0;
    let head = oid(&mut bytes, has_head)?;
    let has_last = take::<1>(&mut bytes)?[0] != 0;
    let last = oid(&mut bytes, has_last)?;
    let rows = u64::from_le_bytes(take(&mut bytes)?);
    let parents = u64::from_le_bytes(take(&mut bytes)?);
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
        parents,
        last,
    })
}

/// `state` を一時ファイルに書いてから置き換える（書きかけを読ませない）。書いたバイト数を返す。
fn write_state(files: &Files, state: &State) -> Result<u64> {
    let bytes = encode_state(state);
    let tmp = files
        .state
        .with_extension(format!("tmp-{}", std::process::id()));
    let result = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &files.state));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    Ok(bytes.len() as u64)
}

/// `state` だけを読む（表は読まない。daemon が書いたものが今の History と合うかを確かめるのに使う）。
pub fn read_state(path: &Path) -> Option<State> {
    let files = Files::new(path);
    let _lock = files.lock(false)?;
    decode_state(&std::fs::read(&files.state).ok()?)
}

/// 読めなければ（ない・壊れている・形式が古い）`None`。
pub fn read(path: &Path) -> Option<CachedHistory> {
    let files = Files::new(path);
    let _lock = files.lock(false)?;
    let state = decode_state(&std::fs::read(&files.state).ok()?)?;
    let n = usize::try_from(state.rows).ok()?;
    let (ids, times, ends, parents) = (
        Mapped::open(&files.ids)?,
        Mapped::open(&files.times)?,
        Mapped::open(&files.ends)?,
        Mapped::open(&files.parents)?,
    );
    let ids: &[Oid] = ids.array(n)?;
    if ids.last().copied() != state.last {
        return None;
    }
    let graph = newest_first(
        ids,
        times.array(n)?,
        ends.array(n)?,
        parents.array(usize::try_from(state.parents).ok()?)?,
    )?;
    Some(CachedHistory {
        tips: state.tips,
        shallow: state.shallow,
        head: state.head,
        order: state.order,
        graph,
    })
}

/// 古い方から並べた表を、新しい方から並べた表にする。親が自分より古い行を指していなければ `None`。
fn newest_first(ids: &[Oid], times: &[i64], ends: &[u32], parents: &[u32]) -> Option<CommitGraph> {
    let n = ids.len();
    if ends.last().map_or(0, |&e| e as usize) != parents.len() {
        return None;
    }
    // 列ごとに別のスレッドでコピーする（linux の 150 万行で、1 つのスレッドなら 10 ms）。
    let (ids, times, links) = std::thread::scope(|s| {
        let ids = s.spawn(|| ids.iter().rev().copied().collect::<Vec<_>>());
        let times = s.spawn(|| times.iter().rev().copied().collect::<Vec<_>>());
        let links = (|| {
            let mut offsets = Vec::with_capacity(n + 1);
            let mut rows = Vec::with_capacity(parents.len());
            offsets.push(0u32);
            for s in (0..n).rev() {
                let start = if s == 0 { 0 } else { ends[s - 1] } as usize;
                for &p in parents.get(start..ends[s] as usize)? {
                    if p as usize >= s {
                        return None;
                    }
                    rows.push((n - 1 - p as usize) as u32);
                }
                offsets.push(rows.len() as u32);
            }
            Some((offsets, rows))
        })();
        (ids.join().unwrap(), times.join().unwrap(), links)
    });
    let (offsets, rows) = links?;
    CommitGraph::from_parts(ids.into(), times.into(), offsets.into(), rows.into())
}

/// 書いたもの。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Written {
    /// 書いたバイト数（表に足した分と `state`）。
    pub bytes: u64,
    /// 足した行の数。
    pub rows: u64,
    /// 切った行の数。
    pub cut: u64,
}

/// キャッシュを `h` にする。前回と同じ所（古い方から一致する行）は書かず、違う所から後ろを切って足す。
pub fn write(path: &Path, h: Entry<'_>) -> Result<Written> {
    std::fs::create_dir_all(path)?;
    let files = Files::new(path);
    let _lock = files
        .lock(true)
        .ok_or_else(|| std::io::Error::other("can’t lock the History cache"))?;
    let result = write_locked(&files, h);
    if result.is_ok() {
        prune_other_formats(path);
    }
    result
}

fn write_locked(files: &Files, h: Entry<'_>) -> Result<Written> {
    let g = h.graph;
    let n = g.len();
    let disk = std::fs::read(&files.state)
        .ok()
        .and_then(|b| decode_state(&b));
    let on_disk = disk.as_ref().map_or(0, |s| s.rows as usize);
    // 古い方から一致する行の数。
    let common = common_rows(files, disk.as_ref(), g).unwrap_or(0);
    let mut written = Written::default();
    // 切る前に、残す行の数の `state` を書く（切ってから落ちても、`state` が表より長くならない）。
    let kept_parents = if common == 0 {
        0
    } else {
        let mut ends = File::open(&files.ends)?;
        ends.seek(SeekFrom::Start((common as u64 - 1) * 4))?;
        u64::from(
            read_array::<u32>(&mut ends, 1).ok_or_else(|| {
                std::io::Error::other("the History cache is shorter than its state")
            })?[0],
        )
    };
    if common < on_disk {
        written.bytes += write_state(
            files,
            &State {
                rows: common as u64,
                parents: kept_parents,
                last: common.checked_sub(1).map(|s| g.ids()[n - 1 - s]),
                ..disk.clone().unwrap()
            },
        )?;
        written.cut = (on_disk - common) as u64;
    }

    // 古い方から数えた行 `s` は、新しい方から数えると `n - 1 - s`。
    let added: Vec<usize> = (common..n).map(|s| n - 1 - s).collect();
    let ids: Vec<Oid> = added.iter().map(|&r| g.ids()[r]).collect();
    let times: Vec<i64> = added.iter().map(|&r| g.commit_times()[r]).collect();
    let mut parents = Vec::new();
    let mut ends = Vec::with_capacity(added.len());
    let mut end = kept_parents as u32;
    for &r in &added {
        for &p in g.parents(r) {
            parents.push((n - 1 - p as usize) as u32);
        }
        end += g.parents(r).len() as u32;
        ends.push(end);
    }
    for (file, keep, bytes) in [
        (&files.ids, common * 20, bytes_of(&ids)),
        (&files.times, common * 8, bytes_of(&times)),
        (&files.ends, common * 4, bytes_of(&ends)),
        (
            &files.parents,
            kept_parents as usize * 4,
            bytes_of(&parents),
        ),
    ] {
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(file)?;
        f.set_len(keep as u64)?;
        f.seek(SeekFrom::End(0))?;
        f.write_all(bytes)?;
        written.bytes += bytes.len() as u64;
    }
    written.rows = added.len() as u64;
    written.bytes += write_state(
        files,
        &State {
            tips: h.tips.to_vec(),
            shallow: h.shallow.to_vec(),
            head: h.head,
            order: h.order,
            rows: n as u64,
            parents: u64::from(end),
            last: g.ids().first().copied(),
        },
    )?;
    Ok(written)
}

/// 表の古い方から、`g` と一致する行の数。`ids` を少しずつ読んで比べる。
fn common_rows(files: &Files, disk: Option<&State>, g: &CommitGraph) -> Option<usize> {
    let disk = disk?;
    let on_disk = usize::try_from(disk.rows).ok()?;
    let limit = on_disk.min(g.len());
    let mut file = File::open(&files.ids).ok()?;
    // 1 回に読む行の数（1 MB 余り）。
    const CHUNK: usize = 1 << 16;
    let mut theirs = g.ids().iter().rev();
    let mut common = 0;
    while common < limit {
        let count = CHUNK.min(limit - common);
        let ours: Vec<Oid> = read_array(&mut file, count)?;
        let same = ours
            .iter()
            .zip(theirs.by_ref())
            .take_while(|(a, b)| a == b)
            .count();
        common += same;
        if same < count {
            break;
        }
    }
    Some(common)
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
    /// 書き終えたときに呼ぶ（Activity に書いた量を出す）。
    on_write: Option<OnWrite>,
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
        Self {
            inner: Arc::new(WriterInner {
                path,
                delay,
                pending: Mutex::default(),
                wake: Condvar::new(),
                on_write,
            }),
        }
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
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
        let mut pending = self.inner.pending.lock().unwrap();
        while pending.writing {
            pending = self.inner.wake.wait(pending).unwrap();
        }
        let Some((history, _)) = pending.next.take() else {
            return Ok(());
        };
        pending.writing = true;
        drop(pending);
        let result = self.inner.write(history.as_ref());
        self.inner.pending.lock().unwrap().writing = false;
        self.inner.wake.notify_all();
        result
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

impl WriterInner {
    fn write(&self, history: &dyn AsEntry) -> Result<()> {
        let written = write(&self.path, history.entry())?;
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
        let mut b = amonhen_model::CommitGraphBuilder::default();
        for i in 0..n {
            let parent = if i + 1 < n {
                Oid([
                    0xee,
                    i + 1,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ])
            } else {
                g.ids()[0]
            };
            b.push(
                Oid([
                    0xee, i, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ]),
                0,
                [parent],
            );
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

    /// 親のない行だけ（親のファイルが空）でも読める。
    #[test]
    fn round_trips_a_history_without_parents() {
        let mut b = amonhen_model::CommitGraphBuilder::default();
        b.push(Oid([1; 20]), 0, []);
        let h = history(b.build());
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/r");
        write(&path, h.entry()).unwrap();
        assert_eq!(read(&path).unwrap(), h);
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
        assert_eq!(read(&path).unwrap(), h);
        let state = read_state(&path).unwrap();
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
        assert_eq!(read(&path).unwrap().graph, grown);

        // 上の 30 行が消え、別の 5 行になった（reset して別のコミット）。
        let other = on_top(&old, 5);
        assert_eq!(&other.ids()[5..], old.ids());
        let w = write(&path, history(other.clone()).entry()).unwrap();
        assert_eq!((w.rows, w.cut), (5, 30));
        assert_eq!(read(&path).unwrap().graph, other);

        // ずっと下の行（古い方から 4,999 行目）が消えた（作り直された枝）。そこから上をすべて書き直す。
        let keep = old.retain(&(0..old.len()).map(|r| r != 15_000).collect::<Vec<_>>());
        let w = write(&path, history(keep.clone()).entry()).unwrap();
        assert_eq!((w.cut, w.rows), (20_005 - 4_999, 19_999 - 4_999));
        assert_eq!(read(&path).unwrap().graph, keep);
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
        let state = std::fs::read(&files.state).unwrap();
        write(&path, history(graph.clone()).entry()).unwrap();
        let short_state = std::fs::read(&files.state).unwrap();

        // 長い表と短い state。
        write(&path, history(grown.clone()).entry()).unwrap();
        std::fs::write(&files.state, &short_state).unwrap();
        assert_eq!(read(&path).unwrap().graph, graph);
        let w = write(&path, history(grown.clone()).entry()).unwrap();
        assert_eq!((w.rows, w.cut), (10, 0));
        assert_eq!(read(&path).unwrap().graph, grown);

        // 短い表と長い state。
        let ids = std::fs::read(&files.ids).unwrap();
        std::fs::write(&files.ids, &ids[..ids.len() - 20]).unwrap();
        assert!(read(&path).is_none());
        std::fs::write(&files.ids, &ids).unwrap();

        // 最後の行が合わない。
        let mut wrong = ids.clone();
        let len = wrong.len();
        wrong[len - 1] ^= 0xff;
        std::fs::write(&files.ids, &wrong).unwrap();
        assert!(read(&path).is_none());

        // 形式が違う。
        let mut old = state.clone();
        old[8] ^= 0xff;
        std::fs::write(&files.state, &old).unwrap();
        assert!(read(&path).is_none());
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
                        let read = read(path).unwrap().graph;
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
        assert_eq!(read(&path).unwrap().graph, grown);

        writer.schedule(Arc::new(history(old.clone())));
        drop(writer);
        assert_eq!(writes.lock().unwrap().len(), 2);
        assert_eq!(read(&path).unwrap().graph, old);
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
