//! hunk の見出しに添える関数名（`@@ -1,3 +1,4 @@ fn main() {`）。git と同じく、hunk の手前の行を上へたどり、
//! 規則に合った最初の行を出す。規則は `.gitattributes` の `diff=<名前>` の `diff.<名前>.xfuncname`（拡張正規表現）
//! か `funcname`（基本正規表現）、なければ git の既定（英字・`_`・`$` で始まる行）。

use amonhen_model::FuncNameRule;
use regex::bytes::Regex;

/// git が見出しに添える関数名の長さの上限（バイト）。
const MAX_LEN: usize = 80;

/// 関数名の行を探す規則。
#[derive(Clone, Debug, Default)]
pub enum FuncName {
    /// git の既定: 英字・`_`・`$` で始まる行。
    #[default]
    Default,
    /// 正規表現を順に試し、最初に合ったものを使う（否定のものが合えば、その行は関数名ではない）。
    Patterns(Vec<(Regex, bool)>),
}

impl FuncName {
    /// 規則から作る。正規表現が読めなければ既定の規則にする（見出しが出ないより、既定のほうがよい）。
    pub fn new(rule: Option<&FuncNameRule>) -> Self {
        let Some(rule) = rule else {
            return Self::Default;
        };
        let mut patterns = Vec::new();
        for line in rule.pattern.split('\n') {
            let (negate, source) = match line.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            let translated = translate(source, rule.extended);
            match Regex::new(&translated) {
                Ok(re) => patterns.push((re, negate)),
                Err(_) => return Self::Default,
            }
        }
        Self::Patterns(patterns)
    }

    /// `line`（改行を除いた 1 行）が関数名の行なら、見出しに添える文字列。
    pub fn matches(&self, line: &[u8]) -> Option<String> {
        let found = match self {
            Self::Default => {
                let first = *line.first()?;
                if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
                    return None;
                }
                line
            }
            Self::Patterns(patterns) => {
                let (re, negate) = patterns.iter().find(|(re, _)| re.is_match(line))?;
                if *negate {
                    return None;
                }
                let caps = re.captures(line)?;
                caps.get(1).or_else(|| caps.get(0))?.as_bytes()
            }
        };
        let mut found = &found[..found.len().min(MAX_LEN)];
        while let Some((last, rest)) = found.split_last() {
            if !last.is_ascii_whitespace() {
                break;
            }
            found = rest;
        }
        if found.is_empty() {
            return None;
        }
        // 上限で UTF-8 の途中を切ったなら、その文字は落とす。
        let found = match std::str::from_utf8(found) {
            Err(e) if e.error_len().is_none() => &found[..e.valid_up_to()],
            _ => found,
        };
        Some(String::from_utf8_lossy(found).into_owned())
    }
}

