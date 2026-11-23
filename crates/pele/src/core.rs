//! 入り口（pele の `src/core.ts`）: 文字列を前処理し、種類を見分け、読み、描く。

use std::rc::Rc;

use crate::detect::{Detected, detect_diagram};
use crate::diagrams::registry;
use crate::errors::{Edit, ErrorCode, Origin, PeleError, Result, input_offset, locating_in};
use crate::js;
use crate::preprocess::{
    Config, cleanup_comments, encode_entities, normalize_line_breaks, preprocess,
};
use crate::svg::palette::{palette, with_palette};
use crate::text::entities::{decode_entities, written_codes};
use crate::text::icons::with_icons;
use crate::text::math::with_math;
use crate::types::{Diagram, IconResolver, LinkInfo, ParseLimits, RenderOptions};
use crate::util::yaml::Value;

/// Mermaid の既定の maxTextSize。
const DEFAULT_LIMIT: usize = 50000;
/// Mermaid は出力に上限を持たない。上限内の文字列でも何 MB もの絵を求めうり、置かれたページを止める。
const DEFAULT_OUTPUT_LIMIT: usize = 4_000_000;
/// Mermaid は 500 で止める。pele は 5,000 の辺を数十ミリ秒で描くが、`A & B --> C & D` は掛け算になり、上限が
/// なければ数 KB の文字列が何百万もの辺を求める。
const DEFAULT_MAX_EDGES: usize = 5000;

/// 描いた図（pele の `RenderResult`）。
#[derive(Clone, Debug, PartialEq)]
pub struct RenderResult {
    pub svg: String,
    /// pele の種類の名前（`DiagramType`、`flowchart` など）。
    pub kind: String,
    /// Mermaid の言い方（`flowchart-v2` など）。
    pub mermaid_type: String,
    pub width: f64,
    pub height: f64,
    pub links: Vec<LinkInfo>,
}

fn tag_errors<T>(found: &Detected, run: impl FnOnce() -> Result<T>) -> Result<T> {
    run().map_err(|mut e| {
        if e.type_.is_none() {
            e.type_ = Some(found.type_.to_string());
        }
        if e.mermaid_type.is_none() {
            e.mermaid_type = Some(found.mermaid_type.to_string());
        }
        e.message = written_codes(&e.message);
        e.snippet = written_codes(&e.snippet);
        e
    })
}

struct Parsed {
    found: Detected,
    diagram: &'static Diagram,
    model: Box<dyn std::any::Any>,
    config: Config,
}

fn parse_source(
    text: &str,
    limit: usize,
    max_edges: usize,
    extra: Option<&Config>,
) -> Result<Parsed> {
    if js::len16(text) > limit {
        return Err(PeleError::new(
            format!("Diagram source is longer than the limit of {limit} characters."),
            ErrorCode::Limit,
        ));
    }
    let pre = preprocess(text, None)?;
    let Some(found) = detect_diagram(&pre.text) else {
        return Err(PeleError::new(
            "No diagram type detected for the given text.",
            ErrorCode::UnsupportedDiagram,
        ));
    };
    let Some(diagram) = registry::diagram(found.type_) else {
        let mut e = PeleError::new(
            format!("Diagram type \"{}\" is not supported yet.", found.type_),
            ErrorCode::UnsupportedDiagram,
        );
        e.type_ = Some(found.type_.to_string());
        e.mermaid_type = Some(found.mermaid_type.to_string());
        return Err(e);
    };
    let config = match extra {
        Some(extra) => {
            // `{ ...extra, ...pre.config }`
            let mut c = extra.clone();
            for (k, v) in pre.config.iter() {
                c.set(k, v.clone());
            }
            c
        }
        None => pre.config.clone(),
    };
    let keep = diagram.keep_comments;
    let parsed = encode_entities(if keep { &pre.with_comments } else { &pre.text }, None) + "\n";
    let source = text.to_string();
    let origin = Origin {
        parsed: parsed.encode_utf16().collect(),
        find: Box::new(move |at| in_source(&source, keep, at)),
    };
    let limits = ParseLimits { max_edges };
    let model = tag_errors(&found, || {
        locating_in(origin, || {
            (diagram.parse)(
                &parsed,
                &config,
                pre.title.as_deref(),
                pre.line_offset,
                &limits,
            )
        })
    })?;
    Ok(Parsed {
        found,
        diagram,
        model,
        config,
    })
}

