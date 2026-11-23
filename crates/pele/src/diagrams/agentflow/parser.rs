//! agentflow の構文解析（pele の `src/diagrams/agentflow/parser.ts`）。Mermaid の jison の文法を手で書いた
//! 下降型にしたもの。知らせの文は Mermaid と同じ。位置の対応（`mappings`）は描くことに届かないので記録しない。

use super::db::AgentflowDb;
use super::lexer::tokenize;
use super::tokens::{self as t, TOKEN_NAMES};
use super::types::{DocItem, FlowText, LabelType, LinkInfo, Loc};
use crate::errors::{Edit, Result, input_offset, syntax_error};
use crate::js;

fn is_id(ty: u8) -> bool {
    matches!(
        ty,
        t::NUM
            | t::NODE_STRING
            | t::DOWN
            | t::MINUS
            | t::DEFAULT
            | t::COMMA
            | t::COLON
            | t::AMP
            | t::BRKT
            | t::MULT
            | t::UNICODE_TEXT
    )
}

fn is_alpha_num(ty: u8) -> bool {
    matches!(
        ty,
        t::NUM
            | t::UNICODE_TEXT
            | t::NODE_STRING
            | t::DIR
            | t::DOWN
            | t::MINUS
            | t::COMMA
            | t::COLON
            | t::AMP
            | t::BRKT
            | t::MULT
    )
}

fn is_no_tags(ty: u8) -> bool {
    matches!(
        ty,
        t::NUM
            | t::NODE_STRING
            | t::SPACE
            | t::MINUS
            | t::AMP
            | t::UNICODE_TEXT
            | t::COLON
            | t::MULT
            | t::BRKT
            | t::STYLE
            | t::LINKSTYLE
            | t::CLASSDEF
            | t::CLASS
            | t::CLICK
            | t::GRAPH
            | t::DIR
            | t::flow
            | t::connector
            | t::global
            | t::end
            | t::DOWN
            | t::UP
            | t::START_LINK
    )
}

fn is_style_part(ty: u8) -> bool {
    matches!(
        ty,
        t::NUM | t::NODE_STRING | t::COLON | t::SPACE | t::BRKT | t::STYLE
    )
}

fn is_separator(ty: u8) -> bool {
    matches!(ty, t::NEWLINE | t::SEMI | t::EOF | t::COMMENT)
}

fn is_trailing(ty: u8) -> bool {
    matches!(ty, t::COMMENT | t::NEWLINE | t::SPACE | t::EOF)
}

struct Frame {
    global: bool,
    /// `flow` の字句の位置。
    at: usize,
    id: Option<FlowText>,
    title: Option<FlowText>,
    data: Option<String>,
    data_from: usize,
    data_to: usize,
    list: Vec<DocItem>,
    collect: bool,
}

const TEXT_TOKENS: &str = "'UNICODE_TEXT', 'TEXT', 'TAGSTART'";
const SEPARATORS: &str = "'COMMENT', 'SEMI', 'NEWLINE', 'EOF'";
const DIRECTIONS: [&str; 5] = ["TB", "BT", "RL", "LR", "TD"];
const STATEMENT_START: &str = "'COMMENT', 'SEMI', 'NEWLINE', 'SPACE', 'EOF', 'flow', 'global', 'connector', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'STYLE', 'LINKSTYLE', 'CLASSDEF', 'CLASS', 'CLICK', 'NODE_STRING'";

