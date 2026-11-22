//! 前と後の絵（アプリが確かめるときに見せるもの）。エンジンの `rewritePreview` の `editDrawing`（History の編集）か、
//! `movePreview` の `drawing` を [`from_move`] で同じ形にしたもの（reset など、ブランチを動かす操作）、rebase・merge の
//! 絵を [`from_rebase`]・[`from_merge`]、cherry-pick・revert の絵を [`from_pick`] で同じ形にしたものを、左に前、
//! 右に後の 2 列で、新しい順に描く。どちらの側も、いちばん下に変わらないいちばん新しいコミット（共通の土台）を描く。

use amonhen_model::tr;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

/// 描く行（印・短い ID・件名・そのあとに添えるもの）。
struct Line {
    mark: &'static str,
    id: String,
    subject: String,
    note: String,
    style: Style,
}

/// 中身の高さ（2 列の見出し・行・土台）。
pub fn height(drawing: &Value) -> u16 {
    let rows = |key: &str, more: &str| {
        drawing[key].as_array().map_or(0, Vec::len)
            + usize::from(drawing[more].as_u64().unwrap_or(0) > 0)
    };
    // 後に何も残らないときは、そう言う 1 行。
    let after = rows("after", "afterMore").max(1);
    let rows = rows("before", "beforeMore").max(after);
    (rows + 1 + usize::from(drawing["base"].is_object())) as u16
}

