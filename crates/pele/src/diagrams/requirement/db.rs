//! requirement の中身を集める（pele の `src/diagrams/requirement/db.ts`）。Map の入れた順は、並びと名前からの
//! 位置の表の組で保つ。

use std::collections::HashMap;

use super::types::{Relation, Requirement, RequirementClass, RequirementElement};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::util::chars::is_space_char;

pub mod requirement_type {
    pub const REQUIREMENT: &str = "Requirement";
    pub const FUNCTIONAL_REQUIREMENT: &str = "Functional Requirement";
    pub const INTERFACE_REQUIREMENT: &str = "Interface Requirement";
    pub const PERFORMANCE_REQUIREMENT: &str = "Performance Requirement";
    pub const PHYSICAL_REQUIREMENT: &str = "Physical Requirement";
    pub const DESIGN_CONSTRAINT: &str = "Design Constraint";
}

pub mod risk_level {
    pub const LOW_RISK: &str = "Low";
    pub const MED_RISK: &str = "Medium";
    pub const HIGH_RISK: &str = "High";
}

pub mod verify_type {
    pub const VERIFY_ANALYSIS: &str = "Analysis";
    pub const VERIFY_DEMONSTRATION: &str = "Demonstration";
    pub const VERIFY_INSPECTION: &str = "Inspection";
    pub const VERIFY_TEST: &str = "Test";
}

pub mod relationships {
    pub const CONTAINS: &str = "contains";
    pub const COPIES: &str = "copies";
    pub const DERIVES: &str = "derives";
    pub const SATISFIES: &str = "satisfies";
    pub const VERIFIES: &str = "verifies";
    pub const REFINES: &str = "refines";
    pub const TRACES: &str = "traces";
}

/// クラスを当てるとその宣言をノードごとに写すので、短い文が何億もの写しを求めうる。これを超えたら断る。
const MAX_STYLE_COPIES: usize = 1_000_000;

#[derive(Debug)]
pub struct RequirementDb {
    pub direction: String,
    pub requirements: Vec<Requirement>,
    pub elements: Vec<RequirementElement>,
    pub relations: Vec<Relation>,
    pub classes: Vec<RequirementClass>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,

    requirement_at: HashMap<String, usize>,
    element_at: HashMap<String, usize>,
    class_at: HashMap<String, usize>,
    // 文法は本体の項目を、それが属する要求や要素より先に知らせる。
    new_id: String,
    new_text: String,
    new_risk: String,
    new_verify_method: String,
    new_element_type: String,
    new_doc_ref: String,
    style_copies: usize,
}

impl Default for RequirementDb {
    fn default() -> Self {
        Self::new()
    }
}

/// ノードの的（要求か要素か）。
#[derive(Clone, Copy)]
enum NodeAt {
    Requirement(usize),
    Element(usize),
}

impl RequirementDb {
    pub fn new() -> Self {
        RequirementDb {
            direction: "TB".to_string(),
            requirements: Vec::new(),
            elements: Vec::new(),
            relations: Vec::new(),
            classes: Vec::new(),
            title: None,
            acc_title: None,
            acc_descr: None,
            requirement_at: HashMap::new(),
            element_at: HashMap::new(),
            class_at: HashMap::new(),
            new_id: String::new(),
            new_text: String::new(),
            new_risk: String::new(),
            new_verify_method: String::new(),
            new_element_type: String::new(),
            new_doc_ref: String::new(),
            style_copies: 0,
        }
    }

    fn count(&mut self, n: usize) -> Result<()> {
        self.style_copies += n;
        if self.style_copies > MAX_STYLE_COPIES {
            return Err(PeleError::typed(
                "Requirement diagram applies too many style declarations.",
                ErrorCode::Limit,
                "requirement",
            ));
        }
        Ok(())
    }

    fn styles_of(&mut self, at: NodeAt) -> &mut Vec<String> {
        match at {
            NodeAt::Requirement(i) => &mut self.requirements[i].css_styles,
            NodeAt::Element(i) => &mut self.elements[i].css_styles,
        }
    }

    fn copy(&mut self, at: NodeAt, styles: &[String]) -> Result<()> {
        self.count(styles.len())?;
        self.styles_of(at).extend(styles.iter().cloned());
        Ok(())
    }

    pub fn set_direction(&mut self, dir: &str) {
        self.direction = dir.to_string();
    }

    pub fn has(&self, id: &str) -> bool {
        self.requirement_at.contains_key(id) || self.element_at.contains_key(id)
    }

