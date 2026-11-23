//! クラスの属性と操作（pele の `src/diagrams/class/members.ts`）。

use crate::diagrams::common::generics::parse_generic_types;
use crate::js;
use crate::util::chars::{is_line_end, is_space};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberType {
    Method,
    Attribute,
}

fn is_visibility(c: u16) -> bool {
    matches!(c, 35 | 43 | 126 | 45)
}

#[derive(Clone, Debug)]
pub struct ClassMember {
    pub id: String,
    pub member_type: MemberType,
    /// `#`・`+`・`~`・`-` か空。
    pub visibility: String,
    pub classifier: String,
    pub parameters: String,
    pub return_type: String,
}

pub struct DisplayDetails {
    pub display_text: String,
    pub css_style: String,
}

impl ClassMember {
    pub fn new(input: &str, member_type: MemberType) -> Self {
        let mut m = ClassMember {
            id: String::new(),
            member_type,
            visibility: String::new(),
            classifier: String::new(),
            parameters: String::new(),
            return_type: String::new(),
        };
        // Mermaid は `run)` のような開く括弧のない操作で TypeError を投げる。ここでは属性として、書いたとおりに残す。
        if member_type != MemberType::Method || !m.parse_method(input) {
            m.parse_attribute(input);
        }
        m.id = if m.id.starts_with(' ') {
            format!(" {}", js::trim(&m.id))
        } else {
            js::trim(&m.id).to_string()
        };
        m
    }

    /// Mermaid の `/([#+~-])?(.+)\((.*)\)([\s$*])?(.*)([$*])?/` と同じ分け方を 1 回の走査で: 最初の文字より後に開く
    /// 括弧があり、その後に閉じる括弧がある最初の行。
    fn parse_method(&mut self, input: &str) -> bool {
        let s: Vec<u16> = input.encode_utf16().collect();
        let n = s.len();
        let line_end = |c: u16| is_line_end(c as u32);
        let mut start = 0usize;
        while start < n {
            let mut end = start;
            while end < n && !line_end(s[end]) {
                end += 1;
            }
            let mut close = end as isize - 1;
            while close > start as isize && s[close as usize] != 41 {
                close -= 1;
            }
            let mut open = close - 1;
            while open > start as isize && s[open as usize] != 40 {
                open -= 1;
            }
            if open > start as isize {
                let (open, close) = (open as usize, close as usize);
                let first = s[start];
                let visible = is_visibility(first) && open > start + 1;
                if visible {
                    self.visibility = js::from16(&[first]);
                }
                self.id = js::from16(&s[if visible { start + 1 } else { start }..open]);
                self.parameters = js::trim(&js::from16(&s[open + 1..close])).to_string();
                let mut rest = close + 1;
                let mut classifier = String::new();
                if rest < n {
                    let next = s[rest];
                    if next == 36 || next == 42 || is_space(next as u32) {
                        if !is_space(next as u32) {
                            classifier = js::from16(&[next]);
                        }
                        rest += 1;
                    }
                }
                let mut stop = rest;
                while stop < n && !line_end(s[stop]) {
                    stop += 1;
                }
                self.return_type = js::trim(&js::from16(&s[rest.min(stop)..stop])).to_string();
                if classifier.is_empty()
                    && (self.return_type.ends_with('$') || self.return_type.ends_with('*'))
                {
                    classifier = self.return_type.pop().unwrap().to_string();
                }
                self.classifier = classifier;
                return true;
            }
            start = end + 1;
        }
        self.member_type = MemberType::Attribute;
        false
    }

    fn parse_attribute(&mut self, input: &str) {
        let mut rest = input;
        if let Some(c) = input.chars().next()
            && c.is_ascii()
            && is_visibility(c as u16)
        {
            self.visibility = c.to_string();
            rest = &rest[1..];
        }
        if input.ends_with('$') || input.ends_with('*') {
            self.classifier = input[input.len() - 1..].to_string();
            rest = &rest[..rest.len().saturating_sub(1)];
        }
        self.id = rest.to_string();
    }

    pub fn get_display_details(&self) -> DisplayDetails {
        let mut display_text = format!("{}{}", self.visibility, parse_generic_types(&self.id));
        if self.member_type == MemberType::Method {
            display_text.push_str(&format!(
                "({})",
                parse_generic_types(js::trim(&self.parameters))
            ));
            if !self.return_type.is_empty() {
                display_text.push_str(" : ");
                display_text.push_str(&parse_generic_types(&self.return_type));
            }
        }
        DisplayDetails {
            display_text: js::trim(&display_text).to_string(),
            css_style: self.parse_classifier().to_string(),
        }
    }

    pub fn parse_classifier(&self) -> &'static str {
        match self.classifier.as_str() {
            "*" => "font-style:italic;",
            "$" => "text-decoration:underline;",
            _ => "",
        }
    }
}
