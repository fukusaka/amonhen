//! wardley の形（pele の `src/diagrams/wardley/model.ts`）。

use super::builder::{
    Flow, NewNode, SourceStrategy, WardleyAnnotation, WardleyBuildResult, WardleyBuilder,
    WardleyForce, WardleyLink, WardleyNote, WardleyTrend,
};
use super::parser::{LabelAst, WardleyAst};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

pub struct WardleyModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub built: WardleyBuildResult,
}

/// 1 までの値は割合、それより大きいものはすでに百分率。
fn to_percent(value: f64, context: &str) -> Result<f64> {
    let normalized = if value <= 1.0 { value * 100.0 } else { value };
    if normalized < 0.0 || normalized > 100.0 {
        return Err(PeleError::typed(
            format!(
                "{context} must be between 0-1 (decimal) or 0-100 (percentage). Received: {}",
                js::fmt(value)
            ),
            ErrorCode::Semantic,
            "wardley",
        ));
    }
    Ok(normalized)
}

/// (x, y)
fn to_coordinates(visibility: f64, evolution: f64, context: &str) -> Result<(f64, f64)> {
    let x = to_percent(evolution, &format!("{context} evolution"))?;
    let y = to_percent(visibility, &format!("{context} visibility"))?;
    Ok((x, y))
}

fn flow_from_port(port: Option<&str>) -> Option<Flow> {
    match port {
        Some("+<>") => Some(Flow::Bidirectional),
        Some("+<") => Some(Flow::Backward),
        Some("+>") => Some(Flow::Forward),
        _ => None,
    }
}

fn flow_from_arrow(arrow: Option<&str>) -> (Option<Flow>, Option<String>) {
    let Some(arrow) = arrow.filter(|a| a.starts_with('+')) else {
        return (None, None);
    };
    // 引用符はどれも ASCII なので、バイトの位置で読んでよい。
    let end = arrow.get(2..).and_then(|s| s.find('\'')).map(|k| k + 2);
    let label = match end {
        Some(end) if arrow.as_bytes().get(1) == Some(&b'\'') => Some(arrow[2..end].to_string()),
        _ => None,
    };
    if arrow.contains("<>") {
        (Some(Flow::Bidirectional), label)
    } else if arrow.contains('<') {
        (Some(Flow::Backward), label)
    } else if arrow.contains('>') {
        (Some(Flow::Forward), label)
    } else {
        (None, label)
    }
}

fn offsets(label: Option<&LabelAst>) -> (Option<f64>, Option<f64>) {
    match label {
        Some(l) => (
            Some((if l.neg_x { -1.0 } else { 1.0 }) * l.offset_x),
            Some((if l.neg_y { -1.0 } else { 1.0 }) * l.offset_y),
        ),
        None => (None, None),
    }
}

