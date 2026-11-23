//! flowchart の形の名前（pele の `src/diagrams/flowchart/shapes.ts`）。Mermaid が説明する名前（短い名前と別名）と、
//! 説明せずに受け付ける名前。

/// 説明のある名前を短い名前に。説明せずに受け付ける名前はそのまま。
fn canonical(name: &str) -> Option<&str> {
    Some(match name {
        "rect" | "proc" | "process" | "rectangle" | "squareRect" => "rect",
        "rounded" | "event" | "roundedRect" => "rounded",
        "stadium" | "terminal" | "pill" => "stadium",
        "fr-rect" | "subprocess" | "subproc" | "framed-rectangle" | "subroutine" => "fr-rect",
        "cyl" | "db" | "database" | "cylinder" => "cyl",
        "datastore" | "data-store" => "datastore",
        "folder" | "directory" => "folder",
        "bucket" => "bucket",
        "console" => "console",
        "browser" => "browser",
        "person" => "person",
        "circle" | "circ" => "circle",
        "bang" => "bang",
        "cloud" => "cloud",
        "diam" | "decision" | "diamond" | "question" => "diam",
        "hex" | "hexagon" | "prepare" => "hex",
        "lean-r" | "lean-right" | "in-out" | "lean_right" => "lean-r",
        "lean-l" | "lean-left" | "out-in" | "lean_left" => "lean-l",
        "trap-b" | "priority" | "trapezoid-bottom" | "trapezoid" => "trap-b",
        "trap-t" | "manual" | "trapezoid-top" | "inv-trapezoid" | "inv_trapezoid" => "trap-t",
        "dbl-circ" | "double-circle" | "doublecircle" => "dbl-circ",
        "text" => "text",
        "notch-rect" | "card" | "notched-rectangle" => "notch-rect",
        "lin-rect" | "lined-rectangle" | "lined-process" | "lin-proc" | "shaded-process" => {
            "lin-rect"
        }
        "sm-circ" | "start" | "small-circle" | "stateStart" => "sm-circ",
        "fr-circ" | "stop" | "framed-circle" | "stateEnd" => "fr-circ",
        "fork" | "join" | "forkJoin" => "fork",
        "hourglass" | "collate" => "hourglass",
        "brace" | "comment" | "brace-l" => "brace",
        "brace-r" => "brace-r",
        "braces" => "braces",
        "bolt" | "com-link" | "lightning-bolt" => "bolt",
        "doc" | "document" => "doc",
        "delay" | "half-rounded-rectangle" => "delay",
        "h-cyl" | "das" | "horizontal-cylinder" => "h-cyl",
        "lin-cyl" | "disk" | "lined-cylinder" => "lin-cyl",
        "curv-trap" | "curved-trapezoid" | "display" => "curv-trap",
        "div-rect" | "div-proc" | "divided-rectangle" | "divided-process" => "div-rect",
        "tri" | "extract" | "triangle" => "tri",
        "win-pane" | "internal-storage" | "window-pane" => "win-pane",
        "f-circ" | "junction" | "filled-circle" => "f-circ",
        "notch-pent" | "loop-limit" | "notched-pentagon" => "notch-pent",
        "flip-tri" | "manual-file" | "flipped-triangle" => "flip-tri",
        "sl-rect" | "manual-input" | "sloped-rectangle" => "sl-rect",
        "docs" | "documents" | "st-doc" | "stacked-document" => "docs",
        "st-rect" | "procs" | "processes" | "stacked-rectangle" => "st-rect",
        "bow-rect" | "stored-data" | "bow-tie-rectangle" => "bow-rect",
        "cross-circ" | "summary" | "crossed-circle" => "cross-circ",
        "tag-doc" | "tagged-document" => "tag-doc",
        "tag-rect" | "tagged-rectangle" | "tag-proc" | "tagged-process" => "tag-rect",
        "flag" | "paper-tape" => "flag",
        "odd" | "rect_left_inv_arrow" => "odd",
        "lin-doc" | "lined-document" => "lin-doc",
        "state"
        | "choice"
        | "note"
        | "composite"
        | "rectWithTitle"
        | "labelRect"
        | "block_arrow"
        | "collapsedGroup"
        | "iconSquare"
        | "iconCircle"
        | "icon"
        | "iconRounded"
        | "imageSquare"
        | "anchor"
        | "kanbanItem"
        | "mindmapCircle"
        | "defaultMindmapNode"
        | "classBox"
        | "erBox"
        | "requirementBox"
        | "usecaseActor"
        | "usecaseActorHollow"
        | "usecaseActorAwesome"
        | "usecaseActorIcon"
        | "usecaseBusiness"
        | "usecaseEllipse"
        | "usecaseJsonTable" => name,
        _ => return None,
    })
}

pub fn is_valid_shape(name: &str) -> bool {
    canonical(name).is_some()
}

/// 括弧の書き方の形を説明のある名前に。
pub fn canonical_shape(name: Option<&str>) -> String {
    let Some(name) = name else {
        return "rect".into();
    };
    let syntax = match name {
        "square" => Some("rect"),
        "round" => Some("rounded"),
        "ellipse" => Some("ellipse"),
        "hexagon" => Some("hex"),
        "diamond" => Some("diam"),
        _ => None,
    };
    syntax
        .or_else(|| canonical(name))
        .unwrap_or("rect")
        .to_string()
}
