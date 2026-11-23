//! 読んだものを形にまとめる（pele の `src/diagrams/usecase/db.ts`）。Mermaid の UsecaseModelBuilder を、その
//! 知らせの文ごと移したもの。

use std::collections::{HashMap, HashSet};

use super::json::Json;
use super::lexer::{line_column, location_text};
use super::types::{
    Actor, ClassDef, LabelType, OMap, Relationship, Span, SystemBoundary, UseCase, UsecaseJsonNode,
    UsecaseModel, UsecaseNote,
};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::util::chars::is_space_char;

#[derive(Clone, Debug)]
pub struct DraftLabel {
    pub text: String,
    pub type_: LabelType,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    Str(String),
    Bool(bool),
}

/// `key_span` は、その項目についての知らせの指す所でもある。
#[derive(Clone, Debug)]
pub struct DraftProperty {
    pub key: String,
    pub value: PropValue,
    pub key_span: Span,
}

#[derive(Clone, Debug)]
pub struct DraftElement {
    pub id: String,
    pub label: DraftLabel,
    pub span: Span,
    pub generated: bool,
    /// actor か usecase。
    pub kind: &'static str,
    /// 囲み（`Drafts::boundaries` の位置）。
    pub parent: Option<usize>,
    pub shape: Option<&'static str>,
    pub metadata: Option<Vec<DraftProperty>>,
    pub stereotype: Option<String>,
    pub classes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DraftBoundary {
    pub id: String,
    pub label: DraftLabel,
    pub span: Span,
    pub generated: bool,
    pub classes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DraftJson {
    pub id: String,
    pub value: Json,
    pub property_order: HashMap<String, Vec<String>>,
    pub span: Span,
    pub classes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DraftEndpoint {
    pub id: String,
    pub label: DraftLabel,
    pub span: Span,
    pub generated: bool,
    /// この言及が要素を宣言するか、`:::` を持つか。
    pub declares: bool,
    pub classed: bool,
}

#[derive(Clone, Debug)]
pub struct DraftRelationship {
    pub source: DraftEndpoint,
    pub target: DraftEndpoint,
    pub span: Span,
    pub id: Option<String>,
    pub id_span: Option<Span>,
    pub type_: &'static str,
    pub arrow_type: u8,
    pub label: Option<DraftLabel>,
    pub minlen: f64,
}

pub struct DraftNote {
    pub target: String,
    pub span: Span,
    pub label: DraftLabel,
}

pub struct DraftMetadata {
    pub target: String,
    pub span: Span,
    pub properties: Vec<DraftProperty>,
}

pub struct DraftClasses {
    pub targets: Vec<(String, Span)>,
    pub classes: Vec<String>,
}

pub struct DraftStyle {
    pub target: String,
    pub span: Span,
    pub styles: Vec<String>,
}

pub struct DraftClassDef {
    pub ids: Vec<String>,
    pub styles: Vec<String>,
}

/// 読み手が読んだすべて（入力の順、まとめる前）。
#[derive(Default)]
pub struct Drafts {
    pub elements: Vec<DraftElement>,
    pub boundaries: Vec<DraftBoundary>,
    pub jsons: Vec<DraftJson>,
    pub relationships: Vec<DraftRelationship>,
    pub notes: Vec<DraftNote>,
    pub metadata: Vec<DraftMetadata>,
    pub classes: Vec<DraftClasses>,
    pub styles: Vec<DraftStyle>,
    pub class_defs: Vec<DraftClassDef>,
    pub direction: Option<String>,
    pub acc_title: String,
    pub acc_description: String,
}

#[derive(Clone)]
struct Origin {
    kind: &'static str,
    span: Span,
    generated: bool,
    /// 作った id のもとのラベル。
    label: Option<String>,
}

#[derive(Clone)]
struct ElementState {
    kind: &'static str,
    id: String,
    label: DraftLabel,
    span: Span,
    parent: Option<usize>,
    shape: Option<&'static str>,
    stereotype: Option<String>,
    stereotype_span: Option<Span>,
    classes: Vec<String>,
    actor_type: Option<&'static str>,
    icon: Option<String>,
    business: Option<bool>,
}

struct BoundaryState {
    id: String,
    label: DraftLabel,
    span: Span,
    classes: Vec<String>,
    type_: Option<&'static str>,
    members: Vec<String>,
}

fn push_unique(target: &mut Vec<String>, values: &[String]) {
    if values.is_empty() {
        return;
    }
    let mut seen: HashSet<String> = target.iter().cloned().collect();
    for value in values {
        if seen.contains(value) {
            continue;
        }
        seen.insert(value.clone());
        target.push(value.clone());
    }
}

/// メタデータの鍵がどの種類の要素にしか属しえないか（知らせの文のため）。
fn infer_kind(properties: &[DraftProperty]) -> Option<&'static str> {
    let mut possible: Vec<&'static str> = vec!["actor", "usecase", "boundary", "edge"];
    for p in properties {
        let value = &p.value;
        let is = |s: &str| *value == PropValue::Str(s.to_string());
        if p.key == "icon" {
            possible = vec!["actor"];
        } else if p.key == "animate" || p.key == "animation" {
            possible = vec!["edge"];
        } else if p.key == "type" {
            possible.clear();
            if is("rect") || is("package") {
                possible.push("boundary");
            } else if is("normal") || is("hollow") || is("awesome") {
                possible.push("actor");
            }
        } else if p.key == "business" {
            possible.retain(|k| *k != "boundary" && *k != "edge");
        } else {
            return None;
        }
    }
    if possible.len() == 1 {
        Some(possible[0])
    } else {
        None
    }
}

/// `text.replace(/\n\s+/g, '\n')`
fn collapse_indents(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.peek().is_some_and(|c| is_space_char(*c)) {
                chars.next();
            }
        }
    }
    out
}

