//! SVG の `<style>` の CSS を、各要素の属性に書き写す。CoreSVG（macOS）は `<style>` の規則をほとんど当てないので、
//! Mermaid の図（merman が出す SVG。色・線・文字の大きさを `<style>` で決める）は、そのままでは線が黒く塗られ、図形の
//! 色が抜ける。librsvg（GTK4）はそのままで描けるが、同じものを渡す。
//!
//! 扱う CSS: 型・`#id`・`.class`・`[attr]`・`[attr="v"]`・`*`、子孫（空白）と子（`>`）の結合、`:not(…)`・
//! `:first-child`・`:last-child`・`:first-of-type`・`:last-of-type`・`:nth-of-type(n)`・`:nth-child(n)`（n は数）、
//! `!important`。ほかの擬似クラス（`:hover` など）を含む規則と `@` の規則（`@keyframes`・`@media` など）は捨てる。
//! 当てる順は CSS と同じ: 属性 < 規則（詳細度・書いた順）< `style` 属性 < `!important` の規則 < `!important` の `style`。
//! 継承は描き手に任せる（親に書いた属性は子に継がれる）。

use std::collections::HashMap;
use std::fmt::Write as _;

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// `svg` の `<style>` を各要素の属性に書き写し、`<style>` を除いた SVG を返す。根の `width`・`height` が百分率
/// なら `viewBox` の大きさにする。読めない SVG なら `None`。
pub fn inline(svg: &str) -> Option<String> {
    inline_with(svg, &[])
}

/// `inline` と同じで、根より外で定めた CSS の変数 `vars`（`("--pele-bg", "#1e1e1e")` など）を当てる。
pub fn inline_with(svg: &str, vars: &[(&str, &str)]) -> Option<String> {
    inline_impl(svg, vars, false)
}

/// `inline_with` と同じで、CoreSVG（macOS）に渡すときに使う。CoreSVG は `font-style` を読まず、斜体の書体も
/// 選ばないので、斜体（`font-style` の `italic`・`oblique`）の文字を `skewX` で傾ける。また、描き手の手がかり
/// （DOT の図が添える）を使う: `<tspan>` の `data-x`（測った左端）で span ごとの `<text>` に分け、
/// `data-coresvg-only` の `<g>`（`text-decoration` の代わりの線）を残し、`text-decoration` を除く。CoreSVG で
/// ないとき（`inline_with`）は、その `<g>` を除き、文字は流れるまま描き手に任せる。
pub fn inline_slanted(svg: &str, vars: &[(&str, &str)]) -> Option<String> {
    inline_impl(svg, vars, true)
}

fn inline_impl(svg: &str, vars: &[(&str, &str)], coresvg: bool) -> Option<String> {
    let doc = roxmltree::Document::parse(svg).ok()?;
    let mut css = String::new();
    for node in doc.descendants() {
        if node.is_element() && node.tag_name().name() == "style" {
            for child in node.children() {
                if let Some(text) = child.text() {
                    css.push_str(text);
                    css.push('\n');
                }
            }
        }
    }
    let rules = parse_css(&css);
    let mut writer = Writer {
        rules: &rules,
        coresvg,
        markers: HashMap::new(),
        vars: Default::default(),
        seed: std::rc::Rc::new(
            vars.iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        ),
    };
    for node in doc.descendants() {
        if node.is_element()
            && node.tag_name().name() == "marker"
            && let Some(id) = node.attribute("id")
        {
            writer.markers.insert(id, node);
        }
    }
    let mut out = String::with_capacity(svg.len());
    writer.write(doc.root_element(), &mut out, true);
    Some(out)
}

// ---- CSS ----

#[derive(Debug)]
struct Rule {
    selector: Selector,
    specificity: (u32, u32, u32),
    order: usize,
    decls: Vec<Decl>,
}

#[derive(Clone, Debug)]
struct Decl {
    name: String,
    value: String,
    important: bool,
}

/// 右から左に読む: `parts[0]` が対象の要素。`parts[i]` と `parts[i + 1]` の間の結合は `parts[i].1`。
#[derive(Clone, Debug)]
struct Selector {
    parts: Vec<(Compound, Combinator)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
}

#[derive(Clone, Debug, Default)]
struct Compound {
    tag: Option<String>,
    ids: Vec<String>,
    classes: Vec<String>,
    attrs: Vec<(String, Option<String>)>,
    pseudos: Vec<Pseudo>,
}

#[derive(Clone, Debug)]
enum Pseudo {
    Not(Box<Compound>),
    NthChild(usize),
    NthLastChild(usize),
    NthOfType(usize),
    NthLastOfType(usize),
}

fn parse_css(css: &str) -> Vec<Rule> {
    let css = strip_comments(css);
    let bytes = css.as_bytes();
    let mut rules = Vec::new();
    let mut i = 0;
    let mut order = 0;
    while i < bytes.len() {
        let Some(open) = find_outside_strings(&css, i, b'{') else {
            break;
        };
        let prelude = css[i..open].trim();
        let close = matching_brace(bytes, open);
        let body = &css[open + 1..close.min(css.len())];
        i = close + 1;
        if prelude.starts_with('@') {
            continue;
        }
        let decls = parse_decls(body);
        if decls.is_empty() {
            continue;
        }
        for text in split_top(prelude, b',') {
            if let Some(selector) = parse_selector(text.trim()) {
                let specificity = specificity(&selector);
                rules.push(Rule {
                    selector,
                    specificity,
                    order,
                    decls: decls.clone(),
                });
                order += 1;
            }
        }
    }
    rules
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..]
            .find("*/")
            .map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    out.push_str(rest);
    out
}

fn find_outside_strings(s: &str, from: usize, target: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut quote = None;
    for (j, &b) in bytes.iter().enumerate().skip(from) {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == target => return Some(j),
            None => {}
        }
    }
    None
}

/// `{` の位置 `open` に対応する `}` の位置（なければ末尾）。
fn matching_brace(bytes: &[u8], open: usize) -> usize {
    let mut depth = 0;
    let mut quote = None;
    for (j, &b) in bytes.iter().enumerate().skip(open) {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return j;
                    }
                }
                _ => {}
            },
        }
    }
    bytes.len()
}

