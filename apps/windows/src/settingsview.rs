//! Settings の画面（GTK4 版の設定の窓のうち、Windows 版に機能のあるもの）。Git は、すべてのリポジトリの段
//! （`--global`）の名前とメールと、Operations・Working Copy・Keys and Values（Repository Settings と同じ部品）。GTK4 版・macOS 版は別の窓とタブにするが、ここではまだ History と右の列の
//! 場所に重ねる。

use amonhen_model::tr;
use windows_reactor::*;

pub struct State {
    /// `--global` の user.name・user.email（読んだものと、欄の今の値）。読むまでは None。
    pub git: Option<[(String, String); 2]>,
    /// システムとすべてのリポジトリの段の設定（Operations・Working Copy の行。読むまでは None）。
    pub global: Option<crate::reposettings::State>,
    /// Git > Programs（読むまでは None）。
    pub programs: Option<crate::programs::State>,
    pub auto_magic_merge: bool,
    pub file_history_merges: bool,
    /// 開いたリポジトリをブックマークに入れる。
    pub auto_bookmark: bool,
    /// 自動の fetch の間隔（分。0 はしない）。
    pub auto_fetch: u32,
    pub write_commit_graph: bool,
    pub use_fsmonitor: bool,
    /// History のキャッシュの大きさ（バイト。読むまでは None）。
    pub cache_size: Option<u64>,
    /// 外のエディタの選べるもの（自動・Other… を除く）と、選んでいる値（空なら自動）。選び直すたびに欄を
    /// 作り直す数（Other… を取り消したとき、欄を元の選びに戻す）。
    pub editors: Vec<crate::editor::App>,
    pub editor: String,
    pub editor_epoch: u32,
    /// このプロセスの環境変数（名前の順）、渡すと決めた名前、探す文字（Git > Environment Variables）。
    pub env: Vec<(String, String)>,
    pub env_added: Vec<String>,
    pub env_query: String,
    /// 共有のコミットのメッセージのテンプレート（Git > Commits の Shared Templates）。
    pub templates: Vec<crate::templates::Template>,
}

/// 足した名前の設定の鍵（GTK4 版・macOS 版と同じ）。
pub const ENV_ADDED: &str = "addedEnvironmentNames";

/// 足した名前（渡せないものは除く。GTK4 版の `environment::added_names`）。
pub fn env_added_names() -> Vec<String> {
    crate::prefs::string(ENV_ADDED)
        .unwrap_or_default()
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|n| !n.is_empty() && amonhen_model::process::env_refusal(n).is_none())
        .map(str::to_owned)
        .collect()
}

/// 足した名前を `AMONHEN_PASS_ENV` に置いて、このアプリが動かす git と ssh に渡す（起動の初めと、選び直したとき）。
pub fn apply_env_added() {
    let mut names = env_added_names();
    names.sort();
    names.dedup();
    // SAFETY: 起動の初めは、まだほかのスレッドがない。選び直したときは GTK4 版と同じく動いている間に変える。環境を
    // 読むのはコア（std の `env::var`・`Command`）で、std の中の鍵で守られている。
    unsafe {
        if names.is_empty() {
            std::env::remove_var(amonhen_model::process::PASS_ENV);
        } else {
            std::env::set_var(amonhen_model::process::PASS_ENV, names.join(","));
        }
    }
}

/// トークン・パスワード・鍵らしい名前か（値を伏せる。GTK4 版の `environment::is_secret`）。
pub fn is_secret(name: &str) -> bool {
    let upper = name.to_uppercase();
    if upper == "SSH_AUTH_SOCK" || upper == amonhen_model::process::PASS_ENV {
        return false;
    }
    upper
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .any(|word| {
            [
                "TOKEN",
                "SECRET",
                "PASSWORD",
                "PASSWD",
                "CREDENTIAL",
                "PRIVATE",
            ]
            .iter()
            .any(|c| word.contains(c))
                || word == "AUTH"
                || word == "PASS"
                || word.ends_with("KEY")
        })
}

