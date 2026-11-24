//! Markdown の中の HTML を、タグと文字に分ける（小さく、壊れた HTML でも止まらない手書きのもの）。タグの名前と属性の
//! 名前は小文字にし、文字と属性の値の文字参照（`&amp;`・`&#39;` など）は戻す。コメント・`<!DOCTYPE>` などは落とし、
//! `<script>`・`<style>` などの中身は読まない（ひと続きの入力をまたいでも）。

/// 1 つのタグの属性の数の上限（それより後は読み飛ばす）。
const MAX_ATTRIBUTES: usize = 32;
/// 属性の値の長さの上限（バイト。超えた属性は落とす）。
pub(super) const MAX_VALUE: usize = 4096;
/// タグの名前・属性の名前の長さの上限（超えたタグは文字のまま、属性は落とす）。
const MAX_NAME: usize = 32;

/// タグの属性（名前と値）。
pub(super) type Attributes = Vec<(String, String)>;

/// タグか文字。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Token {
    Open {
        name: String,
        attributes: Attributes,
    },
    Close {
        name: String,
    },
    Text(String),
}

/// 中身を読まない要素（中身は文字としても出さない）。
const RAW: [&str; 11] = [
    "script",
    "style",
    "textarea",
    "title",
    "iframe",
    "noscript",
    "noembed",
    "noframes",
    "xmp",
    "plaintext",
    "template",
];

/// 分ける人。`<script>` などの中にいるかを、入力をまたいで覚えておく。
#[derive(Default)]
pub(super) struct Tokenizer {
    /// 中身を読まない要素の中なら、その名前（閉じのタグまで読み飛ばす）。
    raw: Option<String>,
}

impl Tokenizer {
    /// 中身を読まない要素の中か（その間の Markdown の文字も出さない）。
    pub(super) fn in_raw(&self) -> bool {
        self.raw.is_some()
    }

    /// 中身を読まない要素を出る（Markdown のブロックが閉じたとき。閉じ忘れで残りがすべて消えないように）。
    pub(super) fn leave_raw(&mut self) {
        self.raw = None;
    }

    /// `input` を分けて `out` に足す。
    pub(super) fn feed(&mut self, input: &str, out: &mut Vec<Token>) {
        let bytes = input.as_bytes();
        let mut i = 0;
        let mut text = String::new();
        let flush = |text: &mut String, out: &mut Vec<Token>| {
            if !text.is_empty() {
                out.push(Token::Text(decode_entities(text)));
                text.clear();
            }
        };
        while i < bytes.len() {
            if let Some(name) = &self.raw {
                // 閉じのタグまで読み飛ばす。
                match find_close(&input[i..], name) {
                    Some((_, end)) => {
                        out.push(Token::Close { name: name.clone() });
                        self.raw = None;
                        i += end;
                    }
                    None => i = bytes.len(),
                }
                continue;
            }
            let Some(lt) = input[i..].find('<') else {
                text.push_str(&input[i..]);
                break;
            };
            text.push_str(&input[i..i + lt]);
            i += lt;
            let rest = &input[i..];
            if let Some(after) = rest.strip_prefix("<!--") {
                // コメントは落とす（閉じていなければ残りすべて）。
                flush(&mut text, out);
                i += 4 + after.find("-->").map_or(after.len(), |e| e + 3);
                continue;
            }
            if rest.starts_with("<!") || rest.starts_with("<?") {
                // `<!DOCTYPE …>`・`<![CDATA[…]]>`・`<?…?>` は落とす。
                flush(&mut text, out);
                i += rest.find('>').map_or(rest.len(), |e| e + 1);
                continue;
            }
            if let Some(after) = rest.strip_prefix("</") {
                if after
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphabetic)
                {
                    let name_len = after
                        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                        .unwrap_or(after.len());
                    if let Some(gt) = after[name_len..].find('>')
                        && name_len <= MAX_NAME
                    {
                        flush(&mut text, out);
                        out.push(Token::Close {
                            name: after[..name_len].to_ascii_lowercase(),
                        });
                        i += 2 + name_len + gt + 1;
                        continue;
                    }
                }
                text.push('<');
                i += 1;
                continue;
            }
            match open_tag(rest) {
                Some((name, attributes, len)) => {
                    flush(&mut text, out);
                    if RAW.contains(&name.as_str()) {
                        self.raw = Some(name.clone());
                    }
                    out.push(Token::Open { name, attributes });
                    i += len;
                }
                None => {
                    // タグにならない `<` は文字のまま。
                    text.push('<');
                    i += 1;
                }
            }
        }
        flush(&mut text, out);
    }
}

