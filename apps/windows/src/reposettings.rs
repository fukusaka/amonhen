//! Repository Settings の画面（GTK4 版の `reposettings.rs`、macOS 版の `RepositorySettings.swift` のうち、Author
//! と Keys and Values）。Author はこのリポジトリ（`--local`）の名前とメール。Keys and Values は git が読む設定の
//! キー・効いている値・設定元で、このリポジトリの段に足す・消す（GTK4 版の `configtable.rs` の Customized に当たる
//! 並び）。Operations・Working Copy の行は Settings > Git（すべてのリポジトリの段）でも使う。

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
    /// フック（名前と、有効＝実行の許可）。読むまでは None。
    pub hooks: Option<Vec<(String, bool)>>,
    /// 中身を見ているフック（名前と中身）。
    pub hook_script: Option<(String, String)>,
    /// Create Hook で選んでいる番号（`missing_hooks` の中）。
    pub new_hook: usize,
    /// LFS: 入っているか、追跡するパターン、LFS のファイルの数、足すパターン。
    pub lfs: bool,
    pub lfs_patterns: Vec<String>,
    pub lfs_files: Option<u64>,
    pub new_pattern: String,
    /// リポジトリの形（`repositoryForm` の info。読むまでは None）。
    pub form: Option<Value>,
    /// ローカルのブランチ（名前の順）と、判定した既定のブランチ（Branches の既定のブランチの選ぶ欄）。
    pub local_branches: Vec<String>,
    pub detected_base: Option<String>,
    /// このリポジトリ専用のテンプレート（`commitTemplate`）: 打ったままの中身、読んだ・書いた中身、置き場所。
    pub template: Option<String>,
    pub template_saved: String,
    pub template_path: String,
    /// `core.hooksPath`（なければ `.git/hooks`）と、フックを開くエディタの名前（Settings > General で選んだもの）。
    pub hooks_path: Option<String>,
    pub editor: String,
    /// rerere が覚えた解決（`rr-cache`）がある（Forget All Recorded Resolutions… を出す）。
    pub rerere_cache: bool,
    /// コアが集めた署名の鍵（形式ごと。`SIGNING_FORMATS` の順）。
    pub signing_keys: Vec<(&'static str, Vec<SigningKey>)>,
    /// 鍵の「その他…」で打っている値（出していなければ None）。
    pub signing_other: Option<String>,
    /// ファイルの行で「ファイル…」を選んだ回数（取りやめても、選ぶ欄を今の値に戻すため作り直す）。
    pub file_epoch: u32,
}

/// 署名の鍵 1 つ（コアの `signingKeys` の答え）。
#[derive(Clone, Debug)]
pub struct SigningKey {
    pub id: String,
    pub label: String,
    pub emails: Vec<String>,
}

/// 署名の形式（`gpg.format` の値）。
pub const SIGNING_FORMATS: &[&str] = &["openpgp", "ssh", "x509"];

fn format_name(format: &str) -> &str {
    match format {
        "openpgp" => "OpenPGP",
        "ssh" => "SSH",
        "x509" => "X.509",
        other => other,
    }
}

/// `signingKeys` の答え（info）の鍵。集められなければ空。
pub fn signing_keys_of(info: &Value) -> Vec<SigningKey> {
    info["keys"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|k| SigningKey {
            id: k["id"].as_str().unwrap_or_default().to_owned(),
            label: k["label"].as_str().unwrap_or_default().to_owned(),
            emails: k["emails"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e.as_str().map(str::to_owned))
                .collect(),
        })
        .collect()
}

/// 鍵の選ぶ欄で「その他…」を表す値。
const OTHER_KEY: &str = "\u{0}other";

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
            hooks: None,
            hook_script: None,
            new_hook: 0,
            lfs: false,
            lfs_patterns: Vec::new(),
            lfs_files: None,
            new_pattern: String::new(),
            form: None,
            local_branches: Vec::new(),
            detected_base: None,
            template: None,
            template_saved: String::new(),
            template_path: String::new(),
            rerere_cache: false,
            signing_keys: Vec::new(),
            signing_other: None,
            file_epoch: 0,
            hooks_path: None,
            editor: String::new(),
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

    /// 今の署名の形式（`gpg.format`。知らないものは OpenPGP）。
    pub fn signing_format(&self) -> &'static str {
        let f = self
            .effective("gpg.format")
            .map(|(v, _)| v.to_lowercase())
            .unwrap_or_default();
        SIGNING_FORMATS
            .iter()
            .find(|x| **x == f)
            .copied()
            .unwrap_or("openpgp")
    }

    /// 署名の選ぶ欄の今の値（署名しないなら空、するなら形式）。
    fn signing_value(&self) -> &'static str {
        let on = self
            .effective("commit.gpgsign")
            .is_some_and(|(v, _)| normalize_bool(v) == "true");
        if on { self.signing_format() } else { "" }
    }

    /// `id` の鍵の形式（集めた鍵になければ None）。
    fn key_format(&self, id: &str) -> Option<&'static str> {
        self.signing_keys
            .iter()
            .find(|(_, keys)| keys.iter().any(|k| k.id == id))
            .map(|(f, _)| *f)
    }

    /// 鍵の選ぶ欄の項目（値と見出し）: 鍵なし、集めた鍵（形式の順、`user.email` の鍵が先）、一覧にない今の値、
    /// その他…。
    fn key_options(&self) -> Vec<(String, String)> {
        let empty = if self.signing_format() == "openpgp" {
            tr!("Key for your email")
        } else {
            tr!("None")
        };
        let email = self
            .effective("user.email")
            .map(|(v, _)| v.to_lowercase())
            .unwrap_or_default();
        let mut options = vec![(String::new(), empty)];
        for (format, keys) in &self.signing_keys {
            let mut keys: Vec<&SigningKey> = keys.iter().collect();
            keys.sort_by_key(|k| {
                email.is_empty() || !k.emails.iter().any(|e| e.to_lowercase() == email)
            });
            options.extend(keys.into_iter().map(|k| {
                (
                    k.id.clone(),
                    format!("{} — {}", format_name(format), k.label),
                )
            }));
        }
        let current = self
            .effective("user.signingkey")
            .map(|(v, _)| v.to_owned())
            .unwrap_or_default();
        if !current.is_empty() && self.key_format(&current).is_none() {
            options.push((current.clone(), current));
        }
        options.push((OTHER_KEY.to_owned(), tr!("Other…")));
        options
    }

    /// `hooksAndLfs` の答え（info）を取り込む。
    pub fn load_hooks(&mut self, info: &Value) {
        self.hooks = Some(
            info["hooks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|h| {
                    (
                        h["name"].as_str().unwrap_or_default().to_owned(),
                        h["enabled"].as_bool().unwrap_or(false),
                    )
                })
                .collect(),
        );
        self.hooks_path = info["hooksPath"].as_str().map(str::to_owned);
        self.editor = crate::editor::chosen().name;
        self.lfs = info["lfs"].as_bool().unwrap_or(false);
        self.lfs_patterns = info["lfsPatterns"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str().map(str::to_owned))
            .collect();
        self.lfs_files = info["lfsFiles"].as_u64();
    }

    /// まだないフック（Create Hook で選べるもの）。
    pub fn missing_hooks(&self) -> Vec<&'static str> {
        let present = self.hooks.as_deref().unwrap_or_default();
        HOOK_NAMES
            .iter()
            .copied()
            .filter(|n| !present.iter().any(|(p, _)| p == n))
            .collect()
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

/// git が呼ぶフックの名前（GTK4 版の `HOOK_NAMES`）。
const HOOK_NAMES: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