impl State {
    /// 外のエディタの欄の項目（自動・入っているもの・Other…）と、選んでいる番号。
    pub fn editor_items(&self) -> (Vec<String>, usize) {
        let mut items = vec![tr!(
            "Automatic ({label})",
            label = crate::editor::automatic().name
        )];
        items.extend(self.editors.iter().map(|a| a.name.clone()));
        items.push(tr!("Other…"));
        let selected = if self.editor.is_empty() {
            0
        } else {
            self.editors
                .iter()
                .position(|a| a.id.eq_ignore_ascii_case(&self.editor))
                .map_or(0, |i| i + 1)
        };
        (items, selected)
    }
}

/// 外のエディタの選べるもの（入っているものと、Other… で選んだもの）。
pub fn editor_choices(value: &str) -> Vec<crate::editor::App> {
    let mut apps = crate::editor::installed();
    if !value.is_empty()
        && !apps.iter().any(|a| a.id.eq_ignore_ascii_case(value))
        && let Some(app) = crate::editor::app_of(value)
    {
        apps.push(app);
    }
    apps
}

/// 自動の fetch の間隔の選べる値（GTK4 版・macOS 版と同じ）。
pub const AUTO_FETCH_CHOICES: [u32; 5] = [0, 5, 15, 30, 60];

