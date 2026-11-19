//! 共有のコミットのメッセージのテンプレートの一覧（GTK4 版の `templates.rs`、macOS 版の `TemplateLibrary`）。
//! この機械のもので、設定（`windows.json`）の `commitTemplates` に JSON（id・name・text の並び。GTK4 版・macOS 版と
//! 同じ形）で置く。Settings > Git で編集し、リポジトリは Repository Settings の Commits で 1 つを選ぶ
//! （`amonhen.commitTemplate` に名前を書く。Amon Hen のコミット欄だけが使う）。

use serde_json::{Value, json};

/// 設定の鍵（GTK4 版・macOS 版と同じ）。
pub const KEY: &str = "commitTemplates";

/// リポジトリが選んだ共有のテンプレートの名前を置く git の設定のキー。
pub const LIBRARY_KEY: &str = "amonhen.commitTemplate";

/// テンプレートの 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub text: String,
}

/// 一覧（並べた順）。
pub fn all() -> Vec<Template> {
    crate::prefs::string(KEY)
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|t| Template {
            id: t["id"].as_str().unwrap_or_default().to_owned(),
            name: t["name"].as_str().unwrap_or_default().to_owned(),
            text: t["text"].as_str().unwrap_or_default().to_owned(),
        })
        .collect()
}

/// 一覧を書く。
pub fn save(templates: &[Template]) {
    let list: Vec<Value> = templates
        .iter()
        .map(|t| json!({ "id": t.id, "name": t.name, "text": t.text }))
        .collect();
    let _ = crate::prefs::set_string(KEY, &Value::Array(list).to_string());
}

/// 一覧の名前（空の名前は除く。同じ名前が 2 つあれば最初のもの）。
pub fn names() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for t in all() {
        if !t.name.is_empty() && !names.contains(&t.name) {
            names.push(t.name);
        }
    }
    names
}

/// 新しいテンプレートの id（時刻から。GTK4 版は UUID）。
pub fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{nanos:x}")
}

/// 設定の段の近さ（リポジトリに近いほど大きい。GTK4 版の `rank`）。
fn rank(scope: &str) -> u8 {
    match scope {
        "global" => 1,
        "local" => 2,
        "worktree" => 3,
        _ => 0,
    }
}

/// 共有のテンプレートが `commit.template` のファイルに譲るか: ファイルのほうがリポジトリに近い段で決まっていれば
/// （GTK4 版の `shared_yields`）。同じ段なら共有のもの。
pub fn shared_yields(shared: &str, file: Option<&str>) -> bool {
    file.is_some_and(|f| rank(f) > rank(shared))
}
