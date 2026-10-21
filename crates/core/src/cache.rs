//! History の永続キャッシュ。次の起動では mmap するだけで、前回の History をすぐ表示する。
//!
//! 形式: 先頭に `Header`、続いて配列ごとの区画を 16 バイト境界に揃えて並べる。配列はメモリ上と
//! 同じ形のまま書くので、読むときは `Slab` で mmap した領域を直接指す（変換もコピーもしない）。
//! チェックポイントだけは可変長なので、読むときに組み立てる。

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use amonhen_layout::{Pod, Slab};
use amonhen_model::{CommitGraph, HistoryOrder, Oid, Result};
use memmap2::Mmap;

use crate::graph::{Checkpoint, GraphLayout, State, Track};

const MAGIC: [u8; 8] = *b"AMONHIST";
/// 形式を変えたら上げる。違うバージョンのキャッシュは読まずに作り直す。
const VERSION: u32 = 3;
const ALIGN: usize = 16;

#[repr(usize)]
#[derive(Clone, Copy)]
enum Section {
    Ids,
    CommitTimes,
    ParentOffsets,
    Parents,
    Nodes,
    Edges,
    Mainline,
    Tips,
    Checkpoints,
}
const SECTIONS: usize = 9;

#[repr(C)]
#[derive(Clone, Copy)]
struct Header {
    magic: [u8; 8],
    version: u32,
    has_head: u32,
    head: [u8; 20],
    lane_count: u32,
    /// 並び順（`HistoryOrder`。0 が日付順、1 がトポロジー順）。
    order: u32,
    reserved: u32,
    /// 区画ごとの（ファイル先頭からのバイト位置, 要素数）。
    sections: [[u64; 2]; SECTIONS],
}

unsafe impl Pod for Header {}

/// キャッシュに入れる History 一式。
pub struct CachedHistory {
    pub tips: Vec<Oid>,
    pub head: Option<Oid>,
    /// `graph` の並び順。
    pub order: HistoryOrder,
    pub graph: CommitGraph,
    pub layout: GraphLayout,
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

/// `dir` の中の、`key`（リポジトリのパスなど）ごとのキャッシュファイル。
pub fn path_for(dir: &Path, key: &str) -> PathBuf {
    // FNV-1a。Rust のバージョンが変わっても同じ名前になるよう、標準のハッシュは使わない。
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    dir.join(format!("history-{hash:016x}.bin"))
}

fn bytes_of<T: Pod>(values: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), std::mem::size_of_val(values)) }
}

/// 一時ファイルに書いてから置き換える（書きかけを読ませない）。
pub fn write(path: &Path, history: &CachedHistory) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let result = write_to(&tmp, history).and_then(|()| Ok(std::fs::rename(&tmp, path)?));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn write_to(path: &Path, h: &CachedHistory) -> Result<()> {
    let checkpoints = encode_checkpoints(&h.layout.checkpoints);
    let sections: [(&[u8], usize); SECTIONS] = [
        (h.graph.ids_slab().as_bytes(), h.graph.len()),
        (h.graph.commit_times_slab().as_bytes(), h.graph.len()),
        (
            h.graph.parent_offsets().as_bytes(),
            h.graph.parent_offsets().len(),
        ),
        (
            h.graph.parent_rows().as_bytes(),
            h.graph.parent_rows().len(),
        ),
        (h.layout.nodes.as_bytes(), h.layout.nodes.len()),
        (h.layout.edges.as_bytes(), h.layout.edges.len()),
        (h.layout.mainline.as_bytes(), h.layout.mainline.len()),
        (bytes_of(&h.tips), h.tips.len()),
        (&checkpoints, checkpoints.len()),
    ];

    let mut header = Header {
        magic: MAGIC,
        version: VERSION,
        has_head: u32::from(h.head.is_some()),
        head: h.head.map_or([0; 20], |o| o.0),
        lane_count: u32::from(h.layout.lane_count),
        order: order_code(h.order),
        reserved: 0,
        sections: [[0; 2]; SECTIONS],
    };
    let mut offset = size_of::<Header>().next_multiple_of(ALIGN);
    for (i, (bytes, count)) in sections.iter().enumerate() {
        header.sections[i] = [offset as u64, *count as u64];
        offset = (offset + bytes.len()).next_multiple_of(ALIGN);
    }

    let mut out = BufWriter::new(File::create(path)?);
    let mut written = 0;
    let mut put = |out: &mut BufWriter<File>, bytes: &[u8]| -> Result<()> {
        out.write_all(bytes)?;
        written += bytes.len();
        let pad = written.next_multiple_of(ALIGN) - written;
        out.write_all(&[0; ALIGN][..pad])?;
        written += pad;
        Ok(())
    };
    put(&mut out, bytes_of(std::slice::from_ref(&header)))?;
    for (bytes, _) in sections {
        put(&mut out, bytes)?;
    }
    out.flush()?;
    Ok(())
}