/// `oid` を指す ref の名前（ローカルのブランチを先に）。
fn labels_of(drawing: &Value, oid: &str, local_only: bool) -> String {
    let mut names: Vec<(bool, &str)> = (drawing["labels"].as_array().into_iter().flatten())
        .filter(|l| l["oid"] == oid)
        .filter(|l| !local_only || l["kind"] == "local")
        .filter_map(|l| Some((l["kind"] != "local", l["name"].as_str()?)))
        .collect();
    names.sort();
    names
        .iter()
        .map(|(_, n)| format!("[{n}]"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 後の側で、いちばん上（何も残らなければ土台）に付くブランチ。`branch` があればそれ（ブランチを動かす操作）、
/// なければ前のいちばん上のローカルのブランチ（History の編集では、一緒に動く）。空なら何も付けない（ブランチを
/// 消すとき）。
fn moved(drawing: &Value) -> String {
    match drawing["branch"].as_str() {
        Some("") => String::new(),
        Some(name) => format!("[{name}]"),
        None => labels_of(
            drawing,
            drawing["before"][0]["oid"].as_str().unwrap_or(""),
            true,
        ),
    }
}

/// `movePreview` の `drawing` を、`editDrawing` と同じ形に。前は外れるコミット、後は加わるコミット。`branch` が
/// 動く。
pub fn from_move(drawing: &Value, branch: &str) -> Value {
    let commits = |key: &str, action: &str| -> Vec<Value> {
        (drawing[key].as_array().into_iter().flatten())
            .map(|c| {
                let mut c = c.clone();
                let hidden = c["hidden"] == true;
                c["action"] = Value::from(match (action, hidden) {
                    ("pick", true) => "hidden",
                    _ => action,
                });
                c
            })
            .collect()
    };
    serde_json::json!({
        "before": commits("lost", "leave"),
        "beforeMore": drawing["lostMore"],
        "beforeMoreAt": drawing["lostMoreAt"],
        "after": commits("gained", "pick"),
        "afterMore": drawing["gainedMore"],
        "afterMoreAt": drawing["gainedMoreAt"],
        "base": drawing["base"],
        "labels": drawing["labels"],
        "branch": branch,
    })
}

/// `shown`（新しい順）の `at` 番目の前に、省いた `more` 個の印を入れる。
fn with_more(mut shown: Vec<Value>, more: &Value, at: &Value) -> Vec<Value> {
    let more = more.as_u64().unwrap_or(0);
    if more > 0 {
        let at = (at.as_u64().unwrap_or(0) as usize).min(shown.len());
        shown.insert(at, serde_json::json!({ "more": more }));
    }
    shown
}

/// 並びのコミットに、することと添える文を付ける。
fn marked(commits: &Value, action: &str, note: Option<&str>) -> Vec<Value> {
    (commits.as_array().into_iter().flatten())
        .map(|c| {
            let mut c = c.clone();
            c["action"] = Value::from(action);
            if let Some(note) = note {
                c["note"] = Value::from(note);
            }
            c
        })
        .collect()
}

/// rebase の絵（`rewritePreview` の `drawing`）を同じ形に。前は今のブランチの載せ直すコミット、後はその写し
/// （新しい ID になる）と、その下に載せ先で進んだコミット。
pub fn from_rebase(drawing: &Value, branch: &str, onto: &str) -> Value {
    // 載せ直すコミットは古い順で来る（省いたところは新しい順で数える）。
    let mut picks: Vec<Value> = drawing["picks"].as_array().cloned().unwrap_or_default();
    picks.reverse();
    let before = with_more(
        marked(&Value::from(picks.clone()), "leave", None),
        &drawing["picksMore"],
        &drawing["picksMoreAt"],
    );
    // 写しは新しいコミットなので、ID は出さない。
    let copies: Vec<Value> = marked(&Value::from(picks), "pick", Some(&tr!("new copy")))
        .into_iter()
        .map(|mut c| {
            c["oid"] = Value::from("");
            c
        })
        .collect();
    let mut after = with_more(copies, &drawing["picksMore"], &drawing["picksMoreAt"]);
    let onto_note = tr!("on {name}", name = onto);
    after.extend(with_more(
        marked(&drawing["onto"], "keep", Some(&onto_note)),
        &drawing["ontoMore"],
        &drawing["ontoMoreAt"],
    ));
    serde_json::json!({
        "before": before, "after": after,
        "base": drawing["base"], "labels": drawing["labels"], "branch": branch,
    })
}

/// merge の絵（`mergePreview` の `drawing`）を同じ形に。前は今のブランチ、後はいちばん上に新しい merge コミット、
/// その下に取り込むコミットと今のブランチのコミット。
pub fn from_merge(drawing: &Value, branch: &str, theirs: &str) -> Value {
    let ours = with_more(
        marked(&drawing["ours"], "pick", None),
        &drawing["oursMore"],
        &drawing["oursMoreAt"],
    );
    let mut after = vec![serde_json::json!({
        "oid": "", "subject": drawing["subject"], "action": "pick",
        "note": tr!("new merge"),
    })];
    let from = tr!("from {name}", name = theirs);
    after.extend(with_more(
        marked(&drawing["theirs"], "in", Some(&from)),
        &drawing["theirsMore"],
        &drawing["theirsMoreAt"],
    ));
    after.extend(marked(&Value::from(ours.clone()), "keep", None));
    serde_json::json!({
        "before": ours, "after": after,
        "base": drawing["base"], "labels": drawing["labels"], "branch": branch,
    })
}

/// cherry-pick・revert の絵（`pickPreview` の `drawing`）を同じ形に。前は今の先端、後はその上に新しいコミット。
/// 土台は先端の親（なければ土台なし）。
pub fn from_pick(drawing: &Value, branch: &str) -> Value {
    let tip: Vec<Value> = drawing["head"]
        .as_array()
        .into_iter()
        .flatten()
        .take(1)
        .cloned()
        .collect();
    let new: Vec<Value> = marked(&drawing["picks"], "pick", Some(&tr!("new")))
        .into_iter()
        .map(|mut c| {
            c["oid"] = Value::from("");
            c
        })
        .collect();
    let mut after = with_more(new, &drawing["picksMore"], &drawing["picksMoreAt"]);
    after.extend(marked(&Value::from(tip.clone()), "keep", None));
    serde_json::json!({
        "before": marked(&Value::from(tip), "pick", None),
        "after": after,
        "base": drawing["head"].get(1).cloned().unwrap_or(Value::Null),
        "labels": drawing["labels"],
        "branch": branch,
    })
}

fn short(oid: &str) -> String {
    oid[..oid.len().min(8)].to_owned()
}

/// 片側の行（新しい順）。`more` 個を `at` 番目の前で省く。
fn side(drawing: &Value, after: bool) -> Vec<Line> {
    let (key, more, at) = match after {
        false => ("before", "beforeMore", "beforeMoreAt"),
        true => ("after", "afterMore", "afterMoreAt"),
    };
    let more = drawing[more].as_u64().unwrap_or(0);
    let at = drawing[at].as_u64().unwrap_or(0) as usize;
    let mut lines = Vec::new();
    for (i, commit) in drawing[key].as_array().into_iter().flatten().enumerate() {
        if more > 0 && i == at {
            lines.push(elided(more));
        }
        // 列の途中で省いたところ（rebase・merge の絵は、いくつかの並びをつなぐ）。
        if let Some(count) = commit["more"].as_u64() {
            lines.push(elided(count));
            continue;
        }
        let oid = commit["oid"].as_str().unwrap_or("");
        let action = commit["action"].as_str().unwrap_or("pick");
        let mut note = Vec::new();
        // ブランチは、後ではいちばん上の新しいコミットに付く（一緒に動く）。
        let labels = match (after, i) {
            (false, _) => labels_of(drawing, oid, false),
            (true, 0) => moved(drawing),
            (true, _) => String::new(),
        };
        if !labels.is_empty() {
            note.push(labels);
        }
        let style = match (after, action) {
            (false, "drop") => Style::new()
                .fg(Color::Red)
                .add_modifier(Modifier::CROSSED_OUT),
            (false, "squash" | "fixup" | "leave") => Style::new().fg(Color::Indexed(244)),
            (false, _) => Style::new(),
            // 後の側で変わらないもの（rebase の載せ先・merge の今のブランチ）。
            (true, "keep") => Style::new(),
            (true, "in") => Style::new().fg(Color::Cyan),
            (true, _) => Style::new().fg(Color::Green),
        };
        if after {
            let combined = commit["combined"].as_u64().unwrap_or(0);
            if combined > 0 {
                note.push(tr!("+{count} combined", count = combined));
            }
            if let Some(text) = commit["note"].as_str() {
                note.push(text.to_owned());
            }
            match action {
                "edit" => note.push(tr!("stops here")),
                // どこからも辿れない（Reflog にだけある）コミット。
                "hidden" => note.push(tr!("only in the reflog")),
                _ => {}
            }
        } else {
            match action {
                "pick" => {}
                // ブランチから外れる（消えはしない。Undo で戻る）。
                "leave" => note.push(tr!("leaves the branch")),
                action => note.push(action.to_owned()),
            }
        }
        lines.push(Line {
            mark: "●",
            id: short(oid),
            subject: commit["subject"].as_str().unwrap_or("").to_owned(),
            note: note.join("  "),
            style,
        });
    }
    if more > 0 && at >= lines.len() {
        lines.push(elided(more));
    }
    // どちらの側も、書き換えない土台の上に載る。前では、そこを指すブランチを添える。後では、何も残らなければ動く
    // ブランチがそこに付く。
    if let Some(base) = drawing["base"].as_object() {
        let oid = base.get("oid").and_then(Value::as_str).unwrap_or("");
        let mut note = Vec::new();
        let labels = match (after, lines.is_empty()) {
            (false, _) => labels_of(drawing, oid, false),
            (true, true) => moved(drawing),
            (true, false) => String::new(),
        };
        if !labels.is_empty() {
            note.push(labels);
        }
        note.push(tr!("unchanged"));
        lines.push(Line {
            mark: "○",
            id: short(base.get("oid").and_then(Value::as_str).unwrap_or("")),
            subject: base
                .get("subject")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            note: note.join("  "),
            style: Style::new(),
        });
    }
    lines
}

fn elided(count: u64) -> Line {
    Line {
        mark: "┆",
        id: String::new(),
        subject: tr!("{count} more", count = count),
        note: String::new(),
        style: Style::new().fg(Color::Indexed(244)),
    }
}

fn put(buf: &mut Buffer, x: u16, y: u16, width: u16, line: &Line) {
    let room = usize::from(width);
    let mut at = buf.set_stringn(x, y, line.mark, room, line.style).0 + 1;
    let used = |at: u16| usize::from(at.saturating_sub(x));
    if !line.id.is_empty() {
        at =
            buf.set_stringn(
                at,
                y,
                &line.id,
                room.saturating_sub(used(at)),
                Style::new().fg(Color::Indexed(244)),
            )
            .0 + 1;
    }
    at = buf
        .set_stringn(
            at,
            y,
            &line.subject,
            room.saturating_sub(used(at)),
            line.style,
        )
        .0;
    if !line.note.is_empty() && used(at) + 2 < room {
        buf.set_stringn(
            at + 2,
            y,
            &line.note,
            room.saturating_sub(used(at) + 2),
            Style::new().fg(Color::Cyan),
        );
    }
}

/// `area` に描く（呼ぶ側が枠と題を描く）。
pub fn draw(drawing: &Value, area: Rect, buf: &mut Buffer) {
    if area.height < 2 || area.width < 20 {
        return;
    }
    let column = (area.width - 3) / 2;
    let right = area.x + column + 3;
    let heading = Style::new().add_modifier(Modifier::BOLD);
    buf.set_stringn(area.x, area.y, tr!("Before"), usize::from(column), heading);
    buf.set_stringn(right, area.y, tr!("After"), usize::from(column), heading);
    let before = side(drawing, false);
    let after = side(drawing, true);
    for (y, line) in (area.y + 1..area.bottom()).zip(&before) {
        put(buf, area.x, y, column, line);
    }
    // 後に何も残らなければ、そう言ってから土台。
    let mut y = area.y + 1;
    if drawing["after"].as_array().is_none_or(Vec::is_empty) && y < area.bottom() {
        let none = tr!("(no commits left)");
        let dim = Style::new().fg(Color::Indexed(244));
        buf.set_stringn(right, y, &none, usize::from(column), dim);
        y += 1;
    }
    for (y, line) in (y..area.bottom()).zip(&after) {
        put(buf, right, y, column, line);
    }
}

/// 枠の幅に要るもの（いちばん長い行）。
pub fn width(drawing: &Value) -> u16 {
    let widest = |after: bool| {
        side(drawing, after)
            .iter()
            .map(|l| 2 + l.id.width() + 1 + l.subject.width() + 2 + l.note.width())
            .max()
            .unwrap_or(0)
    };
    ((widest(false).max(widest(true)) * 2 + 3).min(u16::MAX as usize)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(buf: &Buffer, area: Rect) -> Vec<String> {
        (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    /// 前は消えるものに印、後はまとめた数とブランチ。どちらの側も、コミットのすぐ下に土台。
    #[test]
    fn before_and_after_side_by_side() {
        let drawing = json!({
            "before": [
                {"oid": "cccccccc1", "subject": "Four", "action": "drop"},
                {"oid": "bbbbbbbb1", "subject": "Three", "action": "fixup"},
                {"oid": "aaaaaaaa1", "subject": "Two", "action": "pick"},
            ],
            "beforeMore": 0, "beforeMoreAt": 0,
            "after": [{"oid": "aaaaaaaa1", "subject": "Two", "combined": 1, "action": "pick"}],
            "afterMore": 0, "afterMoreAt": 0,
            "base": {"oid": "99999999", "subject": "One", "time": 0},
            "labels": [{"oid": "cccccccc1", "name": "main", "kind": "local"}],
        });
        assert_eq!(height(&drawing), 5);
        let area = Rect::new(0, 0, 80, height(&drawing));
        let mut buf = Buffer::empty(area);
        draw(&drawing, area, &mut buf);
        let lines = text(&buf, area);
        assert!(lines[0].starts_with("Before"), "{lines:?}");
        assert!(lines[0].contains("After"));
        assert!(
            lines[1].starts_with("● cccccccc Four  [main]  drop"),
            "{lines:?}"
        );
        assert!(
            lines[1].contains("● aaaaaaaa Two  [main]  +1 combined"),
            "{lines:?}"
        );
        assert!(lines[2].starts_with("● bbbbbbbb Three  fixup"));
        // 後は Two 1 つなので、そのすぐ下が土台。前は 3 つの下。
        assert!(lines[2].ends_with("○ 99999999 One  unchanged"), "{lines:?}");
        assert_eq!(lines[4], "○ 99999999 One  unchanged");
    }

    /// reset のようにブランチを動かす操作: 前は外れるコミット、後は加わるコミット。ブランチは後のいちばん上に付き、
    /// 何も加わらなければ土台に付く。
    #[test]
    fn a_moving_branch() {
        let back = from_move(
            &json!({
                "lost": [{"oid": "bbbbbbbb1", "subject": "Two"}, {"oid": "aaaaaaaa1", "subject": "One"}],
                "lostMore": 0, "lostMoreAt": 0, "gained": [], "gainedMore": 0, "gainedMoreAt": 0,
                "base": {"oid": "99999999", "subject": "Start", "time": 0},
                "labels": [{"oid": "bbbbbbbb1", "name": "main", "kind": "local"}],
            }),
            "main",
        );
        let area = Rect::new(0, 0, 100, height(&back));
        let mut buf = Buffer::empty(area);
        draw(&back, area, &mut buf);
        let lines = text(&buf, area);
        assert!(
            lines[1].starts_with("● bbbbbbbb Two  [main]  leaves the branch"),
            "{lines:?}"
        );
        assert!(lines[1].contains("(no commits left)"), "{lines:?}");
        assert!(
            lines[2].ends_with("○ 99999999 Start  [main]  unchanged"),
            "{lines:?}"
        );
        assert_eq!(lines[3], "○ 99999999 Start  unchanged");
        // 前へ進む（加わるだけ）なら、前の側は土台だけで、そこにブランチ。
        let ahead = from_move(
            &json!({
                "lost": [], "lostMore": 0, "lostMoreAt": 0,
                "gained": [{"oid": "cccccccc1", "subject": "Three", "hidden": true}],
                "gainedMore": 0, "gainedMoreAt": 0,
                "base": {"oid": "99999999", "subject": "Start", "time": 0},
                "labels": [{"oid": "99999999", "name": "main", "kind": "local"}],
            }),
            "main",
        );
        let area = Rect::new(0, 0, 110, height(&ahead));
        let mut buf = Buffer::empty(area);
        draw(&ahead, area, &mut buf);
        let lines = text(&buf, area);
        assert!(
            lines[1].starts_with("○ 99999999 Start  [main]  unchanged"),
            "{lines:?}"
        );
        assert!(
            lines[1].contains("● cccccccc Three  [main]  only in the reflog"),
            "{lines:?}"
        );
        assert!(
            lines[2].ends_with("○ 99999999 Start  unchanged"),
            "{lines:?}"
        );
    }

    /// rebase: 前は載せ直すコミット、後はその写し（ID なし）が載せ先の上に。
    #[test]
    fn a_rebase() {
        let drawing = from_rebase(
            &json!({
                "picks": [{"oid": "aaaaaaaa1", "subject": "Mine 1"}, {"oid": "bbbbbbbb1", "subject": "Mine 2"}],
                "picksMore": 0, "picksMoreAt": 0,
                "onto": [{"oid": "cccccccc1", "subject": "Theirs"}], "ontoMore": 3, "ontoMoreAt": 1,
                "base": {"oid": "99999999", "subject": "Start", "time": 0},
                "labels": [{"oid": "bbbbbbbb1", "name": "topic", "kind": "local"},
                           {"oid": "cccccccc1", "name": "main", "kind": "local"}],
            }),
            "topic",
            "main",
        );
        let area = Rect::new(0, 0, 110, height(&drawing));
        let mut buf = Buffer::empty(area);
        draw(&drawing, area, &mut buf);
        let lines = text(&buf, area);
        assert!(
            lines[1].starts_with("● bbbbbbbb Mine 2  [topic]  leaves the branch"),
            "{lines:?}"
        );
        assert!(
            lines[1].contains("● Mine 2  [topic]  new copy"),
            "{lines:?}"
        );
        assert!(
            lines[2].starts_with("● aaaaaaaa Mine 1  leaves the branch"),
            "{lines:?}"
        );
        assert!(lines[3].contains("● cccccccc Theirs  on main"), "{lines:?}");
        assert!(lines[4].contains("┆ 3 more"), "{lines:?}");
        assert!(
            lines[5].ends_with("○ 99999999 Start  unchanged"),
            "{lines:?}"
        );
    }

    /// merge: 後のいちばん上に新しい merge コミット、その下に取り込むものと今のブランチのもの。
    #[test]
    fn a_merge() {
        let drawing = from_merge(
            &json!({
                "ours": [{"oid": "aaaaaaaa1", "subject": "Ours"}], "oursMore": 0, "oursMoreAt": 0,
                "theirs": [{"oid": "bbbbbbbb1", "subject": "Theirs"}], "theirsMore": 0, "theirsMoreAt": 0,
                "base": {"oid": "99999999", "subject": "Start", "time": 0},
                "labels": [{"oid": "aaaaaaaa1", "name": "main", "kind": "local"},
                           {"oid": "bbbbbbbb1", "name": "topic", "kind": "local"}],
                "subject": "Merge branch 'topic'",
            }),
            "main",
            "topic",
        );
        let area = Rect::new(0, 0, 110, height(&drawing));
        let mut buf = Buffer::empty(area);
        draw(&drawing, area, &mut buf);
        let lines = text(&buf, area);
        assert!(lines[1].starts_with("● aaaaaaaa Ours  [main]"), "{lines:?}");
        assert!(
            lines[1].contains("● Merge branch 'topic'  [main]  new merge"),
            "{lines:?}"
        );
        assert!(
            lines[2].contains("● bbbbbbbb Theirs  from topic"),
            "{lines:?}"
        );
        assert!(lines[3].contains("● aaaaaaaa Ours"), "{lines:?}");
    }

    /// cherry-pick・revert: 後は、今の先端の上に新しいコミット（ID なし）。ブランチはその上に。
    #[test]
    fn a_pick() {
        let drawing = from_pick(
            &json!({
                "head": [{"oid": "aaaaaaaa1", "subject": "Tip"}, {"oid": "99999999", "subject": "Parent"}],
                "picks": [{"oid": "bbbbbbbb1", "subject": "Revert \"Old\""}], "picksMore": 0, "picksMoreAt": 0,
                "labels": [{"oid": "aaaaaaaa1", "name": "main", "kind": "local"}],
            }),
            "main",
        );
        let area = Rect::new(0, 0, 100, height(&drawing));
        let mut buf = Buffer::empty(area);
        draw(&drawing, area, &mut buf);
        let lines = text(&buf, area);
        assert!(lines[1].starts_with("● aaaaaaaa Tip  [main]"), "{lines:?}");
        assert!(
            lines[1].contains("● Revert \"Old\"  [main]  new"),
            "{lines:?}"
        );
        assert!(lines[2].contains("● aaaaaaaa Tip"), "{lines:?}");
        assert!(
            lines[2].starts_with("○ 99999999 Parent  unchanged"),
            "{lines:?}"
        );
        assert!(
            lines[3].ends_with("○ 99999999 Parent  unchanged"),
            "{lines:?}"
        );
    }

    /// 長い側は、真ん中を省いた数を出す。
    #[test]
    fn elided_rows_say_how_many() {
        let drawing = json!({
            "before": [{"oid": "a", "subject": "New"}, {"oid": "b", "subject": "Old"}],
            "beforeMore": 5, "beforeMoreAt": 1,
            "after": [], "afterMore": 0, "afterMoreAt": 0,
            "base": null, "labels": [],
        });
        let area = Rect::new(0, 0, 60, height(&drawing));
        let mut buf = Buffer::empty(area);
        draw(&drawing, area, &mut buf);
        let lines = text(&buf, area);
        assert!(lines[2].starts_with("┆ 5 more"), "{lines:?}");
        assert!(lines[1].contains("(no commits left)"));
    }
}