/// 括弧と引用符の外にある `sep` で分ける。
fn split_top(s: &str, sep: u8) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quote = None;
    let mut start = 0;
    for (j, &b) in s.as_bytes().iter().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth -= 1,
                _ if b == sep && depth == 0 => {
                    parts.push(&s[start..j]);
                    start = j + 1;
                }
                _ => {}
            },
        }
    }
    parts.push(&s[start..]);
    parts
}

fn parse_decls(body: &str) -> Vec<Decl> {
    let mut decls = Vec::new();
    for item in split_top(body, b';') {
        let Some((name, value)) = item.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let mut value = value.trim();
        let mut important = false;
        if let Some(pos) = value.to_ascii_lowercase().rfind("!important") {
            important = true;
            value = value[..pos].trim_end();
        }
        if name.is_empty() || value.is_empty() {
            continue;
        }
        decls.push(Decl {
            name,
            value: value.to_owned(),
            important,
        });
    }
    decls
}

fn parse_selector(text: &str) -> Option<Selector> {
    if text.is_empty() {
        return None;
    }
    // 結合子で区切る（括弧と [] の中は除く）
    let mut compounds: Vec<(String, Combinator)> = Vec::new();
    let mut current = String::new();
    let mut pending = Combinator::Descendant;
    let mut depth = 0i32;
    let mut quote = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            current.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                current.push(c);
            }
            '(' | '[' => {
                depth += 1;
                current.push(c);
            }
            ')' | ']' => {
                depth -= 1;
                current.push(c);
            }
            ' ' | '\t' | '\n' | '\r' | '>' | '+' | '~' if depth == 0 => {
                let mut combinator = match c {
                    '>' => Some(Combinator::Child),
                    '+' | '~' => return None,
                    _ => None,
                };
                while let Some(&next) = chars.peek() {
                    match next {
                        ' ' | '\t' | '\n' | '\r' => {
                            chars.next();
                        }
                        '>' => {
                            combinator = Some(Combinator::Child);
                            chars.next();
                        }
                        '+' | '~' => return None,
                        _ => break,
                    }
                }
                if !current.is_empty() {
                    compounds.push((std::mem::take(&mut current), pending));
                }
                pending = combinator.unwrap_or(Combinator::Descendant);
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        compounds.push((current, pending));
    }
    // compounds[k].1 は compounds[k] の左の結合。右から左に並べ直す。
    let mut parts = Vec::new();
    for k in (0..compounds.len()).rev() {
        let compound = parse_compound(&compounds[k].0)?;
        let combinator = compounds[k].1;
        parts.push((compound, combinator));
    }
    Some(Selector { parts })
}

fn parse_compound(text: &str) -> Option<Compound> {
    let mut c = Compound::default();
    let bytes = text.as_bytes();
    let mut i = 0;
    let ident_end = |from: usize| {
        let mut j = from;
        while j < bytes.len()
            && (bytes[j].is_ascii_alphanumeric()
                || matches!(bytes[j], b'-' | b'_')
                || bytes[j] >= 0x80)
        {
            j += 1;
        }
        j
    };
    while i < bytes.len() {
        match bytes[i] {
            b'*' => i += 1,
            b'#' => {
                let end = ident_end(i + 1);
                c.ids.push(text[i + 1..end].to_owned());
                i = end;
            }
            b'.' => {
                let end = ident_end(i + 1);
                c.classes.push(text[i + 1..end].to_owned());
                i = end;
            }
            b'[' => {
                let end = text[i..].find(']')? + i;
                let inner = &text[i + 1..end];
                match inner.split_once('=') {
                    Some((name, value)) => {
                        if name.ends_with(['~', '|', '^', '$', '*']) {
                            return None;
                        }
                        let value = value.trim().trim_matches(['"', '\'']);
                        c.attrs
                            .push((name.trim().to_owned(), Some(value.to_owned())));
                    }
                    None => c.attrs.push((inner.trim().to_owned(), None)),
                }
                i = end + 1;
            }
            b':' => {
                let end = ident_end(i + 1);
                let name = &text[i + 1..end];
                let mut arg = None;
                let mut next = end;
                if bytes.get(end) == Some(&b'(') {
                    let close = text[end..].find(')')? + end;
                    // :not( の中に括弧はない前提（merman の出力）
                    arg = Some(&text[end + 1..close]);
                    next = close + 1;
                }
                let number = || {
                    arg.and_then(|a| a.trim().parse::<usize>().ok())
                        .filter(|&n| n > 0)
                };
                c.pseudos.push(match name {
                    "not" => Pseudo::Not(Box::new(parse_compound(arg?.trim())?)),
                    "first-child" => Pseudo::NthChild(1),
                    "last-child" => Pseudo::NthLastChild(1),
                    "first-of-type" => Pseudo::NthOfType(1),
                    "last-of-type" => Pseudo::NthLastOfType(1),
                    "nth-child" => Pseudo::NthChild(number()?),
                    "nth-of-type" => Pseudo::NthOfType(number()?),
                    _ => return None,
                });
                i = next;
            }
            _ => {
                let end = ident_end(i);
                if end == i {
                    return None;
                }
                c.tag = Some(text[i..end].to_ascii_lowercase());
                i = end;
            }
        }
    }
    Some(c)
}

fn specificity(selector: &Selector) -> (u32, u32, u32) {
    fn add(c: &Compound, s: &mut (u32, u32, u32)) {
        s.0 += c.ids.len() as u32;
        s.1 += (c.classes.len() + c.attrs.len()) as u32;
        s.2 += u32::from(c.tag.is_some());
        for p in &c.pseudos {
            match p {
                Pseudo::Not(inner) => add(inner, s),
                _ => s.1 += 1,
            }
        }
    }
    let mut s = (0, 0, 0);
    for (c, _) in &selector.parts {
        add(c, &mut s);
    }
    s
}

// ---- 当てはめ ----

fn matches(selector: &Selector, node: roxmltree::Node) -> bool {
    match_from(&selector.parts, 0, node)
}

fn match_from(parts: &[(Compound, Combinator)], k: usize, node: roxmltree::Node) -> bool {
    if !match_compound(&parts[k].0, node) {
        return false;
    }
    if k + 1 == parts.len() {
        return true;
    }
    let mut ancestor = node.parent_element();
    match parts[k].1 {
        Combinator::Child => ancestor.is_some_and(|a| match_from(parts, k + 1, a)),
        Combinator::Descendant => {
            while let Some(a) = ancestor {
                if match_from(parts, k + 1, a) {
                    return true;
                }
                ancestor = a.parent_element();
            }
            false
        }
    }
}

