//! `PositionXmlParser`（名前空間を見ない SAX から作る DOM）に合わせた木。roxmltree で読み、Java の DOM と同じ
//! 見え方にする: 属性は名前の順（Xerces の `NamedNodeMap`）で `xmlns` も属性、名前は接頭辞つきのまま、行は要素の
//! `<` の行。
//!
//! roxmltree は宣言のない接頭辞を受けつけないので、そのときは根の要素に仮の宣言を足して読み直す（改行は足さない
//! ので行は変わらない）。

/// 要素の子。
#[derive(Clone, Debug)]
pub enum Child {
    Element(usize),
    Text { cdata: bool, value: String },
    Comment(String),
}

#[derive(Clone, Debug)]
pub struct Attr {
    pub name: String,
    pub value: String,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Child>,
    pub parent: Option<usize>,
    pub line: u32,
    /// 元の文での要素の範囲と、開始タグの終わり。
    range: std::ops::Range<usize>,
    start_tag_end: usize,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }

    /// `Element.getAttribute`: なければ空。
    pub fn get_attribute(&self, name: &str) -> &str {
        self.attr(name).unwrap_or("")
    }
}

pub struct Dom {
    pub elements: Vec<Element>,
    text: String,
}

impl Dom {
    pub fn parse(bytes: &[u8]) -> Result<Dom, ParseError> {
        let mut text = decode(bytes);
        let mut injected: Vec<String> = Vec::new();
        loop {
            let options = roxmltree::ParsingOptions {
                allow_dtd: true,
                nodes_limit: u32::MAX,
            };
            match roxmltree::Document::parse_with_options(&text, options) {
                Ok(doc) => return Ok(Dom::build(&doc, &injected)),
                Err(roxmltree::Error::UnknownNamespace(prefix, _)) if injected.len() < 64 => {
                    let prefix = prefix.clone();
                    let Some(at) = root_name_end(&text) else {
                        return Err(ParseError {
                            no_elements: false,
                            message: format!("unknown namespace prefix '{prefix}'"),
                        });
                    };
                    text.insert_str(at, &format!(" xmlns:{prefix}=\"urn:amonhen:{prefix}\""));
                    injected.push(format!("xmlns:{prefix}"));
                }
                Err(e) => {
                    return Err(ParseError {
                        no_elements: error_before_root(&text, e.pos()),
                        message: e.to_string(),
                    });
                }
            }
        }
    }

    fn build(doc: &roxmltree::Document, injected: &[String]) -> Dom {
        let text = doc.input_text().to_owned();
        let line_starts: Vec<usize> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let line_of = |pos: usize| line_starts.partition_point(|&s| s <= pos) as u32;
        let mut dom = Dom {
            elements: Vec::new(),
            text: String::new(),
        };
        let root = doc.root_element();
        dom.add(root, None, &text, &line_of, injected);
        dom.text = text;
        dom
    }

    fn add(
        &mut self,
        node: roxmltree::Node,
        parent: Option<usize>,
        text: &str,
        line_of: &dyn Fn(usize) -> u32,
        injected: &[String],
    ) -> usize {
        let range = node.range();
        let (xmlns, start_tag_end) = scan_start_tag(text, range.start);
        let mut attrs: Vec<Attr> = node
            .attributes()
            .map(|a| Attr {
                name: text.get(a.range_qname()).unwrap_or(a.name()).to_owned(),
                value: a.value().to_owned(),
                line: line_of(a.range().start),
            })
            .collect();
        for (name, value, pos) in xmlns {
            if !injected.contains(&name) {
                attrs.push(Attr {
                    name,
                    value,
                    line: line_of(pos),
                });
            }
        }
        attrs.sort_by(|a, b| super::java::compare(&a.name, &b.name));
        let tag = text
            .get(range.start + 1..)
            .map(|t| {
                t.split(|c: char| c.is_whitespace() || c == '/' || c == '>')
                    .next()
                    .unwrap_or("")
            })
            .filter(|t| !t.is_empty())
            .unwrap_or(node.tag_name().name())
            .to_owned();
        let index = self.elements.len();
        self.elements.push(Element {
            tag,
            attrs,
            children: Vec::new(),
            parent,
            line: line_of(range.start),
            range,
            start_tag_end,
        });
        let mut children = Vec::new();
        for child in node.children() {
            match child.node_type() {
                roxmltree::NodeType::Element => {
                    let c = self.add(child, Some(index), text, line_of, injected);
                    children.push(Child::Element(c));
                }
                roxmltree::NodeType::Text => {
                    let value = child.text().unwrap_or("");
                    if value.is_empty() {
                        continue;
                    }
                    // 処理命令をはさんだ文は Java では 1 つ
                    if let Some(Child::Text { value: prev, .. }) = children.last_mut() {
                        prev.push_str(value);
                    } else {
                        children.push(Child::Text {
                            cdata: false,
                            value: value.to_owned(),
                        });
                    }
                }
                roxmltree::NodeType::Comment => {
                    children.push(Child::Comment(child.text().unwrap_or("").to_owned()));
                }
                _ => {}
            }
        }
        self.elements[index].children = children;
        index
    }

    /// `getElementsByTagName(name)`（文書の順）。
    pub fn elements_by_tag_name(&self, name: &str) -> Vec<usize> {
        // 要素は前順で並んでいる
        (0..self.elements.len())
            .filter(|&i| self.elements[i].tag == name)
            .collect()
    }

