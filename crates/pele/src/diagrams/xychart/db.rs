//! xychart の中身（pele の `src/diagrams/xychart/db.ts`）。

use crate::js;

#[derive(Clone, Debug)]
pub struct XyText {
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct DataPoint {
    pub value: f64,
    pub label: String,
}

#[derive(Clone, Debug)]
pub enum XAxis {
    Band {
        title: String,
        categories: Vec<String>,
    },
    Linear {
        title: String,
        min: f64,
        max: f64,
    },
}

impl XAxis {
    pub fn title(&self) -> &str {
        match self {
            XAxis::Band { title, .. } | XAxis::Linear { title, .. } => title,
        }
    }

    fn title_mut(&mut self) -> &mut String {
        match self {
            XAxis::Band { title, .. } | XAxis::Linear { title, .. } => title,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LinearAxis {
    pub title: String,
    pub min: f64,
    pub max: f64,
}

/// 値ごとの x 軸の場所: 区分か、文字で書いた数。値のない区分は `None`。
pub type PlotData = Vec<(String, Option<f64>)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlotType {
    Line,
    Bar,
}

#[derive(Clone, Debug)]
pub struct Plot {
    pub type_: PlotType,
    pub title: String,
    pub data: PlotData,
    pub point_labels: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct XyChartDb {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    /// `vertical` か `horizontal`。
    pub orientation: Option<&'static str>,
    pub x_axis: XAxis,
    pub y_axis: LinearAxis,
    pub has_set_x_axis: bool,
    /// y の範囲が `y-axis` の文でなくデータから来る間は false。
    pub has_set_y_axis: bool,
    pub plots: Vec<Plot>,
}

impl Default for XyChartDb {
    fn default() -> Self {
        XyChartDb {
            title: None,
            acc_title: None,
            acc_descr: None,
            orientation: None,
            x_axis: XAxis::Band {
                title: String::new(),
                categories: Vec::new(),
            },
            y_axis: LinearAxis {
                title: String::new(),
                min: f64::INFINITY,
                max: f64::NEG_INFINITY,
            },
            has_set_x_axis: false,
            has_set_y_axis: false,
            plots: Vec::new(),
        }
    }
}

impl XyChartDb {
    pub fn set_orientation(&mut self, orientation: &str) {
        self.orientation = Some(if orientation == "horizontal" {
            "horizontal"
        } else {
            "vertical"
        });
    }

    pub fn set_x_axis_title(&mut self, title: &XyText) {
        *self.x_axis.title_mut() = js::trim(&title.text).to_string();
    }

    pub fn set_x_axis_range_data(&mut self, min: f64, max: f64) {
        self.x_axis = XAxis::Linear {
            title: self.x_axis.title().to_string(),
            min,
            max,
        };
        self.has_set_x_axis = true;
    }

    pub fn set_x_axis_band(&mut self, categories: &[XyText]) {
        self.x_axis = XAxis::Band {
            title: self.x_axis.title().to_string(),
            categories: categories
                .iter()
                .map(|c| js::trim(&c.text).to_string())
                .collect(),
        };
        self.has_set_x_axis = true;
    }

    pub fn set_y_axis_title(&mut self, title: &XyText) {
        self.y_axis.title = js::trim(&title.text).to_string();
    }

    pub fn set_y_axis_range_data(&mut self, min: f64, max: f64) {
        self.y_axis = LinearAxis {
            title: std::mem::take(&mut self.y_axis.title),
            min,
            max,
        };
        self.has_set_y_axis = true;
    }

    fn plot_data(&mut self, mut values: Vec<f64>) -> PlotData {
        if values.is_empty() {
            return Vec::new();
        }
        // x 軸の文がなければ、最初の系列が x の範囲を決める。
        if !self.has_set_x_axis {
            self.set_x_axis_range_data(1.0, values.len() as f64);
        }
        if let XAxis::Band { categories, .. } = &self.x_axis
            && values.len() > categories.len()
        {
            values.truncate(categories.len());
        }
        if !self.has_set_y_axis {
            let (mut min, mut max) = (self.y_axis.min, self.y_axis.max);
            for &value in &values {
                if value < min {
                    min = value;
                }
                if value > max {
                    max = value;
                }
            }
            self.y_axis.min = min;
            self.y_axis.max = max;
        }
        match &self.x_axis {
            XAxis::Band { categories, .. } => categories
                .iter()
                .enumerate()
                .map(|(i, c)| (c.clone(), values.get(i).copied()))
                .collect(),
            XAxis::Linear { min, max, .. } => {
                if values.len() == 1 {
                    return vec![(js::fmt(*min), Some(values[0]))];
                }
                let step = (max - min) / (values.len() as f64 - 1.0);
                values
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| (js::fmt(min + i as f64 * step), Some(v)))
                    .collect()
            }
        }
    }

    fn add_plot(&mut self, type_: PlotType, title: &XyText, data: &[DataPoint]) {
        let values = data.iter().map(|d| d.value).collect();
        let mut plot = Plot {
            type_,
            title: js::trim(&title.text).to_string(),
            data: self.plot_data(values),
            point_labels: None,
        };
        // Mermaid は棒の系列の点のラベルを受けて、使わない。
        if type_ == PlotType::Line && data.iter().any(|d| !js::trim(&d.label).is_empty()) {
            plot.point_labels = Some(
                data.iter()
                    .map(|d| js::trim(&d.label).to_string())
                    .collect(),
            );
        }
        self.plots.push(plot);
    }

    pub fn set_line_data(&mut self, title: &XyText, data: &[DataPoint]) {
        self.add_plot(PlotType::Line, title, data);
    }

    pub fn set_bar_data(&mut self, title: &XyText, data: &[DataPoint]) {
        self.add_plot(PlotType::Bar, title, data);
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
