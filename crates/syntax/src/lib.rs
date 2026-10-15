//! tree-sitter による構文の色付け。ファイル全体を解析し、重ならないトークン（バイト範囲と種類）を返す。
//!
//! 差分の断片ではなくファイル全体を解析するので、hunk の途中から始まるコメントや文字列も正しく色が付く。
//! 種類は `amonhen_layout::SyntaxSpan` の定数で、色は FE が決める。

use std::cell::RefCell;
use std::sync::OnceLock;

use amonhen_layout::SyntaxSpan as S;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

/// 色付けするトークン。`start..end` はソースのバイト範囲。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: u32,
    pub end: u32,
    pub kind: u8,
}

/// これより大きいファイルは色付けしない（解析に時間がかかり、差分を見る役には立たない）。
const MAX_SOURCE: usize = 4 << 20;

/// tree-sitter のキャプチャ名と種類。キャプチャ名は、ここの名前の部分をすべて含むもののうち、
/// 部分の数がいちばん多いものに割り当てられる（`function.method` は `function`）。
const NAMES: &[(&str, u8)] = &[
    ("keyword", S::KEYWORD),
    ("include", S::KEYWORD),
    ("conditional", S::KEYWORD),
    ("repeat", S::KEYWORD),
    ("string", S::STRING),
    ("character", S::STRING),
    ("string.escape", S::ESCAPE),
    ("string.special", S::ESCAPE),
    ("string.regex", S::ESCAPE),
    ("escape", S::ESCAPE),
    ("comment", S::COMMENT),
    ("number", S::NUMBER),
    ("float", S::NUMBER),
    ("boolean", S::CONSTANT),
    ("constant", S::CONSTANT),
    ("function", S::FUNCTION),
    ("method", S::FUNCTION),
    ("constructor", S::TYPE),
    ("type", S::TYPE),
    ("property", S::PROPERTY),
    ("field", S::PROPERTY),
    ("attribute", S::ATTRIBUTE),
    ("label", S::ATTRIBUTE),
    ("tag", S::TAG),
    ("operator", S::OPERATOR),
    ("variable.builtin", S::VARIABLE),
    ("variable.parameter", S::VARIABLE),
    ("module", S::MODULE),
    ("namespace", S::MODULE),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Bash,
    C,
    CSharp,
    Cpp,
    Css,
    Go,
    Html,
    Java,
    JavaScript,
    Json,
    Lua,
    Make,
    Python,
    Ruby,
    Rust,
    Swift,
    Toml,
    Tsx,
    TypeScript,
    Yaml,
}

const LANGUAGES: usize = 20;

