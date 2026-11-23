//! ishikawa の読み手（pele の `src/diagrams/ishikawa/parser.ts`）。

use super::db::IshikawaDb;
use super::lexer::{TOKEN_NAMES, t, tokenize};
use crate::errors::{Result, syntax_error};
use crate::js;

const STOP: &str = "'SPACELINE', 'NL', 'EOF'";

/// Mermaid の文法から Jison が作る読み手に従う: 先に空行とコメントがあってよく、次に語と多くても 1 つの改行、
/// そして文。文は文字の行（字下げがあってもなくても）か空行で、空行・改行・文字の終わりのどれかと、その後の
/// いくつもの改行で終わる。
pub fn parse_ishikawa(src: &str, db: &mut IshikawaDb) -> Result<()> {
    let src = js::utf16(src);
    let tokens = tokenize(&src);
    let (types, starts, ends) = (&tokens.types, &tokens.starts, &tokens.ends);
    let mut i = 0;

    let fail = |i: usize, expected: &str| {
        syntax_error(
            "ishikawa",
            &src,
            starts[i],
            &format!(
                "Expecting {expected}, got '{}'",
                TOKEN_NAMES[types[i] as usize]
            ),
            false,
        )
    };

    if types[i] == t::SPACELINE {
        i += 1;
        while types[i] == t::SPACELINE || types[i] == t::NL {
            i += 1;
        }
        if types[i] != t::ISHIKAWA {
            return Err(fail(i, "'SPACELINE', 'NL', 'ISHIKAWA'"));
        }
    } else if types[i] != t::ISHIKAWA {
        return Err(fail(i, "'SPACELINE', 'ISHIKAWA'"));
    }
    i += 1;
    if types[i] == t::NL {
        i += 1;
    }

    loop {
        let type_ = types[i];
        if type_ == t::TEXT || (type_ == t::SPACELIST && types[i + 1] == t::TEXT) {
            let text = if type_ == t::TEXT { i } else { i + 1 };
            i = text + 1;
            if types[i] != t::SPACELINE && types[i] != t::NL && types[i] != t::EOF {
                return Err(fail(i, STOP));
            }
            let level = if type_ == t::TEXT {
                0
            } else {
                (ends[text - 1] - starts[text - 1]) as i64
            };
            db.add_node(
                level,
                js::trim(&js::from16(&src[starts[text]..ends[text]])).to_string(),
            );
        } else if type_ == t::SPACELINE || type_ == t::SPACELIST {
            i += 1;
            if types[i] != t::SPACELINE && types[i] != t::NL && types[i] != t::EOF {
                return Err(fail(
                    i,
                    &if type_ == t::SPACELIST {
                        format!("{STOP}, 'TEXT'")
                    } else {
                        STOP.to_string()
                    },
                ));
            }
        } else {
            return Err(fail(i, "'SPACELINE', 'SPACELIST', 'TEXT'"));
        }
        i += 1;
        while types[i] == t::NL || types[i] == t::EOF {
            i += 1;
        }
        if types[i] == t::END {
            return Ok(());
        }
    }
}
