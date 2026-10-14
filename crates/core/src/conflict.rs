//! コンフリクトマーカーのあるファイルの解析と、コンフリクトごとの解決。
//!
//! ```text
//! <<<<<<< HEAD          ← Current（merge なら今のブランチ、rebase なら上に載せる先）
//! ours
//! ||||||| base          ← diff3 / zdiff3 のときだけ
//! base
//! =======
//! theirs
//! >>>>>>> topic         ← Incoming
//! ```

use std::ops::Range;
use std::sync::Arc;

use amonhen_layout::DiffRow;
use amonhen_projection::{FrontendProfile, StringTableBuilder};
use serde::Deserialize;

use crate::diff::DiffView;

/// ファイルの中の 1 つのコンフリクト。範囲はすべて行（0 始まり、改行込みのバイト範囲の添字）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// `<<<<<<<` の行から `>>>>>>>` の行まで。
    pub lines: Range<usize>,
    pub ours: Range<usize>,
    pub base: Option<Range<usize>>,
    pub theirs: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Choice {
    /// Current（`<<<<<<<` 側）。
    Ours,
    /// Incoming（`>>>>>>>` 側）。
    Theirs,
    /// Current の後に Incoming。
    Both,
    /// 共通の祖先（diff3 のときだけ）。
    Base,
}

