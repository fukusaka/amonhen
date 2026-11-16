//! Repository Settings の画面（GTK4 版の `reposettings.rs`、macOS 版の `RepositorySettings.swift` のうち、Author
//! と Keys and Values）。Author はこのリポジトリ（`--local`）の名前とメール。Keys and Values は git が読む設定の
//! キー・効いている値・設定元で、このリポジトリの段に足す・消す（GTK4 版の `configtable.rs` の Customized に当たる
//! 並び）。ほかのページ（Commits・Operations・Branches・Remotes・Hooks など）はまだない。

use amonhen_model::tr;
use serde_json::Value;
use windows_reactor::*;

/// `configList` の 1 項目。
#[derive(Clone)]
pub struct Entry {
    pub scope: String,
    pub key: String,
    pub value: String,
}

pub fn entries_of(info: &Value) -> Vec<Entry> {
    info["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| Entry {
            scope: e["scope"].as_str().unwrap_or_default().to_owned(),
            key: e["key"].as_str().unwrap_or_default().to_owned(),
            value: e["value"].as_str().unwrap_or_default().to_owned(),
        })
        .collect()
}

pub struct State {
    /// 読むまでは None。後のものほど優先（git と同じ）。
    pub entries: Option<Vec<Entry>>,
    /// Author の欄（このリポジトリの段の名前とメール）。
    pub name: String,
    pub email: String,
    /// 探す文字と、足すキーと値。
    pub query: String,
    pub new_key: String,
    pub new_value: String,
    /// `.git/info/exclude` の中身（読むまでは None）と、読んだ・書いた中身、`.gitignore`（書くときに一緒に渡す）。
    pub exclude: Option<String>,
    pub exclude_saved: String,
    pub gitignore: String,
}

impl State {
    pub fn new() -> Self {
        Self {
            entries: None,
            name: String::new(),
            email: String::new(),
            query: String::new(),
            new_key: String::new(),
            new_value: String::new(),
            exclude: None,
            exclude_saved: String::new(),
            gitignore: String::new(),
        }
    }

    /// `key` のこの段（`scope`）の値。
    pub fn at(&self, scope: &str, key: &str) -> Option<&str> {
        self.entries.as_ref()?.iter().rev().find_map(|e| {
            (e.scope == scope && e.key.eq_ignore_ascii_case(key)).then_some(e.value.as_str())
        })
    }

    /// `key` の効いている値と、その段。
    pub fn effective(&self, key: &str) -> Option<(&str, &str)> {
        self.entries
            .as_ref()?
            .iter()
            .rev()
            .find(|e| e.key.eq_ignore_ascii_case(key))
            .map(|e| (e.value.as_str(), e.scope.as_str()))
    }

    /// 読んだ値を Author の欄に入れる。
    pub fn load(&mut self, entries: Vec<Entry>) {
        self.entries = Some(entries);
        self.name = self.at("local", "user.name").unwrap_or_default().to_owned();
        self.email = self
            .at("local", "user.email")
            .unwrap_or_default()
            .to_owned();
    }
}

pub fn scope_label(scope: &str) -> String {
    match scope {
        "local" => tr!("this repository"),
        "global" => tr!("all repositories"),
        "worktree" => tr!("this worktree"),
        "system" => tr!("system-wide"),
        other => other.to_owned(),
    }
}

