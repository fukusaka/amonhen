//! radar の読んだ形から描く形へ（pele の `src/diagrams/radar/model.ts`）。

use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

use super::parser::{OptionValue, RadarAst};

#[derive(Clone, Debug)]
pub struct RadarAxis {
    pub name: String,
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct RadarCurve {
    pub name: String,
    pub label: String,
    pub entries: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct RadarOptions {
    pub show_legend: bool,
    pub ticks: f64,
    pub max: Option<f64>,
    pub min: f64,
    /// `circle` か `polygon`。
    pub graticule: String,
}

#[derive(Clone, Debug)]
pub struct RadarModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub axes: Vec<RadarAxis>,
    pub curves: Vec<RadarCurve>,
    pub options: RadarOptions,
}

const MAX_TICKS: f64 = 32.0;

fn invalid(message: String) -> PeleError {
    PeleError::typed(message, ErrorCode::Semantic, "radar")
}

/// `\n\s+` を改行 1 つに。
fn squeeze_lines(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        out.push(c);
        i += 1;
        if c == '\n' {
            while i < chars.len() && js::is_js_space(chars[i]) {
                i += 1;
            }
        }
    }
    out
}

pub fn build_radar(ast: RadarAst, title: Option<&str>) -> Result<RadarModel> {
    let axes: Vec<RadarAxis> = ast
        .axes
        .iter()
        .map(|a| RadarAxis {
            name: a.name.clone(),
            label: a.label.clone().unwrap_or_else(|| a.name.clone()),
        })
        .collect();

    let mut curves = Vec::new();
    for curve in &ast.curves {
        let entries: Vec<f64> = if curve.entries[0].axis.is_none() {
            curve.entries.iter().map(|e| e.value).collect()
        } else {
            if axes.is_empty() {
                return Err(invalid(
                    "Axes must be populated before curves for reference entries".into(),
                ));
            }
            // 軸の名前ごとの最初の値（Map の入れた順は使わない）。
            let mut by_axis: Vec<(&str, f64)> = Vec::new();
            for entry in &curve.entries {
                if let Some(a) = &entry.axis
                    && !by_axis.iter().any(|(k, _)| k == a)
                {
                    by_axis.push((a, entry.value));
                }
            }
            let mut out = Vec::new();
            for axis in &axes {
                match by_axis.iter().find(|(k, _)| *k == axis.name) {
                    Some((_, v)) => out.push(*v),
                    None => return Err(invalid(format!("Missing entry for axis {}", axis.label))),
                }
            }
            out
        };
        curves.push(RadarCurve {
            name: curve.name.clone(),
            label: curve.label.clone().unwrap_or_else(|| curve.name.clone()),
            entries,
        });
    }

    let mut options = RadarOptions {
        show_legend: true,
        ticks: 5.0,
        max: None,
        min: 0.0,
        graticule: "circle".into(),
    };
    for option in &ast.options {
        match (option.name.as_str(), &option.value) {
            ("showLegend", OptionValue::Bool(b)) => options.show_legend = *b,
            ("graticule", OptionValue::Str(s)) => options.graticule = s.clone(),
            ("ticks", OptionValue::Num(n)) => options.ticks = *n,
            ("max", OptionValue::Num(n)) => options.max = Some(*n),
            ("min", OptionValue::Num(n)) => options.min = *n,
            _ => {}
        }
    }
    if options.ticks > MAX_TICKS {
        options.ticks = MAX_TICKS;
    }

    Ok(RadarModel {
        title: match ast.title.filter(|t| !t.is_empty()) {
            Some(t) => Some(t),
            None => title.map(str::to_string),
        },
        acc_title: ast
            .acc_title
            .filter(|t| !t.is_empty())
            .map(|t| js::trim_start(&t).to_string()),
        acc_descr: ast
            .acc_descr
            .filter(|t| !t.is_empty())
            .map(|t| squeeze_lines(&t)),
        axes,
        curves,
        options,
    })
}
