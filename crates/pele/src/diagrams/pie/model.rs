//! pie の形（pele の `src/diagrams/pie/model.ts`）。

use super::parser::PieAst;
use crate::diagrams::info::model::{acc_descr, acc_title, or_title};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

pub struct PieModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub show_data: bool,
    /// 名前と値。名前は 1 度ずつ、入れた順（JS の Map）。
    pub sections: Vec<(String, f64)>,
}

pub fn build_pie(ast: PieAst, title: Option<&str>) -> Result<PieModel> {
    let mut sections: Vec<(String, f64)> = Vec::new();
    for (label, value) in ast.sections {
        if value < 0.0 {
            return Err(PeleError::typed(
                format!(
                    "\"{label}\" has invalid value: {}. Negative values are not allowed in pie charts. All slice values must be >= 0.",
                    js::fmt(value)
                ),
                ErrorCode::Semantic,
                "pie",
            ));
        }
        if !sections.iter().any(|(l, _)| *l == label) {
            sections.push((label, value));
        }
    }
    Ok(PieModel {
        title: or_title(ast.title, title),
        acc_title: acc_title(ast.acc_title),
        acc_descr: acc_descr(ast.acc_descr),
        show_data: ast.show_data,
        sections,
    })
}
