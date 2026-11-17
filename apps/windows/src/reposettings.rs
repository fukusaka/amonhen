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

pub struct Callbacks<'a> {
    /// 欄（`name`・`email`・`query`・`key`・`value`）。
    pub field: Box<dyn Fn(&'static str) -> Callback<String> + 'a>,
    /// ボタン（`author`・`add`・`remove:<key>`・`close`）。
    pub button: Box<dyn Fn(String) -> Callback<()> + 'a>,
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