/// Mermaid の populateDb: 構文の木から、Mermaid の順で組む。
pub fn populate(ast: &WardleyAst, db: &mut WardleyBuilder) -> Result<()> {
    if let Some((w, h)) = ast.size {
        db.set_size(w, h);
    }

    if let Some(stages) = &ast.evolution {
        db.set_axes(
            stages
                .iter()
                .map(|s| match &s.second_name {
                    Some(second) if !second.is_empty() => {
                        format!("{} / {}", js::trim(&s.name), js::trim(second))
                    }
                    _ => js::trim(&s.name).to_string(),
                })
                .collect(),
            stages.iter().filter_map(|s| s.boundary).collect(),
        );
    }

    for anchor in &ast.anchors {
        let (x, y) = to_coordinates(
            anchor.visibility,
            anchor.evolution,
            &format!("Anchor \"{}\"", anchor.name),
        )?;
        db.add_node(NewNode {
            id: anchor.name.clone(),
            label: anchor.name.clone(),
            x,
            y,
            class_name: Some("anchor".into()),
            label_offset_x: None,
            label_offset_y: None,
            inertia: None,
            source_strategy: None,
        });
    }

    for component in &ast.components {
        let (x, y) = to_coordinates(
            component.visibility,
            component.evolution,
            &format!("Component \"{}\"", component.name),
        )?;
        let (ox, oy) = offsets(component.label.as_ref());
        db.add_node(NewNode {
            id: component.name.clone(),
            label: component.name.clone(),
            x,
            y,
            class_name: Some("component".into()),
            label_offset_x: ox,
            label_offset_y: oy,
            inertia: Some(Some(component.inertia)),
            source_strategy: Some(
                component
                    .decorator
                    .as_deref()
                    .and_then(SourceStrategy::parse),
            ),
        });
    }

    for note in &ast.notes {
        let (x, y) = to_coordinates(
            note.visibility,
            note.evolution,
            &format!("Note \"{}\"", note.text),
        )?;
        db.add_note(WardleyNote {
            text: note.text.clone(),
            x,
            y,
        });
    }

    for pipeline in &ast.pipelines {
        let Some(y) = db.get_node(&pipeline.parent).and_then(|p| p.y) else {
            return Err(PeleError::typed(
                format!(
                    "Pipeline \"{}\" must reference an existing component with coordinates.",
                    pipeline.parent
                ),
                ErrorCode::Semantic,
                "wardley",
            ));
        };
        db.start_pipeline(&pipeline.parent);
        for component in &pipeline.components {
            let id = format!("{}_{}", pipeline.parent, component.name);
            let (ox, oy) = offsets(component.label.as_ref());
            let x = to_percent(
                component.evolution,
                &format!("Pipeline component \"{}\" evolution", component.name),
            )?;
            db.add_node(NewNode {
                id: id.clone(),
                label: component.name.clone(),
                x,
                y,
                class_name: Some("pipeline-component".into()),
                label_offset_x: ox,
                label_offset_y: oy,
                inertia: Some(None),
                source_strategy: Some(None),
            });
            db.add_pipeline_component(&pipeline.parent, &id);
        }
    }

    for link in &ast.links {
        let dashed = link
            .arrow
            .as_deref()
            .is_some_and(|a| !a.is_empty() && (a.contains("-.->") || a.contains(".-.")));
        let (arrow_flow, arrow_label) = flow_from_arrow(link.arrow.as_deref());
        let flow = flow_from_port(link.from_port.as_deref())
            .or_else(|| flow_from_port(link.to_port.as_deref()))
            .or(arrow_flow);
        let source = db.resolve_node_id(&link.from);
        let target = db.resolve_node_id(&link.to);
        db.add_link(WardleyLink {
            source,
            target,
            dashed,
            label: arrow_label.or_else(|| link.link_label.clone()),
            flow,
        });
    }

    for (component, target) in &ast.evolves {
        if let Some(y) = db.get_node(component).and_then(|n| n.y) {
            let target_x = to_percent(*target, &format!("Evolve target for \"{component}\""))?;
            db.add_trend(WardleyTrend {
                node_id: component.clone(),
                target_x,
                target_y: y,
            });
        }
    }

    if let Some(&(x, y)) = ast.annotations.first() {
        let (x, y) = to_coordinates(x, y, "Annotations box")?;
        db.set_annotations_box(x, y);
    }

    for annotation in &ast.annotation {
        let (x, y) = to_coordinates(
            annotation.x,
            annotation.y,
            &format!("Annotation {}", js::fmt(annotation.number)),
        )?;
        db.add_annotation(WardleyAnnotation {
            number: annotation.number,
            coordinates: vec![(x, y)],
            text: Some(annotation.text.clone()),
        });
    }

    for a in &ast.accelerators {
        let (x, y) = to_coordinates(a.x, a.y, &format!("Accelerator \"{}\"", a.name))?;
        db.add_accelerator(WardleyForce {
            name: a.name.clone(),
            x,
            y,
        });
    }

    for d in &ast.deaccelerators {
        let (x, y) = to_coordinates(d.x, d.y, &format!("Deaccelerator \"{}\"", d.name))?;
        db.add_deaccelerator(WardleyForce {
            name: d.name.clone(),
            x,
            y,
        });
    }
    Ok(())
}

/// `.replace(/\n\s+/g, '\n')`
fn squeeze_indents(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.peek().is_some_and(|&n| js::is_js_space(n)) {
                chars.next();
            }
        }
    }
    out
}

pub fn build_wardley(ast: &WardleyAst, title: Option<&str>) -> Result<WardleyModel> {
    let mut db = WardleyBuilder::default();
    populate(ast, &mut db)?;
    Ok(WardleyModel {
        title: ast
            .title
            .clone()
            .filter(|t| !t.is_empty())
            .or_else(|| title.map(str::to_string)),
        acc_title: ast
            .acc_title
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(|t| js::trim_start(t).to_string()),
        acc_descr: ast
            .acc_descr
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(squeeze_indents),
        built: db.build()?,
    })
}