/// `</name` の閉じのタグを探す（大文字小文字は問わない）。見つかれば、その始まりと `>` の後の位置。
fn find_close(input: &str, name: &str) -> Option<(usize, usize)> {
    let bytes = input.as_bytes();
    let mut from = 0;
    while let Some(p) = input[from..].find("</") {
        let start = from + p;
        let after = start + 2;
        let end = after + name.len();
        if end <= bytes.len()
            && bytes[after..end].eq_ignore_ascii_case(name.as_bytes())
            && bytes
                .get(end)
                .is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'-'))
        {
            let close = input[end..].find('>').map_or(bytes.len(), |g| end + g + 1);
            return Some((start, close));
        }
        from = after;
    }
    None
}

/// 開きのタグ（`<name attr="v" …>` か `<name … />`）を読む。名前・属性・長さ。タグにならなければ None。
fn open_tag(input: &str) -> Option<(String, Attributes, usize)> {
    let bytes = input.as_bytes();
    if !bytes.get(1)?.is_ascii_alphabetic() {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    if i - 1 > MAX_NAME {
        return None;
    }
    let name = input[1..i].to_ascii_lowercase();
    let mut attributes = Vec::new();
    loop {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        match bytes.get(i) {
            None => return None,
            Some(b'>') => return Some((name, attributes, i + 1)),
            _ => {}
        }
        // 属性の名前。
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b'=' | b'>' | b'/')
        {
            i += 1;
        }
        if i == start {
            // `=` だけが続くなど、名前にならない所は 1 文字飛ばす。
            i += 1;
            continue;
        }
        let attr = input[start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if bytes.get(i) == Some(&b'=') {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            match bytes.get(i) {
                Some(&q @ (b'"' | b'\'')) => {
                    let end = input[i + 1..].find(q as char)? + i + 1;
                    value = input[i + 1..end].to_owned();
                    i = end + 1;
                }
                Some(_) => {
                    let start = i;
                    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                        i += 1;
                    }
                    value = input[start..i].to_owned();
                }
                None => return None,
            }
        }
        if attributes.len() < MAX_ATTRIBUTES
            && attr.len() <= MAX_NAME
            && value.len() <= MAX_VALUE
            && !attributes.iter().any(|(a, _)| *a == attr)
        {
            attributes.push((attr, decode_entities(&value)));
        }
    }
}

/// 名前つきの文字参照（よく使うもの）。
const ENTITIES: [(&str, &str); 27] = [
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", "\u{a0}"),
    ("copy", "©"),
    ("reg", "®"),
    ("trade", "™"),
    ("hellip", "…"),
    ("mdash", "—"),
    ("ndash", "–"),
    ("laquo", "«"),
    ("raquo", "»"),
    ("lsquo", "‘"),
    ("rsquo", "’"),
    ("ldquo", "“"),
    ("rdquo", "”"),
    ("middot", "·"),
    ("bull", "•"),
    ("times", "×"),
    ("divide", "÷"),
    ("deg", "°"),
    ("plusmn", "±"),
    ("larr", "←"),
    ("rarr", "→"),
    ("check", "✓"),
];

