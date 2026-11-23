//! sequence の読み手（pele の `src/diagrams/sequence/parser.ts`）。Mermaid の文法が組むのと同じ文の並びを組み、
//! 最後に `apply` に渡す。

use super::db::{Item, Lists, Menu, Participant, SeqDb, Stmt};
use super::lexer::tokenize;
use super::tokens::{TOKEN_NAMES, t};
use super::types::{SeqText, line, placement};
use crate::errors::{PeleError, Result, syntax_error};
use crate::js;

fn signal_of(type_: u8) -> Option<i32> {
    Some(match type_ {
        t::SOLID_OPEN_ARROW => line::SOLID_OPEN,
        t::DOTTED_OPEN_ARROW => line::DOTTED_OPEN,
        t::SOLID_ARROW => line::SOLID,
        t::DOTTED_ARROW => line::DOTTED,
        t::BIDIRECTIONAL_SOLID_ARROW => line::BIDIRECTIONAL_SOLID,
        t::BIDIRECTIONAL_DOTTED_ARROW => line::BIDIRECTIONAL_DOTTED,
        t::SOLID_CROSS => line::SOLID_CROSS,
        t::DOTTED_CROSS => line::DOTTED_CROSS,
        t::SOLID_POINT => line::SOLID_POINT,
        t::DOTTED_POINT => line::DOTTED_POINT,
        t::SOLID_ARROW_TOP => line::SOLID_TOP,
        t::SOLID_ARROW_BOTTOM => line::SOLID_BOTTOM,
        t::STICK_ARROW_TOP => line::STICK_TOP,
        t::STICK_ARROW_BOTTOM => line::STICK_BOTTOM,
        t::SOLID_ARROW_TOP_DOTTED => line::SOLID_TOP_DOTTED,
        t::SOLID_ARROW_BOTTOM_DOTTED => line::SOLID_BOTTOM_DOTTED,
        t::STICK_ARROW_TOP_DOTTED => line::STICK_TOP_DOTTED,
        t::STICK_ARROW_BOTTOM_DOTTED => line::STICK_BOTTOM_DOTTED,
        t::SOLID_ARROW_TOP_REVERSE => line::SOLID_ARROW_TOP_REVERSE,
        t::SOLID_ARROW_BOTTOM_REVERSE => line::SOLID_ARROW_BOTTOM_REVERSE,
        t::STICK_ARROW_TOP_REVERSE => line::STICK_ARROW_TOP_REVERSE,
        t::STICK_ARROW_BOTTOM_REVERSE => line::STICK_ARROW_BOTTOM_REVERSE,
        t::SOLID_ARROW_TOP_REVERSE_DOTTED => line::SOLID_ARROW_TOP_REVERSE_DOTTED,
        t::SOLID_ARROW_BOTTOM_REVERSE_DOTTED => line::SOLID_ARROW_BOTTOM_REVERSE_DOTTED,
        t::STICK_ARROW_TOP_REVERSE_DOTTED => line::STICK_ARROW_TOP_REVERSE_DOTTED,
        t::STICK_ARROW_BOTTOM_REVERSE_DOTTED => line::STICK_ARROW_BOTTOM_REVERSE_DOTTED,
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum K {
    Top,
    Box,
    Loop,
    Rect,
    Opt,
    Break,
    Alt,
    Par,
    Critical,
}

fn block_of(type_: u8) -> Option<K> {
    Some(match type_ {
        t::LOOP => K::Loop,
        t::RECT => K::Rect,
        t::OPT => K::Opt,
        t::BREAK => K::Break,
        t::ALT => K::Alt,
        t::PAR | t::PAR_OVER => K::Par,
        t::CRITICAL => K::Critical,
        t::BOX => K::Box,
        _ => return None,
    })
}

struct Frame {
    kind: K,
    text: String,
    start_type: i32,
    /// 今の区切りの文の並び（`Lists` の番号）。囲みの最初の区切りの 0 番は始まりのために空けてある。
    list: usize,
    /// 済んだ区切りと、それを終えた区切りの文。
    sections: Vec<(usize, String)>,
}

const SIGNAL_TOKENS: &str = "'SOLID_OPEN_ARROW', 'DOTTED_OPEN_ARROW', 'SOLID_ARROW', 'DOTTED_ARROW', 'SOLID_CROSS', 'DOTTED_CROSS', 'SOLID_POINT', 'DOTTED_POINT'";

struct Parser<'a> {
    src: Vec<u16>,
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    i: usize,
    lists: Lists,
    db: &'a mut SeqDb,
}

fn stmt(s: Stmt) -> Item {
    Item::Stmt(Box::new(s))
}

impl Parser<'_> {
    fn type_at(&self, i: usize) -> u8 {
        self.types.get(i).copied().unwrap_or(t::END)
    }

    fn kind(&self) -> u8 {
        self.type_at(self.i)
    }

    fn fail(&self, expected: &str) -> PeleError {
        let ty = self.kind();
        let at = self.starts[self.i.min(self.starts.len() - 1)];
        if ty == t::ERROR {
            return syntax_error("sequence", &self.src, at, "", true);
        }
        syntax_error(
            "sequence",
            &self.src,
            at,
            &format!("Expecting {expected}, got '{}'", TOKEN_NAMES[ty as usize]),
            false,
        )
    }

    fn expect(&mut self, type_: u8) -> Result<String> {
        if self.kind() != type_ {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[type_ as usize])));
        }
        self.i += 1;
        Ok(self.texts[self.i - 1].clone())
    }

    fn new_list(&mut self, items: Vec<Item>) -> usize {
        self.lists.push(items);
        self.lists.len() - 1
    }

    /// 参加者の名前（`addParticipant` の文になる）。
    fn actor(&mut self) -> Result<String> {
        let ty = self.kind();
        if ty != t::ACTOR && ty != t::LINK {
            return Err(self.fail("'ACTOR'"));
        }
        self.i += 1;
        Ok(self.texts[self.i - 1].clone())
    }

    fn add(actor: &str) -> Item {
        stmt(Stmt::Participant {
            kind: Participant::Add,
            actor: actor.to_string(),
            draw: None,
            description: None,
            config: None,
        })
    }

    fn text2(&mut self) -> Result<SeqText> {
        let raw = self.expect(t::TXT)?;
        let units = js::utf16(js::trim(&raw));
        Ok(self
            .db
            .parse_message(&js::from16(&units[1.min(units.len())..])))
    }

    fn participant(&mut self) -> Result<Stmt> {
        let ty = self.kind();
        self.i += 1;
        if ty == t::DESTROY {
            let actor = self.actor()?;
            self.expect(t::NEWLINE)?;
            return Ok(Stmt::Participant {
                kind: Participant::Destroy,
                actor,
                draw: None,
                description: None,
                config: None,
            });
        }
        let actor = self.actor()?;
        let mut config = None;
        if self.kind() == t::CONFIG_START && self.type_at(self.i - 1) == t::ACTOR {
            self.i += 1;
            let content = self.expect(t::CONFIG_CONTENT)?;
            self.expect(t::CONFIG_END)?;
            config = Some(js::trim(&content).to_string());
        }
        let draw = Some(if ty == t::PARTICIPANT {
            "participant"
        } else {
            "actor"
        });
        let mut description = None;
        if self.kind() == t::AS {
            self.i += 1;
            let alias = self.expect(t::REST_OF_LINE)?;
            self.expect(t::NEWLINE)?;
            description = Some(self.db.parse_message(&alias));
        } else if self.kind() == t::NEWLINE {
            self.i += 1;
        } else {
            return Err(self.fail("'AS', 'NEWLINE'"));
        }
        Ok(Stmt::Participant {
            kind: Participant::Add,
            actor,
            draw,
            description,
            config,
        })
    }

    fn signal(&mut self, from: String) -> Result<Item> {
        let mut central_from = false;
        if self.kind() == t::CENTRAL {
            central_from = true;
            self.i += 1;
        }
        let Some(signal_type) = signal_of(self.kind()) else {
            return Err(self.fail(SIGNAL_TOKENS));
        };
        self.i += 1;
        let mark = self.kind();
        if mark == t::CENTRAL || (!central_from && (mark == t::PLUS || mark == t::MINUS)) {
            self.i += 1;
        }
        let to = self.actor()?;
        let msg = self.text2()?;
        let mut activate = false;
        let mut central = None;
        let mut after: Vec<Item> = Vec::new();
        let signal = |actor: &str, signal_type: i32| {
            stmt(Stmt::Signal {
                actor: actor.to_string(),
                signal_type,
            })
        };
        if central_from {
            let dual = mark == t::CENTRAL;
            activate = dual;
            central = Some(if dual {
                line::CENTRAL_CONNECTION_DUAL
            } else {
                line::CENTRAL_CONNECTION_REVERSE
            });
            if dual {
                after.push(signal(&to, line::CENTRAL_CONNECTION));
            }
            after.push(signal(&from, line::CENTRAL_CONNECTION_REVERSE));
        } else if mark == t::PLUS {
            activate = true;
            after.push(signal(&to, line::ACTIVE_START));
        } else if mark == t::MINUS {
            after.push(signal(&from, line::ACTIVE_END));
        } else if mark == t::CENTRAL {
            activate = true;
            central = Some(line::CENTRAL_CONNECTION);
            after.push(signal(&to, line::CENTRAL_CONNECTION));
        }
        let mut out = vec![
            Self::add(&from),
            Self::add(&to),
            stmt(Stmt::Message {
                from,
                to,
                signal_type,
                msg,
                activate,
                central,
            }),
        ];
        out.extend(after);
        Ok(Item::List(self.new_list(out)))
    }

    fn menu(&mut self, kind: Menu) -> Result<Item> {
        self.i += 1;
        let who = self.actor()?;
        let text = self.text2()?;
        let out = vec![
            Self::add(&who),
            stmt(Stmt::Menu {
                kind,
                actor: who,
                text,
            }),
        ];
        Ok(Item::List(self.new_list(out)))
    }

    fn note(&mut self) -> Result<Item> {
        self.i += 1;
        let ty = self.kind();
        if ty == t::OVER {
            self.i += 1;
            let first = self.actor()?;
            let mut out = vec![Self::add(&first)];
            let mut ids = vec![first.clone(), first];
            if self.kind() == t::COMMA {
                self.i += 1;
                let second = self.actor()?;
                out.push(Self::add(&second));
                ids[1] = second;
            }
            let text = self.text2()?;
            out.push(stmt(Stmt::Note {
                actor: ids,
                placement: placement::OVER,
                text,
            }));
            return Ok(Item::List(self.new_list(out)));
        }
        if ty != t::LEFT_OF && ty != t::RIGHT_OF {
            return Err(self.fail("'over', 'left_of', 'right_of'"));
        }
        self.i += 1;
        let who = self.actor()?;
        let place = if ty == t::LEFT_OF {
            placement::LEFTOF
        } else {
            placement::RIGHTOF
        };
        let text = self.text2()?;
        let out = vec![
            Self::add(&who),
            stmt(Stmt::Note {
                actor: vec![who],
                placement: place,
                text,
            }),
        ];
        Ok(Item::List(self.new_list(out)))
    }

    fn autonumber(&mut self) -> Result<Stmt> {
        self.i += 1;
        let mut index = None;
        let mut step = None;
        let mut visible = true;
        if self.kind() == t::NUM {
            index = Some(js::number(&self.texts[self.i]));
            self.i += 1;
            step = Some(if self.kind() == t::NUM {
                self.i += 1;
                js::number(&self.texts[self.i - 1])
            } else {
                1.0
            });
        } else if self.kind() == t::OFF {
            self.i += 1;
            visible = false;
        }
        if self.kind() != t::NEWLINE {
            return Err(self.fail("'NEWLINE', 'NUM', 'off'"));
        }
        self.i += 1;
        Ok(Stmt::SequenceIndex {
            index,
            step,
            visible,
        })
    }

    /// alt・par・critical の区切りを、文法の右再帰と同じく最後から畳み、文が同じ順で形の組み手に届くようにする。
    fn fold(&mut self, frame: &mut Frame, signal_type: i32) -> usize {
        let mut list = frame.list;
        for (section, divider) in std::mem::take(&mut frame.sections).into_iter().rev() {
            let head = stmt(Stmt::Block {
                signal_type,
                text: Some(self.db.parse_message(&divider)),
            });
            let mut items = std::mem::take(&mut self.lists[section]);
            items.push(head);
            items.push(Item::List(list));
            list = self.new_list(items);
        }
        list
    }

    fn close(&mut self, mut frame: Frame) -> usize {
        let text = frame.text.clone();
        let msg = |db: &SeqDb| Some(db.parse_message(&text));
        let block =
            |signal_type: i32, text: Option<SeqText>| stmt(Stmt::Block { signal_type, text });
        let list = frame.list;
        let (list, start, end) = match frame.kind {
            K::Box => (
                list,
                stmt(Stmt::BoxStart(self.db.parse_box_data(&text))),
                stmt(Stmt::BoxEnd),
            ),
            K::Loop => (
                list,
                block(line::LOOP_START, msg(self.db)),
                block(line::LOOP_END, None),
            ),
            K::Rect => (
                list,
                block(line::RECT_START, msg(self.db)),
                block(line::RECT_END, None),
            ),
            K::Opt => (
                list,
                block(line::OPT_START, msg(self.db)),
                block(line::OPT_END, None),
            ),
            K::Break => (
                list,
                block(line::BREAK_START, msg(self.db)),
                block(line::BREAK_END, None),
            ),
            K::Alt => {
                let list = self.fold(&mut frame, line::ALT_ELSE);
                (
                    list,
                    block(line::ALT_START, msg(self.db)),
                    block(line::ALT_END, None),
                )
            }
            K::Par => {
                let list = self.fold(&mut frame, line::PAR_AND);
                (
                    list,
                    block(frame.start_type, msg(self.db)),
                    block(line::PAR_END, None),
                )
            }
            _ => {
                let list = self.fold(&mut frame, line::CRITICAL_OPTION);
                (
                    list,
                    block(line::CRITICAL_START, msg(self.db)),
                    block(line::CRITICAL_END, None),
                )
            }
        };
        self.lists[list][0] = start;
        self.lists[list].push(end);
        list
    }
}