pub struct Callbacks<'a> {
    pub field: Box<dyn Fn(usize) -> Callback<String> + 'a>,
    pub save_git: Callback<()>,
    /// Git の Operations・Working Copy の選ぶ欄（行の id と、選んだ番号。すべてのリポジトリの段に書く）。
    pub git_choice: Box<dyn Fn(&'static str) -> Callback<Option<usize>> + 'a>,
    /// Git の Keys and Values の欄（`query`・`key`・`value`）とボタン（`add`・`remove:<key>`）。すべてのリポジトリの段。
    pub keys_field: Box<dyn Fn(&'static str) -> Callback<String> + 'a>,
    pub keys_button: Box<dyn Fn(String) -> Callback<()> + 'a>,
    pub toggle: Box<dyn Fn(&'static str) -> Callback<bool> + 'a>,
    pub close: Callback<()>,
    /// 自動の fetch の間隔を選んだ（`AUTO_FETCH_CHOICES` の番号）。
    pub auto_fetch: Callback<Option<usize>>,
    /// History のキャッシュを消す。
    pub clear_cache: Callback<()>,
    /// 外のエディタを選んだ（[`State::editor_items`] の番号）。
    pub editor: Callback<Option<usize>>,
    /// 環境変数を渡すかを切り替えた（名前ごと）。
    pub env: Box<dyn Fn(String) -> Callback<bool> + 'a>,
    /// 環境変数を探す文字。
    pub env_query: Callback<String>,
    /// 共有のテンプレート: 名前（0）・本文（1）の書き換え、足す、外す。
    pub template: Box<dyn Fn(usize, usize) -> Callback<String> + 'a>,
    pub template_add: Callback<()>,
    pub template_remove: Box<dyn Fn(usize) -> Callback<()> + 'a>,
    /// Appearance: History の見せ方を切り替えた（View の項目の id と、チェックの値）。
    pub display: Box<dyn Fn(&'static str) -> Callback<bool> + 'a>,
    /// Appearance: History の並び順を選んだ（0 は日付順、1 はトポロジー順）。
    pub order: Callback<Option<usize>>,
    /// Theme: 構文の色のテーマを選んだ（`theme::names` の番号）。
    pub syntax_theme: Callback<Option<usize>>,
    /// Theme のボタン（`import`・`remove:<name>`）。
    pub theme_button: Box<dyn Fn(String) -> Callback<()> + 'a>,
}

/// Git > Operations・Working Copy（すべてのリポジトリの段。Repository Settings と同じ行で、効いている値の設定元を
/// 添える。GTK4 版・macOS 版の Settings > Git の同じページ）。
fn git_choices(state: &State, callbacks: &Callbacks) -> View {
    let Some(global) = &state.global else {
        return View::empty();
    };
    let (operations, working_copy, author) = crate::reposettings::choice_views(
        global,
        "global",
        &*callbacks.git_choice,
        &*callbacks.keys_button,
    );
    let (signing, mailmap): (crate::reposettings::Rows, crate::reposettings::Rows) = author
        .into_iter()
        .partition(|(k, _)| !crate::reposettings::is_mailmap_row(k));
    let title = |text: &str| {
        TextBlock::new()
            .text(text.to_owned())
            .font_weight(FontWeight::SEMI_BOLD)
            .margin(Thickness::new(0.0, 14.0, 0.0, 4.0))
    };
    StackPanel::new().spacing(8.0).children((
        title(&tr!("Signing")),
        StackPanel::new().spacing(10.0).children((
            View::keyed_fragment(signing),
            crate::reposettings::signing_other_view(
                global,
                &*callbacks.keys_field,
                &*callbacks.keys_button,
            ),
        )),
        title("mailmap"),
        StackPanel::new()
            .spacing(10.0)
            .children((View::keyed_fragment(mailmap),)),
        title("Operations"),
        StackPanel::new()
            .spacing(10.0)
            .children((View::keyed_fragment(operations),)),
        title("Working Copy"),
        StackPanel::new()
            .spacing(10.0)
            .children((View::keyed_fragment(working_copy),)),
        title(&tr!("Programs")),
        crate::programs::view(state.programs.as_ref(), global, &*callbacks.git_choice),
        title("Keys and Values"),
        crate::reposettings::keys_view(
            global,
            "global",
            &*callbacks.keys_field,
            &*callbacks.keys_button,
            &*callbacks.git_choice,
        ),
    ))
}

/// Appearance（GTK4 版の Appearance のタブの History と Items。CI の状態はこの FE にない）。値は View の
/// History の見せ方と同じもの（どちらで変えても同じ設定）。
fn appearance(callbacks: &Callbacks) -> View {
    use crate::render::items;
    let display = |id: &'static str, value: bool, label: String| {
        CheckBox::new()
            .is_checked(value)
            .on_is_checked_changed((callbacks.display)(id))
            .content(TextBlock::new().text(label))
    };
    let shown = crate::render::history_items();
    let item = |id: &'static str, bit: u32, label: &'static str| {
        display(
            id,
            shown & bit != 0,
            amonhen_model::i18n::translate(label).into_owned(),
        )
    };
    StackPanel::new().children((
        heading(tr!("Appearance")),
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(12.0)
            .children((
                TextBlock::new()
                    .text(tr!("Order"))
                    .vertical_alignment(VerticalAlignment::Center),
                ComboBox::new()
                    .items_source(vec![tr!("Date Order"), tr!("Topological Order")])
                    .selected_index(usize::from(
                        crate::prefs::history_order() == amonhen_model::HistoryOrder::Topo,
                    ))
                    .on_selection_changed(callbacks.order.clone()),
            )),
        display(
            "one-line-history",
            crate::render::one_line(),
            tr!("Show each commit on one line"),
        ),
        display(
            "highlight-ancestors",
            crate::render::highlight_ancestors(),
            tr!("Highlight the selected commit’s ancestors"),
        ),
        TextBlock::new()
            .text(tr!("Items"))
            .font_size(13.0)
            .opacity(0.8)
            .margin(Thickness::new(0.0, 12.0, 0.0, 0.0)),
        item("history-item-refs", items::REFS, "Branches and Tags"),
        item("history-item-author", items::AUTHOR, "Author"),
        item("history-item-date", items::DATE, "Date"),
        item("history-item-id", items::ID, "Commit ID"),
        item("history-item-signature", items::SIGNATURE, "Signature"),
        note(tr!("What each commit’s row shows besides the subject.")),
        // 子の数がタプルの上限を超えないよう、後半を 1 つにまとめる。
        StackPanel::new().children((
            // 構文と差分の色（GTK4 版の Theme の Color Theme。この FE は明るい外観だけ）。
            heading(tr!("Color Theme")),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(12.0)
                .children((
                    TextBlock::new()
                        .text(tr!("Light appearance"))
                        .vertical_alignment(VerticalAlignment::Center),
                    {
                        let names = crate::theme::names();
                        let selected = crate::theme::selected();
                        ComboBox::new()
                            .selected_index(names.iter().position(|n| *n == selected).unwrap_or(0))
                            .items_source(names)
                            .on_selection_changed(callbacks.syntax_theme.clone())
                    },
                )),
            StackPanel::new()
                .spacing(4.0)
                .children((View::keyed_fragment(
                    crate::theme::imported()
                        .into_iter()
                        .map(|t| {
                            (
                                format!("imported:{}", t.name),
                                StackPanel::new()
                                    .orientation(Orientation::Horizontal)
                                    .spacing(8.0)
                                    .children((
                                        TextBlock::new()
                                            .text(t.name.clone())
                                            .vertical_alignment(VerticalAlignment::Center),
                                        TextBlock::new()
                                            .text(if t.dark { tr!("Dark") } else { tr!("Light") })
                                            .opacity(0.6)
                                            .vertical_alignment(VerticalAlignment::Center),
                                        Button::new()
                                            .on_click((callbacks.theme_button)(format!(
                                                "remove:{}",
                                                t.name
                                            )))
                                            .content(TextBlock::new().text(tr!("Remove"))),
                                    )),
                            )
                        })
                        .collect::<Vec<_>>(),
                ),)),
            Button::new()
                .on_click((callbacks.theme_button)("import".to_owned()))
                .content(TextBlock::new().text(tr!("Import VSCode Theme…"))),
            display(
                "changed-files-as-tree",
                crate::prefs::bool(crate::prefs::HISTORY_FILE_TREE),
                tr!("Show changed files as a tree left of the diff"),
            ),
            display(
                "diff-standard-colors",
                crate::prefs::bool(crate::theme::DIFF_STANDARD_COLORS),
                tr!("Use standard colors for diffs"),
            ),
        )),
    ))
}

/// バイト数を読みやすく。
fn size(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64;
    let mut unit = "";
    for u in ["kB", "MB", "GB", "TB"] {
        value /= 1000.0;
        unit = u;
        if value < 1000.0 {
            break;
        }
    }
    format!("{value:.1} {unit}")
}

fn heading(text: String) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_size(15.0)
        .margin(Thickness::new(0.0, 16.0, 0.0, 6.0))
}

fn note(text: String) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_size(12.0)
        .opacity(0.65)
        .text_wrapping(TextWrapping::Wrap)
}