    /// `<style>` の子を、Java の DOM のように文・CDATA・注釈に分けて返す（roxmltree は CDATA を文とつなぐ）。
    pub fn raw_children(&self, element: usize) -> Vec<Child> {
        let e = &self.elements[element];
        let mut out = Vec::new();
        let start = e.start_tag_end;
        if start >= e.range.end || self.text.as_bytes().get(start - 1) != Some(&b'>') {
            return out;
        }
        let Some(close) = self.text[start..e.range.end].rfind("</") else {
            return out;
        };
        let content = &self.text[start..start + close];
        let mut pending = String::new();
        let flush = |pending: &mut String, out: &mut Vec<Child>, cdata: bool| {
            if !pending.is_empty() {
                out.push(Child::Text {
                    cdata,
                    value: std::mem::take(pending),
                });
            }
        };
        let mut rest = content;
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix("<![CDATA[") {
                flush(&mut pending, &mut out, false);
                let end = r.find("]]>").unwrap_or(r.len());
                pending.push_str(&r[..end]);
                flush(&mut pending, &mut out, true);
                rest = r.get(end + 3..).unwrap_or("");
            } else if let Some(r) = rest.strip_prefix("<!--") {
                flush(&mut pending, &mut out, false);
                let end = r.find("-->").unwrap_or(r.len());
                out.push(Child::Comment(r[..end].to_owned()));
                rest = r.get(end + 3..).unwrap_or("");
            } else if let Some(r) = rest.strip_prefix("<?") {
                let end = r.find("?>").unwrap_or(r.len());
                rest = r.get(end + 2..).unwrap_or("");
            } else if rest.starts_with('<') {
                // 子の要素: 要素として数えるだけ
                flush(&mut pending, &mut out, false);
                let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
                if !rest.starts_with("</") {
                    out.push(Child::Comment(String::new()));
                }
                rest = &rest[end..];
            } else {
                let end = rest.find('<').unwrap_or(rest.len());
                pending.push_str(&unescape(&rest[..end]));
                rest = &rest[end..];
            }
        }
        flush(&mut pending, &mut out, false);
        out
    }
}

/// 読めない XML。Java は SAX のエラーを覚えて途中までの木で続けるので、文はまねられない。
pub struct ParseError {
    /// 根の開始タグを読み終える前に止まった（Java の木には要素がない）。
    pub no_elements: bool,
    pub message: String,
}

/// エラーの位置が、最初の開始タグの終わりより前か。
fn error_before_root(text: &str, pos: roxmltree::TextPos) -> bool {
    let line_start = text
        .split_inclusive('\n')
        .take(pos.row.saturating_sub(1) as usize)
        .map(str::len)
        .sum::<usize>();
    let offset = text[line_start..]
        .char_indices()
        .nth(pos.col.saturating_sub(1) as usize)
        .map(|(i, _)| line_start + i)
        .unwrap_or(text.len());
    match root_name_end(text) {
        Some(name_end) => offset < scan_start_tag(text, name_end.saturating_sub(1)).1,
        None => true,
    }
}

/// 文字コード: BOM を落とし、UTF-8 として読めなければ Latin-1。
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// 根の要素の名前の終わり（仮の宣言を入れる場所）。
fn root_name_end(text: &str) -> Option<usize> {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' {
            let next = b.get(i + 1).copied();
            if next == Some(b'?') {
                i = text[i..].find("?>").map(|j| i + j + 2)?;
                continue;
            }
            if next == Some(b'!') {
                if text[i..].starts_with("<!--") {
                    i = text[i..].find("-->").map(|j| i + j + 3)?;
                } else {
                    // DOCTYPE（内部サブセットの [] を飛ばす）
                    let mut depth = 0;
                    let mut j = i;
                    while j < b.len() {
                        match b[j] {
                            b'[' => depth += 1,
                            b']' => depth -= 1,
                            b'>' if depth == 0 => break,
                            _ => {}
                        }
                        j += 1;
                    }
                    i = j + 1;
                }
                continue;
            }
            let mut j = i + 1;
            while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'/' && b[j] != b'>' {
                j += 1;
            }
            return Some(j);
        }
        i += 1;
    }
    None
}

/// 開始タグの `xmlns` 属性（名前・値・位置）と、タグの終わり（`>` の次）。
fn scan_start_tag(text: &str, start: usize) -> (Vec<(String, String, usize)>, usize) {
    let b = text.as_bytes();
    let mut xmlns = Vec::new();
    let mut i = start + 1;
    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'/' && b[i] != b'>' {
        i += 1;
    }
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            return (xmlns, b.len());
        }
        if b[i] == b'>' {
            return (xmlns, i + 1);
        }
        if b[i] == b'/' {
            i += 1;
            continue;
        }
        let name_start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'>' {
            i += 1;
        }
        let name = &text[name_start..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let quote = b[i];
                let value_start = i + 1;
                i = value_start;
                while i < b.len() && b[i] != quote {
                    i += 1;
                }
                if name == "xmlns" || name.starts_with("xmlns:") {
                    xmlns.push((name.to_owned(), unescape(&text[value_start..i]), name_start));
                }
                i += 1;
            }
        }
    }
}

/// 文の実体参照（定義済みと文字参照）。
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let r = &rest[i..];
        let Some(end) = r.find(';') else {
            out.push_str(r);
            return out;
        };
        let name = &r[1..end];
        let c = match name {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => out.push(c),
            None => out.push_str(&r[..=end]),
        }
        rest = &r[end + 1..];
    }
    out.push_str(rest);
    out
}