pub fn parse_sequence(src: &str, db: &mut SeqDb) -> Result<()> {
    let src16 = js::utf16(src);
    let tokens = tokenize(&src16);
    let mut p = Parser {
        src: src16,
        types: tokens.types,
        texts: tokens.texts,
        starts: tokens.starts,
        i: 0,
        lists: Vec::new(),
        db,
    };

    while p.kind() == t::NEWLINE {
        p.i += 1;
    }
    p.expect(t::SD)?;

    let mut stack: Vec<Frame> = Vec::new();
    let top = p.new_list(Vec::new());
    let mut frame = Frame {
        kind: K::Top,
        text: String::new(),
        start_type: 0,
        list: top,
        sections: Vec::new(),
    };

    loop {
        let ty = p.kind();
        let list = frame.list;
        if frame.kind == K::Box {
            if ty == t::PARTICIPANT || ty == t::PARTICIPANT_ACTOR || ty == t::DESTROY {
                let s = p.participant()?;
                p.lists[list].push(stmt(s));
            } else if ty == t::NEWLINE {
                p.i += 1;
            } else if ty == t::END_KW {
                p.i += 1;
                let parent = stack.pop().expect("box frame");
                let done = p.close(std::mem::replace(&mut frame, parent));
                p.lists[frame.list].push(Item::List(done));
            } else {
                return Err(
                    p.fail("'end', 'NEWLINE', 'participant', 'participant_actor', 'destroy'")
                );
            }
            continue;
        }
        match ty {
            t::NEWLINE | t::INVALID => p.i += 1,
            t::PARTICIPANT | t::PARTICIPANT_ACTOR | t::DESTROY => {
                let s = p.participant()?;
                p.lists[list].push(stmt(s));
            }
            t::CREATE => {
                p.i += 1;
                let next = p.kind();
                if next != t::PARTICIPANT && next != t::PARTICIPANT_ACTOR && next != t::DESTROY {
                    return Err(p.fail("'participant', 'participant_actor', 'destroy'"));
                }
                let mut s = p.participant()?;
                if let Stmt::Participant { kind, .. } = &mut s {
                    *kind = Participant::Create;
                }
                p.lists[list].push(stmt(s));
            }
            t::ACTOR => {
                let from = p.actor()?;
                let item = p.signal(from)?;
                p.lists[list].push(item);
                p.expect(t::NEWLINE)?;
            }
            t::LINK => {
                let next = p.type_at(p.i + 1);
                let item = if next == t::ACTOR || next == t::LINK {
                    p.menu(Menu::ALink)?
                } else {
                    let from = p.actor()?;
                    p.signal(from)?
                };
                p.lists[list].push(item);
                p.expect(t::NEWLINE)?;
            }
            t::LINKS | t::PROPERTIES | t::DETAILS => {
                let kind = match ty {
                    t::LINKS => Menu::Links,
                    t::PROPERTIES => Menu::Properties,
                    _ => Menu::Details,
                };
                let item = p.menu(kind)?;
                p.lists[list].push(item);
                p.expect(t::NEWLINE)?;
            }
            t::NOTE => {
                let item = p.note()?;
                p.lists[list].push(item);
                p.expect(t::NEWLINE)?;
            }
            t::AUTONUMBER => {
                let s = p.autonumber()?;
                p.lists[list].push(stmt(s));
            }
            t::ACTIVATE | t::DEACTIVATE => {
                p.i += 1;
                let who = p.actor()?;
                p.expect(t::NEWLINE)?;
                p.lists[list].push(stmt(Stmt::Signal {
                    actor: who,
                    signal_type: if ty == t::ACTIVATE {
                        line::ACTIVE_START
                    } else {
                        line::ACTIVE_END
                    },
                }));
            }
            t::TITLE | t::LEGACY_TITLE => {
                let units = js::utf16(&p.texts[p.i]);
                p.i += 1;
                let from = if ty == t::TITLE { 6 } else { 7 };
                let title = js::from16(&units[from.min(units.len())..]);
                p.db.set_diagram_title(&title);
            }
            t::ACC_TITLE => {
                p.i += 1;
                let value = p.expect(t::ACC_TITLE_VALUE)?;
                p.db.set_acc_title(js::trim(&value));
            }
            t::ACC_DESCR => {
                p.i += 1;
                let value = p.expect(t::ACC_DESCR_VALUE)?;
                p.db.set_acc_description(js::trim(&value));
            }
            t::ACC_DESCR_MULTILINE_VALUE => {
                let value = js::trim(&p.texts[p.i]).to_string();
                p.i += 1;
                p.db.set_acc_description(&value);
            }
            t::LOOP
            | t::RECT
            | t::OPT
            | t::BREAK
            | t::ALT
            | t::PAR
            | t::PAR_OVER
            | t::CRITICAL
            | t::BOX => {
                p.i += 1;
                let text = p.expect(t::REST_OF_LINE)?;
                let list = p.new_list(vec![Item::Empty]);
                let parent = std::mem::replace(
                    &mut frame,
                    Frame {
                        kind: block_of(ty).expect("block"),
                        text,
                        start_type: if ty == t::PAR_OVER {
                            line::PAR_OVER_START
                        } else {
                            line::PAR_START
                        },
                        list,
                        sections: Vec::new(),
                    },
                );
                stack.push(parent);
            }
            t::ELSE | t::AND | t::OPTION => {
                let want = match ty {
                    t::ELSE => K::Alt,
                    t::AND => K::Par,
                    _ => K::Critical,
                };
                if frame.kind != want {
                    return Err(p.fail("'end'"));
                }
                p.i += 1;
                let text = p.expect(t::REST_OF_LINE)?;
                frame.sections.push((list, text));
                frame.list = p.new_list(Vec::new());
            }
            t::END_KW => {
                if frame.kind == K::Top {
                    return Err(p.fail("'NEWLINE', 'EOF'"));
                }
                p.i += 1;
                let parent = stack.pop().expect("frame");
                let done = p.close(std::mem::replace(&mut frame, parent));
                p.lists[frame.list].push(Item::List(done));
            }
            t::END => {
                if frame.kind != K::Top {
                    return Err(p.fail("'end'"));
                }
                let lists = std::mem::take(&mut p.lists);
                return p.db.apply(&lists, list);
            }
            _ => return Err(p.fail("'NEWLINE', 'ACTOR', 'participant', 'note', 'end'")),
        }
    }
}