impl Language {
    /// ファイル名（拡張子）から言語を決める。わからなければ `None`。
    pub fn from_path(path: &str) -> Option<Self> {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        match name {
            "Makefile" | "makefile" | "GNUmakefile" => return Some(Self::Make),
            "Gemfile" | "Rakefile" => return Some(Self::Ruby),
            ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" => return Some(Self::Bash),
            _ => {}
        }
        let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match ext.as_str() {
            "sh" | "bash" | "zsh" => Self::Bash,
            "c" | "h" => Self::C,
            "cs" => Self::CSharp,
            "cc" | "cpp" | "cxx" | "c++" | "hh" | "hpp" | "hxx" | "h++" | "ipp" | "inl" | "mm" => {
                Self::Cpp
            }
            "css" => Self::Css,
            "go" => Self::Go,
            "html" | "htm" | "xhtml" => Self::Html,
            "java" => Self::Java,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "json" | "jsonc" => Self::Json,
            "lua" => Self::Lua,
            "mk" | "mak" => Self::Make,
            "py" | "pyi" | "pyw" => Self::Python,
            "rb" | "rake" | "gemspec" => Self::Ruby,
            "rs" => Self::Rust,
            "swift" => Self::Swift,
            "toml" => Self::Toml,
            "tsx" => Self::Tsx,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "yml" | "yaml" => Self::Yaml,
            _ => return None,
        })
    }

    fn config(self) -> Option<&'static HighlightConfiguration> {
        static CONFIGS: [OnceLock<Option<HighlightConfiguration>>; LANGUAGES] =
            [const { OnceLock::new() }; LANGUAGES];
        CONFIGS[self as usize]
            .get_or_init(|| {
                let (language, query) = self.grammar();
                let mut config = HighlightConfiguration::new(
                    language.into(),
                    format!("{self:?}"),
                    &query,
                    "",
                    "",
                )
                .ok()?;
                let names: Vec<&str> = NAMES.iter().map(|(n, _)| *n).collect();
                config.configure(&names);
                Some(config)
            })
            .as_ref()
    }

    fn grammar(self) -> (tree_sitter_language::LanguageFn, String) {
        let one = |l, q: &str| (l, q.to_owned());
        match self {
            Self::Bash => one(
                tree_sitter_bash::LANGUAGE,
                tree_sitter_bash::HIGHLIGHT_QUERY,
            ),
            Self::C => one(tree_sitter_c::LANGUAGE, tree_sitter_c::HIGHLIGHT_QUERY),
            Self::CSharp => one(
                tree_sitter_c_sharp::LANGUAGE,
                tree_sitter_c_sharp::HIGHLIGHTS_QUERY,
            ),
            // C++ のクエリは C のクエリに足す形で書かれている。
            Self::Cpp => (
                tree_sitter_cpp::LANGUAGE,
                [
                    tree_sitter_cpp::HIGHLIGHT_QUERY,
                    tree_sitter_c::HIGHLIGHT_QUERY,
                ]
                .join("\n"),
            ),
            Self::Css => one(tree_sitter_css::LANGUAGE, tree_sitter_css::HIGHLIGHTS_QUERY),
            Self::Go => one(tree_sitter_go::LANGUAGE, tree_sitter_go::HIGHLIGHTS_QUERY),
            Self::Html => one(
                tree_sitter_html::LANGUAGE,
                tree_sitter_html::HIGHLIGHTS_QUERY,
            ),
            Self::Java => one(
                tree_sitter_java::LANGUAGE,
                tree_sitter_java::HIGHLIGHTS_QUERY,
            ),
            Self::JavaScript => (
                tree_sitter_javascript::LANGUAGE,
                [
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                ]
                .join("\n"),
            ),
            Self::Json => one(
                tree_sitter_json::LANGUAGE,
                tree_sitter_json::HIGHLIGHTS_QUERY,
            ),
            Self::Lua => one(tree_sitter_lua::LANGUAGE, tree_sitter_lua::HIGHLIGHTS_QUERY),
            Self::Make => one(
                tree_sitter_make::LANGUAGE,
                tree_sitter_make::HIGHLIGHTS_QUERY,
            ),
            Self::Python => one(
                tree_sitter_python::LANGUAGE,
                tree_sitter_python::HIGHLIGHTS_QUERY,
            ),
            Self::Ruby => one(
                tree_sitter_ruby::LANGUAGE,
                tree_sitter_ruby::HIGHLIGHTS_QUERY,
            ),
            Self::Rust => one(
                tree_sitter_rust::LANGUAGE,
                tree_sitter_rust::HIGHLIGHTS_QUERY,
            ),
            Self::Swift => one(
                tree_sitter_swift::LANGUAGE,
                tree_sitter_swift::HIGHLIGHTS_QUERY,
            ),
            Self::Toml => one(
                tree_sitter_toml_ng::LANGUAGE,
                tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
            ),
            // TypeScript のクエリは JavaScript のクエリに足す形で書かれている（先に書いた方が優先）。
            Self::TypeScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
                [
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                ]
                .join("\n"),
            ),
            Self::Tsx => (
                tree_sitter_typescript::LANGUAGE_TSX,
                [
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                ]
                .join("\n"),
            ),
            Self::Yaml => one(
                tree_sitter_yaml::LANGUAGE,
                tree_sitter_yaml::HIGHLIGHTS_QUERY,
            ),
        }
    }
}

