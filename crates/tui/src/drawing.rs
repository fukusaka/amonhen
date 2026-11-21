//! 書き換えの前と後の絵（アプリが当てる前に見せるもの）。エンジンの `rewritePreview` の `editDrawing` を、左に前、
//! 右に後の 2 列で、新しい順に描く。どちらの側も、いちばん下に書き換えないいちばん新しいコミット（共通の土台）を描く。

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
    let top = drawing["before"][0]["oid"].as_str().unwrap_or("");
    let mut lines = Vec::new();
    for (i, commit) in drawing[key].as_array().into_iter().flatten().enumerate() {
        if more > 0 && i == at {
            lines.push(elided(more));
        }
        let oid = commit["oid"].as_str().unwrap_or("");
        let action = commit["action"].as_str().unwrap_or("pick");
        let mut note = Vec::new();
        // ブランチは、後ではいちばん上の新しいコミットに付く（一緒に動く）。
        let labels = match (after, i) {
            (false, _) => labels_of(drawing, oid, false),
            (true, 0) => labels_of(drawing, top, true),
            (true, _) => String::new(),
        };
        if !labels.is_empty() {
            note.push(labels);
        }
        let style = match (after, action) {
            (false, "drop") => Style::new()
                .fg(Color::Red)
                .add_modifier(Modifier::CROSSED_OUT),
            (false, "squash" | "fixup") => Style::new().fg(Color::Indexed(244)),
            (false, _) => Style::new(),
            (true, _) => Style::new().fg(Color::Green),
        };
        if after {
            let combined = commit["combined"].as_u64().unwrap_or(0);
            if combined > 0 {
                note.push(tr!("+{count} combined", count = combined));
            }
            if action == "edit" {
                note.push(tr!("stops here"));
            }
        } else if action != "pick" {
            note.push(action.to_owned());
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
    // どちらの側も、書き換えない土台の上に載る。
    if let Some(base) = drawing["base"].as_object() {
        lines.push(Line {
            mark: "○",
            id: short(base.get("oid").and_then(Value::as_str).unwrap_or("")),
            subject: base
                .get("subject")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            note: tr!("unchanged"),
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
