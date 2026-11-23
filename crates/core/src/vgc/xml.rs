//! XML のイベント（package:xml の `parseEvents` と同じ切り方）。vgc は XML を木にせず、イベントを先頭から順に読む。
//! package:xml のイベントの読み手は寛容で、名前空間の接頭辞を確かめず、DTD の実体を展開せず（`&name;` は既定の 5 つと
//! 数値の参照だけを戻し、ほかはそのまま残す）、属性の値の空白を正規化せず、開始と終了の対応も確かめない。roxmltree は
//! これらを XML の仕様どおりに扱って vgc と違う結果（またはエラー）になるので、ここでは自前で読む。
//! 読み手は遅延で、壊れた所に来たときに初めて失敗する。

use super::dart::{Error, R, is_ws};

/// XML のイベント。
#[derive(Clone, Debug, PartialEq)]
pub enum Event<'a> {
    /// 開始タグ。属性は（修飾付きの名前、実体を戻した値）。
    Start {
        name: &'a str,
        attributes: Vec<(&'a str, String)>,
        self_closing: bool,
    },
    End {
        name: &'a str,
    },
    /// 文字（実体を戻したもの）。
    Text(String),
    /// CDATA の中身。
    CData(&'a str),
    /// コメント・XML 宣言・処理命令・DOCTYPE。
    Other,
}

/// 名前の局所部（最初の `:` より後）。
pub fn local_name(name: &str) -> &str {
    match name.find(':') {
        Some(i) if i > 0 => &name[i + 1..],
        _ => name,
    }
}

/// イベントを順に返す。
pub struct Events<'a> {
    s: &'a str,
    pos: usize,
    done: bool,
}

impl<'a> Events<'a> {
    pub fn new(s: &'a str) -> Self {
        Self {
            s,
            pos: 0,
            done: false,
        }
    }