struct Resolver<'a> {
    src: &'a [u16],
    drafts: &'a Drafts,
    model: UsecaseModel,
    symbols: HashMap<String, Origin>,
    elements: OMap<ElementState>,
    boundaries: OMap<BoundaryState>,
    edges: HashMap<String, usize>,
    first: HashMap<String, usize>,
}

impl Resolver<'_> {
    fn at(&self, span: Span) -> String {
        location_text(self.src, span.0, span.1)
    }

    fn error(&self, message: String, span: Span) -> PeleError {
        let (line, column) = line_column(self.src, span.0);
        let mut e = PeleError::typed(message, ErrorCode::Semantic, "usecase");
        e.line = line;
        e.column = column;
        e
    }

    fn fail(&self, message: &str, span: Span) -> PeleError {
        self.error(format!("{message} at {}", self.at(span)), span)
    }

    fn conflict(
        &self,
        message: &str,
        current: Span,
        previous: Span,
        current_label: Option<&str>,
        previous_label: Option<&str>,
    ) -> PeleError {
        let suffix = |label: Option<&str>| match label {
            None => String::new(),
            Some(l) => format!(" (label \"{l}\")"),
        };
        self.error(
            format!(
                "{message} at {}{}; previous declaration at {}{}",
                self.at(current),
                suffix(current_label),
                self.at(previous),
                suffix(previous_label)
            ),
            current,
        )
    }

    fn invalid(&self, id: &str, kind: &str, property: &DraftProperty) -> PeleError {
        self.fail(
            &format!(
                "Metadata property '{}' is invalid for {kind} '{id}'",
                property.key
            ),
            property.key_span,
        )
    }

    fn mention(&mut self, id: &str, offset: usize) {
        match self.first.get(id) {
            Some(&previous) if offset >= previous => {}
            _ => {
                self.first.insert(id.to_string(), offset);
            }
        }
    }

    /// 最初の言及の順に（安定な並べ替え）。
    fn in_order(&self, ids: Vec<String>) -> Vec<String> {
        let mut ids = ids;
        ids.sort_by_key(|id| self.first.get(id).copied().unwrap_or(0));
        ids
    }

    fn element_ids(&self) -> Vec<String> {
        self.elements.values().map(|s| s.id.clone()).collect()
    }

    fn register(
        &mut self,
        id: &str,
        label: &DraftLabel,
        span: Span,
        generated: bool,
        kind: &'static str,
    ) {
        self.symbols.insert(
            id.to_string(),
            Origin {
                kind,
                span,
                generated,
                label: Some(label.text.clone()),
            },
        );
    }

    /// id は図の中で 1 つのものを指し、ラベルから作った id は 1 度しか宣言できない。
    fn claim(
        &self,
        id: &str,
        label: &DraftLabel,
        span: Span,
        generated: bool,
        kind: &str,
    ) -> Result<()> {
        let Some(origin) = self.symbols.get(id) else {
            return Ok(());
        };
        let label = generated.then_some(label.text.as_str());
        let origin_label = if origin.generated {
            origin.label.as_deref()
        } else {
            None
        };
        if origin.kind != kind {
            return Err(self.conflict(
                &format!("ID '{id}' is declared as both {} and {kind}", origin.kind),
                span,
                origin.span,
                label,
                origin_label,
            ));
        }
        if origin.generated || generated {
            return Err(self.conflict(
                &format!("Generated ID '{id}' collides with another declaration"),
                span,
                origin.span,
                label,
                origin_label,
            ));
        }
        Ok(())
    }

    fn unique(&mut self, id: &str, kind: &'static str, span: Span) -> Result<()> {
        if let Some(previous) = self.symbols.get(id) {
            return Err(self.conflict(
                &format!(
                    "ID '{id}' is declared more than once ({} and {kind})",
                    previous.kind
                ),
                span,
                previous.span,
                None,
                if previous.generated {
                    previous.label.as_deref()
                } else {
                    None
                },
            ));
        }
        self.symbols.insert(
            id.to_string(),
            Origin {
                kind,
                span,
                generated: false,
                label: None,
            },
        );
        Ok(())
    }

    /// メタデータの文は宣言の言ったことを置き換える。2 つの宣言は合っていなければならない。
    fn actor_property(
        &self,
        state: &mut ElementState,
        property: &DraftProperty,
        replace: bool,
    ) -> Result<()> {
        let key = property.key.as_str();
        let clash = |previous: Option<PropValue>| -> Result<()> {
            if !replace && previous.is_some_and(|p| p != property.value) {
                return Err(self.conflict(
                    &format!("Actor '{}' has conflicting {key} metadata", state.id),
                    property.key_span,
                    state.span,
                    None,
                    None,
                ));
            }
            Ok(())
        };
        match (&property.value, key) {
            (PropValue::Str(v), "type")
                if matches!(v.as_str(), "normal" | "hollow" | "awesome") =>
            {
                clash(state.actor_type.map(|t| PropValue::Str(t.to_string())))?;
                state.actor_type = Some(match v.as_str() {
                    "normal" => "normal",
                    "hollow" => "hollow",
                    _ => "awesome",
                });
            }
            (PropValue::Str(v), "icon") => {
                clash(state.icon.clone().map(PropValue::Str))?;
                state.icon = Some(v.clone());
            }
            (PropValue::Bool(b), "business") => {
                clash(state.business.map(PropValue::Bool))?;
                state.business = Some(*b);
            }
            _ => return Err(self.invalid(&state.id, "actor", property)),
        }
        Ok(())
    }

    fn element_metadata(
        &self,
        state: &mut ElementState,
        properties: Option<&[DraftProperty]>,
        replace: bool,
    ) -> Result<()> {
        for property in properties.unwrap_or(&[]) {
            if state.kind == "actor" {
                self.actor_property(state, property, replace)?;
            } else if let ("business", PropValue::Bool(b)) =
                (property.key.as_str(), &property.value)
            {
                if !replace && state.business.is_some_and(|x| x != *b) {
                    return Err(self.conflict(
                        &format!("Use case '{}' has conflicting business metadata", state.id),
                        property.key_span,
                        state.span,
                        None,
                        None,
                    ));
                }
                state.business = Some(*b);
            } else {
                return Err(self.invalid(&state.id, "usecase", property));
            }
        }
        Ok(())
    }

    fn parent_id(&self, parent: Option<usize>) -> Option<String> {
        parent
            .map(|p| self.drafts.boundaries[p].id.clone())
            .filter(|id| !id.is_empty())
    }

    fn publish(&mut self, id: &str) {
        let state = self.elements.get(id).unwrap().clone();
        let stereotype = state.stereotype.clone().filter(|s| !s.is_empty());
        let parent_id = self.parent_id(state.parent);
        if state.kind == "actor" {
            self.model.use_cases.delete(&state.id);
            let icon = state.icon.clone().filter(|i| !i.is_empty());
            let actor = Actor {
                id: state.id.clone(),
                label: state.label.text.clone(),
                label_type: state.label.type_,
                business: state.business.unwrap_or(false),
                classes: state.classes.clone(),
                type_: if icon.is_some() {
                    "icon"
                } else {
                    state.actor_type.unwrap_or("normal")
                },
                styles: self
                    .model
                    .actors
                    .get(&state.id)
                    .map(|a| a.styles.clone())
                    .unwrap_or_default(),
                icon,
                stereotype,
                parent_id,
            };
            self.model.actors.set(&state.id, actor);
        } else {
            self.model.actors.delete(&state.id);
            let use_case = UseCase {
                id: state.id.clone(),
                label: state.label.text.clone(),
                label_type: state.label.type_,
                business: state.business.unwrap_or(false),
                classes: state.classes.clone(),
                shape: state.shape.unwrap_or("ellipse"),
                styles: self
                    .model
                    .use_cases
                    .get(&state.id)
                    .map(|u| u.styles.clone())
                    .unwrap_or_default(),
                stereotype,
                parent_id,
            };
            self.model.use_cases.set(&state.id, use_case);
        }
    }

    fn declare_element(&mut self, draft: &DraftElement) -> Result<()> {
        self.claim(
            &draft.id,
            &draft.label,
            draft.span,
            draft.generated,
            draft.kind,
        )?;
        let parent = draft
            .parent
            .filter(|&p| !self.drafts.boundaries[p].id.is_empty());
        let Some(existing) = self.elements.get(&draft.id).cloned() else {
            let mut state = ElementState {
                kind: draft.kind,
                id: draft.id.clone(),
                label: draft.label.clone(),
                span: draft.span,
                classes: draft.classes.clone(),
                parent,
                shape: draft.shape,
                stereotype: draft.stereotype.clone(),
                stereotype_span: draft
                    .stereotype
                    .as_ref()
                    .filter(|s| !s.is_empty())
                    .map(|_| draft.span),
                actor_type: None,
                icon: None,
                business: None,
            };
            self.element_metadata(&mut state, draft.metadata.as_deref(), false)?;
            self.elements.set(&draft.id, state);
            self.register(
                &draft.id,
                &draft.label,
                draft.span,
                draft.generated,
                draft.kind,
            );
            return Ok(());
        };
        let mut existing = existing;
        if existing.label.text != draft.label.text || existing.label.type_ != draft.label.type_ {
            return Err(self.conflict(
                &format!("ID '{}' has conflicting labels", draft.id),
                draft.span,
                existing.span,
                None,
                None,
            ));
        }
        if let (Some(a), Some(b)) = (draft.shape, existing.shape)
            && a != b
        {
            return Err(self.conflict(
                &format!("Use case '{}' has conflicting shapes", draft.id),
                draft.span,
                existing.span,
                None,
                None,
            ));
        }
        // 題が空の id になる囲みは親ではないが、親の場所はふさぐ。
        if let (Some(p), Some(e)) = (parent, existing.parent) {
            let (pb, eb) = (&self.drafts.boundaries[p], &self.drafts.boundaries[e]);
            if !eb.id.is_empty() && pb.id != eb.id {
                return Err(self.conflict(
                    &format!(
                        "Element '{}' belongs to more than one system boundary",
                        draft.id
                    ),
                    pb.span,
                    eb.span,
                    None,
                    None,
                ));
            }
        }
        if let (Some(a), Some(b)) = (
            draft.stereotype.as_deref().filter(|s| !s.is_empty()),
            existing.stereotype.as_deref().filter(|s| !s.is_empty()),
        ) && a != b
        {
            return Err(self.conflict(
                &format!("Element '{}' has conflicting stereotypes", draft.id),
                draft.span,
                existing.stereotype_span.unwrap_or(existing.span),
                None,
                None,
            ));
        }
        if existing.shape.is_none() {
            existing.shape = draft.shape;
        }
        if existing.parent.is_none() {
            existing.parent = draft.parent;
        }
        if existing.stereotype.is_none() {
            existing.stereotype = draft.stereotype.clone();
        }
        if existing.stereotype_span.is_none() {
            existing.stereotype_span = draft
                .stereotype
                .as_ref()
                .filter(|s| !s.is_empty())
                .map(|_| draft.span);
        }
        push_unique(&mut existing.classes, &draft.classes);
        self.element_metadata(&mut existing, draft.metadata.as_deref(), false)?;
        self.elements.set(&draft.id, existing);
        Ok(())
    }

    fn declare_boundary(&mut self, draft: &DraftBoundary) -> Result<()> {
        self.claim(
            &draft.id,
            &draft.label,
            draft.span,
            draft.generated,
            "boundary",
        )?;
        if let Some(existing) = self.boundaries.get(&draft.id) {
            if existing.label.text != draft.label.text || existing.label.type_ != draft.label.type_
            {
                return Err(self.conflict(
                    &format!("Boundary '{}' has conflicting titles", draft.id),
                    draft.span,
                    existing.span,
                    None,
                    None,
                ));
            }
            let existing = self.boundaries.get_mut(&draft.id).unwrap();
            push_unique(&mut existing.classes, &draft.classes);
            return Ok(());
        }
        self.boundaries.set(
            &draft.id,
            BoundaryState {
                id: draft.id.clone(),
                label: draft.label.clone(),
                span: draft.span,
                classes: draft.classes.clone(),
                type_: None,
                members: Vec::new(),
            },
        );
        self.register(
            &draft.id,
            &draft.label,
            draft.span,
            draft.generated,
            "boundary",
        );
        Ok(())
    }

    /// 何も宣言しない関係の端は use case になる。
    fn endpoint(&mut self, end: &DraftEndpoint) -> Result<&'static str> {
        if end.classed && !end.declares {
            return Err(self.fail(
                &format!(
                    "Relationship endpoint '{}' uses ::: without declaring the node",
                    end.id
                ),
                end.span,
            ));
        }
        if let Some(origin) = self.symbols.get(&end.id) {
            return Ok(origin.kind);
        }
        self.elements.set(
            &end.id,
            ElementState {
                kind: "usecase",
                id: end.id.clone(),
                label: end.label.clone(),
                span: end.span,
                parent: None,
                shape: Some("ellipse"),
                stereotype: None,
                stereotype_span: None,
                classes: Vec::new(),
                actor_type: None,
                icon: None,
                business: None,
            },
        );
        self.register(&end.id, &end.label, end.span, end.generated, "usecase");
        self.mention(&end.id, end.span.0);
        Ok("usecase")
    }

    /// クラスやスタイルを当てる的の classes と styles。
    fn stylable(&mut self, id: &str, span: Span) -> Result<(&mut Vec<String>, &mut Vec<String>)> {
        let kind = self.symbols.get(id).map(|o| o.kind);
        let found = match kind {
            Some("actor") => self.model.actors.has(id),
            Some("usecase") => self.model.use_cases.has(id),
            Some("boundary") => self.model.system_boundaries.has(id),
            Some("json") => self.model.json_nodes.has(id),
            _ => self.edges.contains_key(id),
        };
        if !found {
            return Err(self.fail(
                &format!("Class/style target '{id}' is unresolved or anonymous"),
                span,
            ));
        }
        Ok(match kind {
            Some("actor") => {
                let a = self.model.actors.get_mut(id).unwrap();
                (&mut a.classes, &mut a.styles)
            }
            Some("usecase") => {
                let u = self.model.use_cases.get_mut(id).unwrap();
                (&mut u.classes, &mut u.styles)
            }
            Some("boundary") => {
                let b = self.model.system_boundaries.get_mut(id).unwrap();
                (&mut b.classes, &mut b.styles)
            }
            Some("json") => {
                let j = self.model.json_nodes.get_mut(id).unwrap();
                (&mut j.classes, &mut j.styles)
            }
            _ => {
                let r = &mut self.model.relationships[self.edges[id]];
                (&mut r.classes, &mut r.styles)
            }
        })
    }
}

