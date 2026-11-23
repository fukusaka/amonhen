//! venn の読んだもの（pele の `src/diagrams/venn/db.ts`）。

use std::collections::HashSet;

use super::types::{VennStyle, VennSubset, VennText};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

/// Mermaid は id・ラベル・スタイルの値から引用符を 1 組だけ外す。
pub fn normalize_text(text: &str) -> String {
    let trimmed = js::trim(text);
    if js::len16(trimmed) >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

/// JS の既定の `sort`（UTF-16 の符号単位の順）。
pub fn sort16(list: &mut [String]) {
    list.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
}

fn normalize_ids(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = ids.iter().map(|s| normalize_text(s)).collect();
    sort16(&mut out);
    out
}

#[derive(Default)]
pub struct VennDb {
    pub title: Option<String>,
    pub subsets: Vec<VennSubset>,
    pub text_nodes: Vec<VennText>,
    pub styles: Vec<VennStyle>,
    known: HashSet<String>,
    current: Option<Vec<String>>,
    /// 字下げした `text` の行が前の set・union のものか。字句解析が読む。
    indent: bool,
}

impl VennDb {
    pub fn set_diagram_title(&mut self, title: &str) {
        self.title = Some(title.to_string());
    }

    /// 大きさのない集合は 10、n 個の集合の重なりは 10 / n²。
    pub fn add_subset_data(&mut self, ids: &[String], label: Option<&str>, size: Option<f64>) {
        let sets = normalize_ids(ids);
        self.current = Some(sets.clone());
        if sets.len() == 1 {
            self.known.insert(sets[0].clone());
        }
        let n = ids.len() as f64;
        self.subsets.push(VennSubset {
            sets,
            size: size.unwrap_or(10.0 / (n * n)),
            label: label.filter(|l| !l.is_empty()).map(normalize_text),
        });
    }

    pub fn add_text_data(&mut self, ids: &[String], id: &str, label: Option<&str>) {
        self.text_nodes.push(VennText {
            sets: normalize_ids(ids),
            id: normalize_text(id),
            label: label.filter(|l| !l.is_empty()).map(normalize_text),
        });
    }

    pub fn add_style_data(&mut self, ids: &[String], data: &[(String, String)]) {
        let mut styles: Vec<(String, String)> = Vec::new();
        for (key, value) in data {
            let value = if value.is_empty() {
                value.clone()
            } else {
                normalize_text(value)
            };
            match styles.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = value,
                None => styles.push((key.clone(), value)),
            }
        }
        self.styles.push(VennStyle {
            targets: normalize_ids(ids),
            styles,
        });
    }

    pub fn validate_union_identifiers(&self, ids: &[String]) -> Result<()> {
        let unknown: Vec<String> = ids
            .iter()
            .map(|s| normalize_text(s))
            .filter(|id| !self.known.contains(id))
            .collect();
        if !unknown.is_empty() {
            return Err(PeleError::typed(
                format!("unknown set identifier: {}", unknown.join(", ")),
                ErrorCode::Semantic,
                "venn",
            ));
        }
        Ok(())
    }

    pub fn current_sets(&self) -> Option<&Vec<String>> {
        self.current.as_ref()
    }

    pub fn indent_mode(&self) -> bool {
        self.indent
    }

    pub fn set_indent_mode(&mut self, enabled: bool) {
        self.indent = enabled;
    }
}