/// 文字参照を戻す（知らない名前・壊れたものはそのまま）。
pub(super) fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let semi = after.find(';').filter(|&s| s <= 10);
        let decoded = semi.and_then(|s| {
            let name = &after[..s];
            if let Some(number) = name.strip_prefix('#') {
                let code = match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => number.parse::<u32>().ok(),
                }?;
                // NUL と、文字にならない数は置き換えの文字。
                let c = char::from_u32(code)
                    .filter(|&c| c != '\0')
                    .unwrap_or('\u{fffd}');
                Some((c.to_string(), s))
            } else {
                ENTITIES
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| ((*v).to_owned(), s))
            }
        });
        match decoded {
            Some((value, s)) => {
                out.push_str(&value);
                rest = &after[s + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: &str) -> Vec<Token> {
        let mut out = Vec::new();
        Tokenizer::default().feed(input, &mut out);
        out
    }

    fn open(name: &str, attributes: &[(&str, &str)]) -> Token {
        Token::Open {
            name: name.into(),
            attributes: attributes
                .iter()
                .map(|(a, v)| ((*a).into(), (*v).into()))
                .collect(),
        }
    }

    fn close(name: &str) -> Token {
        Token::Close { name: name.into() }
    }

    fn text(t: &str) -> Token {
        Token::Text(t.into())
    }

    #[test]
    fn reads_tags_with_and_without_quotes() {
        assert_eq!(
            tokens(r#"<P Align="center" x='a"b' w=100 open><IMG src=a.png/>t</p>"#),
            vec![
                open(
                    "p",
                    &[
                        ("align", "center"),
                        ("x", "a\"b"),
                        ("w", "100"),
                        ("open", "")
                    ]
                ),
                open("img", &[("src", "a.png/")]),
                text("t"),
                close("p"),
            ]
        );
        assert_eq!(
            tokens("<br/><img src=\"x\" />"),
            vec![open("br", &[]), open("img", &[("src", "x")])]
        );
        // 同じ属性は最初のもの。
        assert_eq!(
            tokens("<a href=1 href=2>"),
            vec![open("a", &[("href", "1")])]
        );
    }

    #[test]
    fn keeps_broken_markup_as_text() {
        assert_eq!(tokens("a < b"), vec![text("a < b")]);
        assert_eq!(tokens("x <img src=\"a"), vec![text("x <img src=\"a")]);
        assert_eq!(tokens("1 </ 2"), vec![text("1 </ 2")]);
        assert_eq!(tokens("<p"), vec![text("<p")]);
        assert_eq!(tokens("<3 <>"), vec![text("<3 <>")]);
        // 長すぎる名前はタグにしない。
        let long = format!("<{}>", "a".repeat(40));
        assert_eq!(tokens(&long), vec![text(&long)]);
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(
            tokens("a &amp; b &lt;c&gt; &#39;&#x41;&nbsp;&unknown; &amp &#xD800; &#0;"),
            vec![text("a & b <c> 'A\u{a0}&unknown; &amp \u{fffd} \u{fffd}")]
        );
        assert_eq!(
            tokens("<a title=\"&quot;x&quot;\">"),
            vec![open("a", &[("title", "\"x\"")])]
        );
    }

    #[test]
    fn drops_comments_and_declarations() {
        assert_eq!(
            tokens("a<!-- <b>hidden</b> -->b<!DOCTYPE html><?x?>c<!-- open"),
            vec![text("a"), text("b"), text("c")]
        );
    }

    #[test]
    fn skips_raw_text_elements_across_inputs() {
        let mut t = Tokenizer::default();
        let mut out = Vec::new();
        t.feed("a<script>if (a<b) { x(\"</p>\") }", &mut out);
        assert!(t.in_raw());
        t.feed("more</SCRIPT >b<style>p{}</style>c", &mut out);
        assert!(!t.in_raw());
        assert_eq!(
            out,
            vec![
                text("a"),
                open("script", &[]),
                close("script"),
                text("b"),
                open("style", &[]),
                close("style"),
                text("c"),
            ]
        );
        t.feed("<textarea>x", &mut out);
        t.leave_raw();
        assert!(!t.in_raw());
    }

    #[test]
    fn bounds_attributes() {
        let many: String = (0..100).map(|i| format!(" a{i}=x")).collect();
        let Token::Open { attributes, .. } = &tokens(&format!("<p{many}>"))[0] else {
            panic!()
        };
        assert_eq!(attributes.len(), MAX_ATTRIBUTES);
        let long = format!("<img src=\"{}\" alt=a>", "x".repeat(MAX_VALUE + 1));
        assert_eq!(tokens(&long), vec![open("img", &[("alt", "a")])]);
    }
}
