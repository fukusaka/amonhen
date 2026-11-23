//! journey の読み手（pele の `src/diagrams/journey/parser.ts`）。

use super::db::JourneyDb;
use super::lexer::{TOKEN_NAMES, t, tokenize};
use crate::errors::{PeleError, Result, syntax_error};
use crate::js;

const STATEMENT: &str = "'EOF', 'SPACE', 'NEWLINE', 'title', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'section', 'taskName'";

/// UTF-16 の位置 `from` からの残り（`substring(from)`、ここでは頭が ASCII か 1 単位の文字）。
fn substring(text: &str, from: usize) -> String {
    js::from16(&js::utf16(text)[from.min(js::len16(text))..])
}

pub fn parse_journey(src: &str, db: &mut JourneyDb) -> Result<()> {
    let src16 = js::utf16(src);
    let tokens = tokenize(&src16);
    let (types, texts, starts) = (&tokens.types, &tokens.texts, &tokens.starts);

    let fail = |i: usize, expected: &str| -> PeleError {
        let got = if types[i] == t::END {
            "1"
        } else {
            TOKEN_NAMES[types[i] as usize]
        };
        syntax_error(
            "journey",
            &src16,
            starts[i],
            &format!("Expecting {expected}, got '{got}'"),
            false,
        )
    };

    if types[0] != t::JOURNEY {
        return Err(fail(0, "'journey'"));
    }

    let mut i = 1;
    loop {
        let text = &texts[i];
        match types[i] {
            t::NEWLINE => {}
            t::TITLE => db.set_diagram_title(&substring(text, 6)),
            t::ACC_TITLE => {
                i += 1;
                db.set_acc_title(js::trim(&texts[i]));
            }
            t::ACC_DESCR => {
                i += 1;
                db.set_acc_description(js::trim(&texts[i]));
            }
            t::ACC_DESCR_MULTILINE_VALUE => db.set_acc_description(js::trim(text)),
            t::SECTION => db.add_section(&substring(text, 8)),
            t::TASK_NAME => {
                i += 1;
                if types[i] != t::TASK_DATA {
                    return Err(fail(i, "'taskData'"));
                }
                db.add_task(text, &texts[i]);
            }
            t::EOF => return Ok(()),
            _ => return Err(fail(i, STATEMENT)),
        }
        i += 1;
    }
}