/// フックが動くとき（GTK4 版の `hook_when`）。
fn hook_when(hook: &str) -> String {
    match hook {
        "pre-commit" => tr!("Before a commit"),
        "pre-merge-commit" => tr!("Before a merge commit"),
        "prepare-commit-msg" => tr!("Before the message is edited"),
        "commit-msg" => tr!("On the commit message"),
        "post-commit" => tr!("After a commit"),
        "pre-rebase" => tr!("Before a rebase"),
        "post-checkout" => tr!("After a checkout"),
        "post-merge" => tr!("After a merge"),
        "pre-push" => tr!("Before a push"),
        "pre-auto-gc" => tr!("Before an automatic gc"),
        "post-rewrite" => tr!("After commits are rewritten"),
        "applypatch-msg" | "pre-applypatch" | "post-applypatch" => tr!("Around git am"),
        "post-index-change" => tr!("After the index changes"),
        "fsmonitor-watchman" => tr!("For fsmonitor"),
        "reference-transaction" => tr!("When refs change"),
        "push-to-checkout" | "pre-receive" | "update" | "post-receive" | "post-update"
        | "proc-receive" | "sendemail-validate" => tr!("On the receiving side"),
        _ => String::new(),
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

/// 見出しの下の git の行（GTK4 版の左の列の git とリポジトリの形）: このリポジトリを動かす git の版と機械、shallow・
/// partial clone・sparse checkout・git maintenance の札（説明はツールチップ）。
fn git_line(form: Option<&Value>) -> View {
    let Some(form) = form else {
        return View::empty();
    };
    let git = TextBlock::new()
        .text(format!(
            "git {} · {}",
            form["gitVersion"].as_str().unwrap_or("?"),
            tr!("This computer")
        ))
        .font_size(12.0)
        .opacity(0.6)
        .vertical_alignment(VerticalAlignment::Center)
        .tooltip(tr!(
            "This computer’s git runs this repository. Choose it in Settings > Git."
        ));
    let mut items: Vec<(String, String)> = Vec::new();
    if form["shallow"].as_bool() == Some(true) {
        items.push((
            "shallow clone".to_owned(),
            tr!(
                "Older history isn’t fetched. Fetch more in Fetching and Folders, or at the end of History."
            ),
        ));
    }
    if let Some(partial) = form["partial"].as_object() {
        let title = match partial.get("filter").and_then(Value::as_str) {
            Some(filter) => tr!("Contents fetched when asked ({filter})", filter = filter),
            None => tr!("Contents fetched when asked"),
        };
        let remote = partial.get("remote").and_then(Value::as_str).unwrap_or("?");
        items.push((
            title,
            tr!(
                "Partial clone: file contents are fetched from {remote} when you ask for them or an operation needs them.",
                remote = remote
            ),
        ));
    }
    if let Some(sparse) = form["sparse"].as_object() {
        let cone = sparse.get("cone").and_then(Value::as_bool) == Some(true);
        let count = sparse
            .get("folders")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let help = match (cone, count) {
            (false, _) => {
                tr!("Sparse checkout: only the files matching its patterns are checked out.")
            }
            (true, 0) => tr!("Sparse checkout: only the files at the top are checked out."),
            (true, 1) => tr!("Sparse checkout: only 1 folder is checked out."),
            (true, n) => tr!(
                "Sparse checkout: only {count} folders are checked out.",
                count = n
            ),
        };
        items.push((tr!("Some folders only"), help));
    }
    if form["maintenance"].as_bool() == Some(true) {
        items.push((
            "maintenance".to_owned(),
            tr!("Registered with git maintenance, which tidies it up in the background."),
        ));
    }
    let tags = items.into_iter().map(|(title, help)| {
        (
            title.clone(),
            Border::new()
                .background(Brush::Solid(crate::palette::bar_strong()))
                .corner_radius(4.0)
                .padding(Thickness::new(6.0, 1.0, 6.0, 1.0))
                .content(TextBlock::new().text(title).font_size(11.0).opacity(0.75))
                .tooltip(help),
        )
    });
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(6.0)
        .children((git, View::keyed_fragment(tags)))
}

/// git が古くて使えない行の理由（GTK4 版の `repoform::too_old`。`form` はコアの `repositoryForm` の info）。
fn too_old(needed: &str, form: &Value) -> View {
    let version = form["gitVersion"].as_str().unwrap_or("?");
    TextBlock::new()
        .text(format!(
            "⚠ {}",
            tr!(
                "Needs git {needed} or later. This repository’s git is {version}.",
                needed = needed,
                version = version
            )
        ))
        .font_size(12.0)
        .foreground(Brush::Solid(Color::rgb(0xb8, 0x6e, 0x00)))
        .text_wrapping(TextWrapping::Wrap)
        .into()
}

/// メッセージのテンプレートのファイル（git の `commit.template`）。
pub const TEMPLATE_KEY: &str = "commit.template";

/// テンプレートの選び方（GTK4 版の `template_choice`）: 0 はなし、1 はこのリポジトリ専用、2 はほかのファイル、3 から
/// 先は共有のテンプレート（`names` の順）。
pub fn template_choice(state: &State, names: &[String]) -> usize {
    let file = state.effective(TEMPLATE_KEY).filter(|(v, _)| !v.is_empty());
    if let Some((shared, scope)) = state.effective(crate::templates::LIBRARY_KEY)
        && let Some(i) = names.iter().position(|n| n == shared)
        && !crate::templates::shared_yields(scope, file.map(|(_, s)| s))
    {
        return 3 + i;
    }
    match file.map(|(v, _)| v) {
        None | Some("") => 0,
        Some(path) if is_own_template(path) => 1,
        Some(_) => 2,
    }
}

/// このリポジトリ専用のテンプレート（`<git dir>/amonhen/commit-template`）か。
pub fn is_own_template(path: &str) -> bool {
    path.replace('\\', "/")
        .ends_with("/amonhen/commit-template")
}

/// Branches の既定のブランチのキー（Amon Hen の設定。なければ判定したもの）。
pub const DEFAULT_BRANCH: &str = "amonhen.defaultBranch";

/// 既定のブランチの選ぶ欄の値（先頭は「判定したもの」で空）。
pub fn default_branch_values(state: &State) -> Vec<String> {
    let mut values = vec![String::new()];
    values.extend(state.local_branches.iter().cloned());
    if let Some(chosen) = state.at("local", DEFAULT_BRANCH)
        && !state.local_branches.iter().any(|b| b == chosen)
    {
        values.push(chosen.to_owned());
    }
    values
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

/// pull の取り込み方を `method` にする命令（`scope` の段に。GTK4 版と同じく、`pull.rebase` をいつも書いて曖昧さを
/// なくす）。
pub fn pull_commands(state: &State, method: &str, scope: &str) -> Vec<Value> {
    let set = |key: &str, value: Option<&str>| config_set(key, value, scope);
    let mut commands = Vec::new();
    if state
        .at(scope, "pull.ff")
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

/// 「Basic」のキー: ページが尋ねるもの（GTK4 版の `BASIC_KEYS`。docs/ux.md の 4.5）。
const BASIC_KEYS: &[&str] = &[
    "user.name",
    "user.email",
    "commit.gpgSign",
    "gpg.format",
    "user.signingKey",
    "gpg.ssh.allowedSignersFile",
    "commit.template",
    "amonhen.defaultBranch",
    "pull.rebase",
    "pull.ff",
    "fetch.prune",
    "rebase.autoStash",
    "rebase.updateRefs",
    "merge.conflictStyle",
    "rerere.enabled",
    "rerere.autoUpdate",
    "core.autocrlf",
    "core.fileMode",
];

/// 行と列の切り替えの覚え場所（アプリ全体。macOS 版・GTK4 版と同じ名前）。
const ROWS_KEY: &str = "configTableRows";
const COLUMNS_KEY: &str = "configTableColumns";
const ROWS: [&str; 3] = ["basic", "customized", "all"];
const COLUMNS: [&str; 2] = ["combined", "levels"];

/// 行の切り替え（Basic・Customized・All）と列の切り替え（Combined・Levels）。初めは Customized と Levels。
fn keys_modes() -> (&'static str, &'static str) {
    let rows = crate::prefs::string(ROWS_KEY).unwrap_or_default();
    let columns = crate::prefs::string(COLUMNS_KEY).unwrap_or_default();
    (
        ROWS.iter()
            .find(|r| **r == rows)
            .copied()
            .unwrap_or("customized"),
        COLUMNS
            .iter()
            .find(|c| **c == columns)
            .copied()
            .unwrap_or("levels"),
    )
}

/// Keys and Values の切り替えを選んだ（`keys-rows`・`keys-columns`）。ほかの id なら false。
pub fn keys_mode_chosen(id: &str, index: usize) -> bool {
    let (key, value) = match id {
        "keys-rows" => (ROWS_KEY, ROWS.get(index)),
        "keys-columns" => (COLUMNS_KEY, COLUMNS.get(index)),
        _ => return false,
    };
    if let Some(value) = value {
        let _ = crate::prefs::set_string(key, value);
    }
    true
}

/// 表の 1 行: キーと、段ごとの値（同じ段にいくつもあれば改行で並べる）、効いている値と段。
struct KeyRow {
    key: String,
    values: Vec<(&'static str, String)>,
    effective: Option<(String, String)>,
}

/// 出す行（`rows` の切り替えと探す文字で絞り、キーの順に）。
fn key_rows(entries: &[Entry], scope: &str, rows: &str, query: &str) -> Vec<KeyRow> {
    const SCOPES: [&str; 4] = ["system", "global", "local", "worktree"];
    let mut keys: Vec<String> = Vec::new();
    let mut push = |k: &str| {
        if !keys.iter().any(|x| x.eq_ignore_ascii_case(k)) {
            keys.push(k.to_owned());
        }
    };
    if rows == "basic" {
        BASIC_KEYS.iter().for_each(|k| push(k));
    } else {
        // 書ける段: このリポジトリなら local と worktree、すべてのリポジトリなら global。
        let own = |s: &str| {
            if scope == "global" {
                s == "global"
            } else {
                s == "local" || s == "worktree"
            }
        };
        entries
            .iter()
            .filter(|e| rows == "all" || own(&e.scope))
            .for_each(|e| push(&e.key));
    }
    let query = query.to_lowercase();
    let mut out: Vec<KeyRow> = keys
        .into_iter()
        .map(|key| {
            let mine: Vec<&Entry> = entries
                .iter()
                .filter(|e| e.key.eq_ignore_ascii_case(&key))
                .collect();
            let values = SCOPES
                .iter()
                .filter_map(|sc| {
                    let list: Vec<&str> = mine
                        .iter()
                        .filter(|e| e.scope == *sc)
                        .map(|e| e.value.as_str())
                        .collect();
                    (!list.is_empty()).then(|| (*sc, list.join("\n")))
                })
                .collect();
            let effective = mine.last().map(|e| (e.value.clone(), e.scope.clone()));
            KeyRow {
                key,
                values,
                effective,
            }
        })
        .filter(|r| {
            query.is_empty()
                || r.key.to_lowercase().contains(&query)
                || r.values
                    .iter()
                    .any(|(_, v)| v.to_lowercase().contains(&query))
        })
        .collect();
    out.sort_by_key(|r| r.key.to_lowercase());
    out
}

/// Keys and Values（GTK4 版の `configtable.rs`）: 行は Basic／Customized／All、列は Combined（効いている値と
/// 設定元）／Levels（段ごとの値、効いている段は太字）。探す欄、`scope` の段（`local` か `global`）の値を消す
/// −、キーと値を足す欄。
pub fn keys_view(
    state: &State,
    scope: &str,
    field: &dyn Fn(&'static str) -> Callback<String>,
    button: &dyn Fn(String) -> Callback<()>,
    choice: &dyn Fn(&'static str) -> Callback<Option<usize>>,
) -> View {
    let global = scope == "global";
    let entries: &[Entry] = state.entries.as_deref().unwrap_or_default();
    let (rows_mode, columns_mode) = keys_modes();
    let levels = columns_mode == "levels";
    // Levels の段の列（すべてのリポジトリの段ならシステム全体まで。worktree はあるときだけ）。
    let scopes: Vec<&'static str> = if global {
        vec!["system", "global"]
    } else if entries.iter().any(|e| e.scope == "worktree") {
        vec!["system", "global", "local", "worktree"]
    } else {
        vec!["system", "global", "local"]
    };
    let columns = |n: usize| {
        let mut c = vec![GridLength::Pixel(240.0)];
        c.extend((0..n).map(|_| GridLength::Star(1.0)));
        c.push(GridLength::Pixel(40.0));
        c
    };
    let cell = |text: String, column: usize, bold: bool, dim: bool| -> View {
        let t = TextBlock::new()
            .text(text)
            .font_size(12.0)
            .is_text_selection_enabled(!dim)
            .text_trimming(TextTrimming::CharacterEllipsis)
            .vertical_alignment(VerticalAlignment::Center)
            .margin(Thickness::new(0.0, 0.0, 8.0, 0.0))
            .grid_column(column as i32);
        let t = if bold {
            t.font_weight(FontWeight::SEMI_BOLD)
        } else {
            t
        };
        if dim { t.opacity(0.6).into() } else { t.into() }
    };
    let rows = key_rows(entries, scope, rows_mode, &state.query)
        .into_iter()
        .map(|r| {
            let here = state.at(scope, &r.key).is_some();
            let remove: View = if here {
                crate::small::button("−", |b| b.on_click(button(format!("remove:{}", r.key))))
                    .tooltip(if global {
                        tr!("Remove from All Repositories")
                    } else {
                        tr!("Remove from This Repository")
                    })
            } else {
                View::empty()
            };
            let mut cells: Vec<(String, View)> =
                vec![("key".to_owned(), cell(r.key.clone(), 0, false, false))];
            let n = if levels {
                let effective = r.effective.as_ref().map(|(_, s)| s.as_str());
                for (i, sc) in scopes.iter().enumerate() {
                    let value = r
                        .values
                        .iter()
                        .find(|(s, _)| s == sc)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    cells.push((
                        sc.to_string(),
                        cell(value, i + 1, effective == Some(*sc), false),
                    ));
                }
                scopes.len()
            } else {
                let (value, source) = r
                    .effective
                    .as_ref()
                    .map(|(v, s)| (v.clone(), scope_label(s)))
                    .unwrap_or_default();
                cells.push(("value".to_owned(), cell(value, 1, false, false)));
                cells.push(("source".to_owned(), cell(source, 2, false, true)));
                2
            };
            cells.push((
                "remove".to_owned(),
                Border::new().grid_column(n as i32 + 1).content(remove),
            ));
            (
                format!("{}:{columns_mode}", r.key.to_lowercase()),
                Border::new().min_height(28.0).content(
                    Grid::new()
                        .columns(columns(n))
                        .children((View::keyed_fragment(cells),)),
                ),
            )
        });
    // 列の見出し。
    let mut heads: Vec<(String, View)> = vec![("key".to_owned(), cell(tr!("Key"), 0, false, true))];
    let n = if levels {
        for (i, sc) in scopes.iter().enumerate() {
            heads.push((sc.to_string(), cell(scope_label(sc), i + 1, false, true)));
        }
        scopes.len()
    } else {
        heads.push(("value".to_owned(), cell(tr!("Value"), 1, false, true)));
        2
    };
    let header = Grid::new()
        .columns(columns(n))
        .margin(Thickness::new(0.0, 6.0, 0.0, 2.0))
        .children((View::keyed_fragment(heads),));
    let switches = StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(8.0)
        .children((
            ComboBox::new()
                .items_source(vec!["Basic".to_owned(), "Customized".to_owned(), "All".to_owned()])
                .selected_index(ROWS.iter().position(|r| *r == rows_mode).unwrap_or(1))
                .on_selection_changed(choice("keys-rows"))
                .tooltip(if global {
                    tr!("Basic: the settings the pages here ask about, set or not. Customized: the keys set for all repositories. All: every key git reads.")
                } else {
                    tr!("Basic: the settings the pages here ask about, set or not. Customized: the keys set in this repository or this worktree. All: every key git reads.")
                }),
            ComboBox::new()
                .items_source(vec!["Combined".to_owned(), "Levels".to_owned()])
                .selected_index(usize::from(levels))
                .on_selection_changed(choice("keys-columns"))
                .tooltip(tr!(
                    "Combined: the value in effect and where it comes from. Levels: a column per source, the one in effect in bold."
                )),
            TextBox::new()
                .text(state.query.clone())
                .placeholder_text(tr!("Search by key or value"))
                .min_width(240.0)
                .on_text_changed(field("query")),
        ));
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
                .on_text_changed(field("key"))
                .grid_column(0),
            TextBox::new()
                .text(state.new_value.clone())
                .placeholder_text(tr!("Value"))
                .on_text_changed(field("value"))
                .grid_column(1),
            crate::tip::holder(
                crate::small::button("+", |b| {
                    b.is_enabled(state.new_key.trim().contains('.'))
                        .on_click(button("add".to_owned()))
                }),
                if global {
                    tr!("Add a key for all repositories")
                } else {
                    tr!("Add a key to this repository")
                },
                2,
            ),
        ));
    StackPanel::new().children((
        switches,
        header,
        StackPanel::new().children((View::keyed_fragment(rows.collect::<Vec<_>>()),)),
        add,
    ))
}

/// Keys and Values のボタン（`add`・`remove:<key>`）で `scope` の段に書く命令（足したら欄を空にする）。
pub fn keys_commands(state: &mut State, key: &str, scope: &str) -> Option<Vec<Value>> {
    if key == "add" {
        let value = state.new_value.trim();
        let command = config_set(
            state.new_key.trim(),
            (!value.is_empty()).then_some(value),
            scope,
        );
        state.new_key.clear();
        state.new_value.clear();
        Some(vec![command])
    } else {
        if let Some(keys) = key.strip_prefix("revert:") {
            return Some(
                keys.split(',')
                    .map(|k| config_set(k, None, scope))
                    .collect(),
            );
        }
        let k = key.strip_prefix("remove:")?;
        Some(vec![config_set(k, None, scope)])
    }
}

/// 鍵の付いた行の並び。
pub type Rows = Vec<(String, View)>;

/// Operations と Working Copy の選ぶ欄の行（pull・各行。効いている値と、その設定元を添える）。Repository
/// Settings と Settings > Git（すべてのリポジトリの段）で使う。
pub fn choice_views(
    state: &State,
    scope: &str,
    choice: &dyn Fn(&'static str) -> Callback<Option<usize>>,
    button: &dyn Fn(String) -> Callback<()>,
) -> (Rows, Rows, Rows) {
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
        // ↩︎: この段で決めていれば、消して上の段（なければ git の既定）に戻す。
        let set_here: Vec<&str> = keys
            .iter()
            .copied()
            .filter(|k| state.at(scope, k).is_some())
            .collect();
        let revert: View = if set_here.is_empty() {
            View::empty()
        } else {
            crate::small::button("↩︎", |b| {
                b.on_click(button(format!("revert:{}", set_here.join(","))))
            })
            .tooltip(tr!("Stop overriding here. The value above takes over."))
        };
        // ⓘ: 段ごとの値（マウスを載せると出す）。
        let details: Vec<String> = keys
            .iter()
            .flat_map(|k| {
                state
                    .entries
                    .iter()
                    .flatten()
                    .filter(|e| e.key.eq_ignore_ascii_case(k))
                    .map(|e| format!("{}: {} = {}", scope_label(&e.scope), e.key, e.value))
            })
            .collect();
        let info: View = if details.is_empty() {
            View::empty()
        } else {
            TextBlock::new()
                .text("ⓘ")
                .opacity(0.6)
                .vertical_alignment(VerticalAlignment::Center)
                .tooltip(details.join("\n"))
        };
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
                        .on_selection_changed(choice(id)),
                    revert,
                    TextBlock::new()
                        .text(source)
                        .font_size(12.0)
                        .opacity(0.6)
                        .vertical_alignment(VerticalAlignment::Center),
                    info,
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
    // Signing: 1 つの選択（署名しない／OpenPGP／SSH／X.509）と鍵。鍵の形式が違えば知らせる。
    let signing = ["", "openpgp", "ssh", "x509"];
    let key_options = state.key_options();
    let key = state
        .effective("user.signingkey")
        .map(|(v, _)| v.to_owned())
        .unwrap_or_default();
    let format = state.signing_format();
    let mismatch = state
        .key_format(&key)
        .filter(|k| *k != format)
        .map(|k| {
            format!(
                "⚠ {}",
                tr!(
                    "This is an {kind} key, but the format is {format}.",
                    kind = format_name(k),
                    format = format_name(format)
                )
            )
        })
        .unwrap_or_default();
    let mut author = vec![
        (
            "signing".to_owned(),
            choice_row(
                "signing",
                tr!("Signing"),
                String::new(),
                &["commit.gpgsign", "gpg.format"],
                signing
                    .iter()
                    .map(|v| {
                        if v.is_empty() {
                            tr!("No signing")
                        } else {
                            format_name(v).to_owned()
                        }
                    })
                    .collect(),
                signing
                    .iter()
                    .position(|v| *v == state.signing_value())
                    .unwrap_or(0),
            ),
        ),
        (
            // 項目が変わったら作り直す（ComboBox の項目は後から差し替えない）。
            format!("signing-key:{}", key_options.len()),
            choice_row(
                "signing-key",
                tr!("Key"),
                mismatch,
                &["user.signingkey"],
                key_options.iter().map(|(_, l)| l.clone()).collect(),
                key_options.iter().position(|(v, _)| *v == key).unwrap_or(0),
            ),
        ),
    ];
    // ファイルの行（なし／今のファイル／ファイル…）。
    for (id, question, help, _) in file_rows() {
        let current = state
            .effective(id)
            .map(|(v, _)| v.to_owned())
            .unwrap_or_default();
        let mut labels = vec![tr!("None")];
        if !current.is_empty() {
            labels.push(tr!("File: {path}", path = current.as_str()));
        }
        labels.push(tr!("A file…"));
        let row = (
            format!("{id}:{}:{current}", state.file_epoch),
            choice_row(
                id,
                question,
                help,
                &[id],
                labels,
                usize::from(!current.is_empty()),
            ),
        );
        if id == "mailmap.file" {
            author.extend(mailmap_rows().into_iter().map(row_of));
        }
        author.push(row);
    }
    (operations, working_copy, author)
}

/// mailmap の行か（Author の行を Signing と mailmap に分ける）。
pub fn is_mailmap_row(key: &str) -> bool {
    key.starts_with("log.mailmap") || key.starts_with("mailmap.file")
}

/// git が読むファイルの行（キー、問い、説明、選ぶダイアログの題）。
fn file_rows() -> Vec<(&'static str, String, String, String)> {
    vec![
        (
            "gpg.ssh.allowedSignersFile",
            tr!("Allowed signers"),
            tr!("Public keys trusted to verify SSH signatures."),
            tr!("Choose an allowed signers file"),
        ),
        (
            "mailmap.file",
            tr!("Mailmap file"),
            tr!("Read after the repository’s .mailmap."),
            tr!("Choose a mailmap file"),
        ),
    ]
}

/// ファイルの行で「ファイル…」を選んだなら、そのキーとダイアログの題（選ぶ欄は作り直す）。
pub fn file_pick(state: &mut State, id: &str, index: usize) -> Option<(&'static str, String)> {
    let (key, _, _, title) = file_rows().into_iter().find(|r| r.0 == id)?;
    let has = state.effective(key).is_some_and(|(v, _)| !v.is_empty());
    if index != 1 + usize::from(has) {
        return None;
    }
    state.file_epoch += 1;
    Some((key, title))
}

/// `key` を `scope` の段で `path` にする命令（選んだファイル）。
pub fn file_command(key: &str, path: &str, scope: &str) -> Value {
    config_set(key, Some(path), scope)
}

/// mailmap の行（`log.mailmap`）。
fn mailmap_rows() -> Vec<OperationRow> {
    vec![(
        "log.mailmap",
        tr!("Use the mailmap"),
        tr!(
            "Shows the names and emails from the mailmap instead of the recorded ones, like git log (Blame always does)."
        ),
        "log.mailmap",
        vec![("true", tr!("On")), ("false", tr!("Off"))],
        "false",
    )]
}

/// 選ぶ欄で `id` の行の `index` 番目を選んだ。鍵の「その他…」なら打つ欄を出し（命令はなし）、ほかは
/// `choice_commands` と同じ。
pub fn choose(state: &mut State, id: &str, index: usize, scope: &str) -> Option<Vec<Value>> {
    if keys_mode_chosen(id, index) {
        return None;
    }
    if id == "signing-key" {
        let (value, _) = state.key_options().get(index)?.clone();
        if value == OTHER_KEY {
            state.signing_other = Some(
                state
                    .effective("user.signingkey")
                    .map(|(v, _)| v.to_owned())
                    .unwrap_or_default(),
            );
            return None;
        }
        state.signing_other = None;
    }
    choice_commands(state, id, index, scope)
}

/// 鍵の「その他…」の欄（出していなければ空）。Enter の代わりに Save で書く。
pub fn signing_other_view(
    state: &State,
    field: &dyn Fn(&'static str) -> Callback<String>,
    button: &dyn Fn(String) -> Callback<()>,
) -> View {
    let Some(text) = &state.signing_other else {
        return View::empty();
    };
    let placeholder = if state.signing_format() == "ssh" {
        "~/.ssh/id_ed25519.pub".to_owned()
    } else {
        tr!("Fingerprint or key ID")
    };
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(8.0)
        .children((
            TextBox::new()
                .text(text.clone())
                .placeholder_text(placeholder)
                .min_width(320.0)
                .on_text_changed(field("signing-other")),
            Button::new()
                .on_click(button("signing-other".to_owned()))
                .content(TextBlock::new().text(tr!("Save"))),
        ))
}

/// 「その他…」で打った鍵を書く命令（空なら消す。今と同じなら None）。欄は閉じる。
pub fn signing_other_commands(state: &mut State, scope: &str) -> Option<Vec<Value>> {
    let value = state.signing_other.take()?.trim().to_owned();
    let current = state
        .effective("user.signingkey")
        .map(|(v, _)| v.to_owned())
        .unwrap_or_default();
    (value != current).then(|| {
        vec![config_set(
            "user.signingkey",
            (!value.is_empty()).then_some(value.as_str()),
            scope,
        )]
    })
}

/// 選ぶ欄で `id` の行の `index` 番目を選んだときに書く命令（`scope` の段に。`global` なら全リポジトリ）。今の値と
/// 同じなら（作り直した欄が今の値を知らせてきたときなど）None。
pub fn choice_commands(state: &State, id: &str, index: usize, scope: &str) -> Option<Vec<Value>> {
    if id == "signing" {
        let value = ["", "openpgp", "ssh", "x509"].get(index).copied()?;
        if value == state.signing_value() {
            return None;
        }
        if value.is_empty() {
            return Some(vec![config_set("commit.gpgsign", Some("false"), scope)]);
        }
        let mut commands = vec![config_set("commit.gpgsign", Some("true"), scope)];
        if state.signing_format() != value || state.at(scope, "gpg.format").is_some() {
            commands.push(config_set("gpg.format", Some(value), scope));
        }
        return Some(commands);
    }
    if id == "signing-key" {
        let (value, _) = state.key_options().get(index)?.clone();
        let current = state
            .effective("user.signingkey")
            .map(|(v, _)| v.to_owned())
            .unwrap_or_default();
        if value == OTHER_KEY || value == current {
            return None;
        }
        let mut commands = vec![config_set(
            "user.signingkey",
            (!value.is_empty()).then_some(value.as_str()),
            scope,
        )];
        // 選んだ鍵の形式が今の形式と違えば、形式も合わせる。
        if let Some(f) = state.key_format(&value)
            && f != state.signing_format()
        {
            commands.push(config_set("gpg.format", Some(f), scope));
        }
        return Some(commands);
    }
    if file_rows().iter().any(|r| r.0 == id) {
        // 「なし」はこの段の上書きを消す（上の段にあればそれが効く）。今のファイルはそのまま。
        let has = state.at(scope, id).is_some();
        return (index == 0 && has).then(|| vec![config_set(id, None, scope)]);
    }
    if id == "pull" {
        let method = ["merge", "rebase", "ffOnly"][index.min(2)];
        if method == pull_method(state) {
            return None;
        }
        return Some(pull_commands(state, method, scope));
    }
    let (_, _, _, key, options, default) = operation_rows()
        .into_iter()
        .chain(working_copy_rows())
        .chain(mailmap_rows())
        .find(|r| r.0 == id)?;
    let (value, _) = options.get(index)?;
    let now = state
        .effective(key)
        .map_or(default, |(v, _)| v)
        .to_lowercase();
    let same = now == *value
        || (*value == "true" && ["yes", "on", "1"].contains(&now.as_str()))
        || (*value == "false" && ["no", "off", "0", ""].contains(&now.as_str()));
    if same {
        return None;
    }
    Some(vec![config_set(key, Some(value), scope)])
}

/// `commands` を戻す命令（`entries` は書く前の設定）。`configSet` ごとに、その段の前の値に（なければ消す）。
/// 後ろから戻す。ほかの命令は戻さない。
pub fn undo_commands(entries: &[Entry], commands: &[Value]) -> Vec<Value> {
    commands
        .iter()
        .rev()
        .filter(|c| c["op"] == "configSet")
        .filter_map(|c| {
            let key = c["key"].as_str()?;
            let scope = c["scope"].as_str().unwrap_or("local");
            let old = entries
                .iter()
                .rev()
                .find(|e| e.scope == scope && e.key.eq_ignore_ascii_case(key))
                .map(|e| e.value.as_str());
            Some(config_set(key, old, scope))
        })
        .collect()
}

/// `key` を `scope` の段（`local` か `global`）で `value` にする命令（None なら消す）。
fn config_set(key: &str, value: Option<&str>, scope: &str) -> Value {
    if scope == "global" {
        serde_json::json!({ "op": "configSet", "key": key, "value": value, "scope": "global" })
    } else {
        serde_json::json!({ "op": "configSet", "key": key, "value": value })
    }
}

pub struct Callbacks<'a> {
    /// 欄（`name`・`email`・`query`・`key`・`value`）。
    pub field: Box<dyn Fn(&'static str) -> Callback<String> + 'a>,
    /// ボタン（`author`・`add`・`remove:<key>`・`close`）。
    pub button: Box<dyn Fn(String) -> Callback<()> + 'a>,
    /// Operations の選ぶ欄（行の id と、選んだ番号）。
    pub choice: Box<dyn Fn(&'static str) -> Callback<Option<usize>> + 'a>,
    /// フックの有効の切り替え（フックの名前ごと）。
    pub toggle: Box<dyn Fn(String) -> Callback<bool> + 'a>,
    /// 右クリックのメニュー（`button` と同じ鍵）。
    pub menu: Box<dyn Fn() -> Callback<String> + 'a>,
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let head = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .margin(Thickness::new(10.0, 6.0, 8.0, 6.0))
        .grid_row(0)
        .children((
            StackPanel::new().spacing(2.0).grid_column(0).children((
                TextBlock::new()
                    .text(tr!("Repository Settings"))
                    .font_size(14.0),
                git_line(state.form.as_ref()),
            )),
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
    let (mut operations, working_copy, author) =
        choice_views(state, "local", &*callbacks.choice, &*callbacks.button);
    let (signing, mailmap): (Rows, Rows) =
        author.into_iter().partition(|(k, _)| !is_mailmap_row(k));
    let signing_other = signing_other_view(state, &*callbacks.field, &*callbacks.button);
    // 覚えた解決をすべて忘れる（覚えたものがあるときだけ。GTK4 版の `forget_resolutions_row`）。
    if state.rerere_cache {
        operations.push((
            "forget-resolutions".to_owned(),
            Button::new()
                .on_click((callbacks.button)("forget-resolutions".to_owned()))
                .horizontal_alignment(HorizontalAlignment::Right)
                .content(TextBlock::new().text(tr!("Forget All Recorded Resolutions…"))),
        ));
    }

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
    // Hooks: 名前・いつ動くか・有効（実行の許可）と、中身を見るボタン。
    let hooks: View = match &state.hooks {
        None => TextBlock::new().text(tr!("Loading…")).opacity(0.6).into(),
        Some(list) => {
            let rows = list.iter().map(|(name, enabled)| {
                let row = Grid::new()
                    .columns([
                        GridLength::Pixel(200.0),
                        GridLength::Pixel(220.0),
                        GridLength::Auto,
                        GridLength::Auto,
                    ])
                    .children((
                        TextBlock::new()
                            .text(name.clone())
                            .vertical_alignment(VerticalAlignment::Center)
                            .grid_column(0),
                        TextBlock::new()
                            .text(hook_when(name))
                            .font_size(12.0)
                            .opacity(0.6)
                            .vertical_alignment(VerticalAlignment::Center)
                            .grid_column(1),
                        // Windows の git は実行の許可にかかわらずフックを動かすので、切り替えられない。
                        crate::tip::holder(crate::small::check(CheckBox::new())
                                .is_checked(*enabled)
                                .is_enabled(false)
                                .on_is_checked_changed((callbacks.toggle)(name.clone()))
                                .content(TextBlock::new().text(tr!("Enabled"))), tr!(
                                "Hooks can’t be disabled on Windows (git runs them whatever their permissions)"
                            ), 2),
                        StackPanel::new()
                            .orientation(Orientation::Horizontal)
                            .spacing(6.0)
                            .grid_column(3)
                            .children((
                                crate::small::button(tr!("Show"), |b| {
                                    b.on_click((callbacks.button)(format!("hook-show:{name}")))
                                }),
                                // 選んだエディタで編集（GTK4 版・macOS 版の「〜で編集」）。
                                crate::small::button(
                                    tr!("Edit in {app}", app = &state.editor),
                                    |b| b.on_click((callbacks.button)(format!("hook-edit:{name}"))),
                                ),
                            )),
                    ))
                    // 右クリック: 編集とファイルの場所（GTK4 版の `hook_context_menu` のうち Edit With 以外）。
                    .menu(Menu::new(
                        [
                            MenuItem::item(
                                format!("hook-edit:{name}"),
                                tr!("Edit in {app}", app = &state.editor),
                            ),
                            MenuItem::item(format!("hook-reveal:{name}"), tr!("Show in Files")),
                        ],
                        (callbacks.menu)(),
                    ));
                (name.clone(), Border::new().min_height(32.0).content(row))
            });
            let missing = state.missing_hooks();
            let create = StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .margin(Thickness::new(0.0, 8.0, 0.0, 0.0))
                .children((
                    ComboBox::new()
                        .items_source(
                            missing
                                .iter()
                                .map(|n| match hook_when(n) {
                                    w if w.is_empty() => (*n).to_owned(),
                                    w => format!("{n} — {w}"),
                                })
                                .collect::<Vec<_>>(),
                        )
                        .selected_index(state.new_hook.min(missing.len().saturating_sub(1)))
                        .is_enabled(!missing.is_empty())
                        .on_selection_changed((callbacks.choice)("hook-new")),
                    crate::tip::holder(
                        Button::new()
                            .is_enabled(!missing.is_empty())
                            .on_click((callbacks.button)("hook-create".to_owned()))
                            .content(TextBlock::new().text(tr!("Create Hook…"))),
                        tr!("Puts a #!/bin/sh skeleton in place and makes it executable."),
                        0,
                    ),
                ));
            let script: View = match &state.hook_script {
                Some((name, text)) => StackPanel::new()
                    .spacing(4.0)
                    .margin(Thickness::new(0.0, 8.0, 0.0, 0.0))
                    .children((
                        TextBlock::new().text(name.clone()).opacity(0.75),
                        Border::new()
                            .background(Brush::Solid(crate::palette::bar_soft()))
                            .padding(Thickness::new(8.0, 6.0, 8.0, 6.0))
                            .content(
                                TextBlock::new()
                                    .text(text.clone())
                                    .font_size(12.0)
                                    .is_text_selection_enabled(true)
                                    .text_wrapping(TextWrapping::Wrap),
                            ),
                    )),
                None => View::empty(),
            };
            let empty: View = if list.is_empty() {
                TextBlock::new()
                    .text(tr!("No hooks. .sample files aren’t counted."))
                    .opacity(0.6)
                    .into()
            } else {
                View::empty()
            };
            StackPanel::new().children((empty, View::keyed_fragment(rows), create, script))
        }
    };
    // LFS: 入っているか、追跡するパターン（− で外す）、足すパターン。
    let lfs_status = if state.lfs {
        match state.lfs_files {
            Some(n) => tr!("Installed. {count} files are in LFS.", count = n),
            None => tr!("Installed (filter.lfs is set)"),
        }
    } else {
        tr!("Not installed. Install it with your package manager (git-lfs) and git lfs install.")
    };
    let patterns = state.lfs_patterns.iter().map(|p| {
        (
            p.clone(),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .children((
                    TextBlock::new()
                        .text(p.clone())
                        .vertical_alignment(VerticalAlignment::Center),
                    crate::small::button("−", |b| {
                        b.on_click((callbacks.button)(format!("lfs-untrack:{p}")))
                    })
                    .tooltip(tr!("Remove")),
                )),
        )
    });
    let lfs = StackPanel::new().spacing(6.0).children((
        TextBlock::new()
            .text(lfs_status)
            .text_wrapping(TextWrapping::Wrap),
        if state.lfs_patterns.is_empty() {
            TextBlock::new()
                .text(tr!("No LFS patterns in .gitattributes."))
                .opacity(0.6)
                .into()
        } else {
            View::empty()
        },
        View::keyed_fragment(patterns),
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(8.0)
            .children((
                TextBox::new()
                    .text(state.new_pattern.clone())
                    .placeholder_text("*.psd")
                    .width(200.0)
                    .on_text_changed((callbacks.field)("lfs")),
                Button::new()
                    .is_enabled(state.lfs && !state.new_pattern.trim().is_empty())
                    .on_click((callbacks.button)("lfs-track".to_owned()))
                    .content(TextBlock::new().text(tr!("Track Pattern…"))),
            )),
    ));
    // Fetching and Folders: shallow clone の続きを取る、partial clone の中身をすべて取る、git maintenance（GTK4 版の
    // `repoform::fetching_page`。sparse checkout のフォルダを選ぶのはまだ）。
    let fetching: View = match &state.form {
        None => TextBlock::new().text(tr!("Loading…")).opacity(0.6).into(),
        Some(form) => {
            let action_row = |title: String, help: String, buttons: Vec<(String, String)>| {
                let buttons = buttons.into_iter().map(|(key, label)| {
                    (
                        key.clone(),
                        Button::new()
                            .on_click((callbacks.button)(key))
                            .content(TextBlock::new().text(label)),
                    )
                });
                StackPanel::new().spacing(4.0).children((
                    TextBlock::new()
                        .text(title)
                        .text_wrapping(TextWrapping::Wrap),
                    TextBlock::new()
                        .text(help)
                        .font_size(12.0)
                        .opacity(0.65)
                        .text_wrapping(TextWrapping::Wrap),
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(8.0)
                        .children((View::keyed_fragment(buttons),)),
                ))
            };
            let mut rows: Vec<(String, View)> = Vec::new();
            if form["shallow"].as_bool() == Some(true) {
                rows.push((
                    "shallow".to_owned(),
                    action_row(
                        tr!("Older history isn’t fetched (shallow clone)"),
                        tr!("Fetch more of it from the remote, as at the end of History."),
                        vec![
                            ("deepen:100".to_owned(), tr!("100 Commits")),
                            ("deepen:1000".to_owned(), tr!("1,000 Commits")),
                            // 数を尋ねる・日付まで（GTK4 版の「さらに取る」の Other… と Up to a Date…）。
                            ("deepen-other".to_owned(), tr!("Other…")),
                            ("deepen-since".to_owned(), tr!("Up to a Date…")),
                            ("unshallow".to_owned(), tr!("Fetch All")),
                        ],
                    ),
                ));
            }
            if let Some(partial) = form["partial"].as_object() {
                let remote = partial
                    .get("remote")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let filter = partial.get("filter").and_then(Value::as_str).unwrap_or("?");
                let can = form["canFetchAllContents"].as_bool().unwrap_or(true);
                rows.push((
                    "partial".to_owned(),
                    action_row(
                        tr!(
                            "Partial clone: file contents are fetched when you ask for them or an operation needs them ({filter})",
                            filter = filter
                        ),
                        tr!(
                            "Diffs, blame and contents: searches fetch what they need from {remote}.",
                            remote = remote
                        ),
                        if can {
                            vec![("fetch-all-contents".to_owned(), tr!("Fetch All Contents…"))]
                        } else {
                            Vec::new()
                        },
                    ),
                ));
                if !can {
                    rows.push(("partial-old".to_owned(), too_old("2.36", form)));
                }
            }
            if rows.is_empty() {
                rows.push((
                    "full".to_owned(),
                    TextBlock::new()
                        .text(tr!("This repository has all of its history and contents."))
                        .opacity(0.65)
                        .into(),
                ));
            }
            // 取り出すフォルダ（sparse checkout。GTK4 版の `folders_section`）。作業ツリーのあるリポジトリだけ。
            if form["workingTree"].as_bool() != Some(false) {
                let can = form["canSparseCheckout"].as_bool().unwrap_or(true);
                let check_out_all = || -> Vec<(String, String)> {
                    if can {
                        vec![("sparse-all".to_owned(), tr!("Check Out All Folders…"))]
                    } else {
                        Vec::new()
                    }
                };
                let strings = |v: &Value| -> Vec<String> {
                    v.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|s| s.as_str().map(str::to_owned))
                        .collect()
                };
                match form["sparse"].as_object() {
                    Some(sparse) if sparse.get("cone").and_then(Value::as_bool) == Some(true) => {
                        let folders = strings(sparse.get("folders").unwrap_or(&Value::Null));
                        // `git clone --sparse` の直後は、最上位のファイルだけでフォルダはない。
                        let (title, help) = if folders.is_empty() {
                            (
                                tr!("Only the files at the top are checked out"),
                                tr!("Add the folders to check out with +."),
                            )
                        } else {
                            (
                                tr!("Only these folders are checked out"),
                                tr!(
                                    "Files at the top, and directly in the folders above these, are checked out too."
                                ),
                            )
                        };
                        rows.push(("sparse".to_owned(), action_row(title, help, check_out_all())));
                        for folder in &folders {
                            rows.push((
                                format!("sparse-folder:{folder}"),
                                StackPanel::new()
                                    .orientation(Orientation::Horizontal)
                                    .spacing(8.0)
                                    .children((
                                        TextBlock::new()
                                            .text(folder.clone())
                                            .is_text_selection_enabled(true)
                                            .vertical_alignment(VerticalAlignment::Center),
                                        crate::tip::holder(crate::small::button("−", |b| b
                                                .is_enabled(can)
                                                .on_click((callbacks.button)(format!(
                                                    "sparse-remove:{folder}"
                                                )))), tr!("Stop checking out this folder"), 0),
                                    )),
                            ));
                        }
                        rows.push((
                            "sparse-add".to_owned(),
                            crate::tip::holder(crate::small::button("+", |b| b
                                    .is_enabled(can)
                                    .on_click((callbacks.button)("sparse-add".to_owned()))), tr!("Check out one more folder"), 0),
                        ));
                    }
                    Some(sparse) => {
                        // cone mode でないパターンは変えない（git 2.37 から非推奨）。やめることだけできる。
                        rows.push((
                            "sparse".to_owned(),
                            action_row(
                                tr!("Only the files matching these patterns are checked out"),
                                tr!("The patterns aren’t in cone mode, so Amon Hen doesn’t change them."),
                                check_out_all(),
                            ),
                        ));
                        for pattern in strings(sparse.get("folders").unwrap_or(&Value::Null)) {
                            rows.push((
                                format!("sparse-pattern:{pattern}"),
                                TextBlock::new()
                                    .text(pattern)
                                    .is_text_selection_enabled(true)
                                    .into(),
                            ));
                        }
                    }
                    None => rows.push((
                        "sparse".to_owned(),
                        action_row(
                            tr!("All folders are checked out"),
                            tr!(
                                "In a large repository, you can check out only the folders you work in (sparse checkout)."
                            ),
                            if can {
                                vec![(
                                    "sparse-add".to_owned(),
                                    tr!("Check Out Only Some Folders…"),
                                )]
                            } else {
                                Vec::new()
                            },
                        ),
                    )),
                }
            }
            // 取り出すフォルダを選べない git なら、その理由（GTK4 版の `too_old`）。
            if form["workingTree"].as_bool() != Some(false)
                && !form["canSparseCheckout"].as_bool().unwrap_or(true)
            {
                rows.push(("sparse-old".to_owned(), too_old("2.25", form)));
            }
            // git maintenance に登録できない git でも行は出し、止めて理由を添える（GTK4 版と同じ）。
            {
                let can = form["canMaintain"].as_bool().unwrap_or(true);
                let on = form["maintenance"].as_bool() == Some(true);
                rows.push((
                    "maintenance".to_owned(),
                    StackPanel::new()
                        .spacing(4.0)
                        .children((
                            CheckBox::new()
                                .is_checked(on)
                                .is_enabled(can)
                                .on_is_checked_changed((callbacks.toggle)("maintenance".to_owned()))
                                .content(TextBlock::new().text(tr!(
                                    "Keep this repository maintained in the background"
                                ))),
                            TextBlock::new()
                                .text(tr!(
                                    "Every hour git prefetches and writes the commit-graph; it’s set in your global git config and systemd or cron (git maintenance)."
                                ))
                                .font_size(12.0)
                                .opacity(0.65)
                                .text_wrapping(TextWrapping::Wrap),
                            if can {
                                View::empty()
                            } else {
                                too_old("2.31", form)
                            },
                        )),
                ));
            }
            StackPanel::new()
                .spacing(12.0)
                .children((View::keyed_fragment(rows),))
        }
    };
    // Commits: メッセージのテンプレート（GTK4 版の `commits_page`）。共有のもの（Settings > Git で書いたもの）も選べる。
    let names = crate::templates::names();
    let choice = template_choice(state, &names);
    let file_label = match state.effective(TEMPLATE_KEY) {
        Some((path, _)) if choice == 2 => tr!("File: {path}", path = path),
        _ => tr!("File…"),
    };
    let template_source = if choice >= 3 {
        state.effective(crate::templates::LIBRARY_KEY)
    } else {
        state.effective(TEMPLATE_KEY)
    }
    .map(|(_, scope)| scope_label(scope))
    .unwrap_or_else(|| tr!("git’s default"));
    let own_editor: View = match (&state.template, choice) {
        (Some(text), 1) => {
            let dirty = lf(text) != state.template_saved;
            StackPanel::new().spacing(6.0).children((
                TextBox::new()
                    .text(text.clone())
                    .accepts_return(true)
                    .min_height(120.0)
                    .width(520.0)
                    .on_text_changed((callbacks.field)("template")),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .horizontal_alignment(HorizontalAlignment::Right)
                    .children((
                        Button::new()
                            .is_enabled(dirty)
                            .on_click((callbacks.button)("template-revert".to_owned()))
                            .content(TextBlock::new().text(tr!("Revert"))),
                        Button::new()
                            .is_enabled(dirty)
                            .on_click((callbacks.button)("template-save".to_owned()))
                            .content(TextBlock::new().text(tr!("Save"))),
                    )),
            ))
        }
        _ => View::empty(),
    };
    let commits = StackPanel::new().spacing(4.0).children((
        TextBlock::new().text(tr!("Message template")),
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(8.0)
            .children((
                ComboBox::new()
                    .items_source({
                        let mut items = vec![tr!("None"), tr!("This repository’s own"), file_label];
                        items.extend(names.iter().cloned());
                        items
                    })
                    .selected_index(choice)
                    .on_selection_changed((callbacks.choice)("template")),
                TextBlock::new()
                    .text(template_source)
                    .font_size(12.0)
                    .opacity(0.6)
                    .vertical_alignment(VerticalAlignment::Center),
            )),
        own_editor,
    ));
    // Remotes: 名前と URL（git の設定の `remote.<名前>.url`）、編集・Prune・削除（サイドバーのリモートの右クリックと
    // 同じ操作。GTK4 版の Remotes の表。つながるかの確かめはまだない）。
    let mut remotes: Vec<(String, String)> = Vec::new();
    for e in entries {
        let key = e.key.to_lowercase();
        if key.starts_with("remote.") && key.ends_with(".url") {
            // 名前は大文字小文字を残す（キーの元の綴りから）。
            let name = &e.key["remote.".len()..e.key.len() - ".url".len()];
            match remotes.iter_mut().find(|(n, _)| n == name) {
                Some(r) => r.1 = e.value.clone(),
                None => remotes.push((name.to_owned(), e.value.clone())),
            }
        }
    }
    let remote_rows = remotes.iter().map(|(name, url)| {
        let button = |key: String, label: String| {
            crate::small::button(label, |b| b.on_click((callbacks.button)(key)))
        };
        (
            name.clone(),
            Grid::new()
                .columns([
                    GridLength::Pixel(120.0),
                    GridLength::Star(1.0),
                    GridLength::Auto,
                ])
                .min_height(32.0)
                .children((
                    TextBlock::new()
                        .text(name.clone())
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(0),
                    TextBlock::new()
                        .text(url.clone())
                        .font_size(12.0)
                        .opacity(0.75)
                        .is_text_selection_enabled(true)
                        .text_trimming(TextTrimming::CharacterEllipsis)
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(1),
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(6.0)
                        .grid_column(2)
                        .children((
                            button(format!("remote-edit:{name}"), tr!("Edit…")),
                            button(format!("remote-prune:{name}"), "Prune…".to_owned()),
                            button(format!("remote-remove:{name}"), tr!("Remove…")),
                        )),
                )),
        )
    });
    let remotes_view = StackPanel::new().spacing(2.0).children((
        View::keyed_fragment(remote_rows),
        Button::new()
            .margin(Thickness::new(0.0, 6.0, 0.0, 0.0))
            .on_click((callbacks.button)("remote-add".to_owned()))
            .content(TextBlock::new().text(tr!("Add Remote…"))),
    ));
    // Branches: 既定のブランチ（GTK4 版の `branches_page`）。
    let chosen = state.at("local", DEFAULT_BRANCH);
    let values = default_branch_values(state);
    let labels: Vec<String> = values
        .iter()
        .map(|v| {
            if v.is_empty() {
                match &state.detected_base {
                    Some(b) => tr!("Detected ({branch})", branch = b),
                    None => tr!("Detected (none)"),
                }
            } else {
                v.clone()
            }
        })
        .collect();
    let selected = values
        .iter()
        .position(|v| Some(v.as_str()) == chosen)
        .unwrap_or(0);
    let missing = chosen.filter(|c| !state.local_branches.iter().any(|b| b == c));
    let branches = StackPanel::new().spacing(2.0).children((
        TextBlock::new().text(tr!("Default branch")),
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(8.0)
            .children((
                ComboBox::new()
                    .items_source(labels)
                    .selected_index(selected)
                    .on_selection_changed((callbacks.choice)("default-branch")),
                TextBlock::new()
                    .text(if chosen.is_none() {
                        tr!("detected")
                    } else {
                        tr!("this repository")
                    })
                    .font_size(12.0)
                    .opacity(0.6)
                    .vertical_alignment(VerticalAlignment::Center),
            )),
        TextBlock::new()
            .text(tr!(
                "Branches measures the others against it. Detected from origin/HEAD, main or master; choose one when there is none."
            ))
            .font_size(12.0)
            .opacity(0.65)
            .text_wrapping(TextWrapping::Wrap),
        match missing {
            Some(c) => TextBlock::new()
                .text(format!(
                    "⚠ {}",
                    tr!(
                        "There is no branch named {branch}, so the detected branch is used.",
                        branch = c
                    )
                ))
                .font_size(12.0)
                .into(),
            None => View::empty(),
        },
    ));
    let author_changed = state.at("local", "user.name").unwrap_or_default() != state.name
        || state.at("local", "user.email").unwrap_or_default() != state.email;
    let keys_and_values = keys_view(
        state,
        "local",
        &*callbacks.field,
        &*callbacks.button,
        &*callbacks.choice,
    );
    let body = ScrollViewer::new().grid_row(1).content(
        StackPanel::new()
            .margin(Thickness::new(24.0, 0.0, 24.0, 24.0))
            .children((
                // 子の数がタプルの上限を超えないよう、前半を 1 つにまとめる。
                StackPanel::new().children((
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
                    heading(tr!("Signing")),
                    StackPanel::new()
                        .max_width(560.0)
                        .horizontal_alignment(HorizontalAlignment::Left)
                        .spacing(12.0)
                        .children((View::keyed_fragment(signing), signing_other)),
                    heading("mailmap".to_owned()),
                    StackPanel::new()
                        .max_width(560.0)
                        .horizontal_alignment(HorizontalAlignment::Left)
                        .children((View::keyed_fragment(mailmap),)),
                    heading("Commits".to_owned()),
                    StackPanel::new()
                        .max_width(560.0)
                        .horizontal_alignment(HorizontalAlignment::Left)
                        .children((commits,)),
                    heading("Remotes".to_owned()),
                    StackPanel::new()
                        .max_width(720.0)
                        .horizontal_alignment(HorizontalAlignment::Left)
                        .children((remotes_view,)),
                    heading("Branches".to_owned()),
                    StackPanel::new()
                        .max_width(560.0)
                        .horizontal_alignment(HorizontalAlignment::Left)
                        .children((branches,)),
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
                )),
                heading(tr!("Ignore in this repository only")),
                StackPanel::new()
                    .max_width(560.0)
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .children((exclude,)),
                heading(tr!("Fetching and Folders")),
                StackPanel::new()
                    .max_width(560.0)
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .children((fetching,)),
                heading("Hooks".to_owned()),
                StackPanel::new()
                    .max_width(720.0)
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .children((hooks,)),
                heading("LFS".to_owned()),
                StackPanel::new()
                    .max_width(560.0)
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .children((lfs,)),
                heading(tr!("Keys and Values")),
                keys_and_values,
            )),
    );
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .children((head, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undoing_puts_back_the_values_before() {
        let entry = |scope: &str, key: &str, value: &str| Entry {
            scope: scope.to_owned(),
            key: key.to_owned(),
            value: value.to_owned(),
        };
        let entries = [
            entry("global", "pull.rebase", "true"),
            entry("local", "pull.rebase", "false"),
        ];
        let commands = [
            config_set("pull.rebase", Some("merges"), "local"),
            config_set("fetch.prune", Some("true"), "global"),
        ];
        assert_eq!(
            undo_commands(&entries, &commands),
            [
                config_set("fetch.prune", None, "global"),
                config_set("pull.rebase", Some("false"), "local"),
            ]
        );
    }

    fn state(entries: &[(&str, &str, &str)]) -> State {
        let mut s = State::new();
        s.entries = Some(
            entries
                .iter()
                .map(|(scope, key, value)| Entry {
                    scope: (*scope).to_owned(),
                    key: (*key).to_owned(),
                    value: (*value).to_owned(),
                })
                .collect(),
        );
        s
    }

    #[test]
    fn signing_writes_gpgsign_and_format_together() {
        let s = state(&[("global", "user.email", "a@example.com")]);
        // OpenPGP は既定の形式なので gpg.format は書かない。
        let c = choice_commands(&s, "signing", 1, "local").expect("命令");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0]["key"], "commit.gpgsign");
        // SSH なら形式も書く。すべてのリポジトリの段なら scope を付ける。
        let c = choice_commands(&s, "signing", 2, "global").expect("命令");
        assert_eq!(c[1]["key"], "gpg.format");
        assert_eq!(c[1]["value"], "ssh");
        assert_eq!(c[1]["scope"], "global");
        // 今と同じ（署名しない）なら書かない。
        assert!(choice_commands(&s, "signing", 0, "local").is_none());
    }

    #[test]
    fn choosing_a_key_of_another_format_sets_the_format() {
        let mut s = state(&[("global", "user.email", "a@example.com")]);
        s.signing_keys = vec![
            ("openpgp", Vec::new()),
            (
                "ssh",
                vec![SigningKey {
                    id: "key::ssh-ed25519 AAAA".into(),
                    label: "id_ed25519".into(),
                    emails: Vec::new(),
                }],
            ),
        ];
        let options = s.key_options();
        assert_eq!(options[0].1, tr!("Key for your email"));
        let index = options
            .iter()
            .position(|(v, _)| v.starts_with("key::"))
            .unwrap();
        let c = choice_commands(&s, "signing-key", index, "local").expect("命令");
        assert_eq!(c[0]["key"], "user.signingkey");
        assert_eq!(c[1]["value"], "ssh");
        // 「その他…」は欄を出すだけ。
        let other = options.len() - 1;
        assert!(choose(&mut s, "signing-key", other, "local").is_none());
        assert_eq!(s.signing_other.as_deref(), Some(""));
    }

    #[test]
    fn file_rows_pick_and_clear() {
        let mut s = state(&[("local", "mailmap.file", "C:/m/.mailmap")]);
        // なし・今のファイル・ファイル… の 3 つ。最後を選ぶとダイアログを出す。
        assert!(file_pick(&mut s, "mailmap.file", 1).is_none());
        assert_eq!(
            file_pick(&mut s, "mailmap.file", 2).map(|p| p.0),
            Some("mailmap.file")
        );
        assert_eq!(s.file_epoch, 1);
        // なしはこの段の上書きを消す。
        let c = choice_commands(&s, "mailmap.file", 0, "local").expect("命令");
        assert!(c[0]["value"].is_null());
        assert!(choice_commands(&s, "mailmap.file", 0, "global").is_none());
    }

    #[test]
    fn key_rows_follow_the_row_switch() {
        let s = state(&[
            ("system", "core.autocrlf", "true"),
            ("global", "user.name", "A"),
            ("local", "user.name", "B"),
            ("local", "remote.origin.fetch", "x"),
            ("local", "remote.origin.fetch", "y"),
        ]);
        let entries = s.entries.as_deref().unwrap();
        // Customized: このリポジトリで決めたキーだけ。値は段ごと、効いているのは最後のもの。
        let rows = key_rows(entries, "local", "customized", "");
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["remote.origin.fetch", "user.name"]);
        assert_eq!(rows[0].values, [("local", "x\ny".to_owned())]);
        assert_eq!(rows[1].effective.as_ref().unwrap().1, "local");
        // すべてのリポジトリの段の Customized は global のキー。
        assert_eq!(key_rows(entries, "global", "customized", "").len(), 1);
        // All はすべて、Basic は決めていないキーも出す。
        assert_eq!(key_rows(entries, "local", "all", "").len(), 3);
        let basic = key_rows(entries, "local", "basic", "pull");
        assert_eq!(basic.len(), 2);
        assert!(basic.iter().all(|r| r.effective.is_none()));
    }

    #[test]
    fn revert_removes_the_keys_at_this_level() {
        let mut s = state(&[]);
        let c = keys_commands(&mut s, "revert:pull.ff,pull.rebase", "global").expect("命令");
        assert_eq!(c.len(), 2);
        assert_eq!(c[1]["key"], "pull.rebase");
        assert!(c[1]["value"].is_null());
        assert_eq!(c[1]["scope"], "global");
    }
}