/// `/}[^\S\n]*\n/g` の範囲（UTF-16 の位置）。Mermaid は読む前に、閉じ括弧と行の終わりの間の空白を除く。
fn brace_trails(src: &[u16]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        if src[i] == b'}' as u16 {
            let mut e = i + 1;
            while e < src.len() && src[e] != 10 && crate::util::chars::is_space(src[e] as u32) {
                e += 1;
            }
            if e < src.len() && src[e] == 10 {
                out.push((i, e + 1));
                i = e + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn is_ws16(c: u16) -> bool {
    char::from_u32(c as u32).is_some_and(js::is_js_space)
}

struct Parser<'a> {
    source: &'a [u16],
    src: &'a [u16],
    edits: Vec<Edit>,
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    locs: Vec<usize>,
    i: usize,
    pending_data: Option<String>,
    data_from: usize,
    data_to: usize,
    db: &'a mut AgentflowDb,
}

impl Parser<'_> {
    fn ty(&self) -> u8 {
        self.types[self.i]
    }

    fn fail<T>(&self, expected: &str) -> Result<T> {
        let ty = self.ty();
        let mut from = self.starts[self.i];
        if ty == t::EOF || ty == t::END {
            // ここではコメントも字句なので、後ろのものは飛ばす。
            let mut k = self.i;
            while k > 0 && is_trailing(self.types[k - 1]) {
                k -= 1;
            }
            let head = &self.src[..self.starts[k].min(self.src.len())];
            let mut e = head.len();
            while e > 0 && is_ws16(head[e - 1]) {
                e -= 1;
            }
            from = e;
        }
        let at = if self.src == self.source {
            from
        } else {
            input_offset(std::slice::from_ref(&self.edits), from)
        };
        if ty == t::ERROR {
            return Err(syntax_error("agentflow", self.source, at, "", true));
        }
        Err(syntax_error(
            "agentflow",
            self.source,
            at,
            &format!("Expecting {expected}, got '{}'", TOKEN_NAMES[ty as usize]),
            false,
        ))
    }

    /// 字句 `a` の始まりから字句 `b` の終わりまで。
    fn span(&self, a: usize, b: usize) -> Loc {
        Loc {
            first_line: self.locs[4 * a],
            first_column: self.locs[4 * a + 1],
            last_line: self.locs[4 * b + 2],
            last_column: self.locs[4 * b + 3],
        }
    }

    fn take(&mut self) -> String {
        let s = self.texts[self.i].clone();
        self.i += 1;
        s
    }

    fn expect(&mut self, ty: u8, also: &str) -> Result<String> {
        if self.ty() != ty {
            let name = TOKEN_NAMES[ty as usize];
            return self.fail(&if also.is_empty() {
                format!("'{name}'")
            } else {
                format!("'{name}', {also}")
            });
        }
        Ok(self.take())
    }

    fn separator(&mut self) -> Result<()> {
        if !is_separator(self.ty()) {
            return self.fail(SEPARATORS);
        }
        self.i += 1;
        Ok(())
    }

    fn run(&mut self, accept: impl Fn(u8) -> bool, expected: &str) -> Result<String> {
        if !accept(self.ty()) {
            return self.fail(expected);
        }
        let mut s = self.take();
        while accept(self.ty()) {
            s.push_str(&self.take());
        }
        Ok(s)
    }

    fn id_string(&mut self) -> Result<String> {
        self.run(is_id, "'NODE_STRING'")
    }

    fn alpha_num(&mut self) -> Result<String> {
        self.run(is_alpha_num, "'NODE_STRING'")
    }

    fn text(&mut self) -> Result<FlowText> {
        let type_ = match self.ty() {
            t::STR => LabelType::String,
            t::MD_STR => LabelType::Markdown,
            t::TEXT | t::TAGSTART | t::TAGEND | t::UNICODE_TEXT => LabelType::Text,
            _ => return self.fail(&format!("{TEXT_TOKENS}, 'TAGEND', 'STR', 'MD_STR'")),
        };
        let mut s = self.take();
        while matches!(
            self.ty(),
            t::TEXT | t::TAGSTART | t::TAGEND | t::UNICODE_TEXT
        ) {
            s.push_str(&self.take());
        }
        Ok(FlowText { text: s, type_ })
    }

    fn closed(&mut self, ty: u8) -> Result<()> {
        self.expect(ty, TEXT_TOKENS).map(|_| ())
    }

    fn edge_text(&mut self) -> Result<FlowText> {
        let type_ = match self.ty() {
            t::STR => LabelType::String,
            t::MD_STR => LabelType::Markdown,
            t::EDGE_TEXT | t::UNICODE_TEXT => LabelType::Text,
            _ => return self.fail("'EDGE_TEXT', 'STR', 'MD_STR'"),
        };
        let mut s = self.take();
        while matches!(self.ty(), t::EDGE_TEXT | t::UNICODE_TEXT) {
            s.push_str(&self.take());
        }
        Ok(FlowText { text: s, type_ })
    }

    fn text_no_tags(&mut self) -> Result<FlowText> {
        let ty = self.ty();
        let type_ = if ty == t::STR {
            LabelType::Text
        } else if ty == t::MD_STR {
            LabelType::Markdown
        } else if is_no_tags(ty) {
            LabelType::Text
        } else {
            return self.fail("'NODE_STRING', 'STR', 'MD_STR'");
        };
        let mut s = self.take();
        while is_no_tags(self.ty()) {
            s.push_str(&self.take());
        }
        Ok(FlowText { text: s, type_ })
    }

    fn shape_data(&mut self) -> String {
        self.data_from = self.i;
        let mut s = self.take();
        while self.ty() == t::SHAPE_DATA {
            s.push_str(&self.take());
        }
        self.data_to = self.i - 1;
        s
    }

    fn attach(&mut self, id: &str, data: &str, from: usize, to: usize) -> Result<()> {
        let loc = self.span(from, to);
        self.db.add_vertex(
            id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(data),
            Some(loc),
        )
    }

    fn attach_data(&mut self, id: &str, frame: &Frame) -> Result<()> {
        let Some(data) = &frame.data else {
            return Ok(());
        };
        let loc = self.span(frame.data_from, frame.data_to);
        self.db.add_vertex(
            id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(data),
            Some(loc),
        )
    }

    fn vertex(&mut self) -> Result<String> {
        let id = self.id_string()?;
        let mut label: Option<FlowText> = None;
        let mut type_: Option<&str> = None;
        let mut props: Option<(String, String)> = None;
        match self.ty() {
            t::SQS => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(t::SQE)?;
                type_ = Some("square");
            }
            t::DOUBLECIRCLESTART => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(t::DOUBLECIRCLEEND)?;
                type_ = Some("doublecircle");
            }
            t::PS => {
                self.i += 1;
                if self.ty() == t::PS {
                    self.i += 1;
                    label = Some(self.text()?);
                    self.closed(t::PE)?;
                    self.expect(t::PE, "")?;
                    type_ = Some("circle");
                } else {
                    label = Some(self.text()?);
                    self.closed(t::PE)?;
                    type_ = Some("round");
                }
            }
            t::ELLIPSE_START => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(t::ELLIPSE_END)?;
                type_ = Some("ellipse");
            }
            t::STADIUMSTART => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(t::STADIUMEND)?;
                type_ = Some("stadium");
            }
            t::SUBROUTINESTART => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(t::SUBROUTINEEND)?;
                type_ = Some("subroutine");
            }
            t::VERTEX_WITH_PROPS_START => {
                self.i += 1;
                let field = self.expect(t::NODE_STRING, "")?;
                self.expect(t::COLON, "")?;
                let value = self.expect(t::NODE_STRING, "")?;
                self.expect(t::PIPE, "")?;
                label = Some(self.text()?);
                self.closed(t::SQE)?;
                type_ = Some("rect");
                props = Some((field, value));
            }
            t::CYLINDERSTART => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(t::CYLINDEREND)?;
                type_ = Some("cylinder");
            }
            t::DIAMOND_START => {
                self.i += 1;
                if self.ty() == t::DIAMOND_START {
                    self.i += 1;
                    label = Some(self.text()?);
                    self.closed(t::DIAMOND_STOP)?;
                    self.expect(t::DIAMOND_STOP, "")?;
                    type_ = Some("hexagon");
                } else {
                    label = Some(self.text()?);
                    self.closed(t::DIAMOND_STOP)?;
                    type_ = Some("diamond");
                }
            }
            t::TAGEND => {
                self.i += 1;
                label = Some(self.text()?);
                self.closed(t::SQE)?;
                type_ = Some("odd");
            }
            t::TRAPSTART => {
                self.i += 1;
                label = Some(self.text()?);
                if self.ty() == t::INVTRAPEND {
                    self.i += 1;
                    type_ = Some("lean_right");
                } else {
                    self.closed(t::TRAPEND)?;
                    type_ = Some("trapezoid");
                }
            }
            t::INVTRAPSTART => {
                self.i += 1;
                label = Some(self.text()?);
                if self.ty() == t::TRAPEND {
                    self.i += 1;
                    type_ = Some("lean_left");
                } else {
                    self.closed(t::INVTRAPEND)?;
                    type_ = Some("inv_trapezoid");
                }
            }
            _ => {}
        }
        if props.is_some() {
            self.db.add_vertex(
                &id,
                label.as_ref(),
                type_,
                None,
                None,
                None,
                props,
                None,
                None,
            )?;
        } else if type_.is_some() {
            self.db.add_vertex(
                &id,
                label.as_ref(),
                type_,
                None,
                None,
                None,
                None,
                None,
                None,
            )?;
        } else {
            self.db
                .add_vertex(&id, None, None, None, None, None, None, None, None)?;
        }
        Ok(id)
    }

    fn styled_vertex(&mut self) -> Result<String> {
        let id = self.vertex()?;
        if self.ty() == t::STYLE_SEPARATOR {
            self.i += 1;
            let class = self.id_string()?;
            self.db.set_class(&id, &class);
        }
        Ok(id)
    }

    fn node(&mut self) -> Result<Vec<String>> {
        let mut ids = vec![self.styled_vertex()?];
        self.pending_data = None;
        loop {
            let data = if self.ty() == t::SHAPE_DATA {
                Some(self.shape_data())
            } else {
                None
            };
            let from = self.data_from;
            let to = self.data_to;
            if self.ty() != t::SPACE {
                self.pending_data = data;
                return Ok(ids);
            }
            self.i += 1;
            while self.ty() == t::SPACE {
                self.i += 1;
            }
            if self.ty() != t::AMP {
                if data.is_some() {
                    return self.fail("'AMP'");
                }
                return Ok(ids);
            }
            self.i += 1;
            if self.ty() != t::SPACE {
                return self.fail("'SPACE'");
            }
            self.i += 1;
            while self.ty() == t::SPACE {
                self.i += 1;
            }
            let next = self.styled_vertex()?;
            if let Some(d) = data {
                let last = ids.last().unwrap().clone();
                self.attach(&last, &d, from, to)?;
            }
            ids.push(next);
        }
    }

    fn link(&mut self) -> Result<LinkInfo> {
        let mut id: Option<String> = None;
        if self.ty() == t::LINK_ID {
            id = Some(self.take());
        }
        if self.ty() == t::LINK {
            let text = self.take();
            let mut info = self.db.destruct_link(&text, None);
            if id.is_some() {
                info.id = id;
            }
            if self.ty() == t::PIPE {
                self.i += 1;
                info.text = Some(self.text()?);
                self.closed(t::PIPE)?;
                if self.ty() == t::SPACE {
                    self.i += 1;
                }
            }
            return Ok(info);
        }
        if self.ty() == t::START_LINK {
            let start = self.take();
            let label = self.edge_text()?;
            let end = self.expect(t::LINK, "'EDGE_TEXT'")?;
            let mut info = self.db.destruct_link(&end, Some(&start));
            info.text = Some(label);
            if id.is_some() {
                info.id = id;
            }
            return Ok(info);
        }
        self.fail("'LINK', 'START_LINK'")
    }

    fn vertex_statement(&mut self) -> Result<Vec<String>> {
        let mut stmt = self.node()?;
        if let Some(d) = self.pending_data.take() {
            let last = stmt.last().unwrap().clone();
            let (from, to) = (self.data_from, self.data_to);
            self.attach(&last, &d, from, to)?;
        }
        let mut nodes = stmt.clone();
        loop {
            let ty = self.ty();
            if ty != t::LINK && ty != t::START_LINK && ty != t::LINK_ID {
                return Ok(nodes);
            }
            let info = self.link()?;
            let next = self.node()?;
            if let Some(d) = self.pending_data.take() {
                let last = next.last().unwrap().clone();
                let (from, to) = (self.data_from, self.data_to);
                self.attach(&last, &d, from, to)?;
            }
            self.db.add_link(&stmt, &next, Some(&info))?;
            let mut joined = next.clone();
            joined.extend(nodes);
            nodes = joined;
            stmt = next;
        }
    }

    fn style(&mut self) -> Result<String> {
        self.run(is_style_part, "'NODE_STRING', 'NUM', 'COLON', 'BRKT'")
    }

    fn styles_opt(&mut self) -> Result<Vec<String>> {
        let mut out = vec![self.style()?];
        while self.ty() == t::COMMA {
            self.i += 1;
            out.push(self.style()?);
        }
        Ok(out)
    }

    fn link_style_statement(&mut self) -> Result<()> {
        self.i += 1;
        self.expect(t::SPACE, "")?;
        let mut positions: Vec<String>;
        if self.ty() == t::DEFAULT {
            positions = vec![self.take()];
        } else {
            positions = vec![self.expect(t::NUM, "'DEFAULT'")?];
            while self.ty() == t::COMMA {
                self.i += 1;
                positions.push(self.expect(t::NUM, "")?);
            }
        }
        self.expect(t::SPACE, "")?;
        if self.ty() != t::INTERPOLATE {
            let styles = self.styles_opt()?;
            return self.db.update_link(&positions, styles);
        }
        self.i += 1;
        self.expect(t::SPACE, "")?;
        let interpolate = self.alpha_num()?;
        if self.ty() == t::SPACE {
            self.i += 1;
            let styles = self.styles_opt()?;
            self.db.update_link_interpolate(&positions, &interpolate)?;
            self.db.update_link(&positions, styles)
        } else {
            self.db.update_link_interpolate(&positions, &interpolate)
        }
    }

    fn link_tail(&mut self, id: &str, url: &str) -> Result<()> {
        if self.ty() != t::SPACE {
            self.db.set_link(id, url, None);
            return Ok(());
        }
        self.i += 1;
        if self.ty() == t::LINK_TARGET {
            let target = self.take();
            self.db.set_link(id, url, Some(&target));
            return Ok(());
        }
        let tooltip = self.expect(t::STR, "'LINK_TARGET'")?;
        if self.ty() == t::SPACE {
            self.i += 1;
            let target = self.expect(t::LINK_TARGET, "")?;
            self.db.set_link(id, url, Some(&target));
        } else {
            self.db.set_link(id, url, None);
        }
        self.db.set_tooltip(id, &tooltip);
        Ok(())
    }

    fn click_statement(&mut self) -> Result<()> {
        let id = self.take();
        match self.ty() {
            t::CALLBACKNAME => {
                self.i += 1;
                if self.ty() == t::CALLBACKARGS {
                    self.i += 1;
                }
                let mut tooltip: Option<String> = None;
                if self.ty() == t::SPACE {
                    self.i += 1;
                    tooltip = Some(self.expect(t::STR, "")?);
                }
                self.db.set_click_event(&id);
                if let Some(tt) = tooltip {
                    self.db.set_tooltip(&id, &tt);
                }
                Ok(())
            }
            t::HREF => {
                self.i += 1;
                let url = self.expect(t::STR, "")?;
                self.link_tail(&id, &url)
            }
            t::STR => {
                let url = self.take();
                self.link_tail(&id, &url)
            }
            _ => {
                self.alpha_num()?;
                if self.ty() == t::SPACE {
                    self.i += 1;
                    let tooltip = self.expect(t::STR, "")?;
                    self.db.set_click_event(&id);
                    self.db.set_tooltip(&id, &tooltip);
                } else {
                    self.db.set_click_event(&id);
                }
                Ok(())
            }
        }
    }

    /// `flow` か `connector` を区切りまで読む: 名前、括弧の中の題（なくてよい）、メタデータ（なくてよい）。
    fn header(&mut self, at: usize) -> Result<Frame> {
        self.expect(t::SPACE, SEPARATORS)?;
        let id = self.text_no_tags()?;
        let mut title = FlowText {
            text: String::new(),
            type_: LabelType::Text,
        };
        if self.ty() == t::SQS {
            self.i += 1;
            title = self.text()?;
            self.closed(t::SQE)?;
        }
        let data = if self.ty() == t::SHAPE_DATA {
            Some(self.shape_data())
        } else {
            None
        };
        if !is_separator(self.ty()) {
            return self.fail(&format!("{SEPARATORS}, 'SHAPE_DATA'"));
        }
        Ok(Frame {
            global: false,
            at,
            id: Some(id),
            title: Some(title),
            data,
            data_from: self.data_from,
            data_to: self.data_to,
            list: Vec::new(),
            collect: false,
        })
    }

    /// 文を 1 つ読む。何も足さない文は None。
    fn statement(&mut self) -> Result<Option<DocItem>> {
        let ty = self.ty();
        match ty {
            t::STYLE => {
                self.i += 1;
                self.expect(t::SPACE, "")?;
                let id = self.id_string()?;
                self.expect(t::SPACE, "")?;
                let styles = self.styles_opt()?;
                self.db
                    .add_vertex(&id, None, None, Some(&styles), None, None, None, None, None)?;
                self.separator()?;
                Ok(Some(DocItem::List(Vec::new())))
            }
            t::LINKSTYLE => {
                self.link_style_statement()?;
                self.separator()?;
                Ok(Some(DocItem::List(Vec::new())))
            }
            t::CLASSDEF => {
                self.i += 1;
                self.expect(t::SPACE, "")?;
                let id = self.id_string()?;
                self.expect(t::SPACE, "")?;
                let styles = self.styles_opt()?;
                self.db.add_class(&id, &styles);
                self.separator()?;
                Ok(Some(DocItem::List(Vec::new())))
            }
            t::CLASS => {
                self.i += 1;
                self.expect(t::SPACE, "")?;
                let ids = self.id_string()?;
                self.expect(t::SPACE, "")?;
                let class = self.id_string()?;
                self.db.set_class(&ids, &class);
                self.separator()?;
                Ok(Some(DocItem::List(Vec::new())))
            }
            t::CLICK => {
                self.click_statement()?;
                self.separator()?;
                Ok(Some(DocItem::List(Vec::new())))
            }
            t::connector => {
                let at = self.i;
                self.i += 1;
                let frame = self.header(at)?;
                self.i += 1;
                let id = self
                    .db
                    .add_connector(frame.id.as_ref(), frame.title.as_ref());
                self.attach_data(&id, &frame)?;
                Ok(Some(DocItem::Str(id)))
            }
            t::direction_tb
            | t::direction_bt
            | t::direction_rl
            | t::direction_lr
            | t::direction_td => {
                self.i += 1;
                Ok(Some(DocItem::Dir(
                    DIRECTIONS[(ty - t::direction_tb) as usize].to_string(),
                )))
            }
            t::acc_title => {
                self.i += 1;
                let value = js::trim(&self.expect(t::acc_title_value, "")?).to_string();
                self.db.set_acc_title(&value);
                Ok(Some(DocItem::Str(value)))
            }
            t::acc_descr => {
                self.i += 1;
                let value = js::trim(&self.expect(t::acc_descr_value, "")?).to_string();
                self.db.set_acc_description(&value);
                Ok(Some(DocItem::Str(value)))
            }
            t::acc_descr_multiline_value => {
                let value = js::trim(&self.take()).to_string();
                self.db.set_acc_description(&value);
                Ok(Some(DocItem::Str(value)))
            }
            _ => {
                if !is_id(ty) {
                    return self.fail(STATEMENT_START);
                }
                let nodes = self.vertex_statement()?;
                self.separator()?;
                Ok(Some(DocItem::List(nodes)))
            }
        }
    }

    /// 塊は再帰せずに入れ子にする。開いている塊は、それぞれ親の並びと集める印を持つ。
    fn document(&mut self) -> Result<()> {
        let mut open: Vec<Frame> = Vec::new();
        let mut list: Vec<DocItem> = Vec::new();
        let mut collect = false;
        loop {
            let ty = self.ty();
            if ty == t::END {
                break;
            }
            if ty == t::end {
                let Some(frame) = open.pop() else {
                    break;
                };
                self.i += 1;
                if frame.global {
                    self.db.add_global(&list);
                    list = frame.list;
                    collect = frame.collect;
                    continue;
                }
                let id = self
                    .db
                    .add_sub_graph(frame.id.as_ref(), &list, frame.title.as_ref());
                self.attach_data(&id, &frame)?;
                list = frame.list;
                collect = frame.collect;
                if collect {
                    list.push(DocItem::Str(id));
                }
            } else if matches!(ty, t::SEMI | t::NEWLINE | t::SPACE | t::EOF) {
                if collect {
                    list.push(DocItem::Str(self.texts[self.i].clone()));
                }
                self.i += 1;
            } else if ty == t::COMMENT {
                self.i += 1;
            } else if ty == t::flow || ty == t::global {
                let at = self.i;
                self.i += 1;
                let frame = if ty == t::global || is_separator(self.ty()) {
                    self.separator()?;
                    Frame {
                        global: ty == t::global,
                        at,
                        id: None,
                        title: None,
                        data: None,
                        data_from: 0,
                        data_to: 0,
                        list: std::mem::take(&mut list),
                        collect,
                    }
                } else {
                    let mut f = self.header(at)?;
                    f.list = std::mem::take(&mut list);
                    f.collect = collect;
                    self.i += 1;
                    f
                };
                let _ = frame.at;
                open.push(frame);
                collect = true;
            } else {
                let item = self.statement()?;
                if let Some(item) = item
                    && collect
                    && !matches!(&item, DocItem::List(v) if v.is_empty())
                {
                    list.push(item);
                }
            }
        }
        if !open.is_empty() {
            return self.fail("'end'");
        }
        Ok(())
    }
}