/// `source` を `path` の拡張子から決めた言語で解析して色付けする。言語がわからない、大きすぎる、
/// 解析できないときは空。トークンは位置の順に並び、重ならない。
pub fn highlight(path: &str, source: &[u8]) -> Vec<Token> {
    let Some(config) = Language::from_path(path).and_then(Language::config) else {
        return Vec::new();
    };
    if source.len() > MAX_SOURCE {
        return Vec::new();
    }
    thread_local! {
        static HIGHLIGHTER: RefCell<Highlighter> = RefCell::new(Highlighter::new());
    }
    HIGHLIGHTER.with_borrow_mut(|highlighter| {
        let Ok(events) = highlighter.highlight(config, source, None, None, |_| None) else {
            return Vec::new();
        };
        let mut tokens: Vec<Token> = Vec::new();
        // 入れ子のキャプチャは、いちばん内側の種類を使う。
        let mut stack: Vec<u8> = Vec::new();
        for event in events {
            match event {
                Ok(HighlightEvent::HighlightStart(h)) => stack.push(NAMES[h.0].1),
                Ok(HighlightEvent::HighlightEnd) => {
                    stack.pop();
                }
                Ok(HighlightEvent::Source { start, end }) => {
                    let Some(&kind) = stack.last() else { continue };
                    let (start, end) = (start as u32, end as u32);
                    match tokens.last_mut() {
                        Some(last) if last.kind == kind && last.end == start => last.end = end,
                        _ => tokens.push(Token { start, end, kind }),
                    }
                }
                Err(_) => return Vec::new(),
            }
        }
        tokens
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(path: &str, source: &str) -> Vec<(&'static str, u8)> {
        let leaked: &'static str = Box::leak(source.to_owned().into_boxed_str());
        highlight(path, leaked.as_bytes())
            .into_iter()
            .map(|t| (&leaked[t.start as usize..t.end as usize], t.kind))
            .collect()
    }

    #[test]
    fn every_language_compiles_its_query() {
        for path in [
            "a.sh", "a.c", "a.cs", "a.cpp", "a.css", "a.go", "a.html", "a.java", "a.js", "a.json",
            "a.lua", "Makefile", "a.py", "a.rb", "a.rs", "a.swift", "a.toml", "a.tsx", "a.ts",
            "a.yaml",
        ] {
            let language = Language::from_path(path).unwrap();
            assert!(
                language.config().is_some(),
                "{language:?} のクエリを読めない"
            );
        }
    }

    #[test]
    fn colors_rust() {
        let k = kinds(
            "src/lib.rs",
            "// note\nfn main() { let s = \"hi\"; let n = 42; }\n",
        );
        assert!(k.contains(&("// note", S::COMMENT)), "{k:?}");
        assert!(k.contains(&("fn", S::KEYWORD)), "{k:?}");
        assert!(k.contains(&("main", S::FUNCTION)), "{k:?}");
        assert!(k.contains(&("\"hi\"", S::STRING)), "{k:?}");
        assert!(
            k.contains(&("42", S::CONSTANT)) || k.contains(&("42", S::NUMBER)),
            "{k:?}"
        );
    }

    #[test]
    fn a_comment_spanning_lines_is_one_token_per_run() {
        let k = kinds("a.c", "/* a\n b */ int x;\n");
        assert_eq!(k[0], ("/* a\n b */", S::COMMENT));
    }

    #[test]
    fn tokens_are_ordered_and_do_not_overlap() {
        let source = "const a = `x${1 + 2}y`; class A { m() { return this.a } }\n";
        let tokens = highlight("a.ts", source.as_bytes());
        assert!(!tokens.is_empty());
        assert!(
            tokens.windows(2).all(|w| w[0].end <= w[1].start),
            "{tokens:?}"
        );
    }

    #[test]
    fn unknown_languages_are_left_plain() {
        assert!(highlight("README", b"fn main() {}").is_empty());
        assert_eq!(Language::from_path("dir/Makefile"), Some(Language::Make));
        assert_eq!(Language::from_path("x/y.H"), Some(Language::C));
    }
}
