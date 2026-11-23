//! sequence の形を組む（pele の `src/diagrams/sequence/db.ts`）。

use std::collections::HashMap;

use super::colors::is_css_color;
use super::types::{SeqActor, SeqBox, SeqBoxData, SeqMessage, SeqNumbering, SeqText, line};
use crate::diagrams::journey::db::join_lines;
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;
use crate::util::yaml::{Value, parse_json, parse_yaml};

#[derive(Clone, Copy, Debug, Default)]
pub struct SeqDbOptions {
    /// 一番上の `wrap` の設定（`%%{wrap}%%` の指示も決める）。
    pub wrap: Option<bool>,
    /// `sequence.wrap`。
    pub sequence_wrap: bool,
}

fn fail(message: String) -> PeleError {
    PeleError::typed(message, ErrorCode::Semantic, "sequence")
}

/// `/^:?wrap:/`・`/^:?nowrap:/` の印の後の長さ（UTF-16 も同じ、ASCII なので）。
fn wrap_prefix(text: &str) -> Option<(bool, usize)> {
    let start = usize::from(text.starts_with(':'));
    let rest = &text[start..];
    if rest.starts_with("wrap:") {
        Some((true, start + 5))
    } else if rest.starts_with("nowrap:") {
        Some((false, start + 7))
    } else {
        None
    }
}

pub fn extract_wrap(raw: &str) -> SeqText {
    let text = js::trim(raw);
    match wrap_prefix(text) {
        Some((wrap, len)) => SeqText {
            text: js::trim(&text[len..]).to_string(),
            wrap: Some(wrap),
        },
        None => SeqText {
            text: text.to_string(),
            wrap: None,
        },
    }
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\r' | '\u{2028}' | '\u{2029}')
}

/// 箱の行を頭の色と残りに分ける。Mermaid の `/^((?:rgba?|hsla?)\s*\(.*\)|\w*)(.*)$/` と同じ。`.` は行の終わりの
/// 文字で止まるので、かっこの前の空白の外にそれがある行はまったく合わない。
fn split_box(s: &str) -> Option<(&str, &str)> {
    // `/^(?:rgba?|hsla?)\s*\(/`
    let function = ["rgba", "rgb", "hsla", "hsl"].iter().find_map(|name| {
        let rest = s.strip_prefix(name)?;
        let after = rest.trim_start_matches(js::is_js_space);
        let open = s.len() - after.len();
        after.starts_with('(').then_some(open + 1)
    });
    if let Some(head) = function
        && let Some(close) = s.rfind(')')
        && close >= head
    {
        if s[head..].chars().any(is_line_terminator) {
            return None;
        }
        return Some((&s[..close + 1], &s[close + 1..]));
    }
    if s.chars().any(is_line_terminator) {
        return None;
    }
    let word = s
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count();
    Some((&s[..word], &s[word..]))
}

