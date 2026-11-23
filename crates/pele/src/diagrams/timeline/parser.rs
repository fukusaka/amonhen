//! timeline の読み手（pele の `src/diagrams/timeline/parser.ts`）。

use super::db::TimelineDb;
use super::lexer::{TOKEN_NAMES, t, tokenize};
use crate::errors::{PeleError, Result, syntax_error};
use crate::js;

const STATEMENT: &str = "'EOF', 'SPACE', 'NEWLINE', 'title', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'section', 'period', 'event'";

/// `text.substring(from)`（UTF-16 の位置）。
fn substring(text: &str, from: usize) -> String {
    let units = js::utf16(text);
    js::from16(&units[from.min(units.len())..])
}

pub fn parse_timeline(src: &str, db: &mut TimelineDb) -> Result<()> {
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
            "timeline",
            &src16,
            starts[i],
            &format!("Expecting {expected}, got '{got}'"),
            false,
        )
    };

    let head = types[0];
    if head != t::TIMELINE && head != t::TIMELINE_LR && head != t::TIMELINE_TD {
        return Err(fail(0, "'timeline', 'timeline_lr', 'timeline_td'"));
    }
    if head != t::TIMELINE {
        db.set_direction(head == t::TIMELINE_TD);
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
            t::PERIOD => db.add_task(text, ""),
            t::EVENT => db.add_event(&substring(text, 2))?,
            t::EOF => return Ok(()),
            _ => return Err(fail(i, STATEMENT)),
        }
        i += 1;
    }
}