/// Operations の行（GTK4 版の `operations_page`）: (id, 見出し, 説明, キー, 選べる値と言葉, 値がないときの値)。
/// pull は `pull.rebase` と `pull.ff` の 2 つで決まるので、別に扱う（`pull_method`）。
pub type OperationRow = (
    &'static str,
    String,
    String,
    &'static str,
    Vec<(&'static str, String)>,
    &'static str,
);

pub fn operation_rows() -> Vec<OperationRow> {
    let on_off = || vec![("true", tr!("On")), ("false", tr!("Off"))];
    vec![
        (
            "fetch.prune",
            tr!("Remove remote-tracking branches deleted on the remote"),
            String::new(),
            "fetch.prune",
            on_off(),
            "false",
        ),
        (
            "rebase.autoStash",
            tr!("Stash changes before rebasing"),
            tr!("History edits always stash, whatever this says."),
            "rebase.autoStash",
            on_off(),
            "false",
        ),
        (
            "rebase.updateRefs",
            tr!("Move stacked branches along when rebasing or editing History"),
            tr!("Branches whose tips are among the rewritten commits. Undo puts them back."),
            "rebase.updateRefs",
            vec![
                ("false", tr!("Leave them")),
                ("true", tr!("Move them along")),
            ],
            "false",
        ),
        (
            "merge.conflictStyle",
            tr!("How conflicts are written in files"),
            tr!(
                "With the base (diff3, zdiff3), the merge editor resolves one-sided conflicts by itself."
            ),
            "merge.conflictStyle",
            vec![
                ("merge", tr!("Both sides")),
                ("diff3", tr!("Both sides and the common base")),
                ("zdiff3", tr!("Both sides and the base, shortened")),
            ],
            "merge",
        ),
        (
            "rerere.enabled",
            tr!("Reuse how conflicts were resolved before"),
            tr!(
                "When the same conflict comes back, git puts the earlier resolution in the file (rerere)."
            ),
            "rerere.enabled",
            on_off(),
            "false",
        ),
        (
            "rerere.autoUpdate",
            tr!("Stage files resolved that way"),
            tr!("Only while the switch above is on."),
            "rerere.autoUpdate",
            on_off(),
            "false",
        ),
    ]
}

/// Working Copy の行（GTK4 版の `working_copy_page`。形は `operation_rows` と同じ）。
pub fn working_copy_rows() -> Vec<OperationRow> {
    vec![
        (
            "core.autocrlf",
            tr!("Line endings"),
            String::new(),
            "core.autocrlf",
            vec![
                ("false", tr!("Keep as committed")),
                ("true", tr!("CRLF in the working tree, LF in commits")),
                ("input", tr!("LF in commits, no change on checkout")),
            ],
            "false",
        ),
        (
            "core.fileMode",
            tr!("Track changes to the executable bit"),
            String::new(),
            "core.fileMode",
            vec![("true", tr!("On")), ("false", tr!("Off"))],
            "true",
        ),
    ]
}

/// 改行を LF に揃える（WinUI の TextBox は `\r` で返す）。
pub fn lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// git の真偽値を `true`・`false` に揃える（`yes`・`on`・`1` も真）。
fn normalize_bool(value: &str) -> String {
    match value.to_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => "true".to_owned(),
        "false" | "no" | "off" | "0" | "" => "false".to_owned(),
        other => other.to_owned(),
    }
}

/// pull の取り込み方（`merge`・`rebase`・`ffOnly`。GTK4 版の `pull_row` の `method`）。
pub fn pull_method(state: &State) -> &'static str {
    if state
        .effective("pull.ff")
        .is_some_and(|(v, _)| v.eq_ignore_ascii_case("only"))
    {
        return "ffOnly";
    }
    match state
        .effective("pull.rebase")
        .map(|(v, _)| v.to_lowercase())
    {
        Some(r) if ["true", "yes", "on", "1", "merges", "interactive"].contains(&r.as_str()) => {
            "rebase"
        }
        _ => "merge",
    }
}

/// pull の取り込み方を `method` にする命令（GTK4 版と同じく、`pull.rebase` をいつも書いて曖昧さをなくす）。
pub fn pull_commands(state: &State, method: &str) -> Vec<Value> {
    let set = |key: &str, value: Option<&str>| serde_json::json!({ "op": "configSet", "key": key, "value": value });
    let mut commands = Vec::new();
    if state
        .at("local", "pull.ff")
        .is_some_and(|v| v.eq_ignore_ascii_case("only"))
        && method != "ffOnly"
    {
        commands.push(set("pull.ff", None));
    }
    match method {
        "rebase" => commands.push(set("pull.rebase", Some("true"))),
        "ffOnly" => {
            commands.push(set("pull.rebase", Some("false")));
            commands.push(set("pull.ff", Some("only")));
        }
        _ => commands.push(set("pull.rebase", Some("false"))),
    }
    commands
}

