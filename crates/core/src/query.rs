//! History の検索の構文。
//!
//! ```text
//! fix parser                 件名・作者・ID に fix を含み、かつ parser を含む（空白は AND）
//! author:linus or author:greg
//! (path:fs/ or path:mm/) contents:"spin_lock"
//! commit:3857c2f subject:"Merge tag"
//! contents:kmalloc after:"1 year ago" before:2026-01-01
//! ```
//!
//! AND は OR より強く結び付く。`and` は書いてもよい（空白と同じ）。値に空白を含めるときは `"..."` で囲む。

use amonhen_model::tr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Field {
    /// 件名・作者名・コミット ID の先頭のどれか。
    Any,
    Author,
    Subject,
    /// コミット ID の先頭。
    Commit,
    /// そのパス（の下）を変えたコミット。
    Path,
    /// 差分に、その文字列を含む行があるコミット。同じ AND に `path:` があれば、そのパスの差分だけを探す
    /// （`git log -G<文字列> -- <パス>`）。
    Contents,
    /// その日時より後のコミット（`git log --after`。値は日付か `1 year ago` のような相対の日時）。
    After,
    /// その日時より前のコミット（`git log --before`）。
    Before,
}

impl Field {
    /// コミットの本文を読めば判定できる（git に問い合わせずに済む）。
    pub fn is_textual(&self) -> bool {
        matches!(
            self,
            Self::Any | Self::Author | Self::Subject | Self::Commit
        )
    }

    /// コミットの日時で判定する。
    pub fn is_date(&self) -> bool {
        matches!(self, Self::After | Self::Before)
    }
}

/// 内容の条件（`contents:`）を探す範囲。同じ AND にある `path:`（`path:` だけの OR も）と `after:`・`before:`。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub paths: Vec<String>,
    pub after: Vec<String>,
    pub before: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    pub field: Field,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    Term(Term),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

impl Expr {
    /// 葉の条件を左から順に。
    pub fn terms(&self) -> Vec<&Term> {
        match self {
            Self::Term(t) => vec![t],
            Self::And(v) | Self::Or(v) => v.iter().flat_map(Self::terms).collect(),
        }
    }

    /// 葉の条件（`terms` と同じ順）ごとの、内容の条件を探す範囲（内容の条件でなければ `None`）。AND の中の
    /// 内容の条件は、その AND の `path:` と日時の条件を満たすコミットしか結果に残らないので、git にもその範囲だけを
    /// 探させる。OR の中は、隣の条件に縛られない。
    pub fn contents_scopes(&self) -> Vec<Option<Scope>> {
        fn walk(expr: &Expr, scope: &Scope, out: &mut Vec<Option<Scope>>) {
            match expr {
                Expr::Term(t) => out.push((t.field == Field::Contents).then(|| scope.clone())),
                Expr::Or(v) => v.iter().for_each(|e| walk(e, scope, out)),
                Expr::And(v) => {
                    let mut inner = scope.clone();
                    // パスの条件は最初の 1 つ（内側の AND のものが優先）。2 つ目からは結果を絞るだけ。
                    if let Some(paths) = v.iter().find_map(Expr::paths_only) {
                        inner.paths = paths;
                    }
                    for e in v {
                        if let Expr::Term(t) = e {
                            match t.field {
                                Field::After => inner.after.push(t.value.clone()),
                                Field::Before => inner.before.push(t.value.clone()),
                                _ => {}
                            }
                        }
                    }
                    v.iter().for_each(|e| walk(e, &inner, out));
                }
            }
        }
        let mut out = Vec::new();
        walk(self, &Scope::default(), &mut out);
        out
    }

