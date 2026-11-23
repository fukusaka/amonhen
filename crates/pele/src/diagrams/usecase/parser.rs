//! use case の構文解析（pele の `src/diagrams/usecase/parser.ts`）。Mermaid は Chevrotain の読み手で読み、選択を
//! 高々 3 つ先の字句で決め、どこにも合わない最初の字句を知らせる。ここも同じ先読みで同じように決めるので、同じ文を
//! 受け付け、知らせで同じ字句を名指す。Mermaid が構文木を作ってから巡るところを、ここでは直に形を作る。

use super::db::{
    DraftBoundary, DraftClassDef, DraftClasses, DraftElement, DraftEndpoint, DraftJson, DraftLabel,
    DraftMetadata, DraftNote, DraftProperty, DraftRelationship, DraftStyle, Drafts, PropValue,
    resolve_usecase,
};
use super::json::parse_ordered_json;
use super::lexer::{line_column, location_text, tokenize};
use super::tokens::{self as t, TOKEN_NAMES};
use super::types::{LabelType, Span, UsecaseModel, arrow};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;
use crate::util::chars::is_word_char;

const LINE: u8 = 1;
const RELATION: u8 = 2;
const STYLE_PART: u8 = 4;
const FOLLOWS_NAME: u8 = 8;
const BOUNDARY_LINE: u8 = 16;
const CLOSES_LABEL: u8 = 32;
const METADATA_FIRST: u8 = 64;

const ARROWS: &str = "DEPENDENCY_ARROW|GENERALIZATION|FORWARD_SOLID|BACKWARD_SOLID|MARKERLESS_SOLID|FORWARD_CIRCLE|BACKWARD_CIRCLE|FORWARD_CROSS|BACKWARD_CROSS";
const NAMES: &str = "IDENTIFIER|PLAIN_STRING|MARKDOWN_STRING";
const LABELS: &str = "PLAIN_STRING|MARKDOWN_STRING|LABEL_TEXT";
const STATEMENTS: &str = "ACC_TITLE_LINE|ACC_DESCR_LINE|ACC_DESCR_BLOCK|DIRECTION|ACTOR|SYSTEM_BOUNDARY|NOTE|JSON_DECLARATION_START|CLASS_DEF|CLASS|STYLE";
const STYLE_PARTS: &str = "WORD|PLAIN_STRING|NUMBER|HASH_COLOR|CSS_IDENTIFIER|CSS_ESCAPED_COMMA|DASH|DOT|PERCENT|CSS_PUNCTUATION|COLON|LPAREN|RPAREN|LBRACKET|RBRACKET|LBRACE|RBRACE|AT|MARKERLESS_SOLID";

fn is_arrow(ty: u8) -> bool {
    matches!(
        ty,
        t::DEPENDENCY_ARROW
            | t::GENERALIZATION
            | t::FORWARD_SOLID
            | t::BACKWARD_SOLID
            | t::MARKERLESS_SOLID
            | t::FORWARD_CIRCLE
            | t::BACKWARD_CIRCLE
            | t::FORWARD_CROSS
            | t::BACKWARD_CROSS
    )
}

fn is_name(ty: u8) -> bool {
    matches!(ty, t::IDENTIFIER | t::PLAIN_STRING | t::MARKDOWN_STRING)
}

fn is_word(ty: u8) -> bool {
    (t::USECASE..=t::IDENTIFIER).contains(&ty)
}