enum Decl {
    Element(usize),
    Boundary(usize),
    Json(usize),
    Edge(usize),
}

/// 読み手の読んだものを、図を読み終えてから形にまとめる: どの id が actor・use case・囲み・JSON・辺か、どの順に
/// 現れるか、それらについての文が合っているか。
pub fn resolve_usecase(src: &[u16], drafts: Drafts, title: Option<&str>) -> Result<UsecaseModel> {
    let mut r = Resolver {
        src,
        drafts: &drafts,
        model: UsecaseModel {
            direction: "LR".to_string(),
            ..Default::default()
        },
        symbols: HashMap::new(),
        elements: OMap::default(),
        boundaries: OMap::default(),
        edges: HashMap::new(),
        first: HashMap::new(),
    };

    let mut declarations: Vec<(usize, Decl)> = Vec::new();
    for rel in &drafts.relationships {
        r.mention(&rel.source.id, rel.source.span.0);
        r.mention(&rel.target.id, rel.target.span.0);
    }
    for (i, d) in drafts.elements.iter().enumerate() {
        r.mention(&d.id, d.span.0);
        declarations.push((d.span.0, Decl::Element(i)));
    }
    for (i, d) in drafts.boundaries.iter().enumerate() {
        r.mention(&d.id, d.span.0);
        declarations.push((d.span.0, Decl::Boundary(i)));
    }
    for (i, d) in drafts.jsons.iter().enumerate() {
        r.mention(&d.id, d.span.0);
        declarations.push((d.span.0, Decl::Json(i)));
    }
    for (i, rel) in drafts.relationships.iter().enumerate() {
        if let (Some(_), Some(span)) = (&rel.id, rel.id_span) {
            declarations.push((span.0, Decl::Edge(i)));
        }
    }
    declarations.sort_by_key(|d| d.0);
    for (_, d) in &declarations {
        match *d {
            Decl::Element(i) => r.declare_element(&drafts.elements[i])?,
            Decl::Boundary(i) => r.declare_boundary(&drafts.boundaries[i])?,
            Decl::Json(i) => {
                let j = &drafts.jsons[i];
                r.unique(&j.id, "json", j.span)?
            }
            Decl::Edge(i) => {
                let rel = &drafts.relationships[i];
                r.unique(rel.id.as_ref().unwrap(), "edge", rel.id_span.unwrap())?
            }
        }
    }

    for id in r.in_order(r.element_ids()) {
        r.publish(&id);
    }
    let boundary_ids: Vec<String> = r.boundaries.values().map(|b| b.id.clone()).collect();
    for id in r.in_order(boundary_ids) {
        let state = r.boundaries.get(&id).unwrap();
        let boundary = SystemBoundary {
            id: state.id.clone(),
            label: state.label.text.clone(),
            label_type: state.label.type_,
            type_: "rect",
            members: Vec::new(),
            classes: Vec::new(),
            styles: Vec::new(),
        };
        r.model.system_boundaries.set(&id, boundary);
    }
    let mut jsons: Vec<usize> = (0..drafts.jsons.len()).collect();
    jsons.sort_by_key(|&i| r.first.get(&drafts.jsons[i].id).copied().unwrap_or(0));
    for i in jsons {
        let d = &drafts.jsons[i];
        r.model.json_nodes.set(
            &d.id,
            UsecaseJsonNode {
                id: d.id.clone(),
                value: d.value.clone(),
                property_order: d.property_order.clone(),
                classes: d.classes.clone(),
                styles: Vec::new(),
            },
        );
    }

    let mut anonymous = 0;
    for draft in &drafts.relationships {
        let (source, target, type_) = (&draft.source, &draft.target, draft.type_);
        let source_kind = r.endpoint(source)?;
        let target_kind = r.endpoint(target)?;
        let related = |kind: &str| kind == "actor" || kind == "usecase" || kind == "json";
        if !related(source_kind) {
            return Err(r.conflict(
                &format!(
                    "Relationship source '{}' cannot be {source_kind}",
                    source.id
                ),
                source.span,
                r.symbols[&source.id].span,
                None,
                None,
            ));
        }
        if !related(target_kind) {
            return Err(r.conflict(
                &format!(
                    "Relationship target '{}' cannot be {target_kind}",
                    target.id
                ),
                target.span,
                r.symbols[&target.id].span,
                None,
                None,
            ));
        }
        if (type_ == "include" || type_ == "extend")
            && (source_kind != "usecase" || target_kind != "usecase")
        {
            return Err(r.fail(
                &format!("{type_} relationship requires use-case endpoints"),
                draft.span,
            ));
        }
        if type_ == "generalization" && (source_kind == "json" || source_kind != target_kind) {
            return Err(r.fail(
                "Generalization requires actor-to-actor or use-case-to-use-case endpoints",
                draft.span,
            ));
        }
        if type_ == "association"
            && (source_kind == "json" || target_kind == "json")
            && draft.arrow_type > 2
        {
            return Err(r.fail(
                &format!(
                    "JSON relationship '{}' to '{}' permits only point, reversed-point, or markerless solid association",
                    source.id, target.id
                ),
                draft.span,
            ));
        }
        let id = match &draft.id {
            Some(id) => id.clone(),
            None => {
                let id = format!("edge-{anonymous}");
                anonymous += 1;
                id
            }
        };
        let relationship = Relationship {
            id: id.clone(),
            explicit_id: draft.id.as_deref().is_some_and(|s| !s.is_empty()),
            source: source.id.clone(),
            target: target.id.clone(),
            type_,
            arrow_type: draft.arrow_type,
            label: draft.label.as_ref().map(|l| l.text.clone()),
            label_type: draft.label.as_ref().map(|l| l.type_),
            minlen: draft.minlen,
            classes: Vec::new(),
            styles: Vec::new(),
            animate: false,
            animation: None,
        };
        r.edges.insert(id, r.model.relationships.len());
        r.model.relationships.push(relationship);
    }

    // 関係が use case を足したかもしれない。すべての要素を最初の言及の順に並べ直す。
    let declared_actors = std::mem::take(&mut r.model.actors);
    let declared_use_cases = std::mem::take(&mut r.model.use_cases);
    for id in r.in_order(r.element_ids()) {
        if let Some(a) = declared_actors.get(&id) {
            r.model.actors.set(&id, a.clone());
        } else if let Some(u) = declared_use_cases.get(&id) {
            r.model.use_cases.set(&id, u.clone());
        } else {
            r.publish(&id);
        }
    }

    for m in &drafts.metadata {
        let target = &m.target;
        let Some(origin) = r.symbols.get(target) else {
            let inferred = infer_kind(&m.properties)
                .map(|k| format!(" (metadata implies {k})"))
                .unwrap_or_default();
            return Err(r.fail(
                &format!("Metadata target '{target}' is unresolved{inferred}"),
                m.span,
            ));
        };
        let kind = origin.kind;
        if kind == "actor" || kind == "usecase" {
            let mut state = r.elements.get(target).unwrap().clone();
            r.element_metadata(&mut state, Some(&m.properties), true)?;
            r.elements.set(target, state);
            continue;
        }
        let has_boundary = r.boundaries.has(target);
        let edge = r.edges.get(target).copied();
        for property in &m.properties {
            let key = property.key.as_str();
            match (&property.value, key) {
                (PropValue::Str(v), "type")
                    if kind == "boundary" && has_boundary && (v == "rect" || v == "package") =>
                {
                    r.boundaries.get_mut(target).unwrap().type_ =
                        Some(if v == "rect" { "rect" } else { "package" });
                }
                (PropValue::Bool(b), "animate") if kind == "edge" && edge.is_some() => {
                    r.model.relationships[edge.unwrap()].animate = *b;
                }
                (PropValue::Str(v), "animation")
                    if kind == "edge" && edge.is_some() && (v == "fast" || v == "slow") =>
                {
                    let rel = &mut r.model.relationships[edge.unwrap()];
                    rel.animation = Some(if v == "fast" { "fast" } else { "slow" });
                    rel.animate = true;
                }
                _ => return Err(r.invalid(target, kind, property)),
            }
        }
    }
    for rel in &mut r.model.relationships {
        if rel.animation.is_some() {
            rel.animate = true;
        }
    }

    for id in r.element_ids() {
        let state = r.elements.get(&id).unwrap();
        let has_icon = state.icon.as_deref().is_some_and(|i| !i.is_empty());
        if state.kind == "actor" {
            if let Some(t) = state.actor_type
                && has_icon
                && t != "normal"
            {
                return Err(r.fail(
                    &format!("Actor '{}' cannot combine icon with type '{t}'", state.id),
                    state.span,
                ));
            }
            if state.business == Some(true) && (has_icon || state.actor_type == Some("awesome")) {
                return Err(r.fail(
                    &format!(
                        "Business actor '{}' must use normal or hollow geometry",
                        state.id
                    ),
                    state.span,
                ));
            }
        } else if state.shape == Some("rect") && state.business == Some(true) {
            return Err(r.fail(
                &format!(
                    "Rectangular use case '{}' cannot be a business use case",
                    state.id
                ),
                state.span,
            ));
        }
        r.publish(&id);
    }

    let mut elements: Vec<&DraftElement> = drafts.elements.iter().collect();
    elements.sort_by_key(|d| d.span.0);
    for d in elements {
        let Some(pid) = r.parent_id(d.parent) else {
            continue;
        };
        let members = &mut r.boundaries.get_mut(&pid).unwrap().members;
        if !members.contains(&d.id) {
            members.push(d.id.clone());
        }
    }
    #[allow(clippy::type_complexity)]
    let states: Vec<(String, Option<&'static str>, Vec<String>, Vec<String>)> = r
        .boundaries
        .values()
        .map(|s| (s.id.clone(), s.type_, s.members.clone(), s.classes.clone()))
        .collect();
    for (id, type_, members, classes) in states {
        let boundary = r.model.system_boundaries.get_mut(&id).unwrap();
        boundary.type_ = type_.unwrap_or("rect");
        boundary.members = members;
        boundary.classes = classes;
    }

    for (notes, draft) in drafts.notes.iter().enumerate() {
        let Some(origin) = r.symbols.get(&draft.target) else {
            return Err(r.fail(
                &format!("Note target '{}' is unresolved", draft.target),
                draft.span,
            ));
        };
        if origin.kind != "actor" && origin.kind != "usecase" {
            return Err(r.conflict(
                &format!(
                    "Note target '{}' must be an actor or use case, not {}",
                    draft.target, origin.kind
                ),
                draft.span,
                origin.span,
                None,
                None,
            ));
        }
        let id = format!("note-{notes}");
        r.model.notes.set(
            &id,
            UsecaseNote {
                id: id.clone(),
                target: draft.target.clone(),
                label: draft.label.text.clone(),
                label_type: draft.label.type_,
            },
        );
    }

    for definition in &drafts.class_defs {
        for id in &definition.ids {
            r.model.class_defs.set(
                id,
                ClassDef {
                    id: id.clone(),
                    styles: definition.styles.clone(),
                },
            );
        }
    }
    for assignment in &drafts.classes {
        for (id, span) in &assignment.targets {
            let (classes, _) = r.stylable(id, *span)?;
            push_unique(classes, &assignment.classes);
        }
    }
    for assignment in &drafts.styles {
        let (_, styles) = r.stylable(&assignment.target, assignment.span)?;
        styles.extend(assignment.styles.iter().cloned());
    }

    let mut model = r.model;
    if let Some(d) = &drafts.direction {
        model.direction = d.clone();
    }
    model.title = title.map(str::to_string);
    model.acc_title = drafts.acc_title.clone();
    model.acc_description = drafts.acc_description.clone();
    model.acc_descr = collapse_indents(&drafts.acc_description);
    Ok(model)
}
