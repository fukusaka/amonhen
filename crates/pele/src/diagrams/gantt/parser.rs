//! gantt の読み手（pele の `src/diagrams/gantt/parser.ts`）。Mermaid の文法の動作を、Mermaid の名前のまま db に
//! 渡す。

use super::db::GanttDb;
use super::lexer::tokenize;
use super::tokens::{self as t, TOKEN_NAMES};
use crate::errors::{Result, syntax_error};
use crate::js;

const WEEKDAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

const LINE: &str = concat!(
    "'EOF', 'SPACE', 'NL', 'weekday_monday', 'weekday_tuesday', 'weekday_wednesday', 'weekday_thursday', ",
    "'weekday_friday', 'weekday_saturday', 'weekday_sunday', 'weekend_friday', 'weekend_saturday', 'dateFormat', ",
    "'inclusiveEndDates', 'topAxis', 'axisFormat', 'tickInterval', 'excludes', 'includes', 'todayMarker', 'title', ",
    "'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'section', 'taskTxt', 'click'"
);

/// 行の頭に来うる字句。文が終わった後に続けられるのもこれだけ。
fn starts_line(type_: u8) -> bool {
    (t::weekday_monday..=t::weekend_saturday).contains(&type_)
        || matches!(
            type_,
            t::EOF
                | t::NL
                | t::dateFormat
                | t::inclusiveEndDates
                | t::topAxis
                | t::axisFormat
                | t::tickInterval
                | t::excludes
                | t::includes
                | t::todayMarker
                | t::title
                | t::acc_title
                | t::acc_descr
                | t::acc_descr_multiline_value
                | t::section
                | t::taskTxt
                | t::click
        )
}

pub fn parse_gantt(src: &str, db: &mut GanttDb) -> Result<()> {
    let src: Vec<u16> = js::utf16(src);
    let tokens = tokenize(&src);
    let types = &tokens.types;
    let text = |i: usize| -> Vec<u16> { src[tokens.starts[i]..tokens.ends[i]].to_vec() };
    let s = |v: &[u16]| js::from16(v);
    // `text.slice(k)`
    let after = |v: Vec<u16>, k: usize| js::from16(&v[k.min(v.len())..]);
    let mut i = 0usize;

    let fail = |i: usize, expected: &str| {
        let type_ = types[i];
        let got = if type_ == t::END {
            "1"
        } else {
            TOKEN_NAMES[type_ as usize]
        };
        syntax_error(
            "gantt",
            &src,
            tokens.starts[i],
            &format!("Expecting {expected}, got '{got}'"),
            false,
        )
    };
    // Mermaid の読み手は、次の字句がその文に続けられると分かってから文の動作をする。
    let end = |i: usize, also: &str| -> Result<()> {
        if starts_line(types[i]) {
            Ok(())
        } else {
            Err(fail(i, &format!("{LINE}{also}")))
        }
    };

    if types[i] != t::gantt {
        return Err(fail(i, "'gantt'"));
    }
    i += 1;

    loop {
        let type_ = types[i];
        let here = text(i);
        if !starts_line(type_) {
            return Err(fail(i, LINE));
        }
        i += 1;
        match type_ {
            t::EOF => {
                if types[i] == t::END {
                    return Ok(());
                }
                end(i, "")?;
            }
            t::NL => {}
            t::dateFormat => {
                end(i, "")?;
                db.set_date_format(&after(here, 11));
            }
            t::inclusiveEndDates => {
                end(i, "")?;
                db.inclusive_end_dates = true;
            }
            t::topAxis => {
                end(i, "")?;
                db.top_axis = true;
            }
            t::axisFormat => {
                end(i, "")?;
                db.axis_format = after(here, 11);
            }
            t::tickInterval => {
                end(i, "")?;
                db.tick_interval = Some(after(here, 13));
            }
            t::excludes => {
                end(i, "")?;
                db.set_excludes(&after(here, 9));
            }
            t::includes => {
                end(i, "")?;
                db.set_includes(&after(here, 9));
            }
            t::todayMarker => {
                end(i, "")?;
                db.today_marker = after(here, 12);
            }
            t::weekend_friday => {
                end(i, "")?;
                db.weekend = "friday".into();
            }
            t::weekend_saturday => {
                end(i, "")?;
                db.weekend = "saturday".into();
            }
            t::title => {
                end(i, "")?;
                db.title = Some(after(here, 6));
            }
            t::acc_title => {
                if types[i] != t::acc_title_value {
                    return Err(fail(i, "'acc_title_value'"));
                }
                let value = s(&text(i));
                i += 1;
                end(i, "")?;
                db.set_acc_title(js::trim(&value));
            }
            t::acc_descr => {
                if types[i] != t::acc_descr_value {
                    return Err(fail(i, "'acc_descr_value'"));
                }
                let value = s(&text(i));
                i += 1;
                end(i, "")?;
                db.set_acc_description(js::trim(&value));
            }
            t::acc_descr_multiline_value => {
                end(i, "")?;
                db.set_acc_description(js::trim(&s(&here)));
            }
            t::section => {
                end(i, "")?;
                db.add_section(&after(here, 8));
            }
            t::taskTxt => {
                if types[i] != t::taskData {
                    return Err(fail(i, "'taskData'"));
                }
                let data = s(&text(i));
                i += 1;
                end(i, "")?;
                db.add_task(&s(&here), &data)?;
            }
            t::click => {
                let mut name: Option<String> = None;
                let mut args: Option<String> = None;
                let mut href: Option<String> = None;
                if types[i] == t::callbackname {
                    name = Some(s(&text(i)));
                    i += 1;
                    if types[i] == t::callbackargs {
                        args = Some(s(&text(i)));
                        i += 1;
                    }
                    if types[i] == t::href {
                        href = Some(s(&text(i)));
                        i += 1;
                        end(i, "")?;
                    } else {
                        end(
                            i,
                            if args.is_none() {
                                ", 'callbackargs', 'href'"
                            } else {
                                ", 'href'"
                            },
                        )?;
                    }
                } else if types[i] == t::href {
                    href = Some(s(&text(i)));
                    i += 1;
                    if types[i] == t::callbackname {
                        name = Some(s(&text(i)));
                        i += 1;
                        if types[i] == t::callbackargs {
                            args = Some(s(&text(i)));
                            i += 1;
                            end(i, "")?;
                        } else {
                            end(i, ", 'callbackargs'")?;
                        }
                    } else {
                        end(i, ", 'callbackname'")?;
                    }
                } else {
                    return Err(fail(i, "'callbackname', 'href'"));
                }
                let ids = s(&here);
                if name.is_some() {
                    db.set_click_event(&ids);
                }
                let _ = args;
                if let Some(href) = href {
                    db.set_link(&ids, &href);
                }
            }
            _ => {
                end(i, "")?;
                db.weekday = Some(WEEKDAYS[(type_ - t::weekday_monday) as usize].to_string());
            }
        }
    }
}