/// Git > Environment Variables（GTK4 版の `environment::page` の、この機械の表）: このプロセスの環境変数を値と一緒に
/// 並べ、git と ssh に渡すものをチェックで選ぶ。いつも渡すもの（`ALWAYS_PASSED` と `GIT_*`）は外せず、渡せない
/// もの（`env_refusal`）は選べない。トークンらしい値は伏せ、伏せた値では探さない。
fn environment(state: &State, callbacks: &Callbacks) -> View {
    use amonhen_model::process::{ALWAYS_PASSED, always_passed, env_refusal};
    let query = state.env_query.to_lowercase();
    let rows = state
        .env
        .iter()
        .filter(|(name, value)| {
            query.is_empty()
                || name.to_lowercase().contains(&query)
                || (!is_secret(name) && value.to_lowercase().contains(&query))
        })
        .map(|(name, value)| {
            let refusal = env_refusal(name);
            let always = always_passed(name);
            let added = state.env_added.iter().any(|n| n.eq_ignore_ascii_case(name));
            let shown = if is_secret(name) {
                "••••••••".to_owned()
            } else {
                value.clone()
            };
            let check = CheckBox::new()
                .is_checked(always || added)
                .is_enabled(!always && refusal.is_none())
                .on_is_checked_changed((callbacks.env)(name.clone()))
                .content(TextBlock::new().text(name.clone()).font_size(12.0));
            let help = match (&refusal, always) {
                (Some(reason), _) => Some(reason.clone()),
                (None, true) => Some(tr!("git and ssh always get this")),
                _ => None,
            };
            let check: View = match help {
                Some(help) => check.tooltip(help),
                None => check,
            };
            (
                name.clone(),
                Grid::new()
                    .columns([GridLength::Pixel(220.0), GridLength::Star(1.0)])
                    .children((
                        Border::new().grid_column(0).content(check),
                        TextBlock::new()
                            .text(shown)
                            .font_size(12.0)
                            .opacity(0.7)
                            .is_text_selection_enabled(!is_secret(name))
                            .text_trimming(TextTrimming::CharacterEllipsis)
                            .vertical_alignment(VerticalAlignment::Center)
                            .grid_column(1),
                    )),
            )
        });
    let mut always: Vec<&str> = ALWAYS_PASSED.to_vec();
    always.push("GIT_*");
    StackPanel::new()
        .spacing(4.0)
        .margin(Thickness::new(0.0, 12.0, 0.0, 0.0))
        .children((
            TextBlock::new().text(tr!("Environment Variables")),
            TextBox::new()
                .text(state.env_query.clone())
                .placeholder_text(tr!("Search by name or value"))
                .on_text_changed(callbacks.env_query.clone()),
            ScrollViewer::new()
                .max_height(260.0)
                .content(StackPanel::new().children((View::keyed_fragment(rows),))),
            note(tr!(
                "Checked variables go to the git and ssh this app runs, through the shared daemon too, but not to SSH hosts. Always passed, when the shell sets them: {names}.",
                names = always.join(", ")
            )),
        ))
}

