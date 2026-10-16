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

use crate::diff::{DiffView, Part, Source};

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
pub fn line_ranges(data: &[u8]) -> Vec<Range<usize>> {
    lines(data)
}

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

/// Magic Merge の規則の範囲。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagicRules {
    /// 間違えようのないものだけ（両側が同じ、片側だけが変えた）。ファイルを開いたときに自動でかける。
    Safe,
    /// すべての規則。
    All,
}

/// Magic Merge: 決まった規則で解けるコンフリクトを解く。解けたものは選んだ中身に置き換え、解けないものは
/// マーカーごと残す。規則は順に:
/// 1. 両側が同じ
/// 2. 片側が共通の祖先と同じ（もう片側だけが変えた。diff3 のときだけ）
/// 3. 空白・改行だけが違う（Current の側にする）
/// 4. 両側が同じ所に import・use などの行だけを足した（両方を並べる。diff3 のときだけ）
/// 5. 単語単位で 3-way マージでき、両側の変更が重ならない（diff3 のときだけ）
///
/// `only` があれば、その番号のコンフリクトだけを解く。解いた内容と、解いたコンフリクトの数。
pub fn magic(data: &[u8], rules: MagicRules, only: Option<usize>) -> (Vec<u8>, usize) {
    let ls = lines(data);
    let take = |r: &Range<usize>| -> &[u8] {
        if r.is_empty() {
            &[]
        } else {
            &data[ls[r.start].start..ls[r.end - 1].end]
        }
    };
    let mut out = Vec::with_capacity(data.len());
    let mut next = 0usize;
    let mut resolved = 0;
    for (i, c) in parse(data).into_iter().enumerate() {
        out.extend_from_slice(&data[next..ls[c.lines.start].start]);
        next = ls[c.lines.end - 1].end;
        let solution = if only.is_none_or(|o| o == i) {
            solve(
                take(&c.ours),
                take(&c.theirs),
                c.base.as_ref().map(take),
                rules,
            )
        } else {
            None
        };
        match solution {
            Some(text) => {
                out.extend_from_slice(&text);
                resolved += 1;
            }
            None => out.extend_from_slice(&data[ls[c.lines.start].start..next]),
        }
    }
    out.extend_from_slice(&data[next..]);
    (out, resolved)
}

/// コンフリクト 1 つを規則で解く。解けなければ None。
fn solve(ours: &[u8], theirs: &[u8], base: Option<&[u8]>, rules: MagicRules) -> Option<Vec<u8>> {
    if ours == theirs {
        return Some(ours.to_vec());
    }
    if base == Some(ours) {
        return Some(theirs.to_vec());
    }
    if base == Some(theirs) {
        return Some(ours.to_vec());
    }
    if rules == MagicRules::Safe {
        return None;
    }
    let squeeze = |b: &[u8]| -> Vec<u8> {
        b.iter()
            .copied()
            .filter(|c| !c.is_ascii_whitespace())
            .collect()
    };
    if squeeze(ours) == squeeze(theirs) {
        return Some(ours.to_vec());
    }
    let base = base?;
    merge_imports(base, ours, theirs).or_else(|| crate::words::merge3(base, ours, theirs))
}

/// import・use・#include などの行か。
fn is_import(line: &[u8]) -> bool {
    let t = String::from_utf8_lossy(line);
    let t = t.trim_start();
    [
        "import ", "use ", "#include", "from ", "require(", "@import", "using ", "#import",
    ]
    .iter()
    .any(|p| t.starts_with(p))
        || (t.starts_with("const ") && t.contains("require("))
}

/// 両側が共通の祖先の同じ位置に、import などの行だけを足したなら、両方を並べる（Current が先、重複は 1 つ）。
fn merge_imports(base: &[u8], ours: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    let split = |data: &[u8]| -> Vec<Vec<u8>> {
        lines(data).into_iter().map(|r| data[r].to_vec()).collect()
    };
    let (b, o, t) = (split(base), split(ours), split(theirs));
    // `side` が「祖先の前 k 行 + 足した行 + 祖先の残り」なら、(k, 足した行)。
    let added = |side: &[Vec<u8>]| -> Option<(usize, Vec<Vec<u8>>)> {
        let max = side.len().min(b.len());
        let p = (0..max).take_while(|&i| side[i] == b[i]).count();
        let q = (0..max - p)
            .take_while(|&i| side[side.len() - 1 - i] == b[b.len() - 1 - i])
            .count();
        (p + q == b.len() && side.len() > b.len()).then(|| (p, side[p..side.len() - q].to_vec()))
    };
    let ((ko, a), (kt, c)) = (added(&o)?, added(&t)?);
    let normalize = |l: &Vec<u8>| String::from_utf8_lossy(l).trim().to_owned();
    if ko != kt || !a.iter().chain(&c).all(|l| is_import(l)) {
        return None;
    }
    let mut merged: Vec<Vec<u8>> = b[..ko].to_vec();
    merged.extend(a.iter().cloned());
    for line in &c {
        if !a.iter().any(|x| normalize(x) == normalize(line)) {
            merged.push(line.clone());
        }
    }
    merged.extend(b[ko..].iter().cloned());
    let mut out: Vec<u8> = Vec::new();
    for line in merged {
        // 改行のない最後の行の後ろに続けるときは、改行を補う。
        if out.last().is_some_and(|&x| x != b'\n') {
            out.push(b'\n');
        }
        out.extend_from_slice(&line);
    }
    Some(out)
}

/// マージエディタの 1 つの欄（Current・Incoming・共通の祖先）。すべてのコンフリクトをその側で解決した内容と、
/// 各コンフリクトがその内容の何行目から何行目になったか（0 始まり、終わりは含まない）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    pub text: Vec<u8>,
    pub regions: Vec<Range<usize>>,
}