fn match_compound(c: &Compound, node: roxmltree::Node) -> bool {
    if let Some(tag) = &c.tag
        && !node.tag_name().name().eq_ignore_ascii_case(tag)
    {
        return false;
    }
    if !c
        .ids
        .iter()
        .all(|id| node.attribute("id") == Some(id.as_str()))
    {
        return false;
    }
    if !c.classes.is_empty() {
        let classes: Vec<&str> = node
            .attribute("class")
            .unwrap_or("")
            .split_ascii_whitespace()
            .collect();
        if !c.classes.iter().all(|k| classes.contains(&k.as_str())) {
            return false;
        }
    }
    for (name, value) in &c.attrs {
        match (node.attribute(name.as_str()), value) {
            (None, _) => return false,
            (Some(v), Some(want)) if v != want => return false,
            _ => {}
        }
    }
    c.pseudos.iter().all(|p| match p {
        Pseudo::Not(inner) => !match_compound(inner, node),
        Pseudo::NthChild(n) => node.prev_siblings().filter(|s| s.is_element()).count() == *n,
        Pseudo::NthLastChild(n) => node.next_siblings().filter(|s| s.is_element()).count() == *n,
        Pseudo::NthOfType(n) => {
            node.prev_siblings()
                .filter(|s| s.is_element() && s.tag_name() == node.tag_name())
                .count()
                == *n
        }
        Pseudo::NthLastOfType(n) => {
            node.next_siblings()
                .filter(|s| s.is_element() && s.tag_name() == node.tag_name())
                .count()
                == *n
        }
    })
}

/// 要素に当てる宣言を、勝つ順（あとのものが勝つ）に並べる。
fn cascade<'a>(rules: &'a [Rule], node: roxmltree::Node) -> HashMap<String, String> {
    let mut matched: Vec<(&'a Rule, &'a Decl)> = Vec::new();
    for rule in rules {
        if matches(&rule.selector, node) {
            for decl in &rule.decls {
                matched.push((rule, decl));
            }
        }
    }
    matched.sort_by_key(|(rule, decl)| (decl.important, rule.specificity, rule.order));
    let inline = node.attribute("style").map(parse_decls).unwrap_or_default();
    let mut out: HashMap<String, String> = HashMap::new();
    let mut set = |d: &Decl| {
        out.insert(d.name.clone(), d.value.clone());
    };
    for (_, d) in matched.iter().filter(|(_, d)| !d.important) {
        set(d);
    }
    for d in inline.iter().filter(|d| !d.important) {
        set(d);
    }
    for (_, d) in matched.iter().filter(|(_, d)| d.important) {
        set(d);
    }
    for d in inline.iter().filter(|d| d.important) {
        set(d);
    }
    out
}

// ---- 書き出し ----

struct Writer<'a, 'input> {
    rules: &'a [Rule],
    /// CoreSVG 向けに書くか（`inline_slanted`。斜体を `skewX` で傾け、手がかりで span を分ける）。
    coresvg: bool,
    markers: HashMap<&'a str, roxmltree::Node<'a, 'input>>,
    /// 要素ごとの CSS の変数（`--x`。祖先から継いだものを含め、値の中の var() は解いてある）。
    vars: std::cell::RefCell<HashMap<roxmltree::NodeId, std::rc::Rc<HashMap<String, String>>>>,
    /// 根より外の変数。
    seed: std::rc::Rc<HashMap<String, String>>,
}

impl<'a, 'input> Writer<'a, 'input> {
    /// 要素の属性と、当たる CSS を合わせたもの（`style` 属性は除く）。CSS の変数は解いて、`var()` を値に置き換え、
    /// `font` の一括指定は `font-size`・`font-family` に分ける（CoreSVG はどちらも読まない）。
    fn attrs(&self, node: roxmltree::Node) -> Vec<(String, String)> {
        let mut attrs: Vec<(String, String)> = Vec::new();
        for a in node.attributes() {
            if a.name() == "style" && a.namespace().is_none() {
                continue;
            }
            attrs.push((qualified(a.namespace(), a.name()), a.value().to_owned()));
        }
        let mut props: Vec<(String, String)> = cascade(self.rules, node).into_iter().collect();
        props.sort();
        for (prop, value) in props {
            match attrs.iter_mut().find(|(n, _)| *n == prop) {
                Some(slot) => slot.1 = value,
                None => attrs.push((prop, value)),
            }
        }
        attrs.retain(|(n, _)| !n.starts_with("--"));
        if attrs.iter().any(|(_, v)| v.contains("var(")) {
            let vars = self.vars(node);
            for (_, v) in attrs.iter_mut() {
                if v.contains("var(") {
                    *v = resolve_vars(v, &vars, 0);
                }
            }
        }
        if let Some(i) = attrs.iter().position(|(n, _)| n == "font") {
            let (_, font) = attrs.remove(i);
            let (size, family, words) = split_font(&font);
            for word in words {
                let key = match word.as_str() {
                    "italic" | "oblique" => "font-style",
                    "bold" | "bolder" | "lighter" => "font-weight",
                    w if is_weight(w) => "font-weight",
                    _ => continue,
                };
                if !attrs.iter().any(|(n, _)| n == key) {
                    attrs.push((key.into(), word));
                }
            }
            if let Some(size) = size
                && !attrs.iter().any(|(n, _)| n == "font-size")
            {
                attrs.push(("font-size".into(), size));
            }
            if let Some(family) = family
                && !attrs.iter().any(|(n, _)| n == "font-family")
            {
                attrs.push(("font-family".into(), family));
            }
        }
        // CoreSVG は `bold` などの言葉の太さを読まない（数なら読む）
        if let Some((_, w)) = attrs.iter_mut().find(|(n, _)| n == "font-weight") {
            match w.trim() {
                "bold" | "bolder" => *w = "700".into(),
                "normal" | "lighter" => *w = "400".into(),
                _ => {}
            }
        }
        attrs
    }

