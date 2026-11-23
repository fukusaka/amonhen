//! journey の形（pele の `src/diagrams/journey/db.ts`）。

use crate::js;

#[derive(Clone, Debug)]
pub struct JourneyTask {
    pub task: String,
    /// 書いたとおり: どんな数でも。数でなければ NaN。
    pub score: f64,
    pub people: Vec<String>,
    /// 作業を書いたときの節の名前。最初の節の前は ''。
    pub section: String,
}

#[derive(Default)]
pub struct JourneyDb {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub sections: Vec<String>,
    pub tasks: Vec<JourneyTask>,
    current_section: String,
}

/// 作業に名のある人すべて。1 度ずつ、UTF-16 の符号単位の順。
pub fn journey_actors(tasks: &[JourneyTask]) -> Vec<String> {
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut names: Vec<String> = Vec::new();
    for task in tasks {
        for name in &task.people {
            if seen.insert(name) {
                names.push(name.clone());
            }
        }
    }
    names.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    names
}

/// `text.replace(/\n\s+/g, '\n')`。
pub fn join_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.peek().is_some_and(|&c| js::is_js_space(c)) {
                chars.next();
            }
        }
    }
    out
}

impl JourneyDb {
    pub fn set_diagram_title(&mut self, text: &str) {
        self.title = Some(text.to_string());
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        self.acc_descr = Some(join_lines(text));
    }

    pub fn add_section(&mut self, text: &str) {
        self.current_section = text.to_string();
        self.sections.push(text.to_string());
    }

    /// `data` は行の最初のコロンからの残り: `:score` か `:score:actor, actor`。
    pub fn add_task(&mut self, task: &str, data: &str) {
        let rest: String = data.chars().skip(1).collect();
        let pieces: Vec<&str> = rest.split(':').collect();
        let people = if pieces.len() == 1 {
            Vec::new()
        } else {
            pieces[1]
                .split(',')
                .map(|name| js::trim(name).to_string())
                .collect()
        };
        self.tasks.push(JourneyTask {
            task: task.to_string(),
            score: js::number(pieces[0]),
            people,
            section: self.current_section.clone(),
        });
    }

    pub fn actors(&self) -> Vec<String> {
        journey_actors(&self.tasks)
    }
}
