//! quadrantChart の中身（pele の `src/diagrams/quadrant/db.ts`）。

use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

#[derive(Clone, Debug, PartialEq)]
pub struct QuadrantText {
    pub text: String,
    pub markdown: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PointStyle {
    pub radius: Option<f64>,
    pub color: Option<String>,
    pub stroke_color: Option<String>,
    pub stroke_width: Option<String>,
}

#[derive(Clone, Debug)]
pub struct QuadrantPoint {
    pub x: f64,
    pub y: f64,
    pub text: QuadrantText,
    pub class_name: String,
    pub style: PointStyle,
}

#[derive(Clone, Debug, Default)]
pub struct QuadrantDb {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    /// 象限 1〜4: 右上、左上、左下、右下。
    pub quadrants: [Option<QuadrantText>; 4],
    pub x_axis_left: Option<QuadrantText>,
    pub x_axis_right: Option<QuadrantText>,
    pub y_axis_bottom: Option<QuadrantText>,
    pub y_axis_top: Option<QuadrantText>,
    pub points: Vec<QuadrantPoint>,
    /// 入れた順を保つ（JS の `Map`）。
    pub classes: Vec<(String, PointStyle)>,
}

fn invalid(style: &str, value: &str, kind: &str) -> PeleError {
    PeleError::typed(
        format!("value for {style} {value} is invalid, please use a valid {kind}"),
        ErrorCode::Semantic,
        "quadrantChart",
    )
}

/// `^\d+$`
fn is_number(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
}

/// `^#?([\dA-Fa-f]{6}|[\dA-Fa-f]{3})$`
fn is_hex(s: &str) -> bool {
    let body = s.strip_prefix('#').unwrap_or(s);
    (body.len() == 6 || body.len() == 3) && body.bytes().all(|c| c.is_ascii_hexdigit())
}

/// `^\d+px$`
fn is_pixels(s: &str) -> bool {
    s.strip_suffix("px").is_some_and(is_number)
}

/// Mermaid の `style.trim().split(/\s*:\s*/)` の初めの 2 つを、1 度の走査で。
fn key_value(style: &str) -> (String, String) {
    let s = js::trim(style);
    let Some(colon) = s.find(':') else {
        return (s.to_string(), "undefined".into());
    };
    let rest = js::trim_start(&s[colon + 1..]);
    let value = match rest.find(':') {
        None => rest,
        Some(next) => js::trim_end(&rest[..next]),
    };
    (js::trim_end(&s[..colon]).to_string(), value.to_string())
}

pub fn parse_styles(styles: &[String]) -> Result<PointStyle> {
    let mut out = PointStyle::default();
    for style in styles {
        let (key, value) = key_value(style);
        match key.as_str() {
            "radius" => {
                if !is_number(&value) {
                    return Err(invalid(&key, &value, "number"));
                }
                out.radius = Some(js::parse_int(&value, 10));
            }
            "color" => {
                if !is_hex(&value) {
                    return Err(invalid(&key, &value, "hex code"));
                }
                out.color = Some(value);
            }
            "stroke-color" => {
                if !is_hex(&value) {
                    return Err(invalid(&key, &value, "hex code"));
                }
                out.stroke_color = Some(value);
            }
            "stroke-width" => {
                if !is_pixels(&value) {
                    return Err(invalid(&key, &value, "number of pixels (eg. 10px)"));
                }
                out.stroke_width = Some(value);
            }
            _ => {
                return Err(PeleError::typed(
                    format!("style named {key} is not supported."),
                    ErrorCode::Semantic,
                    "quadrantChart",
                ));
            }
        }
    }
    Ok(out)
}

fn trimmed(label: QuadrantText) -> QuadrantText {
    QuadrantText {
        text: js::trim(&label.text).to_string(),
        markdown: label.markdown,
    }
}

impl QuadrantDb {
    pub fn set_quadrant_text(&mut self, k: usize, label: QuadrantText) {
        self.quadrants[k] = Some(trimmed(label));
    }

    pub fn set_x_axis_left_text(&mut self, label: QuadrantText) {
        self.x_axis_left = Some(trimmed(label));
    }

    pub fn set_x_axis_right_text(&mut self, label: QuadrantText) {
        self.x_axis_right = Some(trimmed(label));
    }

    pub fn set_y_axis_bottom_text(&mut self, label: QuadrantText) {
        self.y_axis_bottom = Some(trimmed(label));
    }

    pub fn set_y_axis_top_text(&mut self, label: QuadrantText) {
        self.y_axis_top = Some(trimmed(label));
    }

    pub fn add_point(
        &mut self,
        label: QuadrantText,
        class_name: String,
        x: &str,
        y: &str,
        styles: &[String],
    ) -> Result<()> {
        let style = parse_styles(styles)?;
        self.points.push(QuadrantPoint {
            x: js::number(x),
            y: js::number(y),
            text: trimmed(label),
            class_name,
            style,
        });
        Ok(())
    }

    pub fn add_class(&mut self, class_name: String, styles: &[String]) -> Result<()> {
        let style = parse_styles(styles)?;
        match self.classes.iter_mut().find(|(k, _)| *k == class_name) {
            Some(slot) => slot.1 = style,
            None => self.classes.push((class_name, style)),
        }
        Ok(())
    }

    pub fn class(&self, name: &str) -> Option<&PointStyle> {
        self.classes.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    pub fn set_diagram_title(&mut self, text: String) {
        self.title = Some(text);
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        // `text.replace(/\n\s+/g, '\n')`
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
        self.acc_descr = Some(out);
    }
}
