//! agentflow の形（pele の `src/diagrams/agentflow/shapes.ts`）。

use crate::diagrams::flowchart::shapes::is_valid_shape as is_flowchart_shape;

/// agentflow で意味のある形の、書き手向けの名前。
pub fn shape_alias(shape: &str) -> Option<&'static str> {
    Some(match shape {
        "task" => "roundedRect",
        "tool" => "subroutine",
        "input" => "lean-right",
        "decision" => "diamond",
        "refdoc" => "lin-doc",
        "action" => "hexagon",
        "round" => "rect",
        _ => return None,
    })
}

pub const REMOVED_SHAPES: [&str; 27] = [
    "doc",
    "stadium",
    "terminal",
    "circle",
    "trapezoid",
    "inv_trapezoid",
    "inv-trapezoid",
    "doublecircle",
    "double-circle",
    "typeDeclaration",
    "procs",
    "lean_left",
    "lean-left",
    "in-out",
    "cylinder",
    "ellipse",
    "odd",
    "tag-rect",
    "tagged-rectangle",
    "delay",
    "half-rounded-rectangle",
    "lin-rect",
    "lined-rectangle",
    "win-pane",
    "window-pane",
    "curv-trap",
    "curved-trapezoid",
];

pub const ALLOWED_SHAPES: [&str; 13] = [
    "roundedRect",
    "subroutine",
    "subprocess",
    "subproc",
    "framed-rectangle",
    "lean-right",
    "diamond",
    "lin-doc",
    "lined-document",
    "hexagon",
    "hex",
    "connector",
    "collapsedGroup",
];

pub const DEFAULT_SHAPE: &str = "roundedRect";

/// 空の名前はそのまま。
pub fn resolve_shape_alias(shape: Option<&str>) -> Option<String> {
    let shape = shape?;
    if shape.is_empty() {
        return Some(String::new());
    }
    Some(shape_alias(shape).unwrap_or(shape).to_string())
}

/// Mermaid は形の名前を素のオブジェクトに `in` で探すので、`constructor` もそこでは通る。
pub fn is_valid_shape(name: &str) -> bool {
    is_flowchart_shape(name) || name == "constructor"
}

pub trait ShapeSink {
    fn emit_error(&mut self, id: &'static str, message: String, node_id: &str);
    fn emit_warning(&mut self, id: &'static str, message: String, node_id: &str);
}

pub struct ShapedNode<'a> {
    pub id: &'a str,
    pub shape: &'a mut String,
    pub is_group: bool,
}

/// どのノードの形も agentflow が描く形にし、除いた形と描かない形を知らせる。
pub fn normalise_node_shapes(nodes: Vec<ShapedNode>, sink: &mut dyn ShapeSink) {
    for node in nodes {
        if node.is_group {
            continue;
        }
        if node.shape.is_empty() || node.shape == "squareRect" || node.shape == "rect" {
            *node.shape = DEFAULT_SHAPE.into();
        }
        if REMOVED_SHAPES.contains(&node.shape.as_str()) {
            sink.emit_error(
                "SHAPE_REMOVED",
                format!(
                    "shape \"{}\" was removed in v0.8.1, using \"{DEFAULT_SHAPE}\"",
                    node.shape
                ),
                node.id,
            );
            *node.shape = DEFAULT_SHAPE.into();
        } else if !ALLOWED_SHAPES.contains(&node.shape.as_str()) {
            sink.emit_warning(
                "SHAPE_UNSUPPORTED",
                format!(
                    "shape \"{}\" is not supported, using \"{DEFAULT_SHAPE}\"",
                    node.shape
                ),
                node.id,
            );
            *node.shape = DEFAULT_SHAPE.into();
        }
    }
}