/// すべてのコンフリクトを `choice`（`Ours`・`Theirs`・`Base`）の側にした欄。`Base` で、共通の祖先のない
/// コンフリクト（diff3 でない）があれば `None`。
pub fn side(data: &[u8], choice: Choice) -> Option<Side> {
    let ls = lines(data);
    let mut text = Vec::with_capacity(data.len());
    let mut regions = Vec::new();
    let mut out_line = 0;
    let mut next = 0;
    let copy = |range: Range<usize>, text: &mut Vec<u8>, out_line: &mut usize| {
        for i in range {
            text.extend_from_slice(&data[ls[i].clone()]);
            *out_line += 1;
        }
    };
    for c in parse(data) {
        copy(next..c.lines.start, &mut text, &mut out_line);
        let picked = match choice {
            Choice::Ours => c.ours.clone(),
            Choice::Theirs => c.theirs.clone(),
            Choice::Base => c.base.clone()?,
            Choice::Both => return None,
        };
        let start = out_line;
        copy(picked, &mut text, &mut out_line);
        regions.push(start..out_line);
        next = c.lines.end;
    }
    copy(next..ls.len(), &mut text, &mut out_line);
    Some(Side { text, regions })
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
            flags: 0,
            _pad: [0; 2],
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
    let sources = ls
        .iter()
        .map(|r| {
            let mut end = r.end;
            while end > r.start && matches!(data[end - 1], b'\n' | b'\r') {
                end -= 1;
            }
            (Source::New, r.start as u32..end as u32)
        })
        .collect();
    DiffView {
        rows,
        highlights: Vec::new(),
        strings: strings.finish(),
        binary: false,
        added: 0,
        removed: conflicts.len() as u32,
        parts: vec![Part {
            path: String::new(),
            old: Arc::default(),
            new: data.clone(),
            first_row: 0,
            sources,
        }],
        encoding: profile.string,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_merge_joins_imports_one_conflict_or_only_safe_rules() {
        let text = concat!(
            "<<<<<<< HEAD\nuse a;\nuse x;\nfn f() {}\n||||||| base\nuse a;\nfn f() {}\n=======\nuse a;\nuse y;\nfn f() {}\n>>>>>>> t\n",
            "mid\n",
            "<<<<<<< HEAD\nsame\n=======\nsame\n>>>>>>> t\n",
        );
        // import を両側が足したものは、すべての規則のときだけ並べる。
        let (out, n) = magic(text.as_bytes(), MagicRules::All, None);
        assert_eq!(n, 2);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "use a;\nuse x;\nuse y;\nfn f() {}\nmid\nsame\n"
        );
        let (out, n) = magic(text.as_bytes(), MagicRules::Safe, None);
        assert_eq!(n, 1);
        assert_eq!(parse(&out).len(), 1);
        // 番号を指定すると、そのコンフリクトだけ。
        let (out, n) = magic(text.as_bytes(), MagicRules::All, Some(1));
        assert_eq!(n, 1);
        assert!(
            String::from_utf8(out)
                .unwrap()
                .starts_with("<<<<<<< HEAD\nuse a;")
        );
    }

    #[test]
    fn magic_merge_solves_what_the_rules_allow() {
        let text = concat!(
            "keep\n",
            // 1. 同じ
            "<<<<<<< HEAD\nsame\n=======\nsame\n>>>>>>> t\n",
            // 2. Current が祖先のまま → Incoming
            "<<<<<<< HEAD\nold\n||||||| base\nold\n=======\nnew\n>>>>>>> t\n",
            // 3. 空白だけ違う → Current
            "<<<<<<< HEAD\nf(a, b)\n=======\nf(a,b)\n>>>>>>> t\n",
            // 4. 同じ行の別の単語 → 両方
            "<<<<<<< HEAD\ncall(x, b)\n||||||| base\ncall(a, b)\n=======\ncall(a, y)\n>>>>>>> t\n",
            // 解けない: 同じ単語を別々に変えた
            "<<<<<<< HEAD\nv = 1\n||||||| base\nv = 0\n=======\nv = 2\n>>>>>>> t\n",
            "end\n",
        );
        let (out, resolved) = magic(text.as_bytes(), MagicRules::All, None);
        assert_eq!(resolved, 4);
        let out = String::from_utf8(out).unwrap();
        assert_eq!(
            out,
            "keep\nsame\nnew\nf(a, b)\ncall(x, y)\n<<<<<<< HEAD\nv = 1\n||||||| base\nv = 0\n=======\nv = 2\n>>>>>>> t\nend\n"
        );
        assert_eq!(parse(out.as_bytes()).len(), 1);
    }

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
    fn sides_resolve_every_conflict_and_report_where_each_landed() {
        let data = FILE.as_bytes();
        let ours = side(data, Choice::Ours).unwrap();
        assert_eq!(ours.text, b"a\nours 1\nours 2\nb\nx\nc\n");
        assert_eq!(ours.regions, [1..3, 4..5]);
        let theirs = side(data, Choice::Theirs).unwrap();
        assert_eq!(theirs.text, b"a\ntheirs\nb\ny\nc\n");
        assert_eq!(theirs.regions, [1..2, 3..4]);
        // 1 つ目のコンフリクトには共通の祖先がない。
        assert!(side(data, Choice::Base).is_none());
        let only_diff3 = &FILE[FILE.find("b\n").unwrap()..];
        let base = side(only_diff3.as_bytes(), Choice::Base).unwrap();
        assert_eq!(base.text, b"b\no\nc\n");
        assert_eq!(base.regions, vec![1..2]);
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