/// 前処理の書き換えを、元の位置が要るときだけ記録する。
fn in_source(text: &str, keep_comments: bool, at: usize) -> (Vec<u16>, usize) {
    let mut steps: Vec<Vec<Edit>> = Vec::new();
    if let Ok(pre) = preprocess(text, Some(&mut steps)) {
        let body = if keep_comments {
            pre.with_comments.clone()
        } else {
            cleanup_comments(&pre.with_comments, Some(&mut steps))
        };
        encode_entities(&body, Some(&mut steps));
    }
    // 読むために足した改行に合わせる。
    let source = normalize_line_breaks(text) + "\n";
    (source.encode_utf16().collect(), input_offset(&steps, at))
}

/// 文字列の図の種類（pele の `detectType`）。
pub fn detect_type(text: &str) -> Option<&'static str> {
    preprocess(text, None)
        .ok()
        .and_then(|p| detect_diagram(&p.text))
        .map(|d| d.type_)
}

/// 描ける種類の図か。
pub fn supports(text: &str) -> bool {
    detect_type(text).is_some_and(registry::registered)
}

/// 既定の選びで描く。
pub fn render(text: &str) -> Result<RenderResult> {
    render_with(text, &RenderOptions::default())
}

/// 選びを与えて描く（pele の `render(text, options)`）。
pub fn render_with(text: &str, options: &RenderOptions) -> Result<RenderResult> {
    let Parsed {
        found,
        diagram,
        model,
        config,
    } = parse_source(
        text,
        options.limit.unwrap_or(DEFAULT_LIMIT),
        options.max_edges.unwrap_or(DEFAULT_MAX_EDGES),
        options.config.as_ref(),
    )?;
    let section = config.get(diagram.section.unwrap_or(found.type_));
    let fixed =
        matches!(section, Some(Value::Obj(s)) if s.get("useMaxWidth") == Some(&Value::Bool(false)));
    let responsive = options.responsive.unwrap_or(!fixed);
    // ホストのアイコンの解き手と `links` の id は、書いたとおりの名前（符号を文字に戻したもの）を見る。SVG の
    // `data-icon`・`data-id` と同じ。
    let icons: Option<IconResolver> = options.icons.clone().map(|resolve| {
        let wrapped: IconResolver = Rc::new(move |name: &str| resolve(&decode_entities(name)));
        wrapped
    });
    let mut opts = options.clone();
    opts.responsive = Some(responsive);
    opts.icons = icons.clone();
    let math = if diagram.math {
        options.math.clone()
    } else {
        None
    };
    let mut rendered = tag_errors(&found, || {
        with_palette(palette(found.type_, &config), || {
            with_math(math, || {
                with_icons(icons, || (diagram.render)(model.as_ref(), &config, &opts))
            })
        })
    })?;
    let output_limit = options.output_limit.unwrap_or(DEFAULT_OUTPUT_LIMIT);
    if js::len16(&rendered.svg) > output_limit {
        let mut e = PeleError::new(
            format!("Diagram output is longer than the limit of {output_limit} characters."),
            ErrorCode::Limit,
        );
        e.type_ = Some(found.type_.to_string());
        e.mermaid_type = Some(found.mermaid_type.to_string());
        return Err(e);
    }
    for link in &mut rendered.links {
        link.id = decode_entities(&link.id);
    }
    Ok(RenderResult {
        svg: rendered.svg,
        kind: found.type_.to_string(),
        mermaid_type: found.mermaid_type.to_string(),
        width: rendered.width,
        height: rendered.height,
        links: rendered.links,
    })
}