/// 改行まで含めた行のバイト範囲。
fn lines(data: &[u8]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < data.len() {
        let end = data[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(data.len(), |i| start + i + 1);
        out.push(start..end);
        start = end;
    }
    out
}

fn is_marker(line: &[u8], marker: &[u8; 7]) -> bool {
    line.starts_with(marker)
        && line
            .get(7)
            .is_none_or(|&b| b == b' ' || b == b'\n' || b == b'\r')
}

/// コンフリクトを探す。マーカーが閉じていないものは無視する。
pub fn parse(data: &[u8]) -> Vec<Conflict> {
    let ls = lines(data);
    let text = |i: usize| &data[ls[i].clone()];
    let mut out = Vec::new();
    let mut i = 0;
    while i < ls.len() {
        if !is_marker(text(i), b"<<<<<<<") {
            i += 1;
            continue;
        }
        let start = i;
        let (mut base_at, mut sep_at, mut end_at) = (None, None, None);
        let mut j = i + 1;
        while j < ls.len() {
            let t = text(j);
            if is_marker(t, b"|||||||") && base_at.is_none() && sep_at.is_none() {
                base_at = Some(j);
            } else if is_marker(t, b"=======") && sep_at.is_none() {
                sep_at = Some(j);
            } else if is_marker(t, b">>>>>>>") && sep_at.is_some() {
                end_at = Some(j);
                break;
            } else if is_marker(t, b"<<<<<<<") {
                break;
            }
            j += 1;
        }
        match (sep_at, end_at) {
            (Some(sep), Some(end)) => {
                let ours_end = base_at.unwrap_or(sep);
                out.push(Conflict {
                    lines: start..end + 1,
                    ours: start + 1..ours_end,
                    base: base_at.map(|b| b + 1..sep),
                    theirs: sep + 1..end,
                });
                i = end + 1;
            }
            _ => i = start + 1,
        }
    }
    out
}

/// `index` 番目のコンフリクトを `choice` で解決した内容。ほかのコンフリクトはそのまま残す。
pub fn resolve(data: &[u8], index: usize, choice: Choice) -> Option<Vec<u8>> {
    let ls = lines(data);
    let c = parse(data).into_iter().nth(index)?;
    let take = |r: &Range<usize>| -> &[u8] {
        if r.is_empty() {
            &[]
        } else {
            &data[ls[r.start].start..ls[r.end - 1].end]
        }
    };
    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(&data[..ls[c.lines.start].start]);
    match choice {
        Choice::Ours => out.extend_from_slice(take(&c.ours)),
        Choice::Theirs => out.extend_from_slice(take(&c.theirs)),
        Choice::Both => {
            out.extend_from_slice(take(&c.ours));
            // 改行のない行の後ろに続けるときは、改行を補う。
            if out.last().is_some_and(|&b| b != b'\n') {
                out.push(b'\n');
            }
            out.extend_from_slice(take(&c.theirs));
        }
        Choice::Base => out.extend_from_slice(take(c.base.as_ref()?)),
    }
    out.extend_from_slice(&data[ls[c.lines.end - 1].end..]);
    Some(out)
}

/// コンフリクトのあるファイルを、FE 向けの行にする。コンフリクトの行の `old_no` は、
/// そのコンフリクトの番号 + 1（クリックしたコンフリクトを知るため）。`new_no` はファイルの行番号。
pub fn view(data: Arc<Vec<u8>>, profile: &FrontendProfile) -> DiffView {
    let ls = lines(&data);
    let conflicts = parse(&data);
    let mut strings = StringTableBuilder::new(profile.string);
    let buf = strings.share(data.clone());
    let mut rows = Vec::with_capacity(ls.len());
    let mut next = 0;
    let mut row = |kind: u8, conflict: u32, line: usize, strings: &mut StringTableBuilder| {
        let mut r = ls[line].clone();
        while r.end > r.start && matches!(data[r.end - 1], b'\n' | b'\r') {
            r.end -= 1;
        }
        rows.push(DiffRow {
            kind,
            _pad: [0; 3],
            old_no: conflict,
            new_no: line as u32 + 1,
            text: strings.add(buf, &data[r]),
        });
    };
    for (n, c) in conflicts.iter().enumerate() {
        for line in next..c.lines.start {
            row(DiffRow::CONTEXT, 0, line, &mut strings);
        }
        let id = n as u32 + 1;
        for line in c.lines.clone() {
            let kind = if c.ours.contains(&line) {
                DiffRow::OURS
            } else if c.theirs.contains(&line) {
                DiffRow::THEIRS
            } else if c.base.as_ref().is_some_and(|b| b.contains(&line)) {
                DiffRow::BASE
            } else {
                DiffRow::MARKER
            };
            row(kind, id, line, &mut strings);
        }
        next = c.lines.end;
    }
    for line in next..ls.len() {
        row(DiffRow::CONTEXT, 0, line, &mut strings);
    }
    DiffView {
        rows,
        strings: strings.finish(),
        binary: false,
        added: 0,
        removed: conflicts.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "a\n<<<<<<< HEAD\nours 1\nours 2\n=======\ntheirs\n>>>>>>> topic\nb\n<<<<<<< HEAD\nx\n||||||| base\no\n=======\ny\n>>>>>>> topic\nc\n";

    #[test]
    fn finds_conflicts_with_and_without_base() {
        let cs = parse(FILE.as_bytes());
        assert_eq!(cs.len(), 2);
        assert_eq!(
            cs[0],
            Conflict {
                lines: 1..7,
                ours: 2..4,
                base: None,
                theirs: 5..6
            }
        );
        assert_eq!(
            cs[1],
            Conflict {
                lines: 8..15,
                ours: 9..10,
                base: Some(11..12),
                theirs: 13..14
            }
        );
        // 閉じていないマーカーは無視する。
        assert!(parse(b"<<<<<<< HEAD\nx\n=======\n").is_empty());
    }

    #[test]
    fn resolves_one_conflict_at_a_time() {
        let data = FILE.as_bytes();
        let ours = resolve(data, 0, Choice::Ours).unwrap();
        assert!(
            String::from_utf8(ours.clone())
                .unwrap()
                .starts_with("a\nours 1\nours 2\nb\n<<<<<<< HEAD\n")
        );
        assert_eq!(parse(&ours).len(), 1);
        let theirs = resolve(&ours, 0, Choice::Theirs).unwrap();
        assert_eq!(theirs, b"a\nours 1\nours 2\nb\ny\nc\n");
        assert_eq!(
            resolve(data, 1, Choice::Base).unwrap(),
            b"a\n<<<<<<< HEAD\nours 1\nours 2\n=======\ntheirs\n>>>>>>> topic\nb\no\nc\n"
        );
        assert!(
            resolve(data, 1, Choice::Both)
                .unwrap()
                .ends_with(b"b\nx\ny\nc\n")
        );
        assert!(
            resolve(data, 0, Choice::Base).is_none(),
            "base のないコンフリクト"
        );
    }

    #[test]
    fn view_marks_each_conflict_line() {
        let v = view(Arc::new(FILE.as_bytes().to_vec()), &FrontendProfile::MACOS);
        let kinds: Vec<u8> = v.rows.iter().map(|r| r.kind).collect();
        use DiffRow as R;
        assert_eq!(
            kinds,
            [
                R::CONTEXT,
                R::MARKER,
                R::OURS,
                R::OURS,
                R::MARKER,
                R::THEIRS,
                R::MARKER,
                R::CONTEXT,
                R::MARKER,
                R::OURS,
                R::MARKER,
                R::BASE,
                R::MARKER,
                R::THEIRS,
                R::MARKER,
                R::CONTEXT
            ]
        );
        assert_eq!(v.rows[2].old_no, 1);
        assert_eq!(v.rows[9].old_no, 2);
        assert_eq!(v.strings.bytes(v.rows[2].text), b"ours 1");
    }
}