/// 読めなければ（ない・壊れている・形式が古い）`None`。
pub fn read(path: &Path) -> Option<CachedHistory> {
    let file = File::open(path).ok()?;
    let map = Arc::new(unsafe { Mmap::map(&file) }.ok()?);
    let header = *Slab::<Header>::mapped(map.clone(), 0, 1)?.first()?;
    if header.magic != MAGIC || header.version != VERSION {
        return None;
    }
    fn slab<T: Pod>(map: &Arc<Mmap>, header: &Header, s: Section) -> Option<Slab<T>> {
        let [offset, count] = header.sections[s as usize];
        Slab::mapped(
            map.clone(),
            usize::try_from(offset).ok()?,
            usize::try_from(count).ok()?,
        )
    }

    let graph = CommitGraph::from_parts(
        slab(&map, &header, Section::Ids)?,
        slab(&map, &header, Section::CommitTimes)?,
        slab(&map, &header, Section::ParentOffsets)?,
        slab(&map, &header, Section::Parents)?,
    )?;
    let checkpoints = decode_checkpoints(&slab::<u8>(&map, &header, Section::Checkpoints)?)?;
    let layout = GraphLayout::from_parts(
        slab(&map, &header, Section::Nodes)?,
        slab(&map, &header, Section::Edges)?,
        u16::try_from(header.lane_count).ok()?,
        slab(&map, &header, Section::Mainline)?,
        checkpoints,
    )?;
    if layout.nodes.len() != graph.len() {
        return None;
    }
    Some(CachedHistory {
        tips: slab::<Oid>(&map, &header, Section::Tips)?.to_vec(),
        head: (header.has_head != 0).then_some(Oid(header.head)),
        order: order_from_code(header.order)?,
        graph,
        layout,
    })
}

fn encode_checkpoints(checkpoints: &[Checkpoint]) -> Vec<u8> {
    fn track(out: &mut Vec<u8>, t: &Track) {
        out.extend_from_slice(&t.target.to_le_bytes());
        out.extend_from_slice(&t.child_row.to_le_bytes());
        out.extend_from_slice(&t.child_lane.to_le_bytes());
        out.extend_from_slice(&t.color.to_le_bytes());
        out.push(u8::from(t.first_parent));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&(checkpoints.len() as u32).to_le_bytes());
    for c in checkpoints {
        out.extend_from_slice(&c.row.to_le_bytes());
        out.extend_from_slice(&(c.edges_len as u64).to_le_bytes());
        out.extend_from_slice(&(c.state.lanes.len() as u32).to_le_bytes());
        for lane in &c.state.lanes {
            match lane {
                Some(t) => {
                    out.push(1);
                    track(&mut out, t);
                }
                None => out.push(0),
            }
        }
        out.extend_from_slice(&(c.state.riders.len() as u32).to_le_bytes());
        for (lane, t) in &c.state.riders {
            out.extend_from_slice(&lane.to_le_bytes());
            track(&mut out, t);
        }
    }
    out
}

