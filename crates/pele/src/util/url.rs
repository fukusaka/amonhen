//! リンクと画像の URL の確かめ（pele の `src/util/url.ts`）。
//!
//! http(s) の URL は JS では `new URL()` で読み直して書く（WHATWG URL）。ここではその主なところ（スキームとホストの
//! 小文字化、IDNA の punycode、既定のポートを落とす、`.`・`..` の段、パス・クエリ・断片の % の符号化）を写した。
//! IPv4・IPv6 のホストの数の書き換えなど、珍しいところは写していない。

use crate::js;
use crate::svg::builder::{esc, invalid_xml};
use crate::text::entities::decode_entities;
use crate::types::{LinkInfo, RenderOptions};

const BLANK: &str = "about:blank";

/// `/[\u0000-\u001F\u007F-\u009F -‍﻿\uD800-\uDFFF￾￿]/`
fn is_ctrl(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' | '\u{2000}'..='\u{200d}' | '\u{feff}' | '\u{fffe}' | '\u{ffff}')
}

/// `decodeURIComponent`。正しくない % の並びは Err。
fn decode_uri_component(s: &str) -> Result<String, ()> {
    if !s.contains('%') {
        return Ok(s.to_string());
    }
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'%' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let hex = |k: usize| -> Result<u8, ()> {
            let h = b.get(k..k + 2).ok_or(())?;
            let h = std::str::from_utf8(h).map_err(|_| ())?;
            u8::from_str_radix(h, 16).map_err(|_| ())
        };
        let first = hex(i + 1)?;
        if first < 0x80 {
            out.push(first);
            i += 3;
            continue;
        }
        let n = if first & 0xe0 == 0xc0 {
            2
        } else if first & 0xf0 == 0xe0 {
            3
        } else if first & 0xf8 == 0xf0 {
            4
        } else {
            return Err(());
        };
        let mut bytes = vec![first];
        let mut k = i + 3;
        for _ in 1..n {
            if b.get(k) != Some(&b'%') {
                return Err(());
            }
            bytes.push(hex(k + 1)?);
            k += 3;
        }
        let ch = std::str::from_utf8(&bytes).map_err(|_| ())?;
        out.extend_from_slice(ch.as_bytes());
        i = k;
    }
    String::from_utf8(out).map_err(|_| ())
}

fn decode(uri: &str) -> String {
    decode_uri_component(uri).unwrap_or_else(|_| uri.to_string())
}