/// 1 つの文（Mermaid の文法が組む、`apply` に渡すもの）。
#[derive(Clone, Debug)]
pub enum Stmt {
    /// `addParticipant`・`createParticipant`・`destroyParticipant`。
    Participant {
        kind: Participant,
        actor: String,
        draw: Option<&'static str>,
        description: Option<SeqText>,
        config: Option<String>,
    },
    /// `addMessage`。
    Message {
        from: String,
        to: String,
        signal_type: i32,
        msg: SeqText,
        activate: bool,
        central: Option<i32>,
    },
    /// `activeStart`・`activeEnd`・`centralConnection`・`centralConnectionReverse`。
    Signal {
        actor: String,
        signal_type: i32,
    },
    /// `addNote`。2 人の上なら `actor` は 2 つ。
    Note {
        actor: Vec<String>,
        placement: i32,
        text: SeqText,
    },
    /// `addLinks`・`addALink`・`addProperties`・`addDetails`。
    Menu {
        kind: Menu,
        actor: String,
        text: SeqText,
    },
    /// `sequenceIndex`（`autonumber`）。
    SequenceIndex {
        index: Option<f64>,
        step: Option<f64>,
        visible: bool,
    },
    BoxStart(SeqBoxData),
    BoxEnd,
    /// 囲みの始まり・区切り・終わり（`loopStart`・`else`・`altEnd` など）。
    Block {
        signal_type: i32,
        text: Option<SeqText>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Participant {
    Add,
    Create,
    Destroy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    Links,
    ALink,
    Properties,
    Details,
}

/// 文の並びの一つ。並びは `Lists` の番号で指す（深い入れ子を再帰なしで扱うため）。
#[derive(Clone, Debug)]
pub enum Item {
    /// 囲みの始まりのために空けておく所。
    Empty,
    Stmt(Box<Stmt>),
    List(usize),
}

pub type Lists = Vec<Vec<Item>>;

#[derive(Default)]
pub struct SeqDb {
    /// 入れた順。
    pub actors: Vec<(String, SeqActor)>,
    actor_index: HashMap<String, usize>,
    pub created_actors: HashMap<String, usize>,
    pub destroyed_actors: HashMap<String, usize>,
    pub boxes: Vec<SeqBox>,
    pub messages: Vec<SeqMessage>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    current_box: Option<usize>,
    last_created: Option<String>,
    last_destroyed: Option<String>,
    wrap_enabled: Option<bool>,
    sequence_wrap: bool,
    active: HashMap<String, i64>,
}

/// `{ text, wrap?, type? }` の参加者の説明。
struct Description {
    text: String,
    wrap: Option<bool>,
}

impl SeqDb {
    pub fn new(options: SeqDbOptions) -> Self {
        SeqDb {
            wrap_enabled: options.wrap,
            sequence_wrap: options.sequence_wrap,
            ..Default::default()
        }
    }

    pub fn actor(&self, id: &str) -> Option<&SeqActor> {
        self.actor_index.get(id).map(|&k| &self.actors[k].1)
    }

    fn actor_mut(&mut self, id: &str) -> Option<&mut SeqActor> {
        self.actor_index.get(id).map(|&k| &mut self.actors[k].1)
    }

    fn auto_wrap(&self) -> bool {
        self.wrap_enabled.unwrap_or(self.sequence_wrap)
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        self.acc_descr = Some(join_lines(text));
    }

    pub fn set_diagram_title(&mut self, text: &str) {
        self.title = Some(text.to_string());
    }

    pub fn parse_message(&self, s: &str) -> SeqText {
        extract_wrap(s)
    }

    pub fn parse_box_data(&self, s: &str) -> SeqBoxData {
        let found = split_box(s);
        let mut color = match found {
            Some((c, _)) if !c.is_empty() => js::trim(c).to_string(),
            _ => "transparent".to_string(),
        };
        let mut title = match found {
            Some((_, t)) if !t.is_empty() => Some(js::trim(t).to_string()),
            _ => None,
        };
        if !is_css_color(&color) {
            color = "transparent".to_string();
            title = Some(js::trim(s).to_string());
        }
        let Some(title) = title else {
            return SeqBoxData {
                text: None,
                color,
                wrap: None,
            };
        };
        let SeqText { text, wrap } = extract_wrap(&title);
        SeqBoxData {
            text: (!text.is_empty()).then_some(text),
            color,
            wrap,
        }
    }

    fn add_box(&mut self, data: &SeqBoxData) {
        self.boxes.push(SeqBox {
            name: data.text.clone(),
            wrap: data.wrap.unwrap_or(self.auto_wrap()),
            fill: data.color.clone(),
        });
        self.current_box = Some(self.boxes.len() - 1);
    }

    fn box_name(&self, b: usize) -> String {
        self.boxes[b]
            .name
            .clone()
            .unwrap_or_else(|| "undefined".to_string())
    }

    fn add_actor(
        &mut self,
        id: &str,
        name: &str,
        description: Option<&SeqText>,
        mut type_: Option<String>,
        metadata: Option<&str>,
    ) -> Result<()> {
        let mut description = description.map(|d| Description {
            text: d.text.clone(),
            wrap: d.wrap,
        });
        let mut assigned_box = self.current_box;
        let mut doc: Option<crate::util::yaml::Map> = None;
        if let Some(metadata) = metadata {
            let yaml = if metadata.contains('\n') {
                format!("{metadata}\n")
            } else {
                format!("{{\n{metadata}\n}}")
            };
            let parsed = parse_yaml(&yaml).map_err(|message| {
                PeleError::typed(
                    format!("Config of participant \"{id}\" is not valid YAML. {message}"),
                    ErrorCode::Syntax,
                    "sequence",
                )
            })?;
            if let Value::Obj(map) = parsed {
                doc = Some(map);
            }
        }
        let doc_type = doc.as_ref().and_then(|d| d.get("type"));
        let doc_alias = doc.as_ref().and_then(|d| d.get("alias"));
        if let Some(t) = doc_type.filter(|t| !matches!(t, Value::Null)) {
            type_ = Some(t.to_js_string());
        }
        if let Some(alias) = doc_alias.filter(|a| a.truthy())
            && description.as_ref().is_none_or(|d| d.text == name)
        {
            description = Some(Description {
                text: alias.to_js_string(),
                wrap: description.as_ref().and_then(|d| d.wrap),
            });
        }

        if let Some(&k) = self.actor_index.get(id) {
            let old_box = self.actors[k].1.box_;
            if let (Some(current), Some(old)) = (self.current_box, old_box)
                && current != old
            {
                return Err(fail(format!(
                    "A same participant should only be defined in one Box: {} can't be in '{}' and in '{}' at the same time.",
                    self.actors[k].1.name,
                    self.box_name(old),
                    self.box_name(current)
                )));
            }
            assigned_box = old_box.or(self.current_box);
            self.actors[k].1.box_ = assigned_box;
            if name == self.actors[k].1.name && description.is_none() {
                return Ok(());
            }
        }
        // メッセージで名を挙げただけの参加者は種類を持たず、名前をラベルにする。
        let description = match description {
            Some(d) if type_.is_some() => d,
            _ => Description {
                text: name.to_string(),
                wrap: None,
            },
        };

        let actor = SeqActor {
            box_: assigned_box,
            name: name.to_string(),
            description: description.text,
            wrap: description.wrap.unwrap_or(self.auto_wrap()),
            class: None,
            type_: type_.unwrap_or_else(|| "participant".to_string()),
        };
        match self.actor_index.get(id) {
            Some(&k) => self.actors[k].1 = actor,
            None => {
                self.actor_index.insert(id.to_string(), self.actors.len());
                self.actors.push((id.to_string(), actor));
            }
        }
        Ok(())
    }

    fn push_message(&mut self, mut message: SeqMessage) {
        message.id = self.messages.len().to_string();
        self.messages.push(message);
    }

    fn add_signal(
        &mut self,
        from: Option<&str>,
        to: Option<&str>,
        message: Option<&SeqText>,
        type_: i32,
        activate: bool,
        central_connection: Option<i32>,
    ) -> Result<()> {
        // Mermaid はここで前のメッセージをすべて数え直す。参加者ごとの数で同じことが言える。
        if type_ == line::ACTIVE_END {
            let count = match from.filter(|f| !f.is_empty()) {
                Some(f) => self.active.get(f).copied().unwrap_or(0),
                None => 0,
            };
            if count < 1 {
                return Err(fail(format!(
                    "Trying to inactivate an inactive participant ({})",
                    from.unwrap_or("undefined")
                )));
            }
            self.active
                .insert(from.unwrap_or_default().to_string(), count - 1);
        } else if type_ == line::ACTIVE_START
            && let Some(f) = from.filter(|f| !f.is_empty())
        {
            *self.active.entry(f.to_string()).or_insert(0) += 1;
        }
        let wrap = message.and_then(|m| m.wrap).unwrap_or(self.auto_wrap());
        self.push_message(SeqMessage {
            id: String::new(),
            from: from.map(str::to_string),
            to: to.map(str::to_string),
            message: message.map(|m| m.text.clone()).unwrap_or_default(),
            numbering: None,
            wrap,
            type_: Some(type_),
            activate,
            placement: None,
            central_connection: central_connection.unwrap_or(0),
        });
        Ok(())
    }

    fn add_note(&mut self, actor: &[String], placement: i32, message: &SeqText) {
        let wrap = message.wrap.unwrap_or(self.auto_wrap());
        let (a, b) = match actor {
            [a, b] => (a, b),
            [a, ..] => (a, a),
            [] => return,
        };
        self.push_message(SeqMessage {
            id: String::new(),
            from: Some(a.clone()),
            to: Some(b.clone()),
            message: message.text.clone(),
            numbering: None,
            wrap,
            type_: Some(line::NOTE),
            activate: false,
            placement: Some(placement),
            central_connection: 0,
        });
    }

    /// Mermaid は JSON でないメニューの文を記録して無視する。描くものに届くのは `properties` の `class` だけ。
    fn add_properties(&mut self, actor_id: &str, text: &SeqText) {
        let Some(actor) = self.actor_mut(actor_id) else {
            return;
        };
        // `Object.entries` が `class` を返すのは、対応づけ（と配列。番号だけを返す）。文字列は番号を返す。
        if let Some(Value::Obj(map)) = parse_json(&text.text)
            && let Some(class) = map.get("class")
        {
            actor.class = Some(class.clone());
        }
    }

    /// Mermaid は入れ子の文の並びを再帰でたどる。ここでは明示した積み上げで、どんな深さも扱う。
    pub fn apply(&mut self, lists: &Lists, root: usize) -> Result<()> {
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(top) = stack.last_mut() {
            let (list, at) = *top;
            if at >= lists[list].len() {
                stack.pop();
                continue;
            }
            top.1 += 1;
            match &lists[list][at] {
                Item::Empty => {}
                Item::List(k) => stack.push((*k, 0)),
                Item::Stmt(s) => self.apply_one(s)?,
            }
        }
        Ok(())
    }

    fn apply_one(&mut self, param: &Stmt) -> Result<()> {
        match param {
            Stmt::SequenceIndex {
                index,
                step,
                visible,
            } => {
                self.push_message(SeqMessage {
                    id: String::new(),
                    from: None,
                    to: None,
                    message: String::new(),
                    numbering: Some(SeqNumbering {
                        start: *index,
                        step: *step,
                        visible: *visible,
                    }),
                    wrap: false,
                    type_: Some(line::AUTONUMBER),
                    activate: false,
                    placement: None,
                    central_connection: 0,
                });
            }
            Stmt::Participant {
                kind,
                actor,
                draw,
                description,
                config,
            } => match kind {
                Participant::Add => self.add_actor(
                    actor,
                    actor,
                    description.as_ref(),
                    draw.map(str::to_string),
                    config.as_deref(),
                )?,
                Participant::Create => {
                    if self.actor_index.contains_key(actor) {
                        return Err(fail("It is not possible to have actors with the same id, even if one is destroyed before the next is created. Use 'AS' aliases to simulate the behavior".to_string()));
                    }
                    self.last_created = Some(actor.clone());
                    self.add_actor(
                        actor,
                        actor,
                        description.as_ref(),
                        draw.map(str::to_string),
                        config.as_deref(),
                    )?;
                    self.created_actors
                        .insert(actor.clone(), self.messages.len());
                }
                Participant::Destroy => {
                    self.last_destroyed = Some(actor.clone());
                    self.destroyed_actors
                        .insert(actor.clone(), self.messages.len());
                }
            },
            Stmt::Signal { actor, signal_type } => {
                self.add_signal(Some(actor), None, None, *signal_type, false, None)?
            }
            Stmt::Note {
                actor,
                placement,
                text,
            } => self.add_note(actor, *placement, text),
            Stmt::Menu { kind, actor, text } => {
                if *kind == Menu::Properties {
                    self.add_properties(actor, text);
                }
            }
            Stmt::Message {
                from,
                to,
                signal_type,
                msg,
                activate,
                central,
            } => {
                if let Some(created) = &self.last_created {
                    if to != created {
                        return Err(fail(format!(
                            "The created participant {created} does not have an associated creating message after its declaration. Please check the sequence diagram."
                        )));
                    }
                    self.last_created = None;
                } else if let Some(destroyed) = &self.last_destroyed {
                    if to != destroyed && from != destroyed {
                        return Err(fail(format!(
                            "The destroyed participant {destroyed} does not have an associated destroying message after its declaration. Please check the sequence diagram."
                        )));
                    }
                    self.last_destroyed = None;
                }
                self.add_signal(
                    Some(from),
                    Some(to),
                    Some(msg),
                    *signal_type,
                    *activate,
                    *central,
                )?;
            }
            Stmt::BoxStart(data) => self.add_box(data),
            Stmt::BoxEnd => self.current_box = None,
            Stmt::Block { signal_type, text } => {
                self.add_signal(None, None, text.as_ref(), *signal_type, false, None)?
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_boxes() {
        assert_eq!(split_box("rgb(1,2,3) Team"), Some(("rgb(1,2,3)", " Team")));
        assert_eq!(split_box("Aqua Team"), Some(("Aqua", " Team")));
        assert_eq!(split_box("Team\r"), None);
        let db = SeqDb::default();
        let data = db.parse_box_data("Purple wrap:Alice & John");
        assert_eq!(data.color, "Purple");
        assert_eq!(data.text.as_deref(), Some("Alice & John"));
        assert_eq!(data.wrap, Some(true));
        let data = db.parse_box_data("Team A");
        assert_eq!(data.color, "transparent");
        assert_eq!(data.text.as_deref(), Some("Team A"));
    }
}