/// POSIX の正規表現（基本・拡張）を regex の書き方にする。角かっこの中の `\`・`[`・`&`・`~` は文字そのもの、
/// 基本正規表現では `\(` `\)` `\{` `\}` `\|` `\+` `\?` が特別で、`(` `)` `{` `}` `|` `+` `?` は文字そのもの。
fn translate(source: &str, extended: bool) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '[' => {
                // 角かっこの式: `]` は先頭（`^` の後）なら文字そのもの。`[:alpha:]` などはそのまま。
                out.push('[');
                i += 1;
                if chars.get(i) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                if chars.get(i) == Some(&']') {
                    out.push_str("\\]");
                    i += 1;
                }
                while i < chars.len() && chars[i] != ']' {
                    let c = chars[i];
                    if c == '[' && matches!(chars.get(i + 1), Some(':' | '.' | '=')) {
                        let kind = chars[i + 1];
                        let end = (i + 2..chars.len().saturating_sub(1))
                            .find(|&j| chars[j] == kind && chars[j + 1] == ']');
                        if let Some(end) = end {
                            out.extend(&chars[i..end + 2]);
                            i = end + 2;
                            continue;
                        }
                    }
                    if matches!(c, '\\' | '[' | '&' | '~') {
                        out.push('\\');
                    }
                    out.push(c);
                    i += 1;
                }
                if i < chars.len() {
                    out.push(']');
                    i += 1;
                }
            }
            '\\' if !extended => {
                match chars.get(i + 1) {
                    Some(&n @ ('(' | ')' | '{' | '}' | '|' | '+' | '?')) => out.push(n),
                    Some(&n) => {
                        out.push('\\');
                        out.push(n);
                    }
                    None => out.push_str("\\\\"),
                }
                i += 2;
            }
            '(' | ')' | '{' | '}' | '|' | '+' | '?' if !extended => {
                out.push('\\');
                out.push(c);
                i += 1;
            }
            '\\' => {
                match chars.get(i + 1) {
                    Some(&n) => {
                        out.push('\\');
                        out.push(n);
                    }
                    None => out.push_str("\\\\"),
                }
                i += 2;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(pattern: &str, extended: bool) -> FuncName {
        FuncName::new(Some(&FuncNameRule {
            pattern: pattern.to_owned(),
            extended,
        }))
    }

    #[test]
    fn the_default_takes_lines_starting_with_a_letter() {
        let f = FuncName::Default;
        assert_eq!(f.matches(b"fn main() {  ").as_deref(), Some("fn main() {"));
        assert_eq!(f.matches(b"_start:").as_deref(), Some("_start:"));
        assert_eq!(f.matches(b"$x = 1").as_deref(), Some("$x = 1"));
        assert_eq!(f.matches(b"    let a = 1;"), None);
        assert_eq!(f.matches(b"}"), None);
        assert_eq!(f.matches(b""), None);
    }

    #[test]
    fn long_lines_are_cut_at_80_bytes() {
        let line = format!("fn {}", "a".repeat(100));
        assert_eq!(
            FuncName::Default.matches(line.as_bytes()).unwrap().len(),
            80
        );
        // UTF-8 の途中で切れた文字は落とす。
        let line = format!("f{}", "あ".repeat(40));
        let got = FuncName::Default.matches(line.as_bytes()).unwrap();
        assert_eq!(got, format!("f{}", "あ".repeat(26)));
    }

    #[test]
    fn a_pattern_takes_the_first_group_or_the_whole_match() {
        let f = rule(r"^[ \t]*def (\w+)", true);
        assert_eq!(f.matches(b"    def run(self):").as_deref(), Some("run"));
        let f = rule(r"^[ \t]*class [A-Za-z]+", true);
        assert_eq!(
            f.matches(b"  class Foo(Base):").as_deref(),
            Some("  class Foo")
        );
        assert_eq!(f.matches(b"x = 1"), None);
    }

    #[test]
    fn a_negated_pattern_that_matches_rejects_the_line() {
        let f = rule("!^[ \t]*(if|while)\n^[ \t]*[a-z].*\\(", true);
        assert_eq!(f.matches(b"  if (x) {"), None);
        assert_eq!(f.matches(b"  run(x) {").as_deref(), Some("  run("));
    }

    #[test]
    fn basic_regular_expressions_are_translated() {
        let f = rule(r"^\(sub\|proc\) \([a-z]*\)", false);
        assert_eq!(f.matches(b"sub hello {").as_deref(), Some("sub"));
        // 基本正規表現の `(` は文字そのもの。
        let f = rule(r"^fn (", false);
        assert_eq!(f.matches(b"fn (x)").as_deref(), Some("fn ("));
    }

    #[test]
    fn bracket_expressions_follow_posix() {
        let f = rule(r"^[[:alpha:]_][^\]]*", true);
        assert_eq!(f.matches(b"ab\\c]d").as_deref(), Some("ab"));
        let f = rule(r"^[]a]+", true);
        assert_eq!(f.matches(b"]a]b").as_deref(), Some("]a]"));
    }

    #[test]
    fn a_broken_pattern_falls_back_to_the_default() {
        assert!(matches!(rule(r"^\k", true), FuncName::Default));
    }
}