/// `/&#(\w+)(^\w|;)?/g` を `String.fromCharCode(dec)` に。
fn replace_html_entities(s: &str) -> (String, bool) {
    let mut out = String::new();
    let mut found = false;
    let mut rest = s;
    while let Some(k) = rest.find("&#") {
        let after = &rest[k + 2..];
        let n = after
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
            .count();
        if n == 0 {
            out.push_str(&rest[..k + 2]);
            rest = after;
            continue;
        }
        found = true;
        out.push_str(&rest[..k]);
        let code = js::to_int32(js::number(&after[..n])) as u32 & 0xffff;
        out.push_str(&js::from16(&[code as u16]));
        let mut end = n;
        if after[n..].starts_with(';') {
            end += 1;
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    (out, found)
}

/// `/&(newline|tab);/gi` を消す。
fn drop_ctrl_entities(s: &str) -> (String, bool) {
    let mut out = String::new();
    let mut found = false;
    let mut i = 0;
    let lower = s.to_ascii_lowercase();
    while i < s.len() {
        if lower[i..].starts_with("&newline;") {
            i += 9;
            found = true;
        } else if lower[i..].starts_with("&tab;") {
            i += 5;
            found = true;
        } else {
            let c = s[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    (out, found)
}

/// `/(\\|%5[cC])((%(6[eE]|72|74))|[nrt])/g` を消す。
fn drop_ws_escapes(s: &str) -> (String, bool) {
    let mut out = String::new();
    let mut found = false;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let lead = if b[i] == b'\\' {
            1
        } else if b[i] == b'%'
            && b.get(i + 1) == Some(&b'5')
            && matches!(b.get(i + 2), Some(b'c' | b'C'))
        {
            3
        } else {
            0
        };
        if lead > 0 {
            let j = i + lead;
            let tail = if matches!(b.get(j), Some(b'n' | b'r' | b't')) {
                1
            } else if b.get(j) == Some(&b'%')
                && ((b.get(j + 1) == Some(&b'6') && matches!(b.get(j + 2), Some(b'e' | b'E')))
                    || (b.get(j + 1) == Some(&b'7') && matches!(b.get(j + 2), Some(b'2' | b'4'))))
            {
                3
            } else {
                0
            };
            if tail > 0 {
                found = true;
                i = j + tail;
                continue;
            }
        }
        let c = s[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    (out, found)
}

fn drop_ctrl(s: &str) -> String {
    s.chars().filter(|c| !is_ctrl(*c)).collect()
}

/// スキームを隠しうる制御文字・文字参照・逃がしを除く。`percent` なら % の符号も。包みが多すぎて本物の URL で
/// ありえないものは None。
fn unwrap(url: &str, percent: bool) -> Option<String> {
    let mut out = if percent {
        decode(js::trim(url))
    } else {
        js::trim(url).to_string()
    };
    let mut rounds = 0;
    loop {
        rounds += 1;
        if rounds > 32 {
            return None;
        }
        let a = drop_ctrl(&out);
        let (a, _) = replace_html_entities(&a);
        let (a, _) = drop_ctrl_entities(&a);
        let a = drop_ctrl(&a);
        let (a, _) = drop_ws_escapes(&a);
        out = js::trim(&a).to_string();
        if percent {
            out = decode(&out);
        }
        let more = out.chars().any(is_ctrl)
            || replace_html_entities(&out).1
            || drop_ctrl_entities(&out).1
            || drop_ws_escapes(&out).1;
        if !more {
            return Some(out);
        }
    }
}

/// `/^.+(:|&colon;)/m` の最初の当たり（行の頭から、その行の最後の `:` か `&colon;` まで）。
fn scheme_match(s: &str) -> Option<String> {
    for line in s.split(['\n', '\r', '\u{2028}', '\u{2029}']) {
        let mut best: Option<usize> = None;
        for (k, _) in line.char_indices() {
            if k == 0 {
                continue;
            }
            if line[k..].starts_with(':') {
                best = Some(k + 1);
            } else if line[k..].starts_with("&colon;") {
                best = Some(k + 7);
            }
        }
        if let Some(e) = best {
            return Some(line[..e].to_string());
        }
    }
    None
}

/// `/^([^\w]*)(javascript|data|vbscript)/im`
fn invalid_protocol(s: &str) -> bool {
    s.split(['\n', '\r', '\u{2028}', '\u{2029}']).any(|line| {
        let rest = line.trim_start_matches(|c: char| !crate::util::chars::is_word_char(c));
        let l = rest.to_lowercase();
        l.starts_with("javascript") || l.starts_with("data") || l.starts_with("vbscript")
    })
}

/// @braintree/sanitize-url が拒むもの（Mermaid がクリックの先に当てる確かめ）を拒む。それと違い、% の符号は
/// 残す（`file=A%26B` はノート `A&B` を指したまま）。
pub fn sanitize_url(url: &str) -> String {
    if url.is_empty() {
        return BLANK.into();
    }
    let (Some(decoded), Some(kept)) = (unwrap(url, true), unwrap(url, false)) else {
        return BLANK.into();
    };
    if decoded.is_empty() || kept.is_empty() {
        return BLANK.into();
    }
    if decoded.starts_with('.') || decoded.starts_with('/') {
        return kept;
    }
    let Some(scheme) = scheme_match(js::trim_start(&decoded)) else {
        return kept;
    };
    let protocol = js::trim(&scheme.to_lowercase()).to_string();
    if invalid_protocol(&protocol) {
        return BLANK.into();
    }
    let normalized = js::trim_start(&kept).replace('\\', "/");
    if protocol == "mailto:" || protocol.contains("://") {
        return normalized;
    }
    if protocol == "http:" || protocol == "https:" {
        return match parse_special_url(&normalized) {
            Some(u) => u,
            None => BLANK.into(),
        };
    }
    normalized
}

/// 図に書いた URL。符号を先に読み、スキームを隠せないようにする。
pub fn safe_url(url: &str) -> String {
    sanitize_url(&decode_entities(url))
}

const LINK_SCHEMES: &[&str] = &["http", "https", "mailto", "tel"];
const IMAGE_SCHEMES: &[&str] = &["http", "https"];

/// Mermaid（3 つ以外のスキームを通す）より厳しい: 共有のノートの図が、手元のファイルやネットワークの共有、
/// ほかのアプリを開けないように。
fn allowed(url: &str, schemes: &[String]) -> bool {
    let b = url.as_bytes();
    if b.len() >= 2 && (b[0] == b'/' || b[0] == b'\\') && (b[1] == b'/' || b[1] == b'\\') {
        return false;
    }
    // `/^([a-z][a-z0-9+.-]*):/i`
    if !b.first().is_some_and(|c| c.is_ascii_alphabetic()) {
        return true;
    }
    let n = b
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-'))
        .count();
    if b.get(n) != Some(&b':') {
        return true;
    }
    let scheme = url[..n].to_lowercase();
    schemes.contains(&scheme)
}

/// 開く前に href を読むアプリのため、読んだ URL も確かめる。
fn allow(url: &str, schemes: &[String]) -> Option<String> {
    let decoded = unwrap(url, true)?;
    (allowed(url, schemes) && allowed(&decoded, schemes)).then(|| url.to_string())
}

fn schemes(given: &Option<Vec<String>>, default: &[&str]) -> Vec<String> {
    match given {
        Some(s) => s.clone(),
        None => default.iter().map(|s| s.to_string()).collect(),
    }
}

/// どちらも `sanitize_url` が通した URL を取り、相対か、ホストが許すスキームのときだけ残す。拒んだものは絵から
/// 除く（`about:blank` と書いても、押せば置かれたページをどこかへ動かす）。
pub fn link_url(url: &str, options: &RenderOptions) -> Option<String> {
    if options.links == Some(false) {
        return None;
    }
    allow(url, &schemes(&options.link_schemes, LINK_SCHEMES))
}

pub fn image_url(url: &str, options: &RenderOptions) -> Option<String> {
    if options.images == Some(false) {
        return None;
    }
    allow(url, &schemes(&options.image_schemes, IMAGE_SCHEMES))
}

/// href は元の名前のままなので、確かめるだけでは足りない。
pub fn internal_link(name: &str, options: &RenderOptions) -> bool {
    let url = sanitize_url(name);
    if url == BLANK || link_url(&url, options).is_none() {
        return false;
    }
    // esc() とブラウザが除く文字に合わせる。
    let seen: String = name
        .chars()
        .filter(|c| !invalid_xml(*c) && !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let seen = seen.trim_start_matches(|c: char| (c as u32) <= 0x20);
    link_url(seen, options).is_some()
}

pub fn linked(
    body: &str,
    url: &str,
    id: &str,
    links: &mut Vec<LinkInfo>,
    options: &RenderOptions,
    attrs: &str,
) -> String {
    let Some(href) = link_url(url, options) else {
        return body.to_string();
    };
    let out = format!(
        "<a href=\"{}\"{attrs}{}>{body}</a>",
        esc(&href),
        rel_attr(options)
    );
    links.push(LinkInfo {
        id: id.to_string(),
        href,
        internal: false,
    });
    out
}

pub fn rel_attr(options: &RenderOptions) -> String {
    let rel = options.link_rel.as_deref().unwrap_or("noopener");
    if rel.is_empty() {
        String::new()
    } else {
        format!(" rel=\"{}\"", esc(rel))
    }
}

// --- WHATWG URL（http・https）の読みと書き ---

fn pct_encode(out: &mut String, c: char, set: impl Fn(char) -> bool) {
    if set(c) || !c.is_ascii() {
        let mut buf = [0u8; 4];
        for b in c.encode_utf8(&mut buf).bytes() {
            out.push_str(&format!("%{b:02X}"));
        }
    } else {
        out.push(c);
    }
}

fn c0(c: char) -> bool {
    (c as u32) < 0x20 || c == '\u{7f}'
}

fn fragment_set(c: char) -> bool {
    c0(c) || matches!(c, ' ' | '"' | '<' | '>' | '`')
}

fn query_set(c: char) -> bool {
    c0(c) || matches!(c, ' ' | '"' | '#' | '<' | '>' | '\'')
}

fn path_set(c: char) -> bool {
    query_set(c) && c != '\'' || matches!(c, '?' | '`' | '{' | '}' | '^')
}

fn userinfo_set(c: char) -> bool {
    path_set(c) || matches!(c, '/' | ':' | ';' | '=' | '@' | '[' | '\\' | ']' | '|')
}

fn single_dot(s: &str) -> bool {
    s == "." || s.eq_ignore_ascii_case("%2e")
}

fn double_dot(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        ".." | ".%2e" | "%2e." | "%2e%2e"
    )
}

/// punycode（RFC 3492）。
fn punycode(input: &[char]) -> Option<String> {
    const BASE: u32 = 36;
    const TMIN: u32 = 1;
    const TMAX: u32 = 26;
    const SKEW: u32 = 38;
    const DAMP: u32 = 700;
    fn adapt(mut delta: u32, num: u32, first: bool) -> u32 {
        delta = if first { delta / DAMP } else { delta / 2 };
        delta += delta / num;
        let mut k = 0;
        while delta > ((BASE - TMIN) * TMAX) / 2 {
            delta /= BASE - TMIN;
            k += BASE;
        }
        k + (((BASE - TMIN + 1) * delta) / (delta + SKEW))
    }
    fn digit(d: u32) -> char {
        (if d < 26 {
            b'a' + d as u8
        } else {
            b'0' + (d - 26) as u8
        }) as char
    }
    let mut out: String = input.iter().filter(|c| c.is_ascii()).collect();
    let b = out.len() as u32;
    let mut h = b;
    if b > 0 {
        out.push('-');
    }
    let mut n: u32 = 128;
    let mut delta: u32 = 0;
    let mut bias: u32 = 72;
    while (h as usize) < input.len() {
        let m = input.iter().map(|&c| c as u32).filter(|&c| c >= n).min()?;
        delta = delta.checked_add((m - n).checked_mul(h + 1)?)?;
        n = m;
        for &c in input {
            let c = c as u32;
            if c < n {
                delta = delta.checked_add(1)?;
            }
            if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias + TMAX {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    out.push(digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(digit(q));
                bias = adapt(delta, h + 1, h == b);
                delta = 0;
                h += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Some(out)
}

/// ホストを読む（% を戻し、小文字にし、ASCII でない名前は punycode に）。
fn parse_host(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with('[') {
        // IPv6 はそのまま（小文字に）。
        return raw.ends_with(']').then(|| raw.to_ascii_lowercase());
    }
    let decoded = decode_uri_component(raw).ok()?;
    let lower = decoded.to_lowercase();
    let mut labels = Vec::new();
    for label in lower.split(['.', '\u{3002}', '\u{ff0e}', '\u{ff61}']) {
        if label.is_ascii() {
            labels.push(label.to_string());
        } else {
            let chars: Vec<char> = label.chars().collect();
            labels.push(format!("xn--{}", punycode(&chars)?));
        }
    }
    let host = labels.join(".");
    if host.is_empty()
        || host.chars().any(|c| {
            c0(c)
                || matches!(
                    c,
                    ' ' | '#'
                        | '%'
                        | '/'
                        | ':'
                        | '<'
                        | '>'
                        | '?'
                        | '@'
                        | '['
                        | '\\'
                        | ']'
                        | '^'
                        | '|'
                )
        })
    {
        return None;
    }
    Some(host)
}

/// `new URL(url).toString()`（http・https のとき）。読めなければ None。
fn parse_special_url(input: &str) -> Option<String> {
    // 頭と尾の C0 と空白を除き、タブと改行を除く。
    let trimmed = input.trim_matches(|c: char| (c as u32) <= 0x20);
    let s: String = trimmed
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let colon = s.find(':')?;
    let scheme = s[..colon].to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let mut rest = &s[colon + 1..];
    rest = rest.trim_start_matches(['/', '\\']);
    let auth_end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    let after = &rest[auth_end..];
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(k) => (Some(&authority[..k]), &authority[k + 1..]),
        None => (None, authority),
    };
    let (host_raw, port) = if hostport.starts_with('[') {
        {
            let k = hostport.find(']')?;
            (&hostport[..=k], hostport[k + 1..].strip_prefix(':'))
        }
    } else {
        match hostport.rfind(':') {
            Some(k) => (&hostport[..k], Some(&hostport[k + 1..])),
            None => (hostport, None),
        }
    };
    let host = parse_host(host_raw)?;
    let port = match port {
        None | Some("") => None,
        Some(p) => {
            if !p.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let v: u64 = p.trim_start_matches('0').parse().unwrap_or(0);
            if v > 65535 {
                return None;
            }
            let default = if scheme == "http" { 80 } else { 443 };
            (v != default).then_some(v)
        }
    };
    let mut out = format!("{scheme}://");
    if let Some(ui) = userinfo.filter(|u| !u.is_empty()) {
        let (user, pass) = match ui.find(':') {
            Some(k) => (&ui[..k], Some(&ui[k + 1..])),
            None => (ui, None),
        };
        for c in user.chars() {
            pct_encode(&mut out, c, userinfo_set);
        }
        if let Some(p) = pass.filter(|p| !p.is_empty()) {
            out.push(':');
            for c in p.chars() {
                pct_encode(&mut out, c, userinfo_set);
            }
        }
        out.push('@');
    }
    out.push_str(&host);
    if let Some(p) = port {
        out.push_str(&format!(":{p}"));
    }
    // パス・クエリ・断片。
    let (path_part, query, fragment) = {
        let (before_frag, frag) = match after.find('#') {
            Some(k) => (&after[..k], Some(&after[k + 1..])),
            None => (after, None),
        };
        let (path, query) = match before_frag.find('?') {
            Some(k) => (&before_frag[..k], Some(&before_frag[k + 1..])),
            None => (before_frag, None),
        };
        (path, query, frag)
    };
    let mut segments: Vec<String> = Vec::new();
    let parts: Vec<&str> = if path_part.is_empty() {
        Vec::new()
    } else {
        path_part[1..].split(['/', '\\']).collect()
    };
    let count = parts.len();
    for (i, seg) in parts.iter().enumerate() {
        let last = i + 1 == count;
        if double_dot(seg) {
            segments.pop();
            if last {
                segments.push(String::new());
            }
        } else if single_dot(seg) {
            if last {
                segments.push(String::new());
            }
        } else {
            let mut e = String::new();
            for c in seg.chars() {
                pct_encode(&mut e, c, path_set);
            }
            segments.push(e);
        }
    }
    out.push('/');
    out.push_str(&segments.join("/"));
    if let Some(q) = query {
        out.push('?');
        for c in q.chars() {
            pct_encode(&mut out, c, query_set);
        }
    }
    if let Some(f) = fragment {
        out.push('#');
        for c in f.chars() {
            pct_encode(&mut out, c, fragment_set);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes() {
        assert_eq!(sanitize_url("https://Example.com"), "https://example.com/");
        assert_eq!(sanitize_url("javascript:alert(1)"), BLANK);
        assert_eq!(sanitize_url("./a b"), "./a b");
        assert_eq!(
            sanitize_url("http://a.com/x/../y?q=a b#f"),
            "http://a.com/y?q=a%20b#f"
        );
        assert_eq!(sanitize_url("https://日本.jp/"), "https://xn--wgv71a.jp/");
    }
}