    /// `attrs` を持つ `node`（`<text>`）が斜体なら、`y` の行を軸に傾ける `transform` を足す。
    fn slant(&self, node: roxmltree::Node, attrs: &mut Vec<(String, String)>) {
        if !self.coresvg {
            return;
        }
        let style = match attrs.iter().find(|(k, _)| k == "font-style") {
            Some((_, v)) => Some(v.clone()),
            None => node
                .parent_element()
                .and_then(|p| self.inherited(p, "font-style")),
        };
        if !matches!(style.as_deref().map(str::trim), Some("italic" | "oblique")) {
            return;
        }
        let y = attrs
            .iter()
            .find(|(k, _)| k == "y")
            .and_then(|(_, v)| {
                v.split(|c: char| c.is_ascii_whitespace() || c == ',')
                    .find(|s| !s.is_empty())
                    .and_then(number)
            })
            .unwrap_or(0.0);
        // skewX(-12) は (x, y) を (x - y·tan12°, y) に移すので、先に同じだけ右へずらして基線を動かさない
        let skew = format!("translate({} 0) skewX(-12)", fmt(y * SLANT));
        match attrs.iter_mut().find(|(k, _)| k == "transform") {
            Some((_, t)) => *t = format!("{t} {skew}"),
            None => attrs.push(("transform".into(), skew)),
        }
    }

    /// `node` で見える CSS の変数。
    fn vars(&self, node: roxmltree::Node) -> std::rc::Rc<HashMap<String, String>> {
        if let Some(v) = self.vars.borrow().get(&node.id()) {
            return v.clone();
        }
        let parent = node
            .parent_element()
            .map(|p| self.vars(p))
            .unwrap_or_else(|| self.seed.clone());
        let own: Vec<(String, String)> = cascade(self.rules, node)
            .into_iter()
            .filter(|(k, _)| k.starts_with("--"))
            .collect();
        let vars = if own.is_empty() {
            parent
        } else {
            let mut map = (*parent).clone();
            for (k, v) in &own {
                map.insert(k.clone(), v.clone());
            }
            // 同じ要素で定めた変数どうしの参照も解く（参照の深さは resolve_vars が抑える）
            let resolved: Vec<(String, String)> = own
                .iter()
                .map(|(k, _)| (k.clone(), resolve_vars(&map[k], &map, 0)))
                .collect();
            for (k, v) in resolved {
                map.insert(k, v);
            }
            std::rc::Rc::new(map)
        };
        self.vars.borrow_mut().insert(node.id(), vars.clone());
        vars
    }

    /// 継がれる性質 `name` の値（その要素か、いちばん近い祖先のもの）。
    fn inherited(&self, node: roxmltree::Node, name: &str) -> Option<String> {
        let mut current = Some(node);
        while let Some(n) = current {
            if let Some((_, v)) = self.attrs(n).into_iter().find(|(k, _)| k == name) {
                return Some(v);
            }
            current = n.parent_element();
        }
        None
    }

    /// 書き出す属性か。描き手の手がかり（`data-x`・`data-coresvg-only`）は書かず、CoreSVG には
    /// `text-decoration` も書かない（読まないので、`data-coresvg-only` の線で描く）。
    fn kept(&self, name: &str) -> bool {
        name != "data-x"
            && name != "data-coresvg-only"
            && !(self.coresvg && name == "text-decoration")
    }

    fn write(&self, node: roxmltree::Node, out: &mut String, root: bool) {
        let tag = node.tag_name().name();
        if tag == "style" || tag == "marker" {
            // marker は使う所に図形として置く（CoreSVG は marker を描かない）
            return;
        }
        if !self.coresvg && node.attribute("data-coresvg-only").is_some() {
            return;
        }
        let name = qualified(node.tag_name().namespace(), tag);
        let mut attrs = self.attrs(node);
        if root {
            fix_root_size(&mut attrs);
            attrs.retain(|(n, _)| n != "xmlns" && !n.starts_with("xmlns:"));
            attrs.insert(0, ("xmlns".into(), SVG_NS.into()));
            attrs.insert(1, ("xmlns:xlink".into(), XLINK_NS.into()));
        } else if tag == "svg" {
            // 入れ子の svg（merman の文字の枠）は、文字が見込みより広くても切らない
            set(&mut attrs, "overflow", "visible");
        }
        attrs.retain(|(n, _)| self.kept(n));
        if self.coresvg
            && tag == "image"
            && !attrs.iter().any(|(n, _)| n == "xlink:href")
            && let Some(href) = attrs.iter_mut().find(|(n, _)| n == "href")
        {
            // CoreSVG は SVG 2 の `href` を読まないことがあるので、古い `xlink:href` にする
            href.0 = "xlink:href".into();
        }
        let mut markers = Vec::new();
        attrs.retain(|(n, v)| {
            let kind = match n.as_str() {
                "marker-start" => MarkerAt::Start,
                "marker-mid" => MarkerAt::Mid,
                "marker-end" => MarkerAt::End,
                "marker" => MarkerAt::All,
                _ => return true,
            };
            markers.push((kind, v.clone()));
            false
        });
        if tag == "text" {
            if self.write_split_text(node, &attrs, out) {
                return;
            }
            self.slant(node, &mut attrs);
        }
        let _ = write!(out, "<{name}");
        for (n, v) in &attrs {
            let _ = write!(out, " {n}=\"{}\"", escape(v, true));
        }
        let children: Vec<_> = node
            .children()
            .filter(|c| c.is_element() || c.is_text())
            .collect();
        if children.is_empty() {
            out.push_str("/>");
        } else {
            out.push('>');
            for child in children {
                if child.is_element() {
                    self.write(child, out, false);
                } else if let Some(text) = child.text() {
                    out.push_str(&escape(text, false));
                }
            }
            let _ = write!(out, "</{name}>");
        }
        if !markers.is_empty() {
            self.write_markers(node, &attrs, &markers, out);
        }
    }