    /// 次のイベント。終わりなら `None`、壊れていればエラー（以後は `None`）。
    pub fn next_event(&mut self) -> R<Option<Event<'a>>> {
        if self.done || self.pos >= self.s.len() {
            self.done = true;
            return Ok(None);
        }
        match self.event() {
            Some((event, end)) => {
                self.pos = end;
                Ok(Some(event))
            }
            None => {
                self.done = true;
                // petitparser はいちばん先まで読めた所の失敗を報せる。開始タグなら属性の後の `>` の所
                let (at, message) = self
                    .start_failure(self.pos)
                    .unwrap_or((self.pos, "invalid XML".to_owned()));
                let (line, column) = line_and_column(self.s, at);
                Err(Error(format!(
                    "XmlParserException: {message} at {line}:{column}"
                )))
            }
        }
    }

    fn event(&self) -> Option<(Event<'a>, usize)> {
        let s = self.s;
        let p = self.pos;
        let rest = &s[p..];
        if !rest.starts_with('<') {
            let end = rest.find('<').map_or(s.len(), |i| p + i);
            return Some((Event::Text(decode(&s[p..end])), end));
        }
        if let Some(r) = self.start_element(p) {
            return Some(r);
        }
        if let Some(r) = self.end_element(p) {
            return Some(r);
        }
        if let Some(body) = rest.strip_prefix("<!--") {
            let i = body.find("-->")?;
            return Some((Event::Other, p + 4 + i + 3));
        }
        if let Some(body) = rest.strip_prefix("<![CDATA[") {
            let i = body.find("]]>")?;
            return Some((Event::CData(&body[..i]), p + 9 + i + 3));
        }
        if rest.starts_with("<?") {
            return self.processing(p);
        }
        if rest.starts_with("<!DOCTYPE") {
            return self.doctype(p);
        }
        None
    }

    fn start_element(&self, p: usize) -> Option<(Event<'a>, usize)> {
        let s = self.s;
        let mut i = p + 1;
        let name_end = name_token(s, i)?;
        let name = &s[i..name_end];
        i = name_end;
        let mut attributes = Vec::new();
        loop {
            let after_space = skip_ws(s, i);
            if after_space == i {
                break;
            }
            let Some((attribute, end)) = attribute(s, after_space) else {
                break;
            };
            attributes.push(attribute);
            i = end;
        }
        i = skip_ws(s, i);
        let rest = &s[i..];
        if rest.starts_with('>') {
            Some((
                Event::Start {
                    name,
                    attributes,
                    self_closing: false,
                },
                i + 1,
            ))
        } else if rest.starts_with("/>") {
            Some((
                Event::Start {
                    name,
                    attributes,
                    self_closing: true,
                },
                i + 2,
            ))
        } else {
            None
        }
    }

    /// 名前まで読めた開始タグが閉じなかった位置。
    fn start_failure(&self, p: usize) -> Option<(usize, String)> {
        let s = self.s;
        if !s[p..].starts_with('<') {
            return None;
        }
        let mut i = name_token(s, p + 1)?;
        loop {
            let after_space = skip_ws(s, i);
            if after_space == i {
                break;
            }
            let Some((_, end)) = attribute(s, after_space) else {
                break;
            };
            i = end;
        }
        Some((skip_ws(s, i), "\">\" expected".to_owned()))
    }

    fn end_element(&self, p: usize) -> Option<(Event<'a>, usize)> {
        let s = self.s;
        if !s[p..].starts_with("</") {
            return None;
        }
        let start = p + 2;
        let end = name_token(s, start)?;
        let i = skip_ws(s, end);
        s[i..].starts_with('>').then(|| {
            (
                Event::End {
                    name: &s[start..end],
                },
                i + 1,
            )
        })
    }

    /// `<?xml …?>` と処理命令（`<?名前 …?>`）。
    fn processing(&self, p: usize) -> Option<(Event<'a>, usize)> {
        let s = self.s;
        let name_end = name_token(s, p + 2)?;
        let mut i = name_end;
        let after_space = skip_ws(s, i);
        if after_space > i {
            let j = s[after_space..].find("?>")?;
            i = after_space + j;
        }
        s[i..].starts_with("?>").then_some((Event::Other, i + 2))
    }

    /// `<!DOCTYPE 名前 [外部 ID] [内部サブセット]>`。
    fn doctype(&self, p: usize) -> Option<(Event<'a>, usize)> {
        let s = self.s;
        let b = s.as_bytes();
        let mut i = p + "<!DOCTYPE".len();
        let after = skip_ws(s, i);
        if after == i {
            return None;
        }
        i = name_token(s, after)?;
        // 外部 ID（SYSTEM "…" か PUBLIC "…" "…"）
        let after = skip_ws(s, i);
        if after > i {
            let rest = &s[after..];
            let literals = if rest.starts_with("SYSTEM") {
                Some(("SYSTEM".len(), 1))
            } else if rest.starts_with("PUBLIC") {
                Some(("PUBLIC".len(), 2))
            } else {
                None
            };
            if let Some((keyword, count)) = literals {
                let mut j = after + keyword;
                let mut ok = true;
                for _ in 0..count {
                    let k = skip_ws(s, j);
                    if k == j {
                        ok = false;
                        break;
                    }
                    match quoted(s, k) {
                        Some(end) => j = end,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    i = j;
                }
            }
        }
        i = skip_ws(s, i);
        if b.get(i) == Some(&b'[') {
            i += 1;
            loop {
                let rest = &s[i..];
                if rest.is_empty() {
                    return None;
                }
                if rest.starts_with(']') {
                    i += 1;
                    break;
                }
                if let Some(body) = rest.strip_prefix("<!--") {
                    i += 4 + body.find("-->")? + 3;
                } else if rest.starts_with("<?") {
                    i += rest.find("?>")? + 2;
                } else if rest.starts_with("<!") {
                    // 宣言: 引用の外の `>` まで
                    let mut j = i + 2;
                    loop {
                        match b.get(j) {
                            None => return None,
                            Some(b'>') => break,
                            Some(b'"' | b'\'') => j = quoted(s, j)?,
                            Some(_) => j += 1,
                        }
                    }
                    i = j + 1;
                } else {
                    i += rest.chars().next().map_or(1, char::len_utf8);
                }
            }
            i = skip_ws(s, i);
        }
        (b.get(i) == Some(&b'>')).then_some((Event::Other, i + 1))
    }
}

/// 属性（名前、空白、`=`、空白、引用した値）。値のない属性（`<svg foo>`）は空の値。
fn attribute(s: &str, p: usize) -> Option<((&str, String), usize)> {
    let name_end = name_token(s, p)?;
    let mut i = skip_ws(s, name_end);
    if !s[i..].starts_with('=') {
        return Some(((&s[p..name_end], String::new()), name_end));
    }
    i = skip_ws(s, i + 1);
    let Some(end) = quoted(s, i) else {
        return Some(((&s[p..name_end], String::new()), name_end));
    };
    Some(((&s[p..name_end], decode(&s[i + 1..end - 1])), end))
}

/// `"…"` か `'…'`。閉じた引用の次の位置。
fn quoted(s: &str, p: usize) -> Option<usize> {
    let q = *s.as_bytes().get(p)?;
    if q != b'"' && q != b'\'' {
        return None;
    }
    let i = s[p + 1..].find(q as char)?;
    Some(p + 1 + i + 1)
}

fn skip_ws(s: &str, mut i: usize) -> usize {
    while let Some(c) = s[i..].chars().next() {
        if !is_ws(c) {
            break;
        }
        i += c.len_utf8();
    }
    i
}