/// `raw` は、Mermaid の作った読み手が包みなしで読むように、文字列をそのまま読む。
pub fn parse_agentflow(source: &str, db: &mut AgentflowDb, raw: bool) -> Result<()> {
    let source16: Vec<u16> = source.encode_utf16().collect();
    let trails = if !raw && source.contains('}') {
        brace_trails(&source16)
    } else {
        Vec::new()
    };
    let src16: Vec<u16> = if trails.is_empty() {
        source16.clone()
    } else {
        let mut out = Vec::with_capacity(source16.len());
        let mut last = 0;
        for &(a, b) in &trails {
            out.extend_from_slice(&source16[last..a]);
            out.extend_from_slice(&[b'}' as u16, 10]);
            last = b;
        }
        out.extend_from_slice(&source16[last..]);
        out
    };
    let edits: Vec<Edit> = trails.iter().map(|&(a, b)| (a, b, 2)).collect();
    let tokens = tokenize(&src16);
    let mut p = Parser {
        source: &source16,
        src: &src16,
        edits,
        types: tokens.types,
        texts: tokens.texts,
        starts: tokens.starts,
        locs: tokens.locs,
        i: 0,
        pending_data: None,
        data_from: 0,
        data_to: 0,
        db,
    };

    while matches!(p.ty(), t::SPACE | t::NEWLINE | t::COMMENT) {
        p.i += 1;
    }
    p.expect(t::GRAPH, "'SPACE', 'NEWLINE', 'COMMENT'")?;
    if p.ty() == t::NODIR {
        p.i += 1;
        p.db.set_direction("TB");
    } else {
        let dir = p.expect(t::DIR, "'NODIR'")?;
        let mut ty = p.ty();
        if ty == t::SPACE {
            p.i += 1;
            while p.ty() == t::SPACE {
                p.i += 1;
            }
            ty = p.ty();
            if ty != t::NEWLINE {
                return p.fail("'NEWLINE', 'SPACE'");
            }
        } else if ty != t::SEMI && ty != t::NEWLINE && ty != t::COMMENT {
            return p.fail("'COMMENT', 'SEMI', 'NEWLINE', 'SPACE'");
        }
        p.i += 1;
        p.db.set_direction(&dir);
    }
    p.document()?;
    if p.ty() != t::END {
        return p.fail(STATEMENT_START);
    }
    Ok(())
}