    /// `<text>` の中が `<tspan>` だけなら、`<tspan>` ごとの `<text>` に分けて書く（CoreSVG は `<tspan>` の位置・
    /// 書体・色を読まない）。分けられなければ `false`。
    fn write_split_text(
        &self,
        node: roxmltree::Node,
        attrs: &[(String, String)],
        out: &mut String,
    ) -> bool {
        let mut spans = Vec::new();
        for child in node.children() {
            if child.is_element() {
                if child.tag_name().name() != "tspan" || child.children().any(|c| c.is_element()) {
                    return false;
                }
                spans.push(child);
            } else if child.text().is_some_and(|t| !t.trim().is_empty()) {
                return false;
            }
        }
        if spans.is_empty() {
            return false;
        }
        // DOT の図の手がかり: どの `<tspan>` にも `data-x`（測った左端）があれば、流れる文を CoreSVG のときだけ
        // その位置で分ける（ほかの描き手には流れるまま渡す）。分けた `<text>` は両端の空白を描かない（描き手に
        // よって縮んだり残ったりするので、位置と描く文字がずれる。間の空白は位置の差にある）
        let hinted = spans.iter().all(|s| s.attribute("data-x").is_some());
        if hinted {
            if !self.coresvg {
                return false;
            }
        } else if spans
            .iter()
            .skip(1)
            .any(|s| s.attribute("x").is_none() && s.attribute("y").is_none())
        {
            return false;
        }
        let get = |attrs: &[(String, String)], key: &str| {
            attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
        };
        let Some(mut y) = number(&get(attrs, "y").unwrap_or_else(|| "0".into())) else {
            return false;
        };
        let text_x = get(attrs, "x").unwrap_or_else(|| "0".into());
        let positional = ["x", "y", "dx", "dy"];
        let size = self
            .inherited(node, "font-size")
            .and_then(|s| number(&s))
            .unwrap_or(16.0);
        let mut parts = String::new();
        for span in spans {
            let own = self.attrs(span);
            let mut merged: Vec<(String, String)> = attrs
                .iter()
                .filter(|(k, _)| !positional.contains(&k.as_str()))
                .cloned()
                .collect();
            for (k, v) in own
                .iter()
                .filter(|(k, _)| !positional.contains(&k.as_str()) && self.kept(k))
            {
                set(&mut merged, k, v);
            }
            let span_size = get(&own, "font-size")
                .and_then(|s| number(&s))
                .unwrap_or(size);
            match get(&own, "y") {
                Some(v) => match number(&v) {
                    Some(v) => y = v,
                    None => return false,
                },
                None => {
                    if let Some(dy) = get(&own, "dy") {
                        match length(&dy, span_size) {
                            Some(dy) => y += dy,
                            None => return false,
                        }
                    }
                }
            }
            let content: String = span.children().filter_map(|c| c.text()).collect();
            let (x, content) = match span.attribute("data-x") {
                Some(left) if hinted => {
                    set(&mut merged, "text-anchor", "start");
                    (left.to_owned(), content.trim_matches(' ').to_owned())
                }
                _ => (get(&own, "x").unwrap_or_else(|| text_x.clone()), content),
            };
            if hinted && content.is_empty() {
                continue;
            }
            let _ = write!(parts, "<text x=\"{}\" y=\"{}\"", escape(&x, true), fmt(y));
            set(&mut merged, "y", &fmt(y));
            self.slant(node, &mut merged);
            merged.retain(|(k, _)| k != "y");
            for (k, v) in &merged {
                let _ = write!(parts, " {k}=\"{}\"", escape(v, true));
            }
            let _ = write!(parts, ">{}</text>", escape(&content, false));
        }
        out.push_str(&parts);
        true
    }

    fn write_markers(
        &self,
        node: roxmltree::Node,
        attrs: &[(String, String)],
        markers: &[(MarkerAt, String)],
        out: &mut String,
    ) {
        let get = |key: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let vertices = match node.tag_name().name() {
            "path" => path_vertices(get("d").unwrap_or("")),
            "line" => {
                let n = |k| get(k).and_then(number).unwrap_or(0.0);
                let (a, b) = ((n("x1"), n("y1")), (n("x2"), n("y2")));
                let angle = direction(a, b);
                vec![(a.0, a.1, angle, angle), (b.0, b.1, angle, angle)]
            }
            "polyline" | "polygon" => {
                let nums: Vec<f64> = get("points")
                    .unwrap_or("")
                    .split(|c: char| c.is_ascii_whitespace() || c == ',')
                    .filter_map(|s| s.parse().ok())
                    .collect();
                let mut d = String::new();
                for (i, [x, y]) in nums.as_chunks::<2>().0.iter().enumerate() {
                    let _ = write!(d, "{}{x} {y} ", if i == 0 { "M" } else { "L" });
                }
                if node.tag_name().name() == "polygon" {
                    d.push('Z');
                }
                path_vertices(&d)
            }
            _ => Vec::new(),
        };
        if vertices.is_empty() {
            return;
        }
        let stroke_width = self
            .inherited(node, "stroke-width")
            .and_then(|s| number(&s))
            .unwrap_or(1.0);
        let last = vertices.len() - 1;
        for (at, url) in markers {
            let Some(id) = url
                .trim()
                .strip_prefix("url(")
                .and_then(|s| s.strip_suffix(')'))
            else {
                continue;
            };
            let id = id.trim().trim_matches(['"', '\'']).trim_start_matches('#');
            let Some(marker) = self.markers.get(id) else {
                continue;
            };
            for (i, &(x, y, incoming, outgoing)) in vertices.iter().enumerate() {
                let which = if i == 0 {
                    MarkerAt::Start
                } else if i == last {
                    MarkerAt::End
                } else {
                    MarkerAt::Mid
                };
                if *at != which && *at != MarkerAt::All {
                    continue;
                }
                let angle = match which {
                    MarkerAt::Start => outgoing,
                    MarkerAt::End => incoming,
                    _ => bisect(incoming, outgoing),
                };
                self.write_marker(
                    *marker,
                    (x, y),
                    angle,
                    which == MarkerAt::Start,
                    stroke_width,
                    out,
                );
            }
        }
    }