    /// `path:` か、`path:` だけの OR なら、そのパス。
    fn paths_only(&self) -> Option<Vec<String>> {
        match self {
            Self::Term(t) if t.field == Field::Path => Some(vec![t.value.clone()]),
            Self::Or(v) => v
                .iter()
                .map(Self::paths_only)
                .collect::<Option<Vec<_>>>()
                .map(|p| p.concat()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    Or,
    Word(String),
}

fn tokenize(query: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = query.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '(' => {
                chars.next();
                tokens.push(Token::Open);
            }
            ')' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '|' => {
                chars.next();
                tokens.push(Token::Or);
            }
            _ => {
                // `key:"a b"` や `"a b"` は引用符の中の空白も含めて 1 語。
                let mut word = String::new();
                let mut quoted = false;
                while let Some(&c) = chars.peek() {
                    if c == '"' {
                        quoted = !quoted;
                        chars.next();
                    } else if !quoted && (c.is_whitespace() || c == '(' || c == ')' || c == '|') {
                        break;
                    } else {
                        word.push(c);
                        chars.next();
                    }
                }
                if quoted {
                    return Err(tr!("Unclosed quotation mark"));
                }
                match word.to_ascii_lowercase().as_str() {
                    "or" => tokens.push(Token::Or),
                    "and" => {}
                    _ if !word.is_empty() => tokens.push(Token::Word(word)),
                    _ => {}
                }
            }
        }
    }
    Ok(tokens)
}

fn term(word: &str) -> Term {
    if let Some((key, value)) = word.split_once(':') {
        let field = match key.to_ascii_lowercase().as_str() {
            "author" => Some(Field::Author),
            "subject" | "message" => Some(Field::Subject),
            "commit" | "id" => Some(Field::Commit),
            "path" | "file" => Some(Field::Path),
            "contents" | "content" => Some(Field::Contents),
            "after" | "since" => Some(Field::After),
            "before" | "until" => Some(Field::Before),
            _ => None,
        };
        if let Some(field) = field
            && !value.is_empty()
        {
            return Term {
                field,
                value: value.to_owned(),
            };
        }
    }
    // 知らない接頭辞（`fix:` など）は、語そのものを探す。
    Term {
        field: Field::Any,
        value: word.to_owned(),
    }
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn or(&mut self) -> Result<Expr, String> {
        let mut alternatives = vec![self.and()?];
        while self.tokens.get(self.at) == Some(&Token::Or) {
            self.at += 1;
            alternatives.push(self.and()?);
        }
        Ok(if alternatives.len() == 1 {
            alternatives.pop().unwrap()
        } else {
            Expr::Or(alternatives)
        })
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut all = Vec::new();
        loop {
            match self.tokens.get(self.at) {
                Some(Token::Word(w)) => {
                    all.push(Expr::Term(term(w)));
                    self.at += 1;
                }
                Some(Token::Open) => {
                    self.at += 1;
                    all.push(self.or()?);
                    if self.tokens.get(self.at) != Some(&Token::Close) {
                        return Err(tr!("Unclosed parenthesis"));
                    }
                    self.at += 1;
                }
                _ => break,
            }
        }
        match all.len() {
            0 => Err(tr!("No search condition")),
            1 => Ok(all.pop().unwrap()),
            _ => Ok(Expr::And(all)),
        }
    }
}

/// 検索の文字列を読む。空なら `None`。
pub fn parse(query: &str) -> Result<Option<Expr>, String> {
    let tokens = tokenize(query)?;
    if tokens.is_empty() {
        return Ok(None);
    }
    let mut parser = Parser { tokens, at: 0 };
    let expr = parser.or()?;
    if parser.at != parser.tokens.len() {
        return Err(tr!("Extra parenthesis"));
    }
    Ok(Some(expr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(field: Field, value: &str) -> Expr {
        Expr::Term(Term {
            field,
            value: value.into(),
        })
    }

    #[test]
    fn words_are_anded_and_or_binds_looser() {
        assert_eq!(
            parse("fix parser").unwrap().unwrap(),
            Expr::And(vec![t(Field::Any, "fix"), t(Field::Any, "parser")])
        );
        assert_eq!(
            parse("author:linus or author:greg and fs")
                .unwrap()
                .unwrap(),
            Expr::Or(vec![
                t(Field::Author, "linus"),
                Expr::And(vec![t(Field::Author, "greg"), t(Field::Any, "fs")]),
            ])
        );
    }

    #[test]
    fn parentheses_quotes_and_prefixes() {
        assert_eq!(
            parse(r#"(path:fs/ | path:"mm dir") contents:"spin_lock(""#)
                .unwrap()
                .unwrap(),
            Expr::And(vec![
                Expr::Or(vec![t(Field::Path, "fs/"), t(Field::Path, "mm dir")]),
                t(Field::Contents, "spin_lock("),
            ])
        );
        // 知らない接頭辞や空の値は、語そのものを探す。
        assert_eq!(parse("fix:").unwrap().unwrap(), t(Field::Any, "fix:"));
        assert_eq!(parse("  ").unwrap(), None);
    }

    #[test]
    fn contents_are_searched_within_their_paths_and_dates() {
        let scopes = |q: &str| parse(q).unwrap().unwrap().contents_scopes();
        let scope = |paths: &[&str], after: &[&str]| {
            Some(Scope {
                paths: paths.iter().map(|p| p.to_string()).collect(),
                after: after.iter().map(|p| p.to_string()).collect(),
                before: Vec::new(),
            })
        };
        assert_eq!(
            scopes(r#"(path:fs/ or path:mm/) contents:spin after:"1 year ago""#),
            [None, None, scope(&["fs/", "mm/"], &["1 year ago"]), None]
        );
        // OR の隣の条件には縛られない。内側の AND は外側の日時も受け継ぐ。
        assert_eq!(scopes("contents:a or path:b"), [scope(&[], &[]), None]);
        assert_eq!(
            scopes("after:2025-01-01 (path:x contents:a or contents:b)"),
            [
                None,
                None,
                scope(&["x"], &["2025-01-01"]),
                scope(&[], &["2025-01-01"])
            ]
        );
        assert_eq!(
            parse("since:2025-01-01").unwrap().unwrap(),
            t(Field::After, "2025-01-01")
        );
    }

    #[test]
    fn reports_unbalanced_input() {
        assert!(parse("(a b").is_err());
        assert!(parse("a b)").is_err());
        assert!(parse(r#"subject:"open"#).is_err());
        assert!(parse("a or").is_err());
    }
}