/// Git > Commits の Shared Templates（GTK4 版の `templates::section`）: 名前と本文を並べ、＋で足し、−で外す。
/// リポジトリは Repository Settings の Commits で 1 つを選ぶ。
fn templates(state: &State, callbacks: &Callbacks) -> View {
    let rows = state.templates.iter().enumerate().map(|(i, t)| {
        (
            t.id.clone(),
            StackPanel::new()
                .spacing(4.0)
                .margin(Thickness::new(0.0, 0.0, 0.0, 8.0))
                .children((
                    Grid::new()
                        .columns([GridLength::Star(1.0), GridLength::Auto])
                        .column_spacing(6.0)
                        .children((
                            TextBox::new()
                                .text(t.name.clone())
                                .placeholder_text(tr!("Name"))
                                .on_text_changed((callbacks.template)(i, 0))
                                .grid_column(0),
                            Button::new()
                                .on_click((callbacks.template_remove)(i))
                                .grid_column(1)
                                .content(TextBlock::new().text("−")),
                        )),
                    TextBox::new()
                        .text(t.text.clone())
                        .placeholder_text(tr!("Message"))
                        .accepts_return(true)
                        .min_height(70.0)
                        .on_text_changed((callbacks.template)(i, 1)),
                )),
        )
    });
    StackPanel::new()
        .spacing(4.0)
        .margin(Thickness::new(0.0, 12.0, 0.0, 0.0))
        .children((
            TextBlock::new().text(tr!("Shared Templates")),
            View::keyed_fragment(rows),
            Button::new()
                .on_click(callbacks.template_add.clone())
                .content(TextBlock::new().text("+")),
        ))
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let appearance = appearance(&callbacks);
    let git_choices = git_choices(state, &callbacks);
    let env = environment(state, &callbacks);
    let shared_templates = templates(state, &callbacks);
    let head = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .margin(Thickness::new(10.0, 6.0, 8.0, 6.0))
        .grid_row(0)
        .children((
            TextBlock::new()
                .text("Settings")
                .font_size(14.0)
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(0),
            Button::new()
                .on_click(callbacks.close)
                .grid_column(1)
                .content(TextBlock::new().text(tr!("Close"))),
        ));
    let git: View = match &state.git {
        None => TextBlock::new().text(tr!("Loading…")).opacity(0.6).into(),
        Some(fields) => {
            let changed = fields.iter().any(|(read, now)| read != now);
            let field = |index: usize, label: String| {
                StackPanel::new().spacing(2.0).children((
                    TextBlock::new().text(label).font_size(12.0).opacity(0.75),
                    TextBox::new()
                        .text(fields[index].1.clone())
                        .on_text_changed((callbacks.field)(index)),
                ))
            };
            StackPanel::new().spacing(8.0).children((
                field(0, tr!("Name")),
                field(1, tr!("Email")),
                TextBlock::new()
                    .text("git config --global user.name / user.email")
                    .font_size(12.0)
                    .opacity(0.6),
                Button::new()
                    .is_enabled(changed)
                    .on_click(callbacks.save_git)
                    .content(TextBlock::new().text(tr!("Save"))),
            ))
        }
    };
    let check = |key: &'static str, value: bool, label: String| {
        CheckBox::new()
            .is_checked(value)
            .on_is_checked_changed((callbacks.toggle)(key))
            .content(TextBlock::new().text(label))
    };
    // General の終わり（GTK4 版の General のチェック）。
    let bookmark = check(
        crate::prefs::AUTO_BOOKMARK,
        state.auto_bookmark,
        tr!("Bookmark repositories you open"),
    );
    let bookmark = Border::new()
        .margin(Thickness::new(0.0, 8.0, 0.0, 0.0))
        .content(bookmark);
    let body = ScrollViewer::new().grid_row(1).content(
        StackPanel::new()
            .max_width(560.0)
            .horizontal_alignment(HorizontalAlignment::Left)
            .margin(Thickness::new(24.0, 0.0, 24.0, 24.0))
            .children((
                heading(tr!("General")),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(12.0)
                    .children((
                        TextBlock::new()
                            .text(tr!("Fetch automatically"))
                            .vertical_alignment(VerticalAlignment::Center),
                        ComboBox::new()
                            .items_source(
                                AUTO_FETCH_CHOICES
                                    .iter()
                                    .map(|&m| {
                                        if m == 0 {
                                            tr!("Never")
                                        } else {
                                            tr!("Every {count} minutes", count = m)
                                        }
                                    })
                                    .collect::<Vec<_>>(),
                            )
                            .selected_index(
                                AUTO_FETCH_CHOICES
                                    .iter()
                                    .position(|&m| m == state.auto_fetch)
                                    .unwrap_or(0),
                            )
                            .on_selection_changed(callbacks.auto_fetch),
                    )),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(12.0)
                    .margin(Thickness::new(0.0, 8.0, 0.0, 0.0))
                    .children((
                        TextBlock::new()
                            .text(tr!("External Editor"))
                            .vertical_alignment(VerticalAlignment::Center),
                        View::keyed_fragment([(state.editor_epoch.to_string(), {
                            let (items, selected) = state.editor_items();
                            ComboBox::new()
                                .items_source(items)
                                .selected_index(selected)
                                .on_selection_changed(callbacks.editor)
                        })]),
                    )),
                bookmark,
                heading("Git".to_owned()),
                StackPanel::new().children((git, git_choices, shared_templates, env)),
                appearance,
                heading(tr!("Diff & Merge")),
                check(
                    crate::prefs::AUTO_MAGIC_MERGE,
                    state.auto_magic_merge,
                    tr!("Resolve unambiguous conflicts when the merge editor opens"),
                ),
                StackPanel::new().children((
                    heading("File History".to_owned()),
                    check(
                        crate::prefs::FILE_HISTORY_MERGES,
                        state.file_history_merges,
                        tr!("Show merge commits"),
                    ),
                )),
                // Performance（GTK4 版の Performance のタブのうち、Windows 版にあるもの）。
                heading(tr!("Performance")),
                check(
                    crate::prefs::WRITE_COMMIT_GRAPH,
                    state.write_commit_graph,
                    tr!("Write commit-graph automatically"),
                ),
                note(tr!(
                    "Writes to .git, except in repositories git maintenance looks after."
                )),
                check(
                    crate::prefs::USE_FSMONITOR,
                    state.use_fsmonitor,
                    tr!("Speed up status with fsmonitor"),
                ),
                note(tr!("A git fsmonitor--daemon runs for each repository.")),
                // キャッシュ（GTK4 版の Caches の節。AI がないので History だけ）。
                StackPanel::new().children((
                    TextBlock::new()
                        .text(tr!("Caches"))
                        .font_size(13.0)
                        .opacity(0.8)
                        .margin(Thickness::new(0.0, 12.0, 0.0, 0.0)),
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(12.0)
                        .margin(Thickness::new(0.0, 4.0, 0.0, 0.0))
                        .children((
                            TextBlock::new()
                                .text(format!(
                                    "{}: {}",
                                    tr!("History"),
                                    state.cache_size.map(size).unwrap_or_else(|| "…".to_owned())
                                ))
                                .vertical_alignment(VerticalAlignment::Center),
                            Button::new()
                                .is_enabled(state.cache_size.is_some_and(|n| n > 0))
                                .on_click(callbacks.clear_cache)
                                .content(TextBlock::new().text(tr!("Clear"))),
                        )),
                    note(tr!(
                        "Opens History right away next time. Cleared, it is read from git again."
                    )),
                )),
            )),
    );
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .children((head, body))
}