    fn write_marker(
        &self,
        marker: roxmltree::Node,
        (x, y): (f64, f64),
        angle: f64,
        start: bool,
        stroke_width: f64,
        out: &mut String,
    ) {
        let attrs = self.attrs(marker);
        let get = |key: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let num = |key: &str, default: f64| get(key).and_then(number).unwrap_or(default);
        let rotate = match get("orient").map(str::trim) {
            Some("auto") => angle,
            Some("auto-start-reverse") => {
                if start {
                    angle + 180.0
                } else {
                    angle
                }
            }
            Some(v) => number(v).unwrap_or(0.0),
            None => 0.0,
        };
        let units = if get("markerUnits") == Some("userSpaceOnUse") {
            1.0
        } else {
            stroke_width
        };
        let (width, height) = (num("markerWidth", 3.0), num("markerHeight", 3.0));
        let scale = match get("viewBox").map(|v| {
            v.split(|c: char| c.is_ascii_whitespace() || c == ',')
                .filter_map(|s| s.parse::<f64>().ok())
                .collect::<Vec<_>>()
        }) {
            Some(vb) if vb.len() == 4 && vb[2] > 0.0 && vb[3] > 0.0 => {
                (width / vb[2]).min(height / vb[3])
            }
            _ => 1.0,
        };
        let _ = write!(
            out,
            "<g transform=\"translate({} {}) rotate({}) scale({}) translate({} {})\"",
            fmt(x),
            fmt(y),
            fmt(rotate),
            fmt(units * scale),
            fmt(-num("refX", 0.0)),
            fmt(-num("refY", 0.0))
        );
        const MARKER_ONLY: [&str; 9] = [
            "id",
            "class",
            "viewBox",
            "refX",
            "refY",
            "markerWidth",
            "markerHeight",
            "orient",
            "markerUnits",
        ];
        for (k, v) in attrs
            .iter()
            .filter(|(k, _)| !MARKER_ONLY.contains(&k.as_str()) && !k.starts_with("data-"))
        {
            let _ = write!(out, " {k}=\"{}\"", escape(v, true));
        }
        out.push('>');
        for child in marker.children().filter(|c| c.is_element()) {
            self.write(child, out, false);
        }
        out.push_str("</g>");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarkerAt {
    Start,
    Mid,
    End,
    All,
}

/// `value` の中の `var(--x, 代わり)` を `vars` の値（なければ代わり）に置き換える。
fn resolve_vars(value: &str, vars: &HashMap<String, String>, depth: usize) -> String {
    if depth > 16 || !value.contains("var(") {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find("var(") {
        out.push_str(&rest[..at]);
        let inner_start = at + 4;
        let bytes = rest.as_bytes();
        let mut level = 1;
        let mut j = inner_start;
        while j < bytes.len() && level > 0 {
            match bytes[j] {
                b'(' => level += 1,
                b')' => level -= 1,
                _ => {}
            }
            j += 1;
        }
        let inner = &rest[inner_start..j.saturating_sub(1).max(inner_start)];
        let (name, fallback) = match split_top(inner, b',').as_slice() {
            [name] => (name.trim(), None),
            [name, ..] => (
                name.trim(),
                Some(inner[inner.find(',').unwrap_or(0) + 1..].trim()),
            ),
            [] => ("", None),
        };
        let replacement = match vars.get(name) {
            Some(v) => v.clone(),
            None => fallback.unwrap_or("").to_owned(),
        };
        out.push_str(&resolve_vars(&replacement, vars, depth + 1));
        rest = &rest[j.min(rest.len())..];
    }
    out.push_str(rest);
    out
}

/// `font` の一括指定から大きさと書体（`[style] [weight] size[/line-height] family`）。
fn split_font(font: &str) -> (Option<String>, Option<String>, Vec<String>) {
    let words = split_top(font.trim(), b' ');
    let Some(i) = words.iter().position(|w| {
        let w = w.split('/').next().unwrap_or("");
        w.starts_with(|c: char| c.is_ascii_digit() || c == '.') && !is_weight(w)
    }) else {
        return (None, None, Vec::new());
    };
    let size = words[i].split('/').next().unwrap_or("").to_owned();
    let family = words[i + 1..].join(" ").trim().to_owned();
    let before = words[..i].iter().map(|w| w.trim().to_owned()).collect();
    (Some(size), (!family.is_empty()).then_some(family), before)
}

/// `font` の一括指定の中の数の太さ（`700` など。大きさと違って単位がない）。
fn is_weight(w: &str) -> bool {
    w.len() == 3 && w.ends_with("00") && w.starts_with(|c: char| c.is_ascii_digit())
}

/// 斜体の傾き（tan 12°）。
const SLANT: f64 = 0.212_556_56;

fn set(attrs: &mut Vec<(String, String)>, key: &str, value: &str) {
    match attrs.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = value.to_owned(),
        None => attrs.push((key.to_owned(), value.to_owned())),
    }
}

/// 数（`px` は外す）。
fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    s.strip_suffix("px").unwrap_or(s).trim().parse().ok()
}

/// 長さ（`em` は `font_size` 倍）。
fn length(s: &str, font_size: f64) -> Option<f64> {
    let s = s.trim();
    match s.strip_suffix("em") {
        Some(v) => v.trim().parse::<f64>().ok().map(|v| v * font_size),
        None => number(s),
    }
}

fn fmt(v: f64) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

fn direction(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.1 - a.1).atan2(b.0 - a.0).to_degrees()
}

fn bisect(a: f64, b: f64) -> f64 {
    let mut d = b - a;
    while d > 180.0 {
        d -= 360.0;
    }
    while d < -180.0 {
        d += 360.0;
    }
    a + d / 2.0
}

/// パスの区間: 始点・終点・始点で出る向き・終点に入る向き（度）・moveto か。
type Segment = ((f64, f64), (f64, f64), f64, f64, bool);

