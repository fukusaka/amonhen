//! timeline の形（pele の `src/diagrams/timeline/db.ts`）。

use crate::diagrams::journey::db::join_lines;
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

#[derive(Clone, Debug)]
pub struct TimelinePeriod {
    pub text: String,
    pub events: Vec<String>,
    /// 期間を書いたときの節の名前。最初の節の前は ''。
    pub section: String,
    /// その節の `sections` での番号。最初の節の前は -1。
    pub section_index: i64,
}

#[derive(Default)]
pub struct TimelineDb {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    /// 縦に流す（`TD`）か。既定は `LR`。
    pub down: bool,
    pub sections: Vec<String>,
    pub periods: Vec<TimelinePeriod>,
    current_section: String,
}

impl TimelineDb {
    pub fn set_diagram_title(&mut self, text: &str) {
        self.title = Some(text.to_string());
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        self.acc_descr = Some(join_lines(text));
    }

    pub fn set_direction(&mut self, down: bool) {
        self.down = down;
    }

    pub fn add_section(&mut self, text: &str) {
        self.current_section = text.to_string();
        self.sections.push(text.to_string());
    }

    pub fn add_task(&mut self, period: &str, event: &str) {
        self.periods.push(TimelinePeriod {
            text: period.to_string(),
            events: if event.is_empty() {
                Vec::new()
            } else {
                vec![event.to_string()]
            },
            section: self.current_section.clone(),
            section_index: self.sections.len() as i64 - 1,
        });
    }

    pub fn add_event(&mut self, event: &str) -> Result<()> {
        // Mermaid もここで TypeError で失敗する。
        let Some(period) = self.periods.last_mut() else {
            return Err(PeleError::typed(
                "An event needs a time period before it.",
                ErrorCode::Semantic,
                "timeline",
            ));
        };
        period.events.push(event.to_string());
        Ok(())
    }
}