fn decode_checkpoints(mut bytes: &[u8]) -> Option<Vec<Checkpoint>> {
    fn take<const N: usize>(bytes: &mut &[u8]) -> Option<[u8; N]> {
        let (head, rest) = bytes.split_first_chunk::<N>()?;
        *bytes = rest;
        Some(*head)
    }
    fn track(bytes: &mut &[u8]) -> Option<Track> {
        Some(Track {
            target: u32::from_le_bytes(take(bytes)?),
            child_row: u32::from_le_bytes(take(bytes)?),
            child_lane: u16::from_le_bytes(take(bytes)?),
            color: u16::from_le_bytes(take(bytes)?),
            first_parent: take::<1>(bytes)?[0] != 0,
        })
    }
    let count = u32::from_le_bytes(take(&mut bytes)?);
    let mut checkpoints = Vec::with_capacity(count.min(1 << 16) as usize);
    for _ in 0..count {
        let row = u32::from_le_bytes(take(&mut bytes)?);
        let edges_len = u64::from_le_bytes(take(&mut bytes)?) as usize;
        let lane_count = u32::from_le_bytes(take(&mut bytes)?);
        let mut lanes = Vec::with_capacity(lane_count.min(1 << 16) as usize);
        for _ in 0..lane_count {
            lanes.push(match take::<1>(&mut bytes)?[0] {
                0 => None,
                _ => Some(track(&mut bytes)?),
            });
        }
        let rider_count = u32::from_le_bytes(take(&mut bytes)?);
        let mut riders = Vec::with_capacity(rider_count.min(1 << 16) as usize);
        for _ in 0..rider_count {
            riders.push((u16::from_le_bytes(take(&mut bytes)?), track(&mut bytes)?));
        }
        checkpoints.push(Checkpoint {
            row,
            state: State { lanes, riders },
            edges_len,
        });
    }
    bytes.is_empty().then_some(checkpoints)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{layout, synthetic};

    #[test]
    fn round_trips_and_reads_arrays_in_place() {
        let (graph, head_row) = synthetic::graph(10_000, 9);
        let layout = layout(&graph, head_row);
        let head = head_row.map(|r| graph.ids()[r as usize]);
        let tips = vec![graph.ids()[0], graph.ids()[1]];
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/some/repo");

        write(
            &path,
            &CachedHistory {
                tips: tips.clone(),
                head,
                order: HistoryOrder::Topo,
                graph: graph.clone(),
                layout: layout.clone(),
            },
        )
        .unwrap();
        let read = read(&path).unwrap();
        assert_eq!(read.tips, tips);
        assert_eq!(read.head, head);
        assert_eq!(read.order, HistoryOrder::Topo);
        assert_eq!(read.graph, graph);
        assert_eq!(read.layout.nodes(), layout.nodes());
        assert_eq!(read.layout.edges(), layout.edges());
        assert_eq!(read.layout.lane_count(), layout.lane_count());
        assert_eq!(read.layout.checkpoints.len(), layout.checkpoints.len());
        assert_eq!(
            read.layout.edges_in(5000..5050).collect::<Vec<_>>(),
            layout.edges_in(5000..5050).collect::<Vec<_>>()
        );
    }

    #[test]
    fn rejects_missing_truncated_and_old_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), "/r");
        assert!(read(&path).is_none());

        let (graph, head_row) = synthetic::graph(1000, 1);
        let layout = layout(&graph, head_row);
        write(
            &path,
            &CachedHistory {
                tips: vec![],
                head: None,
                order: HistoryOrder::Date,
                graph,
                layout,
            },
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();

        std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        assert!(read(&path).is_none(), "途中で切れている");

        let mut old = bytes.clone();
        old[8] = VERSION as u8 + 1;
        std::fs::write(&path, &old).unwrap();
        assert!(read(&path).is_none(), "形式が違う");
    }
}
