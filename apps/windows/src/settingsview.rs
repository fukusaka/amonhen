//! Settings の画面（GTK4 版の設定の窓のうち、Windows 版に機能のあるもの）。Git は、すべてのリポジトリの段
//! （`--global`）の名前とメールと、Operations・Working Copy・Keys and Values（Repository Settings と同じ部品）。GTK4 版・macOS 版は別の窓とタブにするが、ここではまだ History と右の列の
//! 場所に重ねる。

use amonhen_model::tr;
use windows_reactor::*;

pub struct State {
    /// 出しているページ（[`PAGES`] の何番目か。GTK4 版・macOS 版の Settings のタブ）。
    pub page: usize,
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
    pub open_through_daemon: bool,
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
    /// 一覧で選んだテンプレート（−・Edit… が使う）。
    pub template_selected: Option<usize>,
    /// SSH Hosts（ページを初めて出したときに作る。選んだ SSH 先につなぎに行くので、出すまではつながない）。
    pub ssh: Option<crate::sshhosts::State>,
    /// Git の部品（`git`・`global`・`programs`）が読み書きする先: この機械なら None、SSH Hosts の Git のページなら
    /// その SSH 先。変えるたびに `git_epoch` を増やし、前の先から遅れて届いた答えを捨てる。
    pub git_target: Option<String>,
    pub git_epoch: u64,
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

/// 窓の背景の横棒の値から、その操作を作る。
pub type BackdropNumber = fn(f64) -> crate::backdrop::Event;

/// 窓の背景の欄の操作（Settings > Appearance と、Repositories の「窓の背景…」のダイアログで同じもの）。
pub struct BackdropCallbacks<'a> {
    /// Choose…・Remove。
    pub button: Box<dyn Fn(crate::backdrop::Event) -> Callback<()> + 'a>,
    pub placement: Callback<Option<usize>>,
    pub style: Callback<Option<usize>>,
    /// 横棒（透過率・画像の透過率・ツールバーの色の濃さ）。
    pub number: Box<dyn Fn(BackdropNumber) -> Callback<f64> + 'a>,
    pub tint: Callback<bool>,
    pub color: Callback<Color>,
}

