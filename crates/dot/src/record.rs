//! record・Mrecord のラベル（`{a|<p>b|{c|d}}`）: 読む、大きさ、ノードいっぱいへの広げ、ポートの場所。
//! 入れ子の `{}` ごとに並ぶ向きが変わる。いちばん外は横（rankdir が LR・RL なら縦）。

use crate::text::{Font, Names, Text, plain_label};

/// 欄のまわりの余白（横・縦）。
const MARGIN_X: f64 = 8.0;
const MARGIN_Y: f64 = 4.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub port: Option<String>,
    /// 葉の文字（入れ子の欄なら None）。
    pub text: Option<Text>,
    pub children: Vec<Field>,
    /// 子を横に並べる。
    pub horizontal: bool,
    /// ノードの中心からの左上と大きさ（`place` のあと）。
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 入れ子の深さの上限（再帰があふれないように）。
const MAX_DEPTH: usize = 64;

struct Reader<'a> {
    c: Vec<char>,
    i: usize,
    names: &'a Names<'a>,
    font: &'a Font,
}

impl Reader<'_> {
    /// `|` で区切った欄の並び（`}` か終わりまで）。
    fn list(&mut self, horizontal: bool, depth: usize) -> Option<Field> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut children = Vec::new();
        loop {
            children.push(self.field(horizontal, depth)?);
            match self.c.get(self.i) {
                Some('|') => self.i += 1,
                _ => break,
            }
        }
        Some(Field {
            port: None,
            text: None,
            children,
            horizontal,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        })
    }

    fn field(&mut self, horizontal: bool, depth: usize) -> Option<Field> {
        while self
            .c
            .get(self.i)
            .is_some_and(|c| *c == ' ' || *c == '\t' || *c == '\n')
        {
            self.i += 1;
        }
        if self.c.get(self.i) == Some(&'{') {
            self.i += 1;
            let inner = self.list(!horizontal, depth + 1)?;
            if self.c.get(self.i) != Some(&'}') {
                return None;
            }
            self.i += 1;
            while self.c.get(self.i).is_some_and(|c| c.is_whitespace()) {
                self.i += 1;
            }
            return Some(inner);
        }
        let mut port: Option<String> = None;
        let mut text = String::new();
        let mut in_port = false;
        let mut port_text = String::new();
        while let Some(&c) = self.c.get(self.i) {
            match c {
                '\\' => {
                    let next = self.c.get(self.i + 1).copied();
                    match next {
                        Some(n @ ('{' | '}' | '|' | '<' | '>')) => {
                            if in_port {
                                port_text.push(n);
                            } else {
                                text.push(n);
                            }
                            self.i += 2;
                        }
                        Some(' ') => {
                            text.push('\u{a0}');
                            self.i += 2;
                        }
                        Some(n) => {
                            if in_port {
                                port_text.push(n);
                            } else {
                                text.push('\\');
                                text.push(n);
                            }
                            self.i += 2;
                        }
                        None => {
                            text.push('\\');
                            self.i += 1;
                        }
                    }
                }
                '<' if !in_port => {
                    in_port = true;
                    self.i += 1;
                }
                '>' if in_port => {
                    in_port = false;
                    port = Some(port_text.trim().to_owned());
                    self.i += 1;
                }
                '|' | '}' => break,
                '{' => return None,
                c => {
                    if in_port {
                        port_text.push(c);
                    } else {
                        text.push(c);
                    }
                    self.i += 1;
                }
            }
        }
        if in_port {
            return None;
        }
        let trimmed = text.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\n');
        let label = plain_label(trimmed, self.names, self.font);
        Some(Field {
            port,
            text: Some(label),
            children: vec![],
            horizontal,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        })
    }
}

/// record のラベルを読む。書き方の誤りなら None（呼ぶ側は文字のまま 1 つの欄にする）。
pub fn parse(label: &str, horizontal: bool, names: &Names, font: &Font) -> Option<Field> {
    let mut r = Reader {
        c: label.chars().collect(),
        i: 0,
        names,
        font,
    };
    let root = r.list(horizontal, 0)?;
    if r.i != r.c.len() {
        return None;
    }
    Some(root)
}