fn flags(ty: u8) -> u8 {
    let mut f = 0;
    let statement = matches!(
        ty,
        t::ACC_TITLE_LINE
            | t::ACC_DESCR_LINE
            | t::ACC_DESCR_BLOCK
            | t::DIRECTION
            | t::ACTOR
            | t::SYSTEM_BOUNDARY
            | t::NOTE
            | t::JSON_DECLARATION_START
            | t::CLASS_DEF
            | t::CLASS
            | t::STYLE
    );
    if ty == t::NEWLINE || ty == t::COMMENT || statement || is_name(ty) {
        f |= LINE;
    }
    if ty == t::IDENTIFIER || is_arrow(ty) {
        f |= RELATION;
    }
    if is_word(ty)
        || matches!(
            ty,
            t::PLAIN_STRING
                | t::NUMBER
                | t::HASH_COLOR
                | t::CSS_IDENTIFIER
                | t::CSS_ESCAPED_COMMA
                | t::DASH
                | t::DOT
                | t::PERCENT
                | t::CSS_PUNCTUATION
                | t::COLON
                | t::LPAREN
                | t::RPAREN
                | t::LBRACKET
                | t::RBRACKET
                | t::LBRACE
                | t::RBRACE
                | t::AT
                | t::MARKERLESS_SOLID
        )
    {
        f |= STYLE_PART;
    }
    if matches!(
        ty,
        t::STEREOTYPE_START | t::CLASS_SEPARATOR | t::IDENTIFIER | t::NEWLINE | t::EOF
    ) || is_arrow(ty)
    {
        f |= FOLLOWS_NAME;
    }
    if matches!(ty, t::NEWLINE | t::COMMENT | t::ACTOR) || is_name(ty) {
        f |= BOUNDARY_LINE;
    }
    if matches!(
        ty,
        t::FORWARD_SOLID | t::MARKERLESS_SOLID | t::FORWARD_CIRCLE | t::FORWARD_CROSS
    ) {
        f |= CLOSES_LABEL;
    }
    if matches!(ty, t::NEWLINE | t::IDENTIFIER | t::PLAIN_STRING | t::RBRACE) {
        f |= METADATA_FIRST;
    }
    f
}

const FORBIDDEN: [&str; 5] = [
    "allowmixing",
    "newpage",
    "package",
    "rectangle",
    "skinparam",
];

struct Item {
    id: String,
    label: DraftLabel,
    span: Span,
    generated: bool,
    shape: Option<&'static str>,
    metadata: Option<Vec<DraftProperty>>,
    stereotype: Option<String>,
    classes: Vec<String>,
    /// この言及が要素について何かを言い、それを宣言するか。
    explicit: bool,
}

struct Tail {
    explicit_id: Option<String>,
    explicit_id_span: Option<Span>,
    type_: &'static str,
    arrow_type: u8,
    label: Option<DraftLabel>,
    minlen: f64,
    target: Item,
    span: Span,
}

struct Parser<'a> {
    src: &'a [u16],
    types: Vec<u8>,
    starts: Vec<usize>,
    ends: Vec<usize>,
    p: usize,
    drafts: Drafts,
    parent: Option<usize>,
    pending_json: Vec<(usize, usize)>,
}

/// `label.replace(/\W/g, '_')`（UTF-16 の符号単位ごとに）。
fn generated_id(label: &str) -> String {
    let mut out = String::new();
    for c in label.chars() {
        if is_word_char(c) {
            out.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                out.push('_');
            }
        }
    }
    out
}

