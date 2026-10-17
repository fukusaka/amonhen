//! 利用者に見せる文の訳。コードには英語で書き（それが鍵）、日本語は `i18n/ja.strings`（アプリの
//! `Localizable.strings` と同じ形）。言語は画面を出す側が決める: アプリは FFI（`amonhen_set_language`）、CLI は
//! `LC_ALL`・`LC_MESSAGES`・`LANG`、共有の daemon と SSH 先は、つないだ相手が挨拶で伝えた言語をその相手への答えに
//! 使う（`with`）。訳がない言語は英語。
//!
//! 文は `tr!` で作る。差し込む値は `{name}` と書き、`tr!("Can’t open {path}: {e}", path = p, e = e)` のように渡す
//! （日本語の訳の側も同じ名前を使うので、語順を変えられる）。

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// 訳のある言語。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En = 0,
    Ja = 1,
}

impl Lang {
    /// `ja`・`ja-JP`・`ja_JP.UTF-8` などから。訳のない言語は英語。
    pub fn parse(tag: &str) -> Lang {
        let primary = tag.split(['-', '_', '.', '@']).next().unwrap_or("");
        if primary.eq_ignore_ascii_case("ja") {
            Lang::Ja
        } else {
            Lang::En
        }
    }

    /// 挨拶で伝えるときの名前。
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ja => "ja",
        }
    }

    /// `AMONHEN_LANG`（daemon を起動した側が渡す）、なければ POSIX の決まり（`LC_ALL` → `LC_MESSAGES` → `LANG`）で
    /// 選ぶ。CLI と daemon 向け。
    pub fn from_env() -> Lang {
        ["AMONHEN_LANG", "LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .map(|value| Lang::parse(&value))
            .unwrap_or(Lang::En)
    }
}

static DEFAULT: AtomicU8 = AtomicU8::new(Lang::En as u8);

thread_local! {
    static CURRENT: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// プロセスの既定の言語（アプリ・CLI が起動したときに決める）。
pub fn set_default(lang: Lang) {
    DEFAULT.store(lang as u8, Ordering::Relaxed);
}

/// 今の言語（`with` の中ならその言語、なければプロセスの既定）。
pub fn current() -> Lang {
    CURRENT.with(|c| c.get()).unwrap_or_else(|| {
        if DEFAULT.load(Ordering::Relaxed) == Lang::Ja as u8 {
            Lang::Ja
        } else {
            Lang::En
        }
    })
}

/// `f` の間だけ、このスレッドの言語を `lang` にする（daemon が、つないだ相手の言語で答えを作るとき）。
pub fn with<R>(lang: Lang, f: impl FnOnce() -> R) -> R {
    let previous = CURRENT.with(|c| c.replace(Some(lang)));
    struct Restore(Option<Lang>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(previous);
    f()
}

fn japanese() -> &'static HashMap<&'static str, String> {
    static TABLE: OnceLock<HashMap<&'static str, String>> = OnceLock::new();
    TABLE.get_or_init(|| parse_strings(include_str!("i18n/ja.strings")))
}

/// `"key" = "value";` の並び（`/* */` のコメント、`\"`・`\\`・`\n`・`\t` のエスケープ）を読む。
fn parse_strings(text: &'static str) -> HashMap<&'static str, String> {
    let mut table = HashMap::new();
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("/*") {
            rest = after.split_once("*/").map_or("", |(_, r)| r);
            continue;
        }
        let Some(after) = rest.strip_prefix('"') else {
            break;
        };
        let Some((key, after)) = quoted(after) else {
            break;
        };
        let Some(after) = after.trim_start().strip_prefix('=') else {
            break;
        };
        let Some(after) = after.trim_start().strip_prefix('"') else {
            break;
        };
        let Some((value, after)) = quoted(after) else {
            break;
        };
        rest = after.trim_start().strip_prefix(';').unwrap_or(after);
        // 鍵はコードの中の文字列と比べるので、エスケープを戻してから leak せずに持つ（表は一度だけ作る）。
        table.insert(&*Box::leak(unescape(key).into_boxed_str()), unescape(value));
    }
    table
}

/// `"` の閉じるまで（エスケープを含めた生の中身と、その後ろ）。
fn quoted(text: &str) -> Option<(&str, &str)> {
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return Some((&text[..i], &text[i + 1..])),
            _ => escaped = false,
        }
    }
    None
}

fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// `key`（英語）の今の言語の文。訳がなければ英語のまま。
pub fn translate(key: &'static str) -> Cow<'static, str> {
    match current() {
        Lang::En => Cow::Borrowed(key),
        Lang::Ja => japanese()
            .get(key)
            .map_or(Cow::Borrowed(key), |v| Cow::Owned(v.clone())),
    }
}

/// `template` の `{name}` に値を入れる。知らない名前はそのまま残す。
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if args.iter().any(|(n, _)| *n == &after[..close]) => {
                let name = &after[..close];
                let (_, value) = args.iter().find(|(n, _)| *n == name).unwrap();
                out.push_str(&value.to_string());
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 取り消したときの文（どの言語でも、`is_cancelled` で見分けられる）。
pub fn cancelled() -> String {
    translate("Cancelled").into_owned()
}

/// `message` が取り消しの文か（daemon・SSH 越しでは文として届くので、どの言語の文でも見分ける）。
pub fn is_cancelled(message: &str) -> bool {
    let message = message.trim();
    message == "Cancelled" || japanese().get("Cancelled").is_some_and(|ja| ja == message)
}

/// 利用者に見せる文を、今の言語で作る。`tr!("Text")`、`tr!("Text {name}", name = value)`。
#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        $crate::i18n::translate($key).into_owned()
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill(
            &$crate::i18n::translate($key),
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_by_language_and_fills_named_values() {
        let table = parse_strings(
            "/* c */\n\"Can’t open {path}: {e}\" = \"{path} を開けない: {e}\";\n\"A \\\"q\\\"\\n\" = \"エー\";",
        );
        assert_eq!(table["Can’t open {path}: {e}"], "{path} を開けない: {e}");
        assert_eq!(table["A \"q\"\n"], "エー");
        assert_eq!(
            fill("{path} を開けない: {e}", &[("path", &"x"), ("e", &42)]),
            "x を開けない: 42"
        );
        // 知らない名前や閉じない括弧はそのまま。
        assert_eq!(fill("{a} {b} {", &[("a", &1)]), "1 {b} {");
        assert_eq!(Lang::parse("ja_JP.UTF-8"), Lang::Ja);
        assert_eq!(Lang::parse("en-US"), Lang::En);
        assert_eq!(Lang::parse("fr"), Lang::En);
        with(Lang::Ja, || {
            assert_eq!(current(), Lang::Ja);
            assert!(is_cancelled(&cancelled()));
        });
        assert_eq!(current(), Lang::En);
        assert_eq!(tr!("Cancelled"), "Cancelled");
        assert!(is_cancelled("Cancelled"));
    }
}