/// 自然な大きさ（`w`・`h` に入れる）。
pub fn size(f: &mut Field, font_size: f64) -> (f64, f64) {
    if let Some(t) = &f.text {
        // アプリの書体が測り（Arial）より広くても欄の線に触れないように
        let w = t.fit_width() + 2.0 * MARGIN_X;
        let h = t.height.max(font_size * 1.2) + 2.0 * MARGIN_Y;
        f.w = w;
        f.h = h;
        return (w, h);
    }
    let (mut w, mut h) = (0.0f64, 0.0f64);
    for c in &mut f.children {
        let (cw, ch) = size(c, font_size);
        if f.horizontal {
            w += cw;
            h = h.max(ch);
        } else {
            w = w.max(cw);
            h += ch;
        }
    }
    f.w = w;
    f.h = h;
    (w, h)
}

/// (x, y) を左上に (w, h) いっぱいに置く。余りは子に等しく分ける。
pub fn place(f: &mut Field, x: f64, y: f64, w: f64, h: f64) {
    let extra = if f.horizontal { w - f.w } else { h - f.h };
    f.x = x;
    f.y = y;
    f.w = w;
    f.h = h;
    let n = f.children.len().max(1) as f64;
    let share = extra / n;
    let mut at = if f.horizontal { x } else { y };
    let horizontal = f.horizontal;
    for c in &mut f.children {
        if horizontal {
            let cw = c.w + share;
            place(c, at, y, cw, h);
            at += cw;
        } else {
            let ch = c.h + share;
            place(c, x, at, w, ch);
            at += ch;
        }
    }
}

/// ポートの欄（左上・大きさ）。
pub fn port<'a>(f: &'a Field, name: &str) -> Option<&'a Field> {
    if f.port.as_deref() == Some(name) {
        return Some(f);
    }
    f.children.iter().find_map(|c| port(c, name))
}

/// 区切りの線（x1, y1, x2, y2）。
pub fn separators(f: &Field, out: &mut Vec<[f64; 4]>) {
    for (i, c) in f.children.iter().enumerate() {
        if i > 0 {
            if f.horizontal {
                out.push([c.x, f.y, c.x, f.y + f.h]);
            } else {
                out.push([f.x, c.y, f.x + f.w, c.y]);
            }
        }
        separators(c, out);
    }
}

/// 葉の欄。
pub fn leaves<'a>(f: &'a Field, out: &mut Vec<&'a Field>) {
    if f.text.is_some() {
        out.push(f);
    }
    for c in &f.children {
        leaves(c, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(s: &str) -> Field {
        parse(s, true, &Names::default(), &Font::new(14.0)).unwrap()
    }

    fn texts(f: &Field) -> Vec<String> {
        let mut l = Vec::new();
        leaves(f, &mut l);
        l.iter()
            .map(|f| {
                f.text.as_ref().unwrap().lines[0]
                    .spans
                    .iter()
                    .map(|s| s.text.clone())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn reads_fields_and_ports() {
        let f = read("<f0> left|<f1> mid\\ dle|{<f2> a|b}| \\{x\\} ");
        assert!(f.horizontal);
        assert_eq!(f.children.len(), 4);
        assert_eq!(f.children[0].port.as_deref(), Some("f0"));
        assert!(!f.children[2].horizontal);
        assert_eq!(texts(&f), vec!["left", "mid\u{a0}dle", "a", "b", "{x}"]);
        assert!(port(&f, "f2").is_some() && port(&f, "zz").is_none());
        assert!(parse("{a|b", true, &Names::default(), &Font::new(14.0)).is_none());
        assert!(parse("a}b", true, &Names::default(), &Font::new(14.0)).is_none());
        assert!(parse("<p a", true, &Names::default(), &Font::new(14.0)).is_none());
        let deep = format!("{}a{}", "{".repeat(200), "}".repeat(200));
        assert!(parse(&deep, true, &Names::default(), &Font::new(14.0)).is_none());
    }

    #[test]
    fn sizes_and_places_fields() {
        let mut f = read("a|{b|c}");
        let (w, h) = size(&mut f, 14.0);
        assert!(w > 0.0 && h > 0.0);
        place(&mut f, -w / 2.0 - 10.0, -h / 2.0, w + 20.0, h);
        let total: f64 = f.children.iter().map(|c| c.w).sum();
        assert!((total - (w + 20.0)).abs() < 1e-9);
        let mut seps = Vec::new();
        separators(&f, &mut seps);
        assert_eq!(seps.len(), 2);
    }
}