fn is_name_start(c: char) -> bool {
    matches!(c,
        ':' | 'A'..='Z' | '_' | 'a'..='z'
        | '\u{c0}'..='\u{d6}'
        | '\u{d8}'..='\u{f6}'
        | '\u{f8}'..='\u{2ff}'
        | '\u{370}'..='\u{37d}'
        | '\u{37f}'..='\u{1fff}'
        | '\u{200c}'..='\u{200d}'
        | '\u{2070}'..='\u{218f}'
        | '\u{2c00}'..='\u{2fef}'
        | '\u{3001}'..='\u{d7ff}'
        | '\u{f900}'..='\u{fdcf}'
        | '\u{fdf0}'..='\u{fffd}')
}

fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || matches!(c, '-' | '.' | '0'..='9' | '\u{b7}' | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}')
}

/// 名前の終わり（名前でなければ `None`）。
fn name_token(s: &str, p: usize) -> Option<usize> {
    let mut chars = s[p..].char_indices();
    match chars.next() {
        Some((_, c)) if is_name_start(c) => {}
        _ => return None,
    }
    for (i, c) in chars {
        if !is_name_char(c) {
            return Some(p + i);
        }
    }
    Some(s.len())
}

/// 実体の参照を戻す（`XmlDefaultEntityMapping.xml()`）。知らない名前はそのまま。
pub fn decode(input: &str) -> String {
    let Some(mut index) = input.find('&') else {
        return input.to_owned();
    };
    let mut out = String::with_capacity(input.len());
    out.push_str(&input[..index]);
    loop {
        let end = input[index + 1..].find(';').map(|e| index + 1 + e);
        match end {
            Some(end) if index + 1 < end => match decode_entity(&input[index + 1..end]) {
                Some(value) => {
                    out.push_str(&value);
                    index = end + 1;
                }
                None => {
                    out.push('&');
                    index += 1;
                }
            },
            _ => {
                out.push('&');
                index += 1;
            }
        }
        match input[index..].find('&') {
            None => {
                out.push_str(&input[index..]);
                break;
            }
            Some(n) => {
                out.push_str(&input[index..index + n]);
                index += n;
            }
        }
    }
    out
}

fn decode_entity(name: &str) -> Option<String> {
    let b = name.as_bytes();
    if b.len() > 1 && b[0] == b'#' {
        let (digits, radix) = if b.len() > 2 && (b[1] == b'x' || b[1] == b'X') {
            (&name[2..], 16)
        } else {
            (&name[1..], 10)
        };
        let value = super::dart::try_parse_int(digits, radix)?;
        if !(0..=0x10ffff).contains(&value) {
            return None;
        }
        // サロゲートは UTF-8 にすると U+FFFD になる
        return Some(
            char::from_u32(value as u32)
                .unwrap_or('\u{fffd}')
                .to_string(),
        );
    }
    let c = match name {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "apos" => "'",
        "quot" => "\"",
        _ => return None,
    };
    Some(c.to_owned())
}

fn line_and_column(s: &str, pos: usize) -> (usize, usize) {
    let before = &s[..pos];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let column = before[line_start..].encode_utf16().count() + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(s: &str) -> Vec<Event<'_>> {
        let mut events = Events::new(s);
        let mut out = Vec::new();
        while let Some(e) = events.next_event().unwrap() {
            out.push(e);
        }
        out
    }

    #[test]
    fn reads_events() {
        let e = all(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE svg PUBLIC \"a\" \"b\" [<!ENTITY x \"]>\">]>\
             <svg a = 'x&amp;y&nbsp;' xlink:href=\"#a\n\"><!-- c --><t>a&lt;<![CDATA[<b>]]></t><g/></svg>",
        );
        assert_eq!(e[0], Event::Other);
        assert_eq!(e[1], Event::Text("\n".into()));
        assert_eq!(e[2], Event::Other);
        assert_eq!(
            e[3],
            Event::Start {
                name: "svg",
                attributes: vec![("a", "x&y&nbsp;".into()), ("xlink:href", "#a\n".into())],
                self_closing: false
            }
        );
        assert_eq!(e[4], Event::Other);
        assert_eq!(e[6], Event::Text("a<".into()));
        assert_eq!(e[7], Event::CData("<b>"));
        assert_eq!(e[8], Event::End { name: "t" });
        assert_eq!(local_name("xlink:href"), "href");
        assert_eq!(decode("&#65;&#x42;&bad;&"), "AB&bad;&");
        // 値のない属性は空の値
        let mut events = Events::new("<a b c=1>");
        assert_eq!(
            events.next_event().unwrap_err().0,
            "XmlParserException: \">\" expected at 1:7"
        );
        let e = all("<a b>");
        assert_eq!(
            e[0],
            Event::Start {
                name: "a",
                attributes: vec![("b", String::new())],
                self_closing: false
            }
        );
    }
}