/// パスの頂点と、そこに入る向き・出る向き（度）。
fn path_vertices(d: &str) -> Vec<(f64, f64, f64, f64)> {
    // 区間（始点・終点・始点での向き・終点での向き）を集める
    let mut segments: Vec<Segment> = Vec::new();
    let tokens = path_tokens(d);
    let mut i = 0;
    let (mut cur, mut start) = ((0.0, 0.0), (0.0, 0.0));
    let mut last_ctrl: Option<(f64, f64)> = None;
    let mut cmd = 'M';
    let tangent = |from: (f64, f64), to: &[(f64, f64)]| {
        to.iter()
            .find(|p| (p.0 - from.0).abs() > 1e-9 || (p.1 - from.1).abs() > 1e-9)
            .map_or(0.0, |p| direction(from, *p))
    };
    while i < tokens.len() {
        if let PathToken::Command(c) = tokens[i] {
            cmd = c;
            i += 1;
            if matches!(c, 'Z' | 'z') {
                if cur != start {
                    let a = direction(cur, start);
                    segments.push((cur, start, a, a, false));
                }
                cur = start;
                last_ctrl = None;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let mut take = |n: usize| -> Option<Vec<f64>> {
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                match tokens.get(i) {
                    Some(PathToken::Number(x)) => {
                        v.push(*x);
                        i += 1;
                    }
                    _ => return None,
                }
            }
            Some(v)
        };
        let abs = |p: (f64, f64), cur: (f64, f64)| if rel { (p.0 + cur.0, p.1 + cur.1) } else { p };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let Some(v) = take(2) else { break };
                cur = abs((v[0], v[1]), cur);
                start = cur;
                segments.push((cur, cur, f64::NAN, f64::NAN, true));
                cmd = if rel { 'l' } else { 'L' };
                last_ctrl = None;
            }
            'L' => {
                let Some(v) = take(2) else { break };
                let p = abs((v[0], v[1]), cur);
                let a = direction(cur, p);
                segments.push((cur, p, a, a, false));
                cur = p;
                last_ctrl = None;
            }
            'H' | 'V' => {
                let Some(v) = take(1) else { break };
                let p = if cmd.eq_ignore_ascii_case(&'H') {
                    (if rel { cur.0 + v[0] } else { v[0] }, cur.1)
                } else {
                    (cur.0, if rel { cur.1 + v[0] } else { v[0] })
                };
                let a = direction(cur, p);
                segments.push((cur, p, a, a, false));
                cur = p;
                last_ctrl = None;
            }
            'C' | 'S' | 'Q' | 'T' => {
                let upper = cmd.to_ascii_uppercase();
                let n = match upper {
                    'C' => 6,
                    'S' | 'Q' => 4,
                    _ => 2,
                };
                let Some(v) = take(n) else { break };
                let reflected = last_ctrl.map_or(cur, |c| (2.0 * cur.0 - c.0, 2.0 * cur.1 - c.1));
                let (c1, c2, p) = match upper {
                    'C' => (
                        abs((v[0], v[1]), cur),
                        abs((v[2], v[3]), cur),
                        abs((v[4], v[5]), cur),
                    ),
                    'S' => (reflected, abs((v[0], v[1]), cur), abs((v[2], v[3]), cur)),
                    'Q' => {
                        let c = abs((v[0], v[1]), cur);
                        (c, c, abs((v[2], v[3]), cur))
                    }
                    _ => (reflected, reflected, abs((v[0], v[1]), cur)),
                };
                let a0 = tangent(cur, &[c1, c2, p]);
                let a1 = tangent(p, &[c2, c1, cur]) + 180.0;
                segments.push((cur, p, a0, a1, false));
                cur = p;
                last_ctrl = Some(c2);
            }
            'A' => {
                let Some(v) = take(7) else { break };
                let p = abs((v[5], v[6]), cur);
                let a = direction(cur, p);
                segments.push((cur, p, a, a, false));
                cur = p;
                last_ctrl = None;
            }
            _ => break,
        }
    }
    let mut vertices: Vec<(f64, f64, f64, f64)> = Vec::new();
    for (k, &(from, to, a0, a1, moveto)) in segments.iter().enumerate() {
        if moveto {
            continue;
        }
        if vertices.is_empty() || segments[k - 1].4 {
            if let Some(last) = vertices.last_mut()
                && (last.0, last.1) == from
            {
                last.3 = a0;
            } else {
                vertices.push((from.0, from.1, a0, a0));
            }
        } else if let Some(last) = vertices.last_mut() {
            last.3 = a0;
        }
        vertices.push((to.0, to.1, a1, a1));
    }
    vertices
}

#[derive(Clone, Copy, Debug)]
enum PathToken {
    Command(char),
    Number(f64),
}