impl Parser<'_> {
    fn ty(&self, k: usize) -> u8 {
        self.types.get(k).copied().unwrap_or(t::EOF)
    }

    fn image(&self, k: usize) -> String {
        js::from16(&self.src[self.starts[k]..self.ends[k]])
    }

    fn span(&self, k: usize) -> Span {
        (self.starts[k], self.ends[k])
    }

    fn fail(&self, detail: &str) -> PeleError {
        let p = self.p;
        let (line, column) = line_column(self.src, self.starts[p]);
        let mut e = PeleError::typed(
            format!(
                "Error parsing usecase diagram: {detail} at {}",
                location_text(self.src, self.starts[p], self.ends[p])
            ),
            ErrorCode::Syntax,
            "usecase",
        );
        e.line = line;
        e.column = column;
        e
    }

    fn mismatch(&self, name: &str) -> PeleError {
        self.fail(&format!(
            "Expecting token of type --> {name} <-- but found --> '{}' <--",
            self.image(self.p)
        ))
    }

    fn expect(&mut self, ty: u8) -> Result<usize> {
        if self.ty(self.p) != ty {
            return Err(self.mismatch(TOKEN_NAMES[ty as usize]));
        }
        self.p += 1;
        Ok(self.p - 1)
    }

    /// Chevrotain は受け付けたはずの字句の並びをすべて挙げる。
    fn no_alternative(&self, expected: &str) -> PeleError {
        let list: Vec<String> = expected
            .split('|')
            .enumerate()
            .map(|(i, s)| format!("  {}. [{s}]", i + 1))
            .collect();
        self.fail(&format!(
            "Expecting: one of these possible Token sequences:\n{}\nbut found: '{}'",
            list.join("\n"),
            self.image(self.p)
        ))
    }

    fn line_end(&mut self) -> Result<()> {
        let ty = self.ty(self.p);
        if ty == t::NEWLINE {
            self.p += 1;
        } else if ty != t::EOF {
            return Err(self.no_alternative("NEWLINE|EOF"));
        }
        Ok(())
    }

    /// 名前か文字列の字句の中身（引用を除く）。
    fn content(&self, k: usize) -> Span {
        let trim = match self.types[k] {
            t::MARKDOWN_STRING => 2,
            t::PLAIN_STRING => 1,
            _ => 0,
        };
        (self.starts[k] + trim, self.ends[k] - trim)
    }

    fn slice(&self, span: Span) -> String {
        js::from16(&self.src[span.0..span.1])
    }

    fn token_label(&self, k: usize) -> DraftLabel {
        DraftLabel {
            text: self.slice(self.content(k)),
            type_: if self.types[k] == t::MARKDOWN_STRING {
                LabelType::Markdown
            } else {
                LabelType::Text
            },
        }
    }

    /// 引用のないラベルは次の区切りまでで、書いたまま入力から取る。
    fn label(&mut self) -> Result<DraftLabel> {
        let ty = self.ty(self.p);
        if ty == t::PLAIN_STRING || ty == t::MARKDOWN_STRING {
            self.p += 1;
            return Ok(self.token_label(self.p - 1));
        }
        if !(t::USECASE..=t::LABEL_SYMBOL).contains(&ty) {
            return Err(self.no_alternative(LABELS));
        }
        let first = self.p;
        while (t::USECASE..=t::LABEL_SYMBOL).contains(&self.ty(self.p)) {
            self.p += 1;
        }
        Ok(DraftLabel {
            text: self.slice((self.starts[first], self.ends[self.p - 1])),
            type_: LabelType::Text,
        })
    }

    fn is_key(ty: u8) -> bool {
        ty == t::IDENTIFIER || ty == t::PLAIN_STRING
    }

    fn property(&mut self) -> Result<DraftProperty> {
        let key = self.p;
        if !Self::is_key(self.ty(key)) {
            return Err(self.no_alternative("IDENTIFIER|PLAIN_STRING"));
        }
        self.p += 1;
        self.expect(t::COLON)?;
        let value = self.p;
        let ty = self.ty(value);
        if !Self::is_key(ty) && ty != t::TRUE && ty != t::FALSE {
            return Err(self.no_alternative("IDENTIFIER|PLAIN_STRING|TRUE|FALSE"));
        }
        self.p += 1;
        let key_span = self.content(key);
        Ok(DraftProperty {
            key: self.slice(key_span),
            value: match ty {
                t::TRUE => PropValue::Bool(true),
                t::FALSE => PropValue::Bool(false),
                _ => PropValue::Str(self.slice(self.content(value))),
            },
            key_span,
        })
    }

    /// ほかの項目が続くか。閉じ括弧の前のコンマや改行は数えないが、Chevrotain は 3 つ先までしか見ないので、
    /// コンマと 2 つの改行、3 つの改行は区切りとみなされ、その後の括弧は誤りになる。
    fn separator_ahead(&self) -> bool {
        let a = self.ty(self.p);
        let b = self.ty(self.p + 1);
        let c = self.ty(self.p + 2);
        if a == t::COMMA {
            return Self::is_key(b) || (b == t::NEWLINE && (c == t::NEWLINE || Self::is_key(c)));
        }
        if a != t::NEWLINE {
            return false;
        }
        b == t::COMMA
            || Self::is_key(b)
            || (b == t::NEWLINE && (c == t::NEWLINE || c == t::COMMA || Self::is_key(c)))
    }

    fn metadata(&mut self) -> Result<Vec<DraftProperty>> {
        self.expect(t::METADATA_START)?;
        let mut properties = Vec::new();
        while self.ty(self.p) == t::NEWLINE {
            self.p += 1;
        }
        if Self::is_key(self.ty(self.p)) {
            properties.push(self.property()?);
            while self.separator_ahead() {
                if self.ty(self.p) == t::COMMA {
                    self.p += 1;
                } else {
                    while self.ty(self.p) == t::NEWLINE {
                        self.p += 1;
                    }
                    if self.ty(self.p) == t::COMMA {
                        self.p += 1;
                    }
                }
                while self.ty(self.p) == t::NEWLINE {
                    self.p += 1;
                }
                properties.push(self.property()?);
            }
            if self.ty(self.p) == t::COMMA {
                self.p += 1;
            }
            while self.ty(self.p) == t::NEWLINE {
                self.p += 1;
            }
        }
        self.expect(t::RBRACE)?;
        Ok(properties)
    }

    fn class_suffix(&mut self, classes: &mut Vec<String>) -> Result<()> {
        loop {
            self.p += 1;
            let id = self.expect(t::IDENTIFIER)?;
            classes.push(self.image(id));
            if self.ty(self.p) != t::COMMA {
                return Ok(());
            }
        }
    }

    /// 括弧のラベルを持てる id か、両方を表す文字列。
    fn name(&mut self, brackets: bool) -> Result<Item> {
        let k = self.p;
        let ty = self.ty(k);
        let mut item = Item {
            id: String::new(),
            label: DraftLabel {
                text: String::new(),
                type_: LabelType::Text,
            },
            span: self.content(k.min(self.types.len() - 1)),
            generated: ty != t::IDENTIFIER,
            shape: None,
            metadata: None,
            stereotype: None,
            classes: Vec::new(),
            explicit: false,
        };
        if ty == t::IDENTIFIER {
            self.p += 1;
            item.id = self.image(k);
            let open = self.ty(self.p);
            if open == t::LPAREN || (brackets && open == t::LBRACKET) {
                self.p += 1;
                item.label = self.label()?;
                self.expect(if open == t::LPAREN {
                    t::RPAREN
                } else {
                    t::RBRACKET
                })?;
                item.shape = Some(if open == t::LPAREN { "ellipse" } else { "rect" });
            } else {
                item.label = self.token_label(k);
            }
        } else if ty == t::PLAIN_STRING || ty == t::MARKDOWN_STRING {
            self.p += 1;
            item.label = self.token_label(k);
            item.id = generated_id(&item.label.text);
        } else {
            return Err(self.no_alternative(NAMES));
        }
        Ok(item)
    }

    fn element(&mut self, actor: bool) -> Result<Item> {
        let mut item = self.name(!actor)?;
        if actor {
            item.shape = None;
        }
        if self.ty(self.p) == t::METADATA_START {
            item.metadata = Some(self.metadata()?);
        }
        if self.ty(self.p) == t::STEREOTYPE_START {
            self.p += 1;
            let text = self.expect(t::STEREOTYPE_TEXT)?;
            self.expect(t::STEREOTYPE_END)?;
            item.stereotype = Some(js::trim(&self.image(text)).to_string());
        }
        if self.ty(self.p) == t::CLASS_SEPARATOR {
            let mut classes = std::mem::take(&mut item.classes);
            self.class_suffix(&mut classes)?;
            item.classes = classes;
        }
        item.explicit = item.shape.is_some()
            || item.metadata.is_some()
            || item.stereotype.as_deref().is_some_and(|s| !s.is_empty());
        Ok(item)
    }

    fn draft(&self, item: &Item, kind: &'static str) -> DraftElement {
        DraftElement {
            id: item.id.clone(),
            kind,
            label: item.label.clone(),
            span: item.span,
            generated: item.generated,
            parent: self.parent,
            shape: item.shape,
            metadata: item.metadata.clone(),
            stereotype: item.stereotype.clone(),
            classes: item.classes.clone(),
        }
    }

    fn endpoint(item: &Item, declares: bool) -> DraftEndpoint {
        DraftEndpoint {
            id: item.id.clone(),
            label: item.label.clone(),
            span: item.span,
            generated: item.generated,
            declares,
            classed: !item.classes.is_empty(),
        }
    }

    /// ラベルと、それを閉じられる演算子のどれかが続くか。
    fn labeled_right(&self, closers: u8) -> bool {
        let ty = self.ty(self.p);
        if !(t::USECASE..=t::MARKDOWN_STRING).contains(&ty) {
            return false;
        }
        let mut k = self.p + 1;
        loop {
            let ty = self.ty(k);
            if ty == t::MARKERLESS_SOLID || flags(ty) & closers != 0 {
                return true;
            }
            if !(t::USECASE..=t::MARKDOWN_STRING).contains(&ty) {
                return false;
            }
            k += 1;
        }
    }

    /// 2 本を超えるダッシュは、両端の間のランクを 1 つずつ増やす。
    fn minimum_length(&self, k: usize) -> f64 {
        let dashes = (self.ends[k] - self.starts[k]) as f64
            - if self.types[k] == t::MARKERLESS_SOLID {
                0.0
            } else {
                1.0
            };
        js::max(1.0, dashes - 1.0)
    }

    fn relation_tail(&mut self) -> Result<Tail> {
        let first = self.p;
        let mut explicit_id = None;
        let mut explicit_id_span = None;
        let mut type_ = "association";
        let mut arrow_type = arrow::SOLID_ARROW;
        let mut minlen = 1.0;
        let mut label = None;
        if self.ty(self.p) == t::IDENTIFIER && self.ty(self.p + 1) == t::AT {
            explicit_id = Some(self.image(self.p));
            explicit_id_span = Some(self.span(self.p));
            self.p += 2;
        }
        let k = self.p;
        let ty = self.ty(self.p);
        self.p += 1;
        if ty == t::DEPENDENCY_ARROW {
            self.expect(t::COLON)?;
            let kw = self.ty(self.p);
            if kw != t::INCLUDE && kw != t::EXTEND {
                return Err(self.no_alternative("INCLUDE|EXTEND"));
            }
            type_ = if kw == t::INCLUDE {
                "include"
            } else {
                "extend"
            };
            label = Some(DraftLabel {
                text: type_.to_string(),
                type_: LabelType::Text,
            });
            self.p += 1;
        } else if ty == t::GENERALIZATION {
            type_ = "generalization";
        } else if ty == t::FORWARD_SOLID {
            minlen = self.minimum_length(k);
        } else if ty == t::BACKWARD_SOLID {
            arrow_type = arrow::BACK_ARROW;
            // 最も短い演算子だけがラベルを開ける: `A <-- label -- B`。
            if self.ends[k] - self.starts[k] == 3 && self.labeled_right(0) {
                label = Some(self.label()?);
                let close = self.expect(t::MARKERLESS_SOLID)?;
                minlen = self.minimum_length(close);
            } else {
                minlen = self.minimum_length(k);
            }
        } else if ty == t::MARKERLESS_SOLID {
            arrow_type = arrow::LINE_SOLID;
            if self.ends[k] - self.starts[k] == 2 && self.labeled_right(CLOSES_LABEL) {
                label = Some(self.label()?);
                let close = self.ty(self.p);
                if close == t::FORWARD_SOLID {
                    arrow_type = arrow::SOLID_ARROW;
                } else if close == t::FORWARD_CIRCLE {
                    arrow_type = arrow::CIRCLE_ARROW;
                } else if close == t::FORWARD_CROSS {
                    arrow_type = arrow::CROSS_ARROW;
                } else if close != t::MARKERLESS_SOLID {
                    return Err(self.no_alternative(
                        "FORWARD_SOLID|MARKERLESS_SOLID|FORWARD_CIRCLE|FORWARD_CROSS",
                    ));
                }
                if arrow_type == arrow::SOLID_ARROW || arrow_type == arrow::LINE_SOLID {
                    minlen = self.minimum_length(self.p);
                }
                self.p += 1;
            } else {
                minlen = self.minimum_length(k);
            }
        } else if ty == t::FORWARD_CIRCLE {
            arrow_type = arrow::CIRCLE_ARROW;
        } else if ty == t::FORWARD_CROSS {
            arrow_type = arrow::CROSS_ARROW;
        } else if ty == t::BACKWARD_CIRCLE || ty == t::BACKWARD_CROSS {
            arrow_type = if ty == t::BACKWARD_CIRCLE {
                arrow::CIRCLE_ARROW_REVERSED
            } else {
                arrow::CROSS_ARROW_REVERSED
            };
            if self.labeled_right(0) {
                label = Some(self.label()?);
                self.expect(t::MARKERLESS_SOLID)?;
            }
        } else {
            self.p -= 1;
            return Err(self.no_alternative(ARROWS));
        }
        let target = self.element(false)?;
        Ok(Tail {
            explicit_id,
            explicit_id_span,
            type_,
            arrow_type,
            label,
            minlen,
            target,
            span: (self.starts[first], self.ends[self.p - 1]),
        })
    }

    fn relationship(&mut self, source: &Item, declaration: bool, tail: Tail) {
        let target = &tail.target;
        if target.explicit {
            let d = self.draft(target, "usecase");
            self.drafts.elements.push(d);
        }
        self.drafts.relationships.push(DraftRelationship {
            source: Self::endpoint(source, declaration),
            target: Self::endpoint(target, target.explicit),
            span: tail.span,
            id: tail.explicit_id,
            id_span: tail.explicit_id_span,
            type_: tail.type_,
            arrow_type: tail.arrow_type,
            label: tail.label,
            minlen: tail.minlen,
        });
    }

    /// `actor A, B` は複数の actor を宣言する。上の段の `actor A --> B` は 1 つを宣言して関係づける。
    fn actors(&mut self, top: bool) -> Result<()> {
        self.p += 1;
        let mut items = vec![self.element(true)?];
        let mut tail = None;
        if self.ty(self.p) == t::COMMA {
            while self.ty(self.p) == t::COMMA {
                self.p += 1;
                items.push(self.element(true)?);
            }
        } else if top && flags(self.ty(self.p)) & RELATION != 0 {
            tail = Some(self.relation_tail()?);
        }
        self.line_end()?;
        for item in &items {
            let d = self.draft(item, "actor");
            self.drafts.elements.push(d);
        }
        if let Some(tail) = tail {
            self.relationship(&items[0], true, tail);
        }
        Ok(())
    }

    fn entity(&mut self, top: bool) -> Result<()> {
        let mut source = self.element(false)?;
        let tail = if top && flags(self.ty(self.p)) & RELATION != 0 {
            Some(self.relation_tail()?)
        } else {
            None
        };
        self.line_end()?;
        if tail.is_none() {
            source.explicit = true;
            if source.shape.is_none() {
                source.shape = Some("ellipse");
            }
        }
        if source.explicit {
            let d = self.draft(&source, "usecase");
            self.drafts.elements.push(d);
        }
        if let Some(tail) = tail {
            let declares = source.explicit;
            self.relationship(&source, declares, tail);
        }
        Ok(())
    }

    fn boundary(&mut self) -> Result<()> {
        self.p += 1;
        let mut named = self.name(true)?;
        let inline = if self.ty(self.p) == t::METADATA_START {
            Some(self.metadata()?)
        } else {
            None
        };
        if self.ty(self.p) == t::CLASS_SEPARATOR {
            self.class_suffix(&mut named.classes)?;
        }
        self.line_end()?;
        let group = DraftBoundary {
            id: named.id.clone(),
            label: named.label.clone(),
            span: named.span,
            generated: named.generated,
            classes: named.classes.clone(),
        };
        self.drafts.boundaries.push(group);
        self.parent = Some(self.drafts.boundaries.len() - 1);
        while flags(self.ty(self.p)) & BOUNDARY_LINE != 0 {
            self.line(false)?;
        }
        self.parent = None;
        self.expect(t::END)?;
        self.line_end()?;
        if let Some(properties) = inline {
            self.drafts.metadata.push(DraftMetadata {
                target: named.id,
                span: named.span,
                properties,
            });
        }
        Ok(())
    }

    fn style(&mut self) -> Result<String> {
        let first = self.p;
        let ty = self.ty(self.p);
        if is_word(ty) || ty == t::CSS_IDENTIFIER {
            self.p += 1;
        } else if ty == t::MARKERLESS_SOLID {
            self.p += 1;
            if !is_word(self.ty(self.p)) {
                return Err(self.mismatch("WORD"));
            }
            self.p += 1;
        } else {
            return Err(self.no_alternative("WORD|CSS_IDENTIFIER|MARKERLESS_SOLID"));
        }
        self.expect(t::COLON)?;
        if flags(self.ty(self.p)) & STYLE_PART == 0 {
            return Err(self.fail(&format!(
                "Expecting: expecting at least one iteration which starts with one of these possible Token sequences::\n  <[{}]>\nbut found: '{}'",
                STYLE_PARTS.replace('|', "] ,["),
                self.image(self.p)
            )));
        }
        while flags(self.ty(self.p)) & STYLE_PART != 0 {
            self.p += 1;
        }
        // 値は入力から取るので、`1px solid red` の中の空白が残る。
        Ok(self
            .slice((self.starts[first], self.ends[self.p - 1]))
            .replace("\\,", ","))
    }

    fn styles(&mut self) -> Result<Vec<String>> {
        let mut out = vec![self.style()?];
        while self.ty(self.p) == t::COMMA {
            self.p += 1;
            out.push(self.style()?);
        }
        Ok(out)
    }

    fn identifiers(&mut self) -> Result<Vec<usize>> {
        let mut out = vec![self.expect(t::IDENTIFIER)?];
        while self.ty(self.p) == t::COMMA {
            self.p += 1;
            out.push(self.expect(t::IDENTIFIER)?);
        }
        Ok(out)
    }

    fn statement_sequences() -> String {
        let names: Vec<&str> = NAMES.split('|').collect();
        let metadata: Vec<String> = names
            .iter()
            .flat_map(|name| {
                ["NEWLINE", "IDENTIFIER", "PLAIN_STRING", "RBRACE"]
                    .iter()
                    .map(move |next| format!("{name}, METADATA_START, {next}"))
            })
            .collect();
        let follows_text =
            format!("STEREOTYPE_START|CLASS_SEPARATOR|IDENTIFIER|{ARROWS}|NEWLINE|EOF");
        let follows: Vec<&str> = follows_text.split('|').collect();
        let mut entities: Vec<String> = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let nexts: Vec<&str> = if i == 0 {
                ["LPAREN", "LBRACKET"]
                    .into_iter()
                    .chain(follows.iter().copied())
                    .collect()
            } else {
                follows.clone()
            };
            for next in nexts {
                entities.push(format!("{name}, {next}"));
            }
        }
        let mut all: Vec<String> = vec![STATEMENTS.to_string()];
        all.extend(metadata.iter().cloned());
        all.extend(entities);
        all.extend(metadata);
        all.join("|")
    }

    fn top_statement(&mut self) -> Result<()> {
        let k = self.p;
        let ty = self.ty(k);
        if ty == t::ACC_TITLE_LINE || ty == t::ACC_DESCR_LINE || ty == t::ACC_DESCR_BLOCK {
            self.p += 1;
            let text = self.image(k);
            if ty == t::ACC_DESCR_BLOCK {
                let open = text.find('{').map_or(0, |i| i + 1);
                let close = text.rfind('}').unwrap_or(0);
                let inner = if close >= open {
                    &text[open..close]
                } else {
                    ""
                };
                self.drafts.acc_description = js::trim(inner).to_string();
            } else {
                let colon = text.find(':').map_or(0, |i| i + 1);
                let value = js::trim(&text[colon..]).to_string();
                if ty == t::ACC_DESCR_LINE {
                    self.drafts.acc_description = value;
                } else {
                    self.drafts.acc_title = value;
                }
            }
        } else if ty == t::DIRECTION {
            self.p += 1;
            let value = self.ty(self.p);
            if !(t::TD..=t::RL).contains(&value) {
                return Err(self.no_alternative("TD|TB|BT|LR|RL"));
            }
            self.drafts.direction = Some(if value == t::TD {
                "TB".to_string()
            } else {
                self.image(self.p)
            });
            self.p += 1;
        } else if ty == t::ACTOR {
            return self.actors(true);
        } else if ty == t::SYSTEM_BOUNDARY {
            return self.boundary();
        } else if ty == t::NOTE {
            self.p += 1;
            self.expect(t::FOR)?;
            let target = self.expect(t::IDENTIFIER)?;
            let text = self.label()?;
            self.drafts.notes.push(DraftNote {
                target: self.image(target),
                span: self.span(target),
                label: text,
            });
        } else if ty == t::JSON_DECLARATION_START {
            self.p += 1;
            let literal = self.expect(t::JSON_OBJECT_LITERAL)?;
            let mut classes = Vec::new();
            if self.ty(self.p) == t::CLASS_SEPARATOR {
                self.class_suffix(&mut classes)?;
            }
            let mut from = self.starts[k] + 4;
            while self.src.get(from).is_some_and(|&c| c == 32 || c == 9) {
                from += 1;
            }
            let mut to = from;
            while self
                .src
                .get(to)
                .is_some_and(|&c| crate::util::chars::is_word(c as u32))
            {
                to += 1;
            }
            self.drafts.jsons.push(DraftJson {
                id: self.slice((from, to)),
                value: super::json::Json::Null,
                property_order: Default::default(),
                span: (from, to),
                classes,
            });
            self.pending_json
                .push((self.drafts.jsons.len() - 1, literal));
        } else if ty == t::CLASS_DEF {
            self.p += 1;
            let ids = self.identifiers()?;
            let ids: Vec<String> = ids.iter().map(|&i| self.image(i)).collect();
            let styles = self.styles()?;
            self.drafts.class_defs.push(DraftClassDef { ids, styles });
        } else if ty == t::CLASS {
            self.p += 1;
            let targets = self.identifiers()?;
            let classes = self.identifiers()?;
            self.drafts.classes.push(DraftClasses {
                targets: targets
                    .iter()
                    .map(|&i| (self.image(i), self.span(i)))
                    .collect(),
                classes: classes.iter().map(|&i| self.image(i)).collect(),
            });
        } else if ty == t::STYLE {
            self.p += 1;
            let target = self.expect(t::IDENTIFIER)?;
            let styles = self.styles()?;
            self.drafts.styles.push(DraftStyle {
                target: self.image(target),
                span: self.span(target),
                styles,
            });
        } else {
            let next = self.ty(self.p + 1);
            if next == t::METADATA_START {
                if flags(self.ty(self.p + 2)) & METADATA_FIRST == 0 {
                    return Err(self.no_alternative(&Self::statement_sequences()));
                }
                self.p += 1;
                let span = self.content(k);
                let id = if ty != t::IDENTIFIER {
                    generated_id(&self.slice(span))
                } else {
                    self.image(k)
                };
                let properties = self.metadata()?;
                self.drafts.metadata.push(DraftMetadata {
                    target: id,
                    span,
                    properties,
                });
            } else {
                let follows = flags(next) & FOLLOWS_NAME != 0
                    || (ty == t::IDENTIFIER && (next == t::LPAREN || next == t::LBRACKET));
                // Mermaid は、試されやすい PlantUML の文を断る。
                if !follows
                    || (ty == t::IDENTIFIER
                        && FORBIDDEN.contains(&self.image(k).to_lowercase().as_str()))
                {
                    return Err(self.no_alternative(&Self::statement_sequences()));
                }
                return self.entity(true);
            }
        }
        self.line_end()
    }

    fn line(&mut self, top: bool) -> Result<()> {
        let ty = self.ty(self.p);
        if ty == t::NEWLINE {
            self.p += 1;
            return Ok(());
        }
        if ty == t::COMMENT {
            self.p += 1;
            return self.line_end();
        }
        if top {
            self.top_statement()
        } else if ty == t::ACTOR {
            self.actors(false)
        } else {
            self.entity(false)
        }
    }
}

pub fn parse_usecase(source: &str, title: Option<&str>) -> Result<UsecaseModel> {
    let src: Vec<u16> = source.encode_utf16().collect();
    let tokens = tokenize(&src)?;
    let mut p = Parser {
        src: &src,
        types: tokens.types,
        starts: tokens.starts,
        ends: tokens.ends,
        p: 0,
        drafts: Drafts::default(),
        parent: None,
        pending_json: Vec::new(),
    };
    p.expect(t::USECASE)?;
    p.line_end()?;
    while flags(p.ty(p.p)) & LINE != 0 {
        p.line(true)?;
    }
    if p.ty(p.p) != t::EOF {
        return Err(p.fail(&format!(
            "Redundant input, expecting EOF but found: {}",
            p.image(p.p)
        )));
    }
    // JSON を見る前に、文全体が整っていなければならない（Mermaid と同じ）。
    for (json, literal) in std::mem::take(&mut p.pending_json) {
        let text = p.image(literal);
        let at = p.starts[literal];
        let parsed = parse_ordered_json(&text, || line_column(&src, at))?;
        let d = &mut p.drafts.jsons[json];
        d.value = parsed.value;
        d.property_order = parsed.property_order;
    }
    resolve_usecase(&src, p.drafts, title)
}
