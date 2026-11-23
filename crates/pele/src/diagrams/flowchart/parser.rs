//! flowchart の構文解析（pele の `src/diagrams/flowchart/parser.ts`）。Mermaid の jison の文法を手で書いた下降型に
//! したもの。知らせの文は Mermaid と同じ。

use super::db::FlowDb;
use super::lexer::tokenize;
use super::tokens::{self as t, TOKEN_NAMES};
use super::types::{DocItem, FlowText, LabelType, LinkInfo};
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
            | t::subgraph
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

struct Frame {
    id: Option<FlowText>,
    title: Option<FlowText>,
    /// 題を id と別に書かなかった。
    same: bool,
    list: Vec<DocItem>,
    collect: bool,
}

const TEXT_TOKENS: &str = "'UNICODE_TEXT', 'TEXT', 'TAGSTART'";
const DIRECTIONS: [&str; 5] = ["TB", "BT", "RL", "LR", "TD"];
const STATEMENT_START: &str = "'SEMI', 'NEWLINE', 'SPACE', 'EOF', 'subgraph', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'STYLE', 'LINKSTYLE', 'CLASSDEF', 'CLASS', 'CLICK', 'NODE_STRING'";

/// `/}\s*\n/g`（`}` から、続く空白の中の最後の改行まで）の範囲（UTF-16 の位置）。
fn brace_trails(src: &[u16]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        if src[i] == b'}' as u16 {
            let mut e = i + 1;
            while e < src.len() && crate::util::chars::is_space(src[e] as u32) {
                e += 1;
            }
            if let Some(k) = (i + 1..e).rev().find(|&k| src[k] == 10) {
                out.push((i, k + 1));
                i = k + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

struct Parser<'a> {
    source: &'a [u16],
    changed: bool,
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    i: usize,
    pending_data: Option<String>,
    db: &'a mut FlowDb,
}

impl Parser<'_> {
    fn ty(&self) -> u8 {
        self.types[self.i]
    }

    fn fail<T>(&self, expected: &str) -> Result<T> {
        let ty = self.ty();
        let at = if !self.changed {
            self.starts[self.i]
        } else {
            let edits: Vec<Edit> = brace_trails(self.source)
                .into_iter()
                .map(|(a, b)| (a, b, 2))
                .collect();
            input_offset(&[edits], self.starts[self.i])
        };
        if ty == t::ERROR {
            return Err(syntax_error("flowchart", self.source, at, "", true));
        }
        Err(syntax_error(
            "flowchart",
            self.source,
            at,
            &format!("Expecting {expected}, got '{}'", TOKEN_NAMES[ty as usize]),
            false,
        ))
    }

    fn take(&mut self) -> String {
        let s = std::mem::take(&mut self.texts[self.i]);
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
        let ty = self.ty();
        if ty != t::NEWLINE && ty != t::SEMI && ty != t::EOF {
            return self.fail("'SEMI', 'NEWLINE', 'EOF'");
        }
        self.i += 1;
        Ok(())
    }

    fn run(
        &mut self,
        first: impl Fn(u8) -> bool,
        more: impl Fn(u8) -> bool,
        expected: &str,
    ) -> Result<String> {
        if !first(self.ty()) {
            return self.fail(expected);
        }
        let mut s = self.take();
        while more(self.ty()) {
            s.push_str(&self.take());
        }
        Ok(s)
    }

    fn id_string(&mut self) -> Result<String> {
        self.run(is_id, is_id, "'NODE_STRING'")
    }

    fn alpha_num(&mut self) -> Result<String> {
        self.run(is_alpha_num, is_alpha_num, "'NODE_STRING'")
    }

    fn text(&mut self) -> Result<FlowText> {
        let ty = self.ty();
        let type_ = match ty {
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
        let type_ = if ty == t::STR || is_no_tags(ty) {
            LabelType::Text
        } else if ty == t::MD_STR {
            LabelType::Markdown
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
        let mut s = self.take();
        while self.ty() == t::SHAPE_DATA {
            s.push_str(&self.take());
        }
        s
    }

    fn add(&mut self, id: &str, label: &FlowText, shape: &str) -> Result<()> {
        self.db
            .add_vertex(id, Some(label), Some(shape), None, None, None, None, None)
    }

    fn vertex(&mut self) -> Result<String> {
        let id = self.id_string()?;
        match self.ty() {
            t::SQS => {
                self.i += 1;
                let label = self.text()?;
                self.closed(t::SQE)?;
                self.add(&id, &label, "square")?;
            }
            t::DOUBLECIRCLESTART => {
                self.i += 1;
                let label = self.text()?;
                self.closed(t::DOUBLECIRCLEEND)?;
                self.add(&id, &label, "doublecircle")?;
            }
            t::PS => {
                self.i += 1;
                if self.ty() == t::PS {
                    self.i += 1;
                    let label = self.text()?;
                    self.closed(t::PE)?;
                    self.expect(t::PE, "")?;
                    self.add(&id, &label, "circle")?;
                } else {
                    let label = self.text()?;
                    self.closed(t::PE)?;
                    self.add(&id, &label, "round")?;
                }
            }
            t::ELLIPSE_START => {
                self.i += 1;
                let label = self.text()?;
                self.closed(t::ELLIPSE_END)?;
                self.add(&id, &label, "ellipse")?;
            }
            t::STADIUMSTART => {
                self.i += 1;
                let label = self.text()?;
                self.closed(t::STADIUMEND)?;
                self.add(&id, &label, "stadium")?;
            }
            t::SUBROUTINESTART => {
                self.i += 1;
                let label = self.text()?;
                self.closed(t::SUBROUTINEEND)?;
                self.add(&id, &label, "subroutine")?;
            }
            t::VERTEX_WITH_PROPS_START => {
                self.i += 1;
                let field = self.expect(t::NODE_STRING, "")?;
                self.expect(t::COLON, "")?;
                let value = self.expect(t::NODE_STRING, "")?;
                self.expect(t::PIPE, "")?;
                let label = self.text()?;
                self.closed(t::SQE)?;
                self.db.add_vertex(
                    &id,
                    Some(&label),
                    Some("rect"),
                    None,
                    None,
                    None,
                    Some((field, value)),
                    None,
                )?;
            }
            t::CYLINDERSTART => {
                self.i += 1;
                let label = self.text()?;
                self.closed(t::CYLINDEREND)?;
                self.add(&id, &label, "cylinder")?;
            }
            t::DIAMOND_START => {
                self.i += 1;
                if self.ty() == t::DIAMOND_START {
                    self.i += 1;
                    let label = self.text()?;
                    self.closed(t::DIAMOND_STOP)?;
                    self.expect(t::DIAMOND_STOP, "")?;
                    self.add(&id, &label, "hexagon")?;
                } else {
                    let label = self.text()?;
                    self.closed(t::DIAMOND_STOP)?;
                    self.add(&id, &label, "diamond")?;
                }
            }
            t::TAGEND => {
                self.i += 1;
                let label = self.text()?;
                self.closed(t::SQE)?;
                self.add(&id, &label, "odd")?;
            }
            t::TRAPSTART => {
                self.i += 1;
                let label = self.text()?;
                if self.ty() == t::INVTRAPEND {
                    self.i += 1;
                    self.add(&id, &label, "lean_right")?;
                } else {
                    self.closed(t::TRAPEND)?;
                    self.add(&id, &label, "trapezoid")?;
                }
            }
            t::INVTRAPSTART => {
                self.i += 1;
                let label = self.text()?;
                if self.ty() == t::TRAPEND {
                    self.i += 1;
                    self.add(&id, &label, "lean_left")?;
                } else {
                    self.closed(t::INVTRAPEND)?;
                    self.add(&id, &label, "inv_trapezoid")?;
                }
            }
            _ => self
                .db
                .add_vertex(&id, None, None, None, None, None, None, None)?,
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

    fn meta(&mut self, id: &str, data: &str) -> Result<()> {
        self.db
            .add_vertex(id, None, None, None, None, None, None, Some(data))
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
            if self.ty() != t::SPACE {
                self.pending_data = data;
                return Ok(ids);
            }
            loop {
                self.i += 1;
                if self.ty() != t::SPACE {
                    break;
                }
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
            loop {
                self.i += 1;
                if self.ty() != t::SPACE {
                    break;
                }
            }
            let next = self.styled_vertex()?;
            if let Some(d) = data {
                let last = ids.last().unwrap().clone();
                self.meta(&last, &d)?;
            }
            ids.push(next);
        }
    }

    fn link(&mut self) -> Result<LinkInfo> {
        let mut id = None;
        if self.ty() == t::LINK_ID {
            id = Some(self.take());
        }
        if self.ty() == t::LINK {
            let raw = self.take();
            let mut info = self.db.destruct_link(&raw, None);
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

    fn vertex_statement(&mut self, collect: bool) -> Result<Vec<String>> {
        let mut stmt = self.node()?;
        if let Some(d) = self.pending_data.take() {
            let last = stmt.last().unwrap().clone();
            self.meta(&last, &d)?;
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
                self.meta(&last, &d)?;
            }
            self.db.add_link(&stmt, &next, Some(&info))?;
            if collect {
                let mut joined = next.clone();
                joined.extend(nodes);
                nodes = joined;
            }
            stmt = next;
        }
    }

    fn style(&mut self) -> Result<String> {
        self.run(
            is_style_part,
            is_style_part,
            "'NODE_STRING', 'NUM', 'COLON', 'BRKT'",
        )
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
        let mut positions = Vec::new();
        if self.ty() == t::DEFAULT {
            positions.push(self.take());
        } else {
            positions.push(self.expect(t::NUM, "'DEFAULT'")?);
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
            self.db.update_link(&positions, styles)?;
        } else {
            self.db.update_link_interpolate(&positions, &interpolate)?;
        }
        Ok(())
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
                let mut tooltip = None;
                if self.ty() == t::SPACE {
                    self.i += 1;
                    tooltip = Some(self.expect(t::STR, "")?);
                }
                self.db.set_click_event(&id);
                if let Some(tt) = tooltip {
                    self.db.set_tooltip(&id, &tt);
                }
            }
            t::HREF => {
                self.i += 1;
                let url = self.expect(t::STR, "")?;
                self.link_tail(&id, &url)?;
            }
            t::STR => {
                let url = self.take();
                self.link_tail(&id, &url)?;
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
            }
        }
        Ok(())
    }

    fn subgraph_header(&mut self) -> Result<Frame> {
        self.i += 1;
        let ty = self.ty();
        if ty == t::NEWLINE || ty == t::SEMI || ty == t::EOF {
            self.i += 1;
            return Ok(Frame {
                id: None,
                title: None,
                same: false,
                list: Vec::new(),
                collect: false,
            });
        }
        self.expect(t::SPACE, "'SEMI', 'NEWLINE', 'EOF'")?;
        let id = self.text_no_tags()?;
        let mut title = id.clone();
        let mut same = true;
        if self.ty() == t::SQS {
            self.i += 1;
            title = self.text()?;
            same = false;
            self.closed(t::SQE)?;
        }
        let ty = self.ty();
        if ty != t::NEWLINE && ty != t::SEMI && ty != t::EOF {
            return self.fail("'SEMI', 'NEWLINE', 'EOF', 'SQS'");
        }
        self.i += 1;
        Ok(Frame {
            id: Some(id),
            title: Some(title),
            same,
            list: Vec::new(),
            collect: false,
        })
    }

    /// 文を 1 つ読む。集めるもの（subgraph の中身）を返す。
    fn statement(&mut self, collect: bool) -> Result<Option<DocItem>> {
        let ty = self.ty();
        match ty {
            t::STYLE => {
                self.i += 1;
                self.expect(t::SPACE, "")?;
                let id = self.id_string()?;
                self.expect(t::SPACE, "")?;
                let styles = self.styles_opt()?;
                self.db
                    .add_vertex(&id, None, None, Some(&styles), None, None, None, None)?;
                self.separator()?;
                Ok(None)
            }
            t::LINKSTYLE => {
                self.link_style_statement()?;
                self.separator()?;
                Ok(None)
            }
            t::CLASSDEF => {
                self.i += 1;
                self.expect(t::SPACE, "")?;
                let id = self.id_string()?;
                self.expect(t::SPACE, "")?;
                let styles = self.styles_opt()?;
                self.db.add_class(&id, &styles);
                self.separator()?;
                Ok(None)
            }
            t::CLASS => {
                self.i += 1;
                self.expect(t::SPACE, "")?;
                let ids = self.id_string()?;
                self.expect(t::SPACE, "")?;
                let class = self.id_string()?;
                self.db.set_class(&ids, &class);
                self.separator()?;
                Ok(None)
            }
            t::CLICK => {
                self.click_statement()?;
                self.separator()?;
                Ok(None)
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
                let nodes = self.vertex_statement(collect)?;
                self.separator()?;
                Ok(if nodes.is_empty() {
                    None
                } else {
                    Some(DocItem::List(nodes))
                })
            }
        }
    }

    /// subgraph は再帰せずに入れ子にする: 開いたものはどれも、親の並びと集めるかを持つ。
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
                let Some(frame) = open.pop() else { break };
                self.i += 1;
                let id = self.db.add_sub_graph(
                    frame.id.as_ref(),
                    &list,
                    frame.title.as_ref(),
                    frame.same,
                );
                list = frame.list;
                collect = frame.collect;
                if collect {
                    list.push(DocItem::Str(id));
                }
            } else if ty == t::SEMI || ty == t::NEWLINE || ty == t::SPACE || ty == t::EOF {
                if collect {
                    list.push(DocItem::Str(self.texts[self.i].clone()));
                }
                self.i += 1;
            } else if ty == t::subgraph {
                let mut frame = self.subgraph_header()?;
                frame.list = std::mem::take(&mut list);
                frame.collect = collect;
                open.push(frame);
                collect = true;
            } else {
                let item = self.statement(collect)?;
                if collect && let Some(item) = item {
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

pub fn parse_flowchart(source: &str, db: &mut FlowDb) -> Result<()> {
    let source16: Vec<u16> = source.encode_utf16().collect();
    let trails = if source.contains('}') {
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
    let changed = src16 != source16;
    let tokens = tokenize(&src16);
    let mut p = Parser {
        source: &source16,
        changed,
        types: tokens.types,
        texts: tokens.texts,
        starts: tokens.starts,
        i: 0,
        pending_data: None,
        db,
    };

    while p.ty() == t::SPACE || p.ty() == t::NEWLINE {
        p.i += 1;
    }
    p.expect(t::GRAPH, "'SPACE', 'NEWLINE'")?;
    if p.ty() == t::NODIR {
        p.i += 1;
        p.db.set_direction("TB");
    } else {
        let dir = p.expect(t::DIR, "'NODIR'")?;
        let mut ty = p.ty();
        if ty == t::SPACE {
            loop {
                p.i += 1;
                if p.ty() != t::SPACE {
                    break;
                }
            }
            ty = p.ty();
            if ty != t::NEWLINE {
                return p.fail("'NEWLINE', 'SPACE'");
            }
        } else if ty != t::SEMI && ty != t::NEWLINE {
            return p.fail("'SEMI', 'NEWLINE', 'SPACE'");
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
