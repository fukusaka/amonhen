//! venn を描く（pele の `src/diagrams/venn/render.ts`）。

use std::collections::{HashMap, HashSet};

use super::db::{VennDb, normalize_text};
use super::layout::{Circle, Spot, find_spot, layout_circles, region_path};
use super::types::VennSubset;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::ink::ink;
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{ResolvedStyle, resolve_style, series_color};
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style as st};
use crate::types::{RenderOptions, Rendered};

const RADIUS: f64 = 100.0;
const MAX_GROWTH: f64 = 1.6;

struct Line {
    id: Option<String>,
    raw: String,
    size: f64,
    base: u32,
    label: Label,
}

struct Region {
    sets: Vec<String>,
    inside: Vec<usize>,
    spot: Spot,
    /// 領域に書くもの: 先にラベル、次に文字。id は文字のもの。
    lines: Vec<Line>,
    style: ResolvedStyle,
    /// 文字をほかの円の上にまたいで書くか。
    across: bool,
}

/// `[";]fill-opacity:`
fn sets_fill_opacity(shape: &str) -> bool {
    shape
        .match_indices("fill-opacity:")
        .any(|(k, _)| k > 0 && matches!(shape.as_bytes()[k - 1], b'"' | b';'))
}

pub fn render_venn(model: &VennDb, _config: &Config, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.8125);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);

    let circles = layout_circles(&model.subsets);
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, c) in circles.iter().enumerate() {
        index.insert(&c.id, i);
    }

    // 同じ対象へのスタイルは足し合わせ、後の宣言が勝つ。
    let mut styles: HashMap<Vec<String>, Vec<(String, String)>> = HashMap::new();
    for s in &model.styles {
        let merged = styles.entry(s.targets.clone()).or_default();
        for (property, value) in &s.styles {
            match merged.iter_mut().find(|(p, _)| p == property) {
                Some(slot) => slot.1 = value.clone(),
                None => merged.push((property.clone(), value.clone())),
            }
        }
    }
    let style_of = |key: &[String]| -> ResolvedStyle {
        let Some(declared) = styles.get(key) else {
            return ResolvedStyle::default();
        };
        let list: Vec<String> = declared.iter().map(|(p, v)| format!("{p}:{v}")).collect();
        resolve_style(&list)
    };

    // 各円に触れる円。その領域に食い込めるのはそれだけ。
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); circles.len()];
    for a in 0..circles.len() {
        for b in a + 1..circles.len() {
            if js::hypot(circles[a].x - circles[b].x, circles[a].y - circles[b].y)
                < circles[a].r + circles[b].r
            {
                neighbors[a].push(b);
                neighbors[b].push(a);
            }
        }
    }

    // 集合と述べた重なりごとに 1 つの領域。同じ集合についての後の文が前のものに代わる。
    let mut stated: Vec<(Vec<String>, &VennSubset)> = Vec::new();
    for subset in &model.subsets {
        match stated.iter_mut().find(|(k, _)| *k == subset.sets) {
            Some(slot) => slot.1 = subset,
            None => stated.push((subset.sets.clone(), subset)),
        }
    }
    let mut regions: Vec<(Vec<String>, Region)> = Vec::new();
    for (key, subset) in &stated {
        let mut sets: Vec<String> = Vec::new();
        for id in &subset.sets {
            if !sets.contains(id) {
                sets.push(id.clone());
            }
        }
        let inside: Vec<usize> = sets.iter().map(|id| index[id.as_str()]).collect();
        let within: HashSet<usize> = inside.iter().copied().collect();
        let mut smallest = inside[0];
        for &i in &inside {
            if circles[i].r < circles[smallest].r {
                smallest = i;
            }
        }
        let style = style_of(key);
        let text = subset.label.clone().or_else(|| {
            if subset.sets.len() == 1 {
                Some(subset.sets[0].clone())
            } else {
                None
            }
        });
        let base =
            (if style.bold { st::BOLD } else { 0 }) | (if style.italic { st::ITALIC } else { 0 });
        let others: Vec<usize> = neighbors[smallest]
            .iter()
            .copied()
            .filter(|i| !within.contains(i))
            .collect();
        let mut spot = find_spot(&circles, &inside, &others);
        // ほかの円が覆う（ほとんど覆う）領域は、それらの上にまたいで書く。
        let across = !others.is_empty() && spot.margin < circles[smallest].r * 0.2;
        if across {
            spot = find_spot(&circles, &inside, &[]);
        }
        let mut region = Region {
            sets,
            inside,
            spot,
            lines: Vec::new(),
            style,
            across,
        };
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            let font_size = if region.sets.len() == 1 { size } else { small };
            let label = layout_label(Some(&text), false, measurer, font_size, 4000.0, base);
            region.lines.push(Line {
                id: None,
                raw: text,
                size: font_size,
                base,
                label,
            });
        }
        regions.push((key.clone(), region));
    }
    for node in &model.text_nodes {
        let raw = node.label.clone().unwrap_or_else(|| node.id.clone());
        if let Some((_, region)) = regions.iter_mut().find(|(k, _)| *k == node.sets) {
            let label = layout_label(Some(&raw), false, measurer, tiny, 4000.0, 0);
            region.lines.push(Line {
                id: Some(node.id.clone()),
                raw,
                size: tiny,
                base: 0,
                label,
            });
        }
    }

    // ここまでの長さは配置の単位。いちばん大きい円を決まった半径にし、限りの中で、どの領域も書くものが入る
    // まで大きくする。
    let mut largest = 0.0;
    for c in &circles {
        if c.r > largest {
            largest = c.r;
        }
    }
    let mut scale = if largest > 0.0 { RADIUS / largest } else { 1.0 };
    let mut growth = 1.0;
    for (_, region) in &regions {
        if region.spot.margin <= 0.0 {
            continue;
        }
        let mut width = 0.0;
        let mut height = 0.0;
        for line in &region.lines {
            width = js::max(width, line.label.width);
            height += line.label.height;
        }
        let need = js::hypot(width, height) / 2.0 + 4.0;
        growth = js::max(
            growth,
            js::min(need / (region.spot.margin * scale), MAX_GROWTH),
        );
    }
    scale *= growth;

    // まだ領域に広すぎるものは、領域に合わせて折り返す。
    for (_, region) in regions.iter_mut() {
        let room = js::max(region.spot.margin * scale * 1.8, 72.0);
        for line in region.lines.iter_mut() {
            if line.label.width > room {
                line.label =
                    layout_label(Some(&line.raw), false, measurer, line.size, room, line.base);
            }
        }
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for c in &circles {
        min_x = js::min(min_x, (c.x - c.r) * scale);
        min_y = js::min(min_y, (c.y - c.r) * scale);
        max_x = js::max(max_x, (c.x + c.r) * scale);
        max_y = js::max(max_y, (c.y + c.r) * scale);
    }
    for (_, region) in &regions {
        let mut wide = 0.0;
        let mut tall = 0.0;
        for line in &region.lines {
            wide = js::max(wide, line.label.width);
            tall += line.label.height;
        }
        min_x = js::min(min_x, region.spot.x * scale - wide / 2.0);
        max_x = js::max(max_x, region.spot.x * scale + wide / 2.0);
        min_y = js::min(min_y, region.spot.y * scale - tall / 2.0);
        max_y = js::max(max_y, region.spot.y * scale + tall / 2.0);
    }
    if circles.is_empty() {
        min_x = 0.0;
        min_y = 0.0;
        max_x = 0.0;
        max_y = 0.0;
    }

    let title_text = model.title.as_deref().map(normalize_text);
    let title = layout_label(
        title_text.as_deref(),
        false,
        measurer,
        size,
        4000.0,
        st::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let inner = js::max(max_x - min_x, title.width);
    let ox = pad + (inner - (max_x - min_x)) / 2.0 - min_x;
    let oy = pad + title_height - min_y;
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let placed: Vec<Circle> = circles
        .iter()
        .map(|c| Circle {
            id: c.id.clone(),
            x: c.x * scale + ox,
            y: c.y * scale + oy,
            r: c.r * scale,
        })
        .collect();

    let no_style = ResolvedStyle::default();
    let region_of = |key: &[String]| regions.iter().find(|(k, _)| k == key).map(|(_, r)| r);

    let mut sets = String::new();
    for (i, c) in placed.iter().enumerate() {
        let color = series_color(i as i64);
        let style = region_of(std::slice::from_ref(&c.id)).map_or(&no_style, |r| &r.style);
        sets.push_str(&format!(
            "<circle class=\"pele-venn-set\" data-id=\"{}\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{color}\" fill-opacity=\"0.2\" stroke=\"{color}\"{}/>",
            esc_text(&c.id),
            num(c.x),
            num(c.y),
            num(c.r),
            style.shape
        ));
    }

    let painted: Vec<&Region> = regions
        .iter()
        .map(|(_, r)| r)
        .filter(|r| r.sets.len() > 1 && !r.style.shape.is_empty())
        .collect();
    // 領域の文字の下の塗りは、その全体の上に最後に描いた重なりか、スタイルが不透明度を決めるときは自分の円。
    let under = |region: &Region| -> String {
        if region.across {
            return String::new();
        }
        let mut fill = if region.sets.len() == 1 && sets_fill_opacity(&region.style.shape) {
            region.style.shape.clone()
        } else {
            String::new()
        };
        for other in &painted {
            if other.sets.iter().all(|id| region.sets.contains(id)) {
                fill = other.style.shape.clone();
            }
        }
        fill
    };

    let mut overlaps = String::new();
    let mut labels = String::new();
    for (_, region) in &regions {
        let id = esc_text(&region.sets.join("|"));
        // 重なりは、スタイルが自分の見た目を与えるときだけ描く。ほかは円が見せる。
        if region.sets.len() > 1 && !region.style.shape.is_empty() {
            let inside: Vec<Circle> = region.inside.iter().map(|&i| placed[i].clone()).collect();
            let d = region_path(&inside, num);
            if !d.is_empty() {
                overlaps.push_str(&format!(
                    "<path class=\"pele-venn-overlap\" data-id=\"{id}\" d=\"{d}\" fill=\"none\"{}/>",
                    region.style.shape
                ));
            }
        }
        let mut total = 0.0;
        for line in &region.lines {
            total += line.label.height;
        }
        if total == 0.0 {
            continue;
        }
        let x = region.spot.x * scale + ox;
        let mut y = region.spot.y * scale + oy - total / 2.0;
        let fill = under(region);
        let mut content = String::new();
        for line in &region.lines {
            let own = match &line.id {
                None => region.style.text.clone(),
                Some(id) => style_of(std::slice::from_ref(id)).text,
            };
            let color = ink(&fill, &own);
            let attrs = match &line.id {
                None => format!(
                    " class=\"pele-label\"{}{}{own}",
                    if line.size == size {
                        String::new()
                    } else {
                        format!(" font-size=\"{}\"", js::fmt(line.size))
                    },
                    color.map_or(String::new(), |c| format!(" fill=\"{c}\""))
                ),
                Some(id) => format!(
                    " class=\"pele-venn-text\" data-id=\"{}\" font-size=\"{}\" fill=\"{}\"{own}",
                    esc_text(id),
                    js::fmt(tiny),
                    color.unwrap_or("var(--_m)")
                ),
            };
            content.push_str(&label_svg(
                &line.label,
                x,
                y + line.label.height / 2.0,
                &attrs,
                None,
            ));
            y += line.label.height;
        }
        labels.push_str(&format!(
            "<g class=\"pele-venn-region\" data-id=\"{id}\">{content}</g>"
        ));
    }

    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&format!("<g class=\"pele-venn-sets\">{sets}</g>"));
    if !overlaps.is_empty() {
        content.push_str(&format!("<g class=\"pele-venn-overlaps\">{overlaps}</g>"));
    }
    if !labels.is_empty() {
        content.push_str(&format!("<g class=\"pele-venn-labels\">{labels}</g>"));
    }
    let svg = svg_document(
        "venn",
        width,
        height,
        size,
        options,
        &Accessible::default(),
        &content,
    );
    Rendered {
        svg,
        width,
        height,
        links: Vec::new(),
    }
}
