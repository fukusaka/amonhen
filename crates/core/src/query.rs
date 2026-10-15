//! History の検索の構文。
//!
//! ```text
//! fix parser                 件名・作者・ID に fix を含み、かつ parser を含む（空白は AND）
//! author:linus or author:greg
//! (path:fs/ or path:mm/) contents:"spin_lock"
//! commit:3857c2f subject:"Merge tag"
//! ```
//!
//! AND は OR より強く結び付く。`and` は書いてもよい（空白と同じ）。値に空白を含めるときは `"..."` で囲む。

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
    /// 差分に、その文字列を含む行があるコミット。
    Contents,
}

impl Field {
    /// コミットの本文を読めば判定できる（git に問い合わせずに済む）。
    pub fn is_textual(&self) -> bool {
        matches!(
            self,
            Self::Any | Self::Author | Self::Subject | Self::Commit
        )
    }
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
                    return Err("引用符が閉じていません".into());
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
                        return Err("括弧が閉じていません".into());
                    }
                    self.at += 1;
                }
                _ => break,
            }
        }
        match all.len() {
            0 => Err("条件がありません".into()),
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
        return Err("余分な括弧があります".into());
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
    fn reports_unbalanced_input() {
        assert!(parse("(a b").is_err());
        assert!(parse("a b)").is_err());
        assert!(parse(r#"subject:"open"#).is_err());
        assert!(parse("a or").is_err());
    }
}