    pub fn add_requirement(&mut self, name: &str, type_: &str) {
        if !self.requirement_at.contains_key(name) {
            self.requirement_at
                .insert(name.to_string(), self.requirements.len());
            self.requirements.push(Requirement {
                name: name.to_string(),
                type_: type_.to_string(),
                requirement_id: self.new_id.clone(),
                text: self.new_text.clone(),
                risk: self.new_risk.clone(),
                verify_method: self.new_verify_method.clone(),
                css_styles: Vec::new(),
                classes: vec!["default".to_string()],
            });
        }
        self.new_id.clear();
        self.new_text.clear();
        self.new_risk.clear();
        self.new_verify_method.clear();
    }

    pub fn set_new_req_id(&mut self, id: &str) {
        self.new_id = id.to_string();
    }

    pub fn set_new_req_text(&mut self, text: &str) {
        self.new_text = text.to_string();
    }

    pub fn set_new_req_risk(&mut self, risk: &str) {
        self.new_risk = risk.to_string();
    }

    pub fn set_new_req_verify_method(&mut self, verify_method: &str) {
        self.new_verify_method = verify_method.to_string();
    }

    pub fn add_element(&mut self, name: &str) {
        if !self.element_at.contains_key(name) {
            self.element_at
                .insert(name.to_string(), self.elements.len());
            self.elements.push(RequirementElement {
                name: name.to_string(),
                type_: self.new_element_type.clone(),
                doc_ref: self.new_doc_ref.clone(),
                css_styles: Vec::new(),
                classes: vec!["default".to_string()],
            });
        }
        self.new_element_type.clear();
        self.new_doc_ref.clear();
    }

    pub fn set_new_element_type(&mut self, type_: &str) {
        self.new_element_type = type_.to_string();
    }

    pub fn set_new_element_doc_ref(&mut self, doc_ref: &str) {
        self.new_doc_ref = doc_ref.to_string();
    }

    pub fn add_relationship(&mut self, type_: &str, src: &str, dst: &str) {
        self.relations.push(Relation {
            type_: type_.to_string(),
            src: src.to_string(),
            dst: dst.to_string(),
        });
    }

    fn node(&self, id: &str) -> Option<NodeAt> {
        self.requirement_at
            .get(id)
            .map(|&i| NodeAt::Requirement(i))
            .or_else(|| self.element_at.get(id).map(|&i| NodeAt::Element(i)))
    }

    pub fn set_css_style(&mut self, ids: &[String], styles: &[String]) -> Result<()> {
        for id in ids {
            // Mermaid は最初の知らない id で止まり、残りにはスタイルを当てない。
            let Some(at) = self.node(id) else {
                return Ok(());
            };
            for s in styles {
                let parts: Vec<String> = s.split(',').map(str::to_string).collect();
                self.copy(at, &parts)?;
            }
        }
        Ok(())
    }

    pub fn set_class(&mut self, ids: &[String], class_names: &[String]) -> Result<()> {
        for id in ids {
            let Some(at) = self.node(id) else {
                continue;
            };
            for name in class_names {
                match at {
                    NodeAt::Requirement(i) => self.requirements[i].classes.push(name.clone()),
                    NodeAt::Element(i) => self.elements[i].classes.push(name.clone()),
                }
                if let Some(&c) = self.class_at.get(name) {
                    let styles = self.classes[c].styles.clone();
                    self.copy(at, &styles)?;
                }
            }
        }
        Ok(())
    }

    pub fn define_class(&mut self, ids: &[String], style: &[String]) -> Result<()> {
        for id in ids {
            let c = match self.class_at.get(id) {
                Some(&c) => c,
                None => {
                    self.class_at.insert(id.clone(), self.classes.len());
                    self.classes.push(RequirementClass {
                        id: id.clone(),
                        styles: Vec::new(),
                        text_styles: Vec::new(),
                    });
                    self.classes.len() - 1
                }
            };
            for s in style {
                if s.contains("color") {
                    self.classes[c]
                        .text_styles
                        .push(s.replacen("fill", "bgFill", 1));
                }
                self.classes[c].styles.push(s.clone());
            }
            let split: Vec<String> = style
                .iter()
                .flat_map(|s| s.split(',').map(str::to_string))
                .collect();
            for i in 0..self.requirements.len() {
                if self.requirements[i].classes.iter().any(|x| x == id) {
                    self.copy(NodeAt::Requirement(i), &split)?;
                }
            }
            for i in 0..self.elements.len() {
                if self.elements[i].classes.iter().any(|x| x == id) {
                    self.copy(NodeAt::Element(i), &split)?;
                }
            }
        }
        Ok(())
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(text.trim_start_matches(is_space_char).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        // `text.replace(/\n\s+/g, '\n')`
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '\n' {
                while chars.peek().is_some_and(|c| is_space_char(*c)) {
                    chars.next();
                }
            }
        }
        self.acc_descr = Some(out);
    }
}