pub struct Callbacks<'a> {
    /// 欄（`name`・`email`・`query`・`key`・`value`）。
    pub field: Box<dyn Fn(&'static str) -> Callback<String> + 'a>,
    /// ボタン（`author`・`add`・`remove:<key>`・`close`）。
    pub button: Box<dyn Fn(String) -> Callback<()> + 'a>,
    /// Operations の選ぶ欄（行の id と、選んだ番号）。
    pub choice: Box<dyn Fn(&'static str) -> Callback<Option<usize>> + 'a>,
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let head = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .margin(Thickness::new(10.0, 6.0, 8.0, 6.0))
        .grid_row(0)
        .children((
            TextBlock::new()
                .text(tr!("Repository Settings"))
                .font_size(14.0)
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(0),
            Button::new()
                .on_click((callbacks.button)("close".to_owned()))
                .grid_column(1)
                .content(TextBlock::new().text(tr!("Close"))),
        ));
    let Some(entries) = &state.entries else {
        return Grid::new()
            .rows([GridLength::Auto, GridLength::Star(1.0)])
            .children((
                head,
                TextBlock::new()
                    .text(tr!("Loading…"))
                    .opacity(0.6)
                    .margin(16.0)
                    .grid_row(1),
            ));
    };
    let heading = |text: String| {
        TextBlock::new()
            .text(text)
            .font_size(15.0)
            .margin(Thickness::new(0.0, 16.0, 0.0, 6.0))
    };
    // Author: 欄が空なら、効いている値（すべてのリポジトリの段など）を薄く出す。
    let field = |label: String, key: &'static str, field: &'static str, value: &str| {
        let inherited = state
            .effective(key)
            .filter(|(_, scope)| *scope != "local")
            .map(|(v, s)| format!("{v} ({})", scope_label(s)))
            .unwrap_or_default();
        StackPanel::new().spacing(2.0).children((
            TextBlock::new().text(label).font_size(12.0).opacity(0.75),
            TextBox::new()
                .text(value)
                .placeholder_text(inherited)
                .on_text_changed((callbacks.field)(field)),
        ))
    };
    // Operations: 行ごとに選ぶ欄と、効いている値の設定元（このリポジトリで上書きしていなければ）。
    let choice_row = |id: &'static str,
                      title: String,
                      help: String,
                      keys: &[&str],
                      labels: Vec<String>,
                      selected: usize| {
        let source = keys
            .iter()
            .find_map(|k| state.effective(k))
            .map(|(_, scope)| scope_label(scope))
            .unwrap_or_else(|| tr!("git’s default"));
        StackPanel::new().spacing(2.0).children((
            TextBlock::new()
                .text(title)
                .text_wrapping(TextWrapping::Wrap),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .children((
                    ComboBox::new()
                        .items_source(labels)
                        .selected_index(selected)
                        .on_selection_changed((callbacks.choice)(id)),
                    TextBlock::new()
                        .text(source)
                        .font_size(12.0)
                        .opacity(0.6)
                        .vertical_alignment(VerticalAlignment::Center),
                )),
            if help.is_empty() {
                View::empty()
            } else {
                TextBlock::new()
                    .text(help)
                    .font_size(12.0)
                    .opacity(0.65)
                    .text_wrapping(TextWrapping::Wrap)
                    .into()
            },
        ))
    };
    let pull = ["merge", "rebase", "ffOnly"];
    let mut operations = vec![(
        "pull".to_owned(),
        choice_row(
            "pull",
            tr!("When pulling, how are the changes brought in?"),
            String::new(),
            &["pull.ff", "pull.rebase"],
            vec![tr!("Merge"), tr!("Rebase"), tr!("Fast-forward")],
            pull.iter()
                .position(|m| *m == pull_method(state))
                .unwrap_or(0),
        ),
    )];
    let row_of = |(id, title, help, key, options, default): OperationRow| {
        let value = state
            .effective(key)
            .map_or_else(|| default.to_owned(), |(v, _)| v.to_owned());
        let value = if options.iter().any(|(v, _)| *v == "true") {
            normalize_bool(&value)
        } else {
            value.to_lowercase()
        };
        let selected = options.iter().position(|(v, _)| *v == value).unwrap_or(0);
        (
            id.to_owned(),
            choice_row(
                id,
                title,
                help,
                &[key],
                options.into_iter().map(|(_, l)| l).collect(),
                selected,
            ),
        )
    };
    operations.extend(operation_rows().into_iter().map(row_of));
    let working_copy: Vec<(String, View)> = working_copy_rows().into_iter().map(row_of).collect();
    // このリポジトリだけで無視するパターン（.git/info/exclude）。
    let exclude: View = match &state.exclude {
        None => TextBlock::new().text(tr!("Loading…")).opacity(0.6).into(),
        Some(text) => {
            let dirty = lf(text) != state.exclude_saved;
            StackPanel::new().spacing(6.0).children((
                TextBlock::new()
                    .text(tr!(
                        ".git/info/exclude, one pattern per line. Shared patterns go in .gitignore."
                    ))
                    .font_size(12.0)
                    .opacity(0.65)
                    .text_wrapping(TextWrapping::Wrap),
                TextBox::new()
                    .text(text.clone())
                    .accepts_return(true)
                    .min_height(140.0)
                    .on_text_changed((callbacks.field)("exclude")),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .horizontal_alignment(HorizontalAlignment::Right)
                    .children((
                        Button::new()
                            .is_enabled(dirty)
                            .on_click((callbacks.button)("exclude-revert".to_owned()))
                            .content(TextBlock::new().text(tr!("Revert"))),
                        Button::new()
                            .is_enabled(dirty)
                            .on_click((callbacks.button)("exclude-save".to_owned()))
                            .content(TextBlock::new().text(tr!("Save"))),
                    )),
            ))
        }
    };
    let author_changed = state.at("local", "user.name").unwrap_or_default() != state.name
        || state.at("local", "user.email").unwrap_or_default() != state.email;
    // Keys and Values: キーごとに効いている値（最後のもの）。探す文字があれば、キーか値に含むものだけ。
    let query = state.query.to_lowercase();
    let mut keys: Vec<&Entry> = Vec::new();
    for e in entries.iter().rev() {
        if !keys.iter().any(|k| k.key.eq_ignore_ascii_case(&e.key))
            && (query.is_empty()
                || e.key.to_lowercase().contains(&query)
                || e.value.to_lowercase().contains(&query))
        {
            keys.push(e);
        }
    }
    keys.sort_by_key(|e| e.key.to_lowercase());
    let rows = keys.into_iter().map(|e| {
        let local = state.at("local", &e.key).is_some();
        let remove: View = if local {
            Button::new()
                .on_click((callbacks.button)(format!("remove:{}", e.key)))
                .content(TextBlock::new().text("−").font_size(12.0))
                .tooltip(tr!("Remove from This Repository"))
        } else {
            View::empty()
        };
        let row = Grid::new()
            .columns([
                GridLength::Pixel(260.0),
                GridLength::Star(1.0),
                GridLength::Pixel(150.0),
                GridLength::Pixel(40.0),
            ])
            .children((
                TextBlock::new()
                    .text(e.key.clone())
                    .font_size(12.0)
                    .text_trimming(TextTrimming::CharacterEllipsis)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(0),
                TextBlock::new()
                    .text(e.value.clone())
                    .font_size(12.0)
                    .is_text_selection_enabled(true)
                    .text_trimming(TextTrimming::CharacterEllipsis)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(1),
                TextBlock::new()
                    .text(scope_label(&e.scope))
                    .font_size(12.0)
                    .opacity(0.6)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(2),
                Border::new().grid_column(3).content(remove),
            ));
        (
            e.key.to_lowercase(),
            Border::new().min_height(28.0).content(row),
        )
    });
    let add = Grid::new()
        .columns([
            GridLength::Pixel(260.0),
            GridLength::Star(1.0),
            GridLength::Auto,
        ])
        .margin(Thickness::new(0.0, 8.0, 0.0, 0.0))
        .children((
            TextBox::new()
                .text(state.new_key.clone())
                .placeholder_text(tr!("Key"))
                .on_text_changed((callbacks.field)("key"))
                .grid_column(0),
            TextBox::new()
                .text(state.new_value.clone())
                .placeholder_text(tr!("Value"))
                .on_text_changed((callbacks.field)("value"))
                .grid_column(1),
            Button::new()
                .is_enabled(state.new_key.trim().contains('.'))
                .on_click((callbacks.button)("add".to_owned()))
                .grid_column(2)
                .content(TextBlock::new().text("+"))
                .tooltip(tr!("Add a key to this repository")),
        ));
    let body = ScrollViewer::new().grid_row(1).content(
        StackPanel::new()
            .margin(Thickness::new(24.0, 0.0, 24.0, 24.0))
            .children((
                heading(tr!("Author")),
                StackPanel::new()
                    .max_width(560.0)
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .spacing(8.0)
                    .children((
                        field(tr!("Name"), "user.name", "name", &state.name),
                        field(tr!("Email"), "user.email", "email", &state.email),
                        Button::new()
                            .is_enabled(author_changed)
                            .on_click((callbacks.button)("author".to_owned()))
                            .content(TextBlock::new().text(tr!("Save"))),
                    )),
                heading("Operations".to_owned()),
                StackPanel::new()
                    .max_width(560.0)
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .spacing(12.0)
                    .children((View::keyed_fragment(operations),)),
                heading("Working Copy".to_owned()),
                StackPanel::new()
                    .max_width(560.0)
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .spacing(12.0)
                    .children((View::keyed_fragment(working_copy),)),
                heading(tr!("Ignore in this repository only")),
                StackPanel::new()
                    .max_width(560.0)
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .children((exclude,)),
                heading(tr!("Keys and Values")),
                TextBox::new()
                    .text(state.query.clone())
                    .placeholder_text(tr!("Search by key or value"))
                    .on_text_changed((callbacks.field)("query")),
                StackPanel::new()
                    .margin(Thickness::new(0.0, 6.0, 0.0, 0.0))
                    .children((View::keyed_fragment(rows),)),
                add,
            )),
    );
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .children((head, body))
}