fn path_tokens(d: &str) -> Vec<PathToken> {
    let mut out = Vec::new();
    let bytes = d.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphabetic() && !matches!(c, b'e' | b'E') {
            out.push(PathToken::Command(c as char));
            i += 1;
        } else if c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.') {
            let start = i;
            i += 1;
            let mut dot = c == b'.';
            while i < bytes.len() {
                let b = bytes[i];
                if b.is_ascii_digit() {
                    i += 1;
                } else if b == b'.' && !dot {
                    dot = true;
                    i += 1;
                } else if matches!(b, b'e' | b'E') {
                    i += 1;
                    if i < bytes.len() && matches!(bytes[i], b'-' | b'+') {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(v) = d[start..i].parse() {
                out.push(PathToken::Number(v));
            }
        } else {
            i += 1;
        }
    }
    out
}

fn qualified(namespace: Option<&str>, name: &str) -> String {
    match namespace {
        Some(XLINK_NS) => format!("xlink:{name}"),
        Some(XML_NS) => format!("xml:{name}"),
        _ => name.to_owned(),
    }
}

/// 百分率・auto の `width`・`height` を `viewBox` の大きさにする（CoreSVG は百分率で大きさを決められない）。
fn fix_root_size(attrs: &mut Vec<(String, String)>) {
    let Some(view_box) = attrs
        .iter()
        .find(|(n, _)| n == "viewBox")
        .map(|(_, v)| v.clone())
    else {
        return;
    };
    let nums: Vec<f64> = view_box
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter_map(|s| s.parse().ok())
        .collect();
    let [_, _, w, h] = nums[..] else { return };
    for (key, value) in [("width", w), ("height", h)] {
        let fixed = format!("{}", (value * 1000.0).round() / 1000.0);
        match attrs.iter_mut().find(|(n, _)| n == key) {
            Some(slot) if slot.1.ends_with('%') || slot.1 == "auto" => slot.1 = fixed,
            Some(_) => {}
            None => attrs.push((key.into(), fixed)),
        }
    }
    // 外の CSS の大きさ（max-width など）は SVG の描き手には意味がない
    attrs.retain(|(n, _)| n != "max-width");
}

fn escape(text: &str, attr: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {

    #[test]
    fn bold_becomes_a_number_coresvg_reads() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" style="--w:bold"><text font-weight="var(--w)">a</text><text style="font:bold 12px serif">b</text><text font-weight="normal">c</text></svg>"#;
        let out = inline(svg).unwrap();
        assert!(out.contains(r#"font-weight="700">a<"#), "{out}");
        assert!(
            out.contains(r#"font-weight="700" font-size="12px" font-family="serif">b<"#),
            "{out}"
        );
        assert!(out.contains(r#"font-weight="400">c<"#), "{out}");
    }

    #[test]
    fn italics_are_slanted_only_when_asked() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><g font-style="italic"><text x="5" y="10">a</text></g><text x="5" y="20" transform="rotate(90)" style="font:italic 12px serif">b</text><text y="30">c</text></svg>"#;
        let plain = inline(svg).unwrap();
        assert!(!plain.contains("skewX"), "{plain}");
        let out = inline_slanted(svg, &[]).unwrap();
        assert!(
            out.contains(r#"<text x="5" y="10" transform="translate(2.126 0) skewX(-12)">a<"#),
            "{out}"
        );
        assert!(
            out.contains(r#"transform="rotate(90) translate(4.251 0) skewX(-12)""#),
            "{out}"
        );
        assert!(out.contains(r#"<text y="30">c<"#), "{out}");
    }

    #[test]
    fn split_spans_are_slanted_on_their_own_line() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><text x="0" y="10"><tspan>a</tspan><tspan x="0" y="40" font-style="italic">b</tspan></text></svg>"#;
        let out = inline_slanted(svg, &[]).unwrap();
        assert!(out.contains(r#"<text x="0" y="10">a</text>"#), "{out}");
        assert!(out.contains(r#"<text x="0" y="40" font-style="italic" transform="translate(8.502 0) skewX(-12)">b</text>"#), "{out}");
    }
    use super::*;

    #[test]
    fn hinted_spans_split_only_for_coresvg() {
        // `data-x` のある `<tspan>` は CoreSVG のときだけ分け（両端の空白は描かない）、`data-coresvg-only` の線も
        // CoreSVG のときだけ残す。ほかの描き手には流れる文と text-decoration をそのまま渡す
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><text x="50" y="10" text-anchor="middle"><tspan data-x="20">plain </tspan><tspan data-x="45" text-decoration="underline">under</tspan><tspan data-x="70"> </tspan><tspan data-x="70" font-weight="bold">red</tspan></text><g data-coresvg-only="1"><path d="M45,12h20"/></g></svg>"#;
        let flow = inline(svg).unwrap();
        assert_eq!(
            flow,
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><text x="50" y="10" text-anchor="middle"><tspan>plain </tspan><tspan text-decoration="underline">under</tspan><tspan> </tspan><tspan font-weight="700">red</tspan></text></svg>"#
        );
        let split = inline_slanted(svg, &[]).unwrap();
        assert_eq!(
            split,
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><text x="20" y="10" text-anchor="start">plain</text><text x="45" y="10" text-anchor="start">under</text><text x="70" y="10" text-anchor="start" font-weight="700">red</text><g><path d="M45,12h20"/></g></svg>"#
        );
    }

    #[test]
    fn rules_become_attributes_in_cascade_order() {
        let svg = r##"<svg id="m" xmlns="http://www.w3.org/2000/svg" width="100%" viewBox="0 0 40 20" style="max-width:40px"><style>#m{fill:#111;font-size:14px}#m .edge{fill:none;stroke:#333}#m .node rect{fill:#eee}#m .node.hot rect{fill:red!important}rect{fill:blue}#m g>path:first-of-type{stroke-width:3}#m .x:hover{fill:pink}@keyframes a{from{opacity:0}}#m .y:not(.z){opacity:.5}</style><g class="node"><rect fill="green" width="5" height="5"/></g><g class="node hot"><rect style="fill:yellow" width="5" height="5"/></g><g><path class="edge" d="M0 0"/><path class="edge" d="M1 1"/></g><circle class="y" r="1"/><circle class="y z" r="1"/></svg>"##;
        let out = inline(svg).unwrap();
        assert!(!out.contains("<style"), "{out}");
        // 根: 大きさは viewBox から、継がれる性質は属性に
        assert!(out.starts_with(r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" id="m" width="40" viewBox="0 0 40 20" fill="#111" font-size="14px" height="20">"##), "{out}");
        // 詳細度で #m .node rect が rect と属性に勝つ
        assert!(
            out.contains(r##"<g class="node"><rect fill="#eee" width="5" height="5"/></g>"##),
            "{out}"
        );
        // !important が style 属性に勝つ
        assert!(
            out.contains(r#"<rect width="5" height="5" fill="red"/>"#),
            "{out}"
        );
        // 子の結合と :first-of-type
        assert!(out.contains(r##"<path class="edge" d="M0 0" fill="none" stroke="#333" stroke-width="3"/><path class="edge" d="M1 1" fill="none" stroke="#333"/>"##), "{out}");
        // :hover の規則は捨て、:not は当てる
        assert!(!out.contains("pink"));
        assert!(
            out.contains(r#"<circle class="y" r="1" opacity=".5"/><circle class="y z" r="1"/>"#),
            "{out}"
        );
    }

    #[test]
    fn css_variables_and_the_font_shorthand_are_resolved() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" width="10" height="10" style="max-width:100%;height:auto;--_fg:var(--pele-fg,#1f1f1f);--_s:var(--pele-surface,#f3f3f3);color:var(--_fg);--_r:var(--pele-radius,4px);font:16px sans-serif;font-family:var(--pele-font,sans-serif)" fill="var(--_fg)"><g style="--_s:#abc"><rect fill="var(--_s)" style="rx:var(--_r)"/></g><rect fill="var(--_s)" stroke="var(--nope)"/></svg>"##;
        let out = inline(svg).unwrap();
        assert!(!out.contains("var("), "{out}");
        assert!(
            out.contains(r##"fill="#1f1f1f""##) && out.contains(r##"color="#1f1f1f""##),
            "{out}"
        );
        assert!(
            out.contains(r#"height="10""#) && out.contains(r#"font-size="16px""#),
            "{out}"
        );
        assert!(out.contains(r#"font-family="sans-serif""#), "{out}");
        assert!(out.contains(r##"<rect fill="#abc" rx="4px"/>"##), "{out}");
        assert!(
            out.contains(r##"<rect fill="#f3f3f3" stroke=""/>"##),
            "{out}"
        );
    }

    #[test]
    fn images_use_xlink_href_for_coresvg() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><image width="4" height="4" href="data:image/png;base64,AA=="/></svg>"#;
        let plain = inline(svg).unwrap();
        assert!(
            plain.contains(r#" href="data:image/png;base64,AA==""#),
            "{plain}"
        );
        let core = inline_slanted(svg, &[]).unwrap();
        assert!(
            core.contains(
                r#"<image width="4" height="4" xlink:href="data:image/png;base64,AA=="/>"#
            ),
            "{core}"
        );
        assert!(!core.contains(" href="), "{core}");
    }

    #[test]
    fn keeps_text_namespaces_and_escapes() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="10" height="10"><use xlink:href="#a"/><text xml:space="preserve">a &lt; b &amp; "c"</text></svg>"##;
        let out = inline(svg).unwrap();
        assert!(out.contains(r##"<use xlink:href="#a"/>"##), "{out}");
        assert!(
            out.contains(r#"<text xml:space="preserve">a &lt; b &amp; "c"</text>"#),
            "{out}"
        );
        assert!(inline("<svg").is_none());
    }
}