pub struct Callbacks<'a> {
    pub field: Box<dyn Fn(usize) -> Callback<String> + 'a>,
    pub save_git: Callback<()>,
    /// Git の Operations・Working Copy の選ぶ欄（行の id と、選んだ番号。すべてのリポジトリの段に書く）。
    pub git_choice: Box<dyn Fn(&'static str) -> Callback<Option<usize>> + 'a>,
    /// 左のページの一覧で選んだ。
    pub page: Callback<Option<usize>>,
    /// Programs の ↩︎（その行を自動に戻す）。
    pub program_revert: Box<dyn Fn(&'static str) -> Callback<()> + 'a>,
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
    /// 共有のテンプレート: 足す、外す（名前と本文はシートで書く）。
    pub template_add: Callback<()>,
    pub template_remove: Box<dyn Fn(usize) -> Callback<()> + 'a>,
    /// 一覧で選んだ、選んだものを編集する（シート）。
    pub template_select: Callback<Option<usize>>,
    pub template_edit: Callback<()>,
    /// Appearance: History の見せ方を切り替えた（View の項目の id と、チェックの値）。
    pub display: Box<dyn Fn(&'static str) -> Callback<bool> + 'a>,
    /// Accounts: CI を読むブランチ（0 は今のブランチと既定のブランチ、1 は今のブランチだけ）と、ブランチごとに読む
    /// コミットの数（[`GITHUB_COMMITS`] の番号）を選んだ。
    pub github_branches: Callback<Option<usize>>,
    pub github_commits: Callback<Option<usize>>,
    /// Appearance: History の並び順を選んだ（0 は日付順、1 はトポロジー順）。
    pub order: Callback<Option<usize>>,
    /// Theme: 構文の色のテーマを選んだ（`theme::names` の番号）。
    pub syntax_theme: Callback<Option<usize>>,
    pub syntax_theme_dark: Callback<Option<usize>>,
    /// Theme のボタン（`import`・`remove:<name>`）。
    pub theme_button: Box<dyn Fn(String) -> Callback<()> + 'a>,
    /// 窓の背景の欄（Choose…・Remove、位置、横棒、ツールバーの色を敷くか・色）。
    pub backdrop: BackdropCallbacks<'a>,
    pub ssh: crate::sshhosts::Callbacks<'a>,
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
        crate::programs::view(
            state.programs.as_ref(),
            global,
            &*callbacks.git_choice,
            &*callbacks.program_revert,
        ),
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

/// Appearance（GTK4 版の Appearance のタブの History・Items・Commit Details のアバター）。値は View の
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
        item("history-item-ci", items::CI, "CI Status"),
        note(tr!("What each commit’s row shows besides the subject.")),
        TextBlock::new()
            .text(tr!("Commit Details"))
            .font_size(13.0)
            .opacity(0.8)
            .margin(Thickness::new(0.0, 12.0, 0.0, 0.0)),
        CheckBox::new()
            .is_checked(crate::prefs::bool(crate::prefs::SHOW_AVATARS))
            .on_is_checked_changed((callbacks.toggle)(crate::prefs::SHOW_AVATARS))
            .content(TextBlock::new().text(tr!("Show avatars"))),
        TextBlock::new()
            .text(tr!(
                "Fetched from Gravatar (sends a hash of the email address to gravatar.com)."
            ))
            .font_size(12.0)
            .opacity(0.6)
            .text_wrapping(TextWrapping::Wrap),
        // 子の数がタプルの上限を超えないよう、後半を 1 つにまとめる。
        StackPanel::new().children((
            backdrop_section(&crate::backdrop::Backdrop::app(), &callbacks.backdrop),
            // 構文と差分の色（GTK4 版の Theme の Color Theme。外観ごとに 1 つ）。
            heading(tr!("Color Theme")),
            theme_choice(tr!("Light appearance"), false, &callbacks.syntax_theme),
            theme_choice(tr!("Dark appearance"), true, &callbacks.syntax_theme_dark),
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
/// Git > Commits の Shared Templates（macOS 版の `TemplateLibrarySection`）: 名前の一覧と ＋・−・Edit…。名前と本文は
/// シートで書く。リポジトリは Repository Settings の Commits で 1 つを選ぶ。
fn templates(state: &State, callbacks: &Callbacks) -> View {
    let rows = state.templates.iter().map(|t| {
        let name = if t.name.trim().is_empty() {
            tr!("Untitled")
        } else {
            t.name.clone()
        };
        (
            t.id.clone(),
            ListViewItem::new()
                .tag(t.id.clone())
                .content(TextBlock::new().text(name).font_size(13.0)),
        )
    });
    // 並びが変わるたびに一覧ごと作り直す（reactor の ListView は消した項目の場所が残る）。
    let key: String = state
        .templates
        .iter()
        .map(|t| format!("{}:{}\n", t.id, t.name))
        .collect();
    let selected = state
        .template_selected
        .filter(|&i| i < state.templates.len());
    StackPanel::new()
        .spacing(4.0)
        .margin(Thickness::new(0.0, 12.0, 0.0, 0.0))
        .children((
            TextBlock::new().text(tr!("Shared Templates")),
            View::keyed_fragment([(
                key,
                ListView::new()
                    .height(140.0)
                    .selected_index(selected)
                    .on_selection_changed(callbacks.template_select.clone())
                    .collection_slot(ListViewSlot::Items, rows),
            )]),
            Grid::new()
                .columns([
                    GridLength::Auto,
                    GridLength::Auto,
                    GridLength::Star(1.0),
                    GridLength::Auto,
                ])
                .column_spacing(6.0)
                .children((
                    Button::new()
                        .on_click(callbacks.template_add.clone())
                        .content(TextBlock::new().text("+")),
                    match selected {
                        Some(i) => Button::new()
                            .grid_column(1)
                            .on_click((callbacks.template_remove)(i))
                            .content(TextBlock::new().text("−")),
                        None => Button::new()
                            .grid_column(1)
                            .is_enabled(false)
                            .content(TextBlock::new().text("−")),
                    },
                    Button::new()
                        .grid_column(3)
                        .is_enabled(selected.is_some())
                        .on_click(callbacks.template_edit.clone())
                        .content(TextBlock::new().text(tr!("Edit…"))),
                )),
        ))
}

/// Git の名前とメール（Settings > Git が読み書きする先の `--global`）。
fn git_identity(state: &State, callbacks: &Callbacks) -> View {
    match &state.git {
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
                    .on_click(callbacks.save_git.clone())
                    .content(TextBlock::new().text(tr!("Save"))),
            ))
        }
    }
}

/// Settings > Accounts で選べる、ブランチごとに読むコミットの数。
pub const GITHUB_COMMITS: [u32; 3] = [100, 50, 30];

/// Settings > Accounts（GTK4 版の `accounts`、macOS 版の `AccountsSettings`）: GitHub から History に読むもの。
/// どちらも読まないなら、ブランチと数は選べない。アバターを使わないなら、1 回だけかも選べない。
fn accounts(callbacks: &Callbacks) -> View {
    use crate::prefs::{self, GITHUB_AVATARS, GITHUB_AVATARS_ONCE, GITHUB_CI_STATUS};
    let check = |key: &'static str, label: String, enabled: bool| {
        CheckBox::new()
            .is_checked(prefs::bool(key))
            .is_enabled(enabled)
            .on_is_checked_changed((callbacks.toggle)(key))
            .content(TextBlock::new().text(label))
    };
    let any = prefs::bool(GITHUB_CI_STATUS) || prefs::bool(GITHUB_AVATARS);
    let choice =
        |label: String, items: Vec<String>, selected: usize, on: Callback<Option<usize>>| {
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(12.0)
                .children((
                    TextBlock::new()
                        .text(label)
                        .vertical_alignment(VerticalAlignment::Center),
                    ComboBox::new()
                        .items_source(items)
                        .selected_index(selected)
                        .is_enabled(any)
                        .on_selection_changed(on),
                ))
        };
    StackPanel::new().spacing(8.0).children((
        heading(tr!("Accounts")),
        TextBlock::new()
            .text("GitHub")
            .font_weight(FontWeight::SEMI_BOLD),
        check(
            GITHUB_CI_STATUS,
            tr!("Show each commit’s CI status in History"),
            true,
        ),
        check(
            GITHUB_AVATARS,
            tr!("Use commit authors’ GitHub avatars"),
            true,
        ),
        choice(
            tr!("Branches to read CI for"),
            vec![
                tr!("Current and default branch"),
                tr!("Current branch only"),
            ],
            usize::from(!prefs::bool(prefs::GITHUB_CI_DEFAULT_BRANCH)),
            callbacks.github_branches.clone(),
        ),
        choice(
            tr!("Commits to read per branch"),
            GITHUB_COMMITS
                .iter()
                .map(|n| tr!("Last {count}", count = n))
                .collect(),
            GITHUB_COMMITS
                .iter()
                .position(|&n| n == prefs::github_ci_commits())
                .unwrap_or(0),
            callbacks.github_commits.clone(),
        ),
        check(
            GITHUB_AVATARS_ONCE,
            tr!("Read avatar URLs only once per window"),
            prefs::bool(GITHUB_AVATARS),
        ),
    ))
}

/// Settings のページ（左の一覧の見出し。訳は `tr!` と同じ表から）。
pub const PAGES: [&str; 7] = [
    "General",
    "Git",
    "SSH Hosts",
    "Accounts",
    "Appearance",
    "Diff & Merge",
    "Performance",
];

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let appearance = appearance(&callbacks);
    let accounts = accounts(&callbacks);
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
                .on_click(callbacks.close.clone())
                .grid_column(1)
                .content(TextBlock::new().text(tr!("Close"))),
        ));
    let git = git_identity(state, &callbacks);
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
    // ページ（GTK4 版・macOS 版のタブ。Windows 版にあるものだけ）。左の一覧で選んだものを出す。
    let ssh = match &state.ssh {
        Some(ssh) => {
            // SSH 先の Git のページ: Settings > Git の部品のうち、その SSH 先の `--global` に効くもの（読む先が
            // その SSH 先に変わって、読み終えたときだけ）。
            let git = (ssh.git_host().is_some()
                && state.git_target == ssh.git_host()
                && state.global.is_some())
            .then(|| {
                StackPanel::new().children((
                    git_identity(state, &callbacks),
                    crate::settingsview::git_choices(state, &callbacks),
                ))
            });
            crate::sshhosts::view(ssh, &callbacks.ssh, git)
        }
        None => View::empty(),
    };
    let pages: [View; 7] = [
        StackPanel::new().children((
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
        )),
        StackPanel::new().children((
            heading("Git".to_owned()),
            StackPanel::new().children((git, git_choices, shared_templates, env)),
        )),
        ssh,
        accounts,
        appearance,
        StackPanel::new().children((
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
        )),
        StackPanel::new().children((
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
            // Shared daemon（GTK4 版の Performance の節）。次に開くリポジトリから効く。
            TextBlock::new()
                .text(tr!("Shared daemon"))
                .font_size(13.0)
                .opacity(0.8)
                .margin(Thickness::new(0.0, 12.0, 0.0, 0.0)),
            check(
                crate::prefs::OPEN_THROUGH_DAEMON,
                state.open_through_daemon,
                tr!("Open repositories through the shared daemon (SSH hosts too)"),
            ),
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
    ];
    let page = state.page.min(PAGES.len() - 1);
    let Some(content) = pages.into_iter().nth(page) else {
        return View::empty();
    };
    let nav = ListView::new()
        .selected_index(Some(page))
        .on_selection_changed(callbacks.page)
        .collection_slot(
            ListViewSlot::Items,
            PAGES.iter().enumerate().map(|(i, title)| {
                (
                    format!("{i}"),
                    ListViewItem::new().content(
                        TextBlock::new().text(amonhen_model::i18n::translate(title).into_owned()),
                    ),
                )
            }),
        );
    let body = Grid::new()
        .columns([GridLength::Pixel(170.0), GridLength::Star(1.0)])
        .grid_row(1)
        .children((
            Border::new()
                .grid_column(0)
                .border_thickness(Thickness::new(0.0, 0.0, 1.0, 0.0))
                .border_brush(Brush::Solid(crate::palette::border()))
                .content(nav),
            ScrollViewer::new()
                .grid_column(1)
                .content(View::keyed_fragment([(
                    format!("page-{page}"),
                    Border::new()
                        // SSH Hosts は SSH 先の一覧と接続のページを並べるので広く。
                        .max_width(if page == 2 { 820.0 } else { 560.0 })
                        .horizontal_alignment(HorizontalAlignment::Left)
                        .margin(Thickness::new(24.0, 0.0, 24.0, 24.0))
                        .content(content),
                )])),
        ));
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .children((head, body))
}

/// 外観ごとのテーマを選ぶ行。
/// 窓の背景（macOS 版・GTK4 版の Theme の Window Transparency・Background Image・Toolbar）。
pub fn backdrop_section(b: &crate::backdrop::Backdrop, callbacks: &BackdropCallbacks) -> View {
    use crate::backdrop::{Event, Placement};
    let labeled = |label: String, control: View| {
        Grid::new()
            .columns([GridLength::Pixel(140.0), GridLength::Star(1.0)])
            .column_spacing(8.0)
            .margin(Thickness::new(0.0, 4.0, 0.0, 4.0))
            .children((
                TextBlock::new()
                    .text(label)
                    .vertical_alignment(VerticalAlignment::Center),
                Border::new().grid_column(1).content(control),
            ))
    };
    let percent = |value: f64, min: f64, max: f64, event: fn(f64) -> Event| -> View {
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(8.0)
            .children((
                Slider::new()
                    .minimum(min)
                    .maximum(max)
                    .step_frequency(5.0)
                    .value((value * 100.0).round())
                    .width(200.0)
                    .on_value_changed((callbacks.number)(event)),
                TextBlock::new()
                    .text(format!("{}%", (value * 100.0).round()))
                    .vertical_alignment(VerticalAlignment::Center),
            ))
    };
    let image_name = b.image.as_deref().map_or_else(
        || tr!("None"),
        |p| {
            std::path::Path::new(p)
                .file_name()
                .map_or_else(|| p.to_owned(), |n| n.to_string_lossy().into_owned())
        },
    );
    let image_rows: View = if b.image.is_some() {
        StackPanel::new().children((
            labeled(
                tr!("Position"),
                ComboBox::new()
                    .items_source(Placement::ALL.iter().map(|p| p.title()).collect::<Vec<_>>())
                    .selected_index(
                        Placement::ALL
                            .iter()
                            .position(|p| *p == b.placement)
                            .unwrap_or(0),
                    )
                    .on_selection_changed(callbacks.placement.clone())
                    .into(),
            ),
            labeled(
                tr!("Transparency"),
                percent(b.image_transparency, 50.0, 95.0, Event::ImageTransparency),
            ),
        ))
    } else {
        View::empty()
    };
    let transparency: View = if b.see_through() {
        labeled(
            tr!("Transparency"),
            percent(b.transparency, 10.0, 90.0, Event::Transparency),
        )
    } else {
        View::empty()
    };
    let tint = b.toolbar_tint();
    let tint_rows: View = match tint {
        Some(color) => StackPanel::new().children((
            labeled(
                tr!("Toolbar color"),
                ColorPicker::new()
                    .color(Color::rgb(color.r, color.g, color.b))
                    .is_alpha_enabled(false)
                    .is_hex_input_visible(true)
                    .is_color_channel_text_input_visible(false)
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .on_color_changed(callbacks.color.clone())
                    .into(),
            ),
            labeled(
                tr!("Toolbar tint"),
                percent(b.toolbar_opacity, 10.0, 100.0, Event::ToolbarOpacity),
            ),
        )),
        None => View::empty(),
    };
    StackPanel::new().children((
        heading(tr!("Window Transparency")),
        labeled(
            tr!("See through in"),
            ComboBox::new()
                .items_source(
                    crate::backdrop::STYLES
                        .iter()
                        .map(|s| crate::backdrop::style_title(s))
                        .collect::<Vec<_>>(),
                )
                .selected_index(b.style_index())
                .on_selection_changed(callbacks.style.clone())
                .into(),
        ),
        transparency,
        heading(tr!("Background Image")),
        labeled(
            tr!("Image"),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .children((
                    TextBlock::new()
                        .text(image_name)
                        .opacity(0.7)
                        .max_width(220.0)
                        .text_trimming(TextTrimming::CharacterEllipsis)
                        .vertical_alignment(VerticalAlignment::Center),
                    Button::new()
                        .on_click((callbacks.button)(Event::ChooseImage))
                        .content(TextBlock::new().text(tr!("Choose…"))),
                    if b.image.is_some() {
                        Button::new()
                            .on_click((callbacks.button)(Event::RemoveImage))
                            .content(TextBlock::new().text(tr!("Remove")))
                    } else {
                        View::empty()
                    },
                )),
        ),
        image_rows,
        heading(tr!("Toolbar")),
        CheckBox::new()
            .is_checked(tint.is_some())
            .on_is_checked_changed(callbacks.tint.clone())
            .content(TextBlock::new().text(tr!("Tint the toolbar"))),
        tint_rows,
    ))
}

fn theme_choice(label: String, dark: bool, on: &Callback<Option<usize>>) -> View {
    let names = crate::theme::names();
    let selected = crate::theme::selected(dark);
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(12.0)
        .children((
            TextBlock::new()
                .text(label)
                .width(120.0)
                .vertical_alignment(VerticalAlignment::Center),
            ComboBox::new()
                .selected_index(names.iter().position(|n| *n == selected).unwrap_or(0))
                .items_source(names)
                .on_selection_changed(on.clone()),
        ))
}
