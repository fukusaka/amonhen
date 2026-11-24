//! 状態と、イベントの処理と、画面の組み立て。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use amonhen_front::Front;
use amonhen_model::{Oid, tr};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_crossterm::crossterm::event::{
    Event as Input, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use serde_json::{Value, json};
use unicode_width::UnicodeWidthStr;

use crate::Clock;
use crate::Result;
use crate::changes::Changes;
use crate::commands::{self, Place};
use crate::details::{Details, Pair, Want, Whitespace};
use crate::diff::{PAN, Theme};
use crate::fileview::FileView;
use crate::history::{History, Marked, WHEEL_ROWS};
use crate::loader::{Loaded, Loader, Request};
use crate::reflog::Reflog;
use crate::sidebar::{self, Picked, Sidebar};

pub enum Event {
    Input(Input),
    /// リポジトリが変わった（`amonhen_watch::Changes` のビット）。
    Changed(u32),
    /// git や ssh が、パスワードなどを尋ねてきた。
    #[cfg(unix)]
    Ask(crate::askpass::Question),
    Loaded(Loaded),
    /// 動いている検索の、ここまでに合った行と、調べ終えた割合（0〜1。内容の検索でなければ 0 のまま）。
    Partial {
        query: String,
        rows: Vec<u32>,
        done: f32,
    },
    Failed(String),
    /// Quick Open を開いたときに探し始めた、手元の探すフォルダの下のリポジトリ（フォルダごとに届く）。
    Scanned(Vec<String>),
    /// clone が終わった（clone した場所か、失敗の理由）。
    Cloned(std::result::Result<String, String>),
    /// ほかの worktree の変更の数を読み直す頃（worktree が 2 つ以上あるとき、10 秒ごと。アプリと同じ）。
    CountWorktrees,
}

/// 描き直す所。一番上と一番下の行は、どれかを描き直すたびに描き直す。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Damage(u8);

impl Damage {
    const SIDEBAR: Self = Self(1);
    const MAIN: Self = Self(2);
    const SIDE: Self = Self(4);
    /// すべて（ペインの間の線と、重ねて出すものも）。
    pub const ALL: Self = Self(u8::MAX);

    fn has(self, pane: Self) -> bool {
        self.0 & pane.0 == pane.0
    }
}

impl std::ops::BitOr for Damage {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for Damage {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// メインの画面。
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    /// Working Copy。
    Changes,
    History,
    Reflog,
    /// 1 つのファイルの Blame か File History（Working Copy の一覧から開く）。
    File,
}

impl View {
    fn picked(self) -> Picked {
        match self {
            View::Changes => Picked::Changes,
            View::History => Picked::History,
            View::Reflog => Picked::Reflog,
            // Working Copy から開く画面。
            View::File => Picked::Changes,
        }
    }
}

/// キーが届く所: メインの画面の一覧（History・変わったファイル）か、その横（詳細・差分）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Sidebar,
    Main,
    Side,
    /// コミットメッセージの欄（Working Copy の一覧の下）。
    Message,
}

/// 画面の割り方（一番上と一番下の行を除いた所）。
struct Layout {
    sidebar: Option<Rect>,
    main: Option<Rect>,
    side: Option<Rect>,
}

/// 変化の知らせのビット（`amonhen_watch::Changes`）。
/// Activity の Performance で測るもの。
const PERF_HISTORY: &str = "history";
const PERF_STATUS: &str = "status";
const PERF_DIFF: &str = "diff";
const PERF_WORKING_DIFF: &str = "workingDiff";

const CHANGED_REFS: u32 = 1;
const CHANGED_INDEX: u32 = 2;
const CHANGED_WORKTREE: u32 = 4;

/// 一番下の行で打っている名前の使い道。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Naming {
    /// 新しいブランチ（始まりのコミットか ref に作って、切り替える）。
    Branch,
    /// 新しいタグ（コミットか ref に）。
    Tag,
    /// ブランチの新しい名前。
    Rename,
    /// stash のメッセージ（空でもよい）。
    Stash,
    /// stash を戻す新しいブランチ（stash を作ったところに作る。git stash branch）。
    StashBranch,
    /// ブランチを出す新しい worktree の場所。
    Worktree,
    /// push の送り先（`<remote>/<branch>`）。
    PushTo,
    /// 履歴のない新しいブランチの名前。
    Orphan,
    /// 足すリモートの名前（次に URL を聞く）。
    RemoteName,
    /// 足すリモートの URL（名前は聞いてある）。
    RemoteUrl,
    /// 直すリモートの新しい名前（次に URL を聞く）。
    RemoteRename,
    /// History のコミットから作る worktree の、新しいブランチ（次に場所を聞く）。
    WorktreeBranch,
    /// パッチの版の番号（`git format-patch -v`）。
    PatchVersion,
    /// アーカイブに入れる 1 つのフォルダ（作業ツリーの上からの）。
    ArchiveFolder,
    /// bundle に入れない基点（受け取る側が持っているコミット）。
    BundleBase,
    /// worktree をロックする理由（始まりは worktree の場所）。
    WorktreeLockReason,
    /// worktree のフォルダの移し先（始まりは今の場所）。
    WorktreeMove,
    /// 手で移した worktree のフォルダの今の場所（始まりは記録にある場所）。
    WorktreeLocate,
    /// 直すリモートの URL（始まりは前の名前、出し方は新しい名前）。
    RemoteEditUrl,
    /// 選んでいるコミットを書き出すアーカイブのファイル（この機械の）。
    Archive,
    /// shallow clone を、この日付より新しいコミットまで取り足す（`git fetch --shallow-since`）。
    DeepenSince,
    /// sparse checkout で取り出すフォルダ（空白で区切る。空ならすべて取り出す）。
    SparseFolders,
    /// clone する場所（この機械の。始まりは URL）。
    CloneDest,
    /// Activity の記録を書き出すファイル（この機械の）。
    ActivityExport,
    /// Range Diff の範囲（git range-diff と同じ: `A...B`、`A..B C..D`、`base A B`）。
    RangeDiffRanges,
    /// submodule（始まりはそのパス）の新しい URL。
    SubmoduleUrl,
    /// submodule（始まりはそのパス）が追うブランチ（空なら既定に戻す）。
    SubmoduleBranch,
    /// それぞれの submodule の中で動かす命令（シェルで）。
    SubmoduleForeach,
    /// 足す submodule の URL（次に場所を聞く）。
    SubmoduleAddUrl,
    /// 足す submodule の場所（始まりは URL）。
    SubmoduleAddPath,
    /// Add Subtree で取り込むフォルダ（次に相手を聞く）。
    SubtreeDir,
    /// subtree の相手（`URL ref`。始まりはフォルダ、`what` は add・pull・push）。
    SubtreeSource,
    /// subtree のフォルダの履歴だけで作るブランチ（始まりはフォルダ）。
    SubtreeSplit,
    /// リポジトリをまとめる bundle のファイル（この機械の）。
    Bundle,
    /// 取り込む bundle のファイル（この機械の）。
    BundleFile,
    /// bundle のブランチを取り込む名前（`refs/remotes/<名前>/*`。ファイルは聞いてある）。
    BundleName,
    /// 当てるパッチのファイル（この機械の）。
    Patch,
    /// 選んでいるコミットのパッチを書くフォルダ（この機械の）。
    SavePatch,
    /// File History を開くファイル（作業ツリーの根からのパス。Tab で補う）。
    OpenFile,
}

/// 選択肢 1 つ: 押すキー、名前、頼む中身。
type Choice = (char, String, Request);

/// コマンドの一覧（`:`）で打っている語と、選んでいる行。
#[derive(Default)]
struct Palette {
    query: String,
    selected: usize,
}

/// Quick Open の候補。
struct Candidate {
    path: String,
    name: String,
    favorite: bool,
}

impl Candidate {
    fn new(path: String) -> Self {
        let bookmark = amonhen_model::bookmarks::Bookmark {
            path,
            name: None,
            group: None,
            favorite: false,
        };
        Self {
            name: bookmark.display_name(),
            path: bookmark.path,
            favorite: false,
        }
    }
}

/// Quick Open: アプリと同じ候補（ブックマーク、最近開いたもの、探すフォルダの下で見つけたもの）を打った語で絞り、
/// 選んだリポジトリで TUI を開き直す。
/// ブランチの切り替え（アプリの Switch Branch…）: 打った語で絞り、最近切り替えたものを先に。合うものがなければ、
/// その名前で新しいブランチを作る。
struct BranchPick {
    query: String,
    selected: usize,
    /// ブランチ（名前と、リモートのものか）。今のブランチは入れない。
    items: Vec<(String, bool)>,
    /// 最近切り替えたブランチ（新しい順。HEAD の reflog の「checkout: moving from A to B」から）。
    recent: Vec<String>,
}

/// 切り替えの候補の 1 つ。
enum BranchChoice<'a> {
    Branch(&'a str, bool),
    New(&'a str),
}

impl BranchPick {
    fn shown(&self) -> Vec<BranchChoice<'_>> {
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let mut shown: Vec<(&String, bool)> = self
            .items
            .iter()
            .filter(|(name, _)| {
                let name = name.to_lowercase();
                words.iter().all(|w| name.contains(w.as_str()))
            })
            .map(|(name, remote)| (name, *remote))
            .collect();
        // 最近のもの（新しい順）、ほかのローカルのブランチ、リモートのブランチ。
        let rank = |name: &str, remote: bool| -> (usize, usize) {
            match self.recent.iter().position(|r| r == name) {
                Some(i) if !remote => (0, i),
                _ => (if remote { 2 } else { 1 }, 0),
            }
        };
        shown.sort_by(|a, b| rank(a.0, a.1).cmp(&rank(b.0, b.1)).then(a.0.cmp(b.0)));
        let mut choices: Vec<BranchChoice<'_>> = shown
            .into_iter()
            .map(|(n, r)| BranchChoice::Branch(n, r))
            .collect();
        let name = self.query.trim();
        if !name.is_empty() && !self.items.iter().any(|(n, r)| !r && n == name) {
            choices.push(BranchChoice::New(name));
        }
        choices
    }
}

struct QuickOpen {
    query: String,
    selected: usize,
    items: Vec<Candidate>,
}

impl QuickOpen {
    /// 候補を足す（もうある場所は足さない。`daemon:` の付いた同じ場所も、付かないものがあれば足さない）。
    fn add(&mut self, paths: impl IntoIterator<Item = String>) {
        for path in paths {
            let plain = path.strip_prefix("daemon:").unwrap_or(&path);
            if self.items.iter().any(|c| {
                c.path == path || c.path == plain || c.path.strip_prefix("daemon:") == Some(&path)
            }) {
                continue;
            }
            self.items.push(Candidate::new(path));
        }
    }

    /// 打った語のすべてを、名前か場所に含むもの（大文字と小文字は区別しない）。お気に入りを先に。
    fn shown(&self) -> Vec<&Candidate> {
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let mut shown: Vec<&Candidate> = self
            .items
            .iter()
            .filter(|b| {
                let text = format!("{} {}", b.name, b.path).to_lowercase();
                words.iter().all(|w| text.contains(w.as_str()))
            })
            .collect();
        shown.sort_by_key(|b| !b.favorite);
        shown
    }
}

/// Working Copy で、最近のコミットから選んでメッセージの欄に入れる一覧。`F` は件名を選んで `fixup! 件名` か
/// `squash! 件名` にし（アプリの Commit ▾ の Fixup Commit For・Squash Commit For。後で `rebase -i --autosquash`
/// がまとめる）、`R` はメッセージをそのまま入れる（アプリの最近のメッセージの一覧）。
struct MessagePick {
    query: String,
    selected: usize,
    kind: PickKind,
    /// 件名（Fixup・Squash）かメッセージ（Recent）。新しい順。読んでいる間は None。
    items: Option<Vec<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PickKind {
    Fixup,
    /// squash!（本文を残す）。Fixup と Tab で切り替える。
    Squash,
    Recent,
    /// 入れるもの（Conventional Commits の種類、Co-authored-by、Signed-off-by）。
    Insert,
}

/// Conventional Commits の種類（アプリの Insert ▾ と同じ並び）。
const COMMIT_TYPES: &[&str] = &[
    "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert",
];

/// 最近のコミット（`F`・`R`・`I`・Alt-↑ が使う）。
#[derive(Default)]
struct Recent {
    /// メッセージ（新しい順）。
    messages: Vec<String>,
    /// 作者（名前、メール）。同じメールは 1 つ。
    authors: Vec<(String, String)>,
    /// Signed-off-by に書く自分。
    me: Option<(String, String)>,
}

impl MessagePick {
    fn new(kind: PickKind, recent: Option<&Recent>) -> Self {
        let mut pick = Self {
            query: String::new(),
            selected: 0,
            kind,
            items: None,
        };
        if let Some(recent) = recent {
            pick.fill(recent);
        }
        pick
    }

    /// 読んだ最近のコミットから並べるものを作る。
    fn fill(&mut self, recent: &Recent) {
        let messages = &recent.messages;
        self.items = Some(match self.kind {
            PickKind::Insert => {
                let mut items: Vec<String> = COMMIT_TYPES.iter().map(|t| format!("{t}:")).collect();
                items.extend(
                    (recent.authors.iter())
                        .map(|(name, email)| format!("Co-authored-by: {name} <{email}>")),
                );
                if let Some((name, email)) = &recent.me {
                    items.push(format!("Signed-off-by: {name} <{email}>"));
                }
                items
            }
            PickKind::Recent => {
                let mut seen = Vec::new();
                for m in messages {
                    if !m.trim().is_empty() && !seen.contains(m) {
                        seen.push(m.clone());
                    }
                }
                seen
            }
            PickKind::Fixup | PickKind::Squash => fixup_subjects(messages),
        });
    }

    /// 並べるもの（何番目か、1 行目）。打った語のすべてを含むもの（大文字と小文字は区別しない）。
    fn shown(&self) -> Vec<(usize, &str)> {
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        (self.items.iter().flatten().enumerate())
            .filter(|(_, s)| {
                let text = s.to_lowercase();
                words.iter().all(|w| text.contains(w.as_str()))
            })
            .map(|(i, s)| (i, s.lines().next().unwrap_or("")))
            .collect()
    }
}

/// 件名の頭の Conventional Commits の種類（`feat: `・`fix(ui)!: ` など）を除いたもの。
fn strip_commit_type(summary: &str) -> &str {
    let kind_end = summary
        .find(|c: char| !c.is_ascii_lowercase())
        .unwrap_or(summary.len());
    if kind_end == 0 {
        return summary;
    }
    let mut rest = &summary[kind_end..];
    if rest.starts_with('(')
        && let Some(close) = rest.find(')')
    {
        rest = &rest[close + 1..];
    }
    rest = rest.strip_prefix('!').unwrap_or(rest);
    match rest.strip_prefix(':') {
        Some(rest) => rest.trim_start(),
        None => summary,
    }
}

/// `Key: 値` の形のトレーラーの行か。
fn is_trailer(line: &str) -> bool {
    line.split_once(": ").is_some_and(|(key, _)| {
        !key.is_empty() && key.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
    })
}

/// `fixup!`・`squash!` の先にできる件名（重ねない。空・同じものは 1 つに。アプリと同じく 15 まで）。
fn fixup_subjects(messages: &[String]) -> Vec<String> {
    let mut subjects: Vec<String> = Vec::new();
    for message in messages {
        let subject = message.lines().next().unwrap_or("").trim().to_owned();
        if subject.is_empty()
            || subject.starts_with("fixup! ")
            || subject.starts_with("squash! ")
            || subjects.contains(&subject)
        {
            continue;
        }
        subjects.push(subject);
        if subjects.len() == 15 {
            break;
        }
    }
    subjects
}

/// 操作ログ（アプリの Activity の Operation Log）: Undo の記録を新しい順に。選んだところまで戻せる。
/// Activity の Operations: この TUI と daemon が動かした git と命令（新しい順）。
struct Operations {
    entries: Vec<Operation>,
    selected: usize,
    /// 読んだ記録そのもの（Export JSON で書く）。
    records: Value,
}

struct Operation {
    seq: u64,
    /// daemon（か SSH 先）のもの。取り消しはそちらに頼む。
    remote: bool,
    kind: String,
    label: String,
    /// 始めた時刻（UNIX 時刻の秒）。
    started: i64,
    /// かかった時間（ミリ秒）。
    duration: Option<f64>,
    /// 終わっていなければ None。
    ok: Option<bool>,
    error: Option<String>,
    output: Option<String>,
    progress: Option<String>,
    cancellable: bool,
}

impl Operation {
    fn from(e: &Value, remote: bool) -> Option<Self> {
        let text = |k: &str| e[k].as_str().map(str::to_owned);
        Some(Self {
            seq: e["seq"].as_u64()?,
            remote,
            kind: text("kind").unwrap_or_default(),
            label: text("label").unwrap_or_default(),
            started: (e["started"].as_u64().unwrap_or(0) / 1000) as i64,
            duration: e["duration"].as_f64(),
            ok: e["ok"].as_bool(),
            error: text("error"),
            output: text("output"),
            progress: text("progress"),
            cancellable: e["cancellable"] == true,
        })
    }
}

struct OperationLog {
    /// 名前・時刻（UNIX 時刻の秒）。
    entries: Vec<(String, i64)>,
    selected: usize,
}

pub struct App {
    loader: Loader,
    /// 裏の仕事（Quick Open のスキャン）が結果を送る先。
    events: Sender<Event>,
    history: History,
    details: Details,
    changes: Changes,
    sidebar: Sidebar,
    /// サイドバーで開いた stash（詳細の枠に、History の選択の代わりに出す）。
    pinned: Option<Oid>,
    /// History を読み直す前に印をつけていたコミット（新しい History での行を聞いている間）。
    remapping: Vec<Marked>,
    /// 2 つに印をつけたときの、差分の取り方。
    pair: Pair,
    reflog: Reflog,
    file: FileView,
    /// ファイルの画面を閉じたときに戻る画面。
    file_back: View,
    view: View,
    focus: Focus,
    /// リポジトリ・ref・History を読んでいる。
    opening: bool,
    /// 読んでいる間に、また ref が変わった。
    reopen: bool,
    /// 次に読むときに History を作り直す（Rebuild History）。作り直しているときは `rebuilding`。
    rebuild_next: bool,
    rebuilding: bool,
    /// 確かめてから行う操作: 尋ねる文と、`y` で頼む中身。
    confirm: Option<(String, String, Request)>,
    /// push を確かめている: `t` で、送り先のブランチを打てる。
    push_to_offered: bool,
    /// shallow clone を取り足す前の `d`: 日付を打つ（選ぶ前に横取りする）。
    deepen_date_offered: bool,
    /// push の確かめで見せている送り先（`<remote>/<branch>`。置き換える push の絵の見出し）。
    pushing_to: String,
    /// チェックアウトしていないブランチの fast-forward の見通しを読んでいる間: ブランチ、上流、進む数。
    fast_forward: Option<(String, String, u64)>,
    /// 読んでいる bundle のファイル（取り込む名前を聞く間、覚えておく）。
    bundle_file: Option<String>,
    /// 読んでいるパッチのファイル（当てるかを聞く間、覚えておく）。
    patch_file: Option<String>,
    /// 前の版と比べている間の題（比べた結果が届くまで覚えておく）。
    range_title: Option<String>,
    /// 今の Range Diff の範囲（`rangeDiff` の ranges。`r` で打ち直す）。
    range_ranges: Vec<String>,
    /// merge か rebase を確かめる前に、見通しを読んでいる: 何を（ref の名前）、どのブランチに、merge か。merge は
    /// 見通し（`mergePreview`）を読んだ後に、コンフリクトの見込みも読む。
    preview: Option<(String, String, bool, Value)>,
    /// reset する先（見通しを読んでいる間、覚えておく）。
    reset_to: Option<String>,
    /// 消すブランチの見通しを読んでいる間: その名前、まだ比べていない上流、それまでで外れるコミットが一番少ない
    /// 見通し（`deletePreview` の `info`）。
    deleting: Option<(String, Option<String>, Option<Value>)>,
    /// History でマージ済みのブランチを折りたたんでいる（アプリの Fold Merged Branches。この TUI の間だけ）。
    fold: bool,
    /// 折りたたんでいる間に、取り込んだ側を開いたマージ（16 進の ID）。
    expanded: Vec<String>,
    /// History の並び順を、トポロジー順（ブランチのコミットをそのマージの下にまとめる）にしている。この TUI の
    /// 間だけ（アプリの設定には書かない）。
    topo_order: bool,
    /// 作業ツリーのファイルがこの機械にある（`ssh://` の先でない）。エディタで開けるか。
    files_here: bool,
    /// エディタで開く作業ツリーのファイル（絶対パス）。
    open_file: Option<String>,
    /// 開き直す worktree（サイドバーの Enter。TUI を閉じて、そこで開き直す）。
    open_worktree: Option<String>,
    /// Quick Open で選んだリポジトリの場所（TUI を閉じて、そこで開き直す）。
    open_location: Option<String>,
    quick_open: Option<QuickOpen>,
    /// ブランチの切り替え（`Ctrl-b`）。
    branch_pick: Option<BranchPick>,
    fixup_pick: Option<MessagePick>,
    /// 書きかけのメッセージとして最後に読んだか書いたもの（読み終えるまでは None で、書かない）。
    draft_saved: Option<String>,
    /// 今のブランチの最近のコミット（`F`・`R`・`I`・Alt-↑ が使う。コミットしたら読み直す）。
    recent_messages: Option<Recent>,
    /// メッセージの欄の Alt-↑・Alt-↓ で呼び出している、最近のメッセージの番目と、呼び出す前に書いていたもの。
    recalled: Option<(usize, String)>,
    /// 最近のメッセージを読み終えたら、Alt-↑ の呼び出しをする。
    recall_when_read: bool,
    /// いくつかから選んで行う操作: 尋ねる文と、キー・名前・頼む中身の組。
    choices: Option<(String, Vec<Choice>)>,
    /// マウスを取り込んでいる（切ると、端末で文字を選べる）。
    mouse: bool,
    /// マウスの取り込みを替えた（端末に伝えるまで）。
    mouse_changed: bool,
    /// `Ctrl-z` で止める（端末を戻してから）。
    #[cfg(unix)]
    suspend: bool,
    /// `Ctrl-l` で、端末に出ているものを当てにせずにすべて書き直す。
    redraw: bool,
    /// 新しいブランチの名前を打っている: 始まり（ref かコミット）、その出し方、打った名前。
    typing_branch: Option<(String, String, String)>,
    /// 直すリモート（サイドバーの `e`）の名前と、今の URL（`remotes` を読んだら）。
    editing_remote: Option<(String, Option<String>)>,
    /// リモートでの様子を読んでいるタグ（サイドバーの `t` は push、`X` はリモートから消す）。
    tag_remote: Option<(String, bool)>,
    /// 消す worktree（変更の数を読んでいる間）。
    removing_worktree: Option<String>,
    /// History を、この語に合うコミットだけにしている（`&`。アプリの Show Only Matching Commits）。
    matching: Option<String>,
    /// worktree ごとの変更のあるファイルの数（パスごと。開いているものは Working Copy の数）。
    worktree_counts: std::collections::HashMap<String, u64>,
    /// 開いている worktree のパスと、ほかの読める worktree のパス（2 つ以上あるときだけ）。
    worktree_paths: Option<(String, Vec<String>)>,
    /// 変更の数をすべて数え直した時刻（5 分ごとに `fresh` で読む）と、数えるのを頼む時計を動かしたか。
    counts_fresh_at: Option<std::time::Instant>,
    counts_clock: bool,
    /// History のコミットから作る worktree の、新しいブランチの名前（場所を聞いている間）。
    worktree_branch: Option<String>,
    /// パッチの版の番号を聞いている間の、コミット（空白区切り）と書く場所。
    patch_version: Option<(String, String)>,
    /// アーカイブの書き方を選んでいる間の、コミットと書くファイル（`o` で 1 つのフォルダを聞く）。
    archive_folder: Option<(String, String)>,
    /// bundle に何を入れるかを選んでいる間の、書くファイル（`a` で基点を聞く）。
    bundle_save: Option<String>,
    /// 署名を読んだタグの名前（変わったら読み直す）。
    tag_signature_names: Vec<String>,
    /// ロックするかを聞いている worktree（`r` で理由を聞く）。
    worktree_lock: Option<String>,
    /// 詳細に出した stash（土台を読んだら、一番下の行に様子を書く）。
    stash_about: Option<String>,
    /// 打っている名前が何のものか。
    naming: Naming,
    /// git や ssh から尋ねられていること（先頭を画面で聞いている）と、打った答え。
    #[cfg(unix)]
    questions: std::collections::VecDeque<crate::askpass::Question>,
    #[cfg(unix)]
    answer: String,
    /// 動いている操作（fetch・push など）の、一番上の行に出す名前。
    running: Option<String>,
    /// 今のブランチの上流（`origin/main`）と、ahead・behind。
    upstream: Option<String>,
    ahead: u64,
    behind: u64,
    /// shallow clone・partial clone の印（一番上の行のブランチの後ろ。アプリの History の上の印）。
    form_marks: Vec<String>,
    /// リポジトリの形（`repositoryForm` の info。読めていなければ null）。
    form: Value,
    /// タグのリモートでの様子（`tagRemotes` の info。読めていなければ null）。
    tag_remotes: Value,
    /// 送った、お気に入り・アーカイブの設定の書き込みのうち、まだ答えのないもの（すべて済んだら読み直す）。
    branch_config_pending: usize,
    /// 途中の操作（merge・rebase・cherry-pick・revert）: 種類と、何番目・全部でいくつ（rebase）。
    sequence: Option<(String, Option<(u64, u64)>)>,
    /// 止まっている `git am` のパッチを、3-way merge で当て直せる（まだ 3-way で当てていない）。
    retry_three_way: bool,
    /// 一番新しい stash の名前（`stash@{0}`）。
    stash: Option<String>,
    /// リモートの名前（上流のないブランチを publish する先）。
    remotes: Vec<String>,
    /// HEAD のコミット（amend で、そのメッセージを欄に入れる）。
    head: Option<String>,
    /// コミットを作っている。
    committing: bool,
    /// コミットができたら push する（Commit and Push。状態を読み直してから、push を確かめる）。
    push_after_commit: bool,
    /// 済んだことの知らせ（次のキーまで出す）。
    notice: Option<String>,
    /// 検索の語を打っている（`/`）。
    typing_query: Option<String>,
    /// 打っている検索の語は、詳細の中を探すもの（`/` を詳細で押した）。
    typing_in_details: bool,
    /// サイドバーの絞り込みの語を打っている（サイドバーで `/`）。
    typing_filter: Option<String>,
    /// 絞り込みの語は Working Copy の一覧のもの（でなければサイドバーのもの）。
    filtering_changes: bool,
    /// キーの一覧を出している（`?`）。
    help: bool,
    /// キーの一覧の、一番上に見えている行（収まらないときに動かす）。
    help_top: usize,
    /// 最後のコミットで hooks が書いたもの（成功でも失敗でも。`!` で全部を出す）。
    hook_output: Option<String>,
    /// 最後に失敗したときの、git などの答えの全部（2 行以上あったとき。`!` で出す。アプリの Details）。
    failure_details: Option<String>,
    /// `!` の枠に出しているのが、失敗の全部か（でなければ hooks の出力）。
    showing_failure: bool,
    /// 最後の失敗の全文（git のコマンドを探す間）。見つかったら、詳細の先頭に `$ git …` を足す。
    failure_seen: Option<String>,
    /// `failure_details` は失敗ではなく、命令の出力（Run in Each Submodule）。
    details_are_output: bool,
    /// hooks の出力を枠に出している（そのときの一番上の行）。
    showing_hooks: Option<usize>,
    /// コマンドの一覧を出している（`:`）。
    palette: Option<Palette>,
    operation_log: Option<OperationLog>,
    /// Activity の Operations（`Tab` で操作ログと切り替える）。
    operations: Option<Operations>,
    /// clone の途中で聞いていること（URL、場所、取り方）。選ぶ前に横取りして、次を聞く。
    clone_plan: Option<ClonePlan>,
    /// octopus merge で取り込む順と、その見通し（`octopusPreview` の `info`）。コンフリクトの見込みを読む間も持つ。
    octopus: Option<(Vec<String>, Value)>,
    /// octopus merge を確かめている間の `l`: この順（コンフリクトしそうなものを最後に回した）で見直す。
    octopus_last: Option<Vec<String>>,
    /// Export JSON で書き出す Operations の記録（場所を打っている間）。
    export_records: Option<Value>,
    /// Activity の History Cache（`historyCache` の info。キャッシュがなければ null）。
    history_cache: Option<Value>,
    /// Activity の Daemon（`daemon status` の答え。動いていなければ null）。
    daemon: Option<Value>,
    /// 内部の出来事（変化の知らせと、それで読み直したもの。新しいものが後ろ、500 まで）。時刻は UNIX 時刻の秒。
    event_log: std::collections::VecDeque<(i64, String)>,
    /// Activity の Events を出している（一番上からの位置）。
    showing_events: Option<usize>,
    /// 画面の速さ: 測るものごとに、頼んだ時刻と、かかった時間（ミリ秒、新しいものが後ろ、200 まで）。
    perf_started: std::collections::HashMap<&'static str, std::time::Instant>,
    perf: Vec<(&'static str, std::collections::VecDeque<f64>)>,
    /// Activity の Performance を出している。
    showing_performance: bool,
    /// History の編集モード（`E`）。
    edit_mode: Option<crate::editmode::EditMode>,
    /// 印をつけたコミット（新しい順）を、編集モードで 1 つにまとめる（`E`。行が届いたら決める）。
    combining: Vec<String>,
    /// 端末のクリップボードに入れる文字（OSC 52。描くループが書く）。
    copy: Option<String>,
    /// 編集モードで、変更があるのに Esc を 1 度押した（もう 1 度で捨てる）。
    edit_discard: bool,
    /// 編集モードの行のメッセージを、エディタで直す（開く中身。開いたら `editing_plan` が立つ）。
    plan_message: Option<String>,
    editing_plan: bool,
    /// 確かめている間に重ねる、前と後の絵（枠の題と、`drawing` の形の絵。編集モードの `editDrawing`、reset の
    /// `movePreview` から作ったもの）。答えたら消える。
    question_drawing: Option<(String, Value)>,
    /// パスを補うときの候補（いくつもあったとき。打っているパスの後ろに出す）。
    path_choices: String,
    /// cherry-pick・revert の確かめる文と頼むこと（前と後の絵を読んでから確かめる）。
    pick_question: Option<(String, String, Request, String)>,
    /// 今の検索: 語と、動いている検索を取り消す印（結果が届いたら `None`）。
    search: Option<(String, Option<Arc<AtomicBool>>)>,
    /// 動いている検索の、調べ終えた割合（0〜1。内容の検索でなければ 0 のまま）。
    search_done: f32,
    theme: Theme,
    /// リポジトリの名前（作業ツリーのフォルダ名）。
    name: String,
    /// 作業ツリーの場所。
    workdir: Option<String>,
    /// メッセージをエディタで開く（`Ctrl-e`。端末を渡してから）。
    edit_message: bool,
    /// メッセージを書き換えるコミット（16 進の ID）と、届いたら今のメッセージ（届くまで `None`）。届いたら
    /// エディタで開き、閉じたら書き換える。
    rewording: Option<(String, Option<String>)>,
    branch: Option<String>,
    clock: Clock,
    size: (u16, u16),
    /// ワーカーに頼んで、まだ答えが来ていない数。
    pending: usize,
    error: Option<String>,
    /// すべてを描き直す。
    dirty: bool,
    /// 描き直すペイン（そのペインの中だけが変わったとき。ほかは `dirty`）。
    damage: Damage,
    /// メインの画面が最後に置いたカーソル（メインを描き直さないフレームで使う）。
    main_cursor: Option<(u16, u16)>,
    quit: bool,
}

/// 一番上の行（リポジトリとブランチ）と一番下の行（案内）を除いた所に、History と詳細を出す。
const BARS: u16 = 2;
/// これ以上の幅なら、History の右に詳細を並べる。狭ければ、詳細は History の所に開く（Enter）。
const SIDE_BY_SIDE: u16 = 116;
/// これ以上の幅なら、左にサイドバーを出す。狭ければ、呼んだとき（`0`）だけメインの画面の所に出す。
const SIDEBAR_FROM: u16 = 100;
/// 並べたときの History の幅は全体の 5 分の 2（少なくともこの幅）。
const HISTORY_MIN: u16 = 60;

impl App {
    pub fn new(front: Arc<dyn Front>, events: Sender<Event>, clock: Clock) -> Self {
        Self {
            loader: Loader::start(front, events.clone()),
            events,
            history: History::default(),
            details: Details::default(),
            changes: Changes::default(),
            sidebar: Sidebar::default(),
            pinned: None,
            remapping: Vec::new(),
            pair: Pair::default(),
            reflog: Reflog::default(),
            file: FileView::default(),
            file_back: View::Changes,
            view: View::History,
            focus: Focus::Main,
            opening: false,
            reopen: false,
            rebuild_next: false,
            rebuilding: false,
            confirm: None,
            choices: None,
            mouse: true,
            mouse_changed: false,
            #[cfg(unix)]
            suspend: false,
            redraw: false,
            typing_branch: None,
            editing_remote: None,
            tag_remote: None,
            removing_worktree: None,
            matching: None,
            worktree_counts: Default::default(),
            worktree_paths: None,
            counts_fresh_at: None,
            counts_clock: false,
            worktree_branch: None,
            patch_version: None,
            archive_folder: None,
            bundle_save: None,
            tag_signature_names: Vec::new(),
            worktree_lock: None,
            stash_about: None,
            naming: Naming::Branch,
            #[cfg(unix)]
            questions: std::collections::VecDeque::new(),
            #[cfg(unix)]
            answer: String::new(),
            running: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            form_marks: Vec::new(),
            form: Value::Null,
            branch_config_pending: 0,
            tag_remotes: Value::Null,
            sequence: None,
            stash: None,
            retry_three_way: false,
            remotes: Vec::new(),
            head: None,
            committing: false,
            push_after_commit: false,
            notice: None,
            typing_query: None,
            typing_in_details: false,
            typing_filter: None,
            filtering_changes: false,
            help: false,
            help_top: 0,
            hook_output: None,
            failure_details: None,
            showing_failure: false,
            failure_seen: None,
            details_are_output: false,
            showing_hooks: None,
            palette: None,
            operation_log: None,
            operations: None,
            history_cache: None,
            daemon: None,
            event_log: Default::default(),
            showing_events: None,
            perf_started: Default::default(),
            perf: Vec::new(),
            showing_performance: false,
            export_records: None,
            clone_plan: None,
            octopus: None,
            octopus_last: None,
            edit_mode: None,
            combining: Vec::new(),
            copy: None,
            edit_discard: false,
            plan_message: None,
            editing_plan: false,
            question_drawing: None,
            path_choices: String::new(),
            pick_question: None,
            push_to_offered: false,
            deepen_date_offered: false,
            pushing_to: String::new(),
            fast_forward: None,
            bundle_file: None,
            patch_file: None,
            range_title: None,
            range_ranges: Vec::new(),
            topo_order: false,
            fold: false,
            preview: None,
            reset_to: None,
            deleting: None,
            expanded: Vec::new(),
            files_here: true,
            open_file: None,
            open_worktree: None,
            open_location: None,
            quick_open: None,
            branch_pick: None,
            fixup_pick: None,
            draft_saved: None,
            recent_messages: None,
            recalled: None,
            recall_when_read: false,
            search: None,
            search_done: 0.0,
            theme: Theme::detect(),
            name: String::new(),
            workdir: None,
            edit_message: false,
            rewording: None,
            branch: None,
            clock,
            size: (0, 0),
            pending: 0,
            error: None,
            dirty: true,
            damage: Damage::default(),
            main_cursor: None,
            quit: false,
        }
    }

    /// リポジトリを読み始める。
    pub fn start(&mut self) {
        // 覚えている History の折りたたみ（アプリと共有）。違えば、届いたときに読み直す。
        self.send(Request::Run {
            name: "foldState",
            command: json!({"op": "foldState"}),
        });
        self.open();
        self.changes.refresh();
        // 書きかけのメッセージ（なければテンプレート）を欄に入れる。
        self.send(Request::DraftOrTemplate {
            local: self.files_here,
        });
        self.request();
    }

    /// 書きかけのメッセージ（欄の中身）が覚えたものと違えば、`<git dir>/amonhen/commit-draft` に書く（空なら消す）。
    /// amend と、最近のメッセージを呼び出している間は書かない（自分で書いたものではない）。
    fn save_draft_if_changed(&mut self) {
        let Some(saved) = &self.draft_saved else {
            return;
        };
        if self.changes.amend || self.recalled.is_some() || self.committing {
            return;
        }
        let text = self.changes.message.text();
        if *saved == text {
            return;
        }
        self.draft_saved = Some(text.clone());
        self.send(Request::Run {
            name: "saveCommitDraft",
            command: json!({"op": "saveCommitDraft", "text": text}),
        });
    }

    /// 終わる前に書く、書きかけのメッセージ。終わるときは裏の仕事（頼んだばかりの保存も）を待たないので、呼んだ
    /// 側が今の中身をその場で書く（同じ中身でも書く）。読み終える前や amend の間は None。
    pub fn take_unsaved_draft(&mut self) -> Option<String> {
        self.draft_saved.as_ref()?;
        if self.changes.amend || self.recalled.is_some() || self.committing {
            return None;
        }
        let text = self.changes.message.text();
        self.draft_saved = Some(text.clone());
        Some(text)
    }

    /// リポジトリ・ref・History を読む。読んでいる間に頼まれた分は、終わってからもう一度。
    /// History の折りたたみを、リポジトリに覚える（`<common dir>/amonhen/fold.json`。アプリと共有）。
    fn save_fold(&mut self) {
        self.send(Request::Run {
            name: "saveFold",
            command: json!({"op": "setFoldState", "folded": self.fold, "except": self.expanded}),
        });
    }

    fn open(&mut self) {
        if self.opening {
            self.reopen = true;
            return;
        }
        self.opening = true;
        // HEAD が動いたかもしれない。
        self.reflog.refresh();
        self.rebuilding = std::mem::take(&mut self.rebuild_next);
        self.send(Request::Open {
            fold: self.fold,
            expanded: self.expanded.clone(),
            rebuild: self.rebuilding,
            tips: self.sidebar.marked_targets(),
            matching: self.matching.clone(),
        });
    }

    /// 作業ツリーのファイル `path`（作業ツリーの上からの）を、git のエディタで開く（端末を渡す）。ファイルが別の
    /// 機械にあれば（`ssh://`）、そう言う。
    fn edit_working_file(&mut self, path: &str) {
        if !self.files_here {
            self.error = Some(tr!(
                "The files are on another machine; open them in a terminal there"
            ));
            return;
        }
        let Some(workdir) = self.workdir.as_deref() else {
            return;
        };
        let file = std::path::Path::new(workdir).join(path);
        match file.is_file() {
            true => self.open_file = Some(file.to_string_lossy().into_owned()),
            false => self.error = Some(tr!("{path} isn’t a file in the working tree", path = path)),
        }
    }

    /// コミットのときに hooks（と git）が書いたものを覚える。2 行以上あれば、一番下の行に足す「`!` で全部」を返す。
    fn remember_hook_output(&mut self, output: &str) -> String {
        // hooks の出力のほうが新しい。
        self.failure_details = None;
        let lines = output.lines().filter(|l| !l.trim().is_empty()).count();
        match lines {
            0 => {
                self.hook_output = None;
                String::new()
            }
            1 => {
                self.hook_output = Some(output.trim_end().to_owned());
                String::new()
            }
            _ => {
                self.hook_output = Some(output.trim_end().to_owned());
                tr!(" (! shows all)")
            }
        }
    }

    /// 詳細の中で `query` を探して、合ったところへ動かす（アプリの詳細の中の検索）。何番目かを一番下の行に。
    fn find_in_details(&mut self, query: &str, forward: bool) {
        self.notice = Some(match self.details.find(query, forward) {
            Some((at, total)) => tr!(
                "“{query}”: {at} of {total} (n N for the next and previous)",
                query = query,
                at = at,
                total = total
            ),
            None => tr!("“{query}” isn’t in the details", query = query),
        });
        self.damage |= Damage::SIDE;
        self.dirty = true;
    }

    /// ブランチの切り替えを開く。最近切り替えたものは、HEAD の reflog を読んでから並べ直す。
    fn open_branch_pick(&mut self) {
        let items = self
            .sidebar
            .branches()
            .into_iter()
            .filter(|(name, remote)| *remote || Some(name.as_str()) != self.branch.as_deref())
            .collect();
        self.branch_pick = Some(BranchPick {
            query: String::new(),
            selected: 0,
            items,
            recent: Vec::new(),
        });
        self.send(Request::Run {
            name: "recentBranches",
            command: json!({"op": "reflog", "name": null, "limit": 200}),
        });
        self.dirty = true;
    }

    /// Working Copy・ref・History を読み直す。
    fn refresh(&mut self) {
        self.changes.refresh();
        self.open();
        self.dirty = true;
    }

    fn send(&mut self, request: Request) {
        // 画面の速さ（Activity の Performance）: 読み直し・差分の読み込みを頼んだ時刻。
        let metric = match &request {
            Request::Open { .. } => Some(PERF_HISTORY),
            Request::Status => Some(PERF_STATUS),
            Request::Commit { .. } | Request::Selection { .. } => Some(PERF_DIFF),
            Request::WorkingDiff { .. } => Some(PERF_WORKING_DIFF),
            _ => None,
        };
        if let Some(metric) = metric {
            self.perf_started.insert(metric, std::time::Instant::now());
        }
        self.pending += 1;
        self.loader.send(request);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.size = (cols, rows);
        self.history.set_height(rows.saturating_sub(BARS));
        self.changes.set_height(rows.saturating_sub(BARS));
        self.sidebar.set_height(rows.saturating_sub(BARS));
        self.reflog.set_height(rows.saturating_sub(BARS));
        self.file.set_height(rows.saturating_sub(BARS));
        // 詳細の幅は、並べても開いても、キーの届く所によらない。
        let layout = self.layout();
        let details = layout.side.or(layout.main).unwrap_or_default();
        self.details
            .set_size(details.width, details.height, &self.clock);
        self.dirty = true;
    }

    fn layout(&self) -> Layout {
        let (cols, rows) = self.size;
        let height = rows.saturating_sub(BARS);
        // 狭ければ、サイドバーは呼んだときだけ、メインの画面の所に出す。
        if cols < SIDEBAR_FROM && self.focus == Focus::Sidebar {
            return Layout {
                sidebar: Some(Rect::new(0, 1, cols, height)),
                main: None,
                side: None,
            };
        }
        let (sidebar, x) = match cols >= SIDEBAR_FROM {
            // サイドバーの右に、区切りの 1 列。
            true => (
                Some(Rect::new(0, 1, sidebar::WIDTH, height)),
                sidebar::WIDTH + 1,
            ),
            false => (None, 0),
        };
        let rest = cols - x;
        if rest >= SIDE_BY_SIDE {
            let left = (rest / 5 * 2).max(HISTORY_MIN);
            // 間に区切りの 1 列。
            return Layout {
                sidebar,
                main: Some(Rect::new(x, 1, left, height)),
                side: Some(Rect::new(x + left + 1, 1, rest - left - 1, height)),
            };
        }
        let all = Some(Rect::new(x, 1, rest, height));
        match self.focus {
            Focus::Sidebar | Focus::Main | Focus::Message => Layout {
                sidebar,
                main: all,
                side: None,
            },
            Focus::Side => Layout {
                sidebar,
                main: None,
                side: all,
            },
        }
    }

    /// 描き直す所があれば、その所（聞いたら、なかったことにする）。キーの一覧とコマンドの一覧はペインに重なる
    /// ので、出している間はすべてを描き直す。
    pub fn take_damage(&mut self) -> Option<Damage> {
        let damage = std::mem::take(&mut self.damage);
        let over = self.help
            || self.quick_open.is_some()
            || self.branch_pick.is_some()
            || self.showing_hooks.is_some()
            || self.fixup_pick.is_some()
            || self.palette.is_some()
            || self.operation_log.is_some()
            || self.operations.is_some()
            || self.history_cache.is_some()
            || self.daemon.is_some()
            || self.showing_events.is_some()
            || self.showing_performance
            || self.edit_mode.is_some();
        if std::mem::take(&mut self.dirty) || (over && damage != Damage::default()) {
            return Some(Damage::ALL);
        }
        (damage != Damage::default()).then_some(damage)
    }

    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// `damage` で描き直すセルの範囲（`draw` はこの中だけに描く）。
    pub fn areas(&self, damage: Damage) -> Vec<Rect> {
        let (cols, rows) = self.size;
        if damage == Damage::ALL || rows < BARS + 1 {
            return vec![Rect::new(0, 0, cols, rows)];
        }
        let layout = self.layout();
        let mut areas = vec![Rect::new(0, 0, cols, 1), Rect::new(0, rows - 1, cols, 1)];
        for (pane, area) in [
            (Damage::SIDEBAR, layout.sidebar),
            (Damage::MAIN, layout.main),
            (Damage::SIDE, layout.side),
        ] {
            if damage.has(pane) {
                areas.extend(area);
            }
        }
        areas
    }

    /// `damage` で描き直すペインのうち、最後に描いてから中身が縦にずれたものと、その行数（正なら上へ）。
    /// フレームの出力が、端末のスクロールに使う。当てにならなくてもよい（中身が替わっていても、違うセルは書く）。
    pub fn scrolls(&self, damage: Damage) -> Vec<(Rect, i32)> {
        let layout = self.layout();
        let main = match self.view {
            View::History => self.history.scrolled(),
            _ => 0,
        };
        let side = match self.view {
            View::Changes => self.changes.diff_scrolled(),
            _ => self.details.scrolled(),
        };
        [
            (Damage::MAIN, layout.main, main),
            (Damage::SIDE, layout.side, side),
        ]
        .into_iter()
        .filter(|&(pane, _, lines)| damage.has(pane) && lines != 0)
        .filter_map(|(_, area, lines)| Some((area?, lines)))
        .collect()
    }

    pub fn quit(&self) -> bool {
        self.quit
    }

    pub fn mouse_captured(&self) -> bool {
        self.mouse
    }

    /// マウスの取り込みを替えたなら、その値（聞いたら、なかったことにする）。
    /// 始めるときのマウスの取り込み（`settings.json` の `tui.mouse`。決めていなければ取り込む）。読めなければ、取り込んだ
    /// まま理由を出す。
    pub fn apply_mouse_setting(&mut self, setting: std::result::Result<Option<bool>, String>) {
        match setting {
            Ok(Some(false)) => {
                self.mouse = false;
                self.mouse_changed = true;
            }
            Ok(_) => {}
            Err(reason) => self.error = Some(reason),
        }
    }

    pub fn take_mouse_change(&mut self) -> Option<bool> {
        std::mem::take(&mut self.mouse_changed).then_some(self.mouse)
    }

    /// `Ctrl-z` を押した（聞いたら、なかったことにする）。止まって続いた後は、描き直す。
    /// 端末のクリップボードに入れる文字。
    pub fn take_copy(&mut self) -> Option<String> {
        self.copy.take()
    }

    #[cfg(unix)]
    pub fn take_suspend(&mut self) -> bool {
        let suspend = std::mem::take(&mut self.suspend);
        self.dirty |= suspend;
        suspend
    }

    /// `Ctrl-l` を押した（聞いたら、なかったことにする）。
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    /// メッセージをエディタで開くなら、今のメッセージと作業ツリーの場所（聞いたら、なかったことにする）。
    pub fn take_edit_message(&mut self) -> Option<(String, Option<String>)> {
        // 編集モードの行のメッセージ。
        if let Some(text) = self.plan_message.take() {
            self.editing_plan = true;
            return Some((text, self.workdir.clone()));
        }
        // コミットのメッセージの書き換えは、今のメッセージが届いてから。
        if let Some((_, Some(message))) = &self.rewording
            && !self.edit_message
        {
            self.edit_message = true;
            return Some((message.clone(), self.workdir.clone()));
        }
        if self.rewording.is_some() {
            return None;
        }
        std::mem::take(&mut self.edit_message)
            .then(|| (self.changes.message.text(), self.workdir.clone()))
    }

    /// 作業ツリーのファイルがこの機械にあるか（`ssh://` の先なら false）。
    pub fn set_files_here(&mut self, here: bool) {
        self.files_here = here;
    }

    /// 終わった後に開き直すもの（worktree か、Quick Open で選んだリポジトリ）。
    pub fn take_next(&mut self) -> Option<crate::Next> {
        match self.open_location.take() {
            Some(location) => Some(crate::Next::Location(location)),
            None => self.open_worktree.take().map(crate::Next::Worktree),
        }
    }

    /// エディタで開くファイルと、作業ツリーの場所（聞いたら、なかったことにする）。
    pub fn take_open_file(&mut self) -> Option<(String, Option<String>)> {
        let file = self.open_file.take()?;
        Some((file, self.workdir.clone()))
    }

    /// エディタを閉じた。だめなら理由を出す。中身が変わったかもしれないので、状態を読み直す。
    pub fn file_opened(&mut self, result: Result<()>) {
        if let Err(error) = result {
            self.error = Some(error.to_string());
        }
        self.changes.refresh();
        self.dirty = true;
    }

    /// エディタを閉じた後のメッセージ。だめなら理由を出し、メッセージはそのまま。
    pub fn message_edited(&mut self, edited: Result<String>) {
        self.edit_message = false;
        // 編集モードの行のメッセージ（当てるまで、何も変わらない）。
        if std::mem::take(&mut self.editing_plan) {
            self.dirty = true;
            match edited {
                Ok(text) => {
                    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
                    let message = lines.join("\n").trim().to_owned();
                    match (message.is_empty(), self.edit_mode.as_mut()) {
                        (true, _) => {
                            self.error = Some(tr!("The message is empty; it is unchanged"))
                        }
                        (false, Some(mode)) => mode.set_message(message),
                        (false, None) => {}
                    }
                }
                Err(error) => self.error = Some(error.to_string()),
            }
            return;
        }
        // コミットのメッセージを書き換えていたなら、変わっていれば書き換える。
        if let Some((oid, before)) = self.rewording.take() {
            self.dirty = true;
            match edited {
                Ok(text) => {
                    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
                    let message = lines.join("\n").trim().to_owned();
                    if message.is_empty() {
                        self.error = Some(tr!("The message is empty; the commit is unchanged"));
                    } else if Some(message.as_str()) == before.as_deref().map(str::trim) {
                        self.notice = Some(tr!("The message is unchanged"));
                    } else {
                        self.run(
                            "rewrite",
                            tr!("Editing the message…"),
                            json!({
                                "op": "rewrite", "label": "Edit Message",
                                "edit": {"kind": "reword", "oid": oid, "message": message},
                            }),
                        );
                    }
                }
                Err(error) => self.error = Some(error.to_string()),
            }
            return;
        }
        match edited {
            // `#` で始まる行は、git と同じくコメント。
            Ok(text) => {
                let text: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
                self.changes.message.set_text(&text.join("\n"));
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        self.dirty = true;
    }

    /// まだ読んでいる。
    pub fn busy(&self) -> bool {
        self.pending > 0
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// メインの画面の中の 1 列（計測で、そこにホイールを送る）。
    /// 前のフレームで組み立てたものを捨てる（次は、すべてを組み立て直す）。
    pub fn forget_drawn(&mut self) {
        self.history.forget_drawn();
        self.changes.forget_drawn();
        self.details.forget_drawn();
    }

    pub fn main_column(&self) -> u16 {
        self.layout().main.map_or(0, |at| at.x + 1)
    }

    pub fn history_rows(&self) -> u32 {
        self.history.total()
    }

    /// 計測の `step` を、`down` の向きにはもう進められない。
    pub fn at_end(&self, down: bool, step: crate::Step) -> bool {
        match step {
            crate::Step::Wheel => self.history.at_end(down),
            crate::Step::Move => {
                let last = self.history.total().saturating_sub(1);
                self.history.selected() == if down { last } else { 0 }
            }
            crate::Step::Diff => self.details.at_end(down),
            crate::Step::Changes => self.changes.at_end(down),
            // 回数で折り返す。
            crate::Step::DiffPan | crate::Step::ChangesPan => false,
        }
    }

    /// 詳細に出ている行の数（差分を含む）。
    pub fn details_lines(&self, step: crate::Step) -> u32 {
        match step {
            crate::Step::Changes | crate::Step::ChangesPan => self.changes.diff_position().1,
            _ => self.details.position().1,
        }
    }

    pub fn last_frame_incomplete(&self) -> bool {
        self.history.incomplete()
    }

    pub fn handle(&mut self, event: Event) {
        self.handle_event(event);
        // 欄を離れていれば、書きかけのメッセージを覚える（打っている間は書かない）。
        if self.focus != Focus::Message {
            self.save_draft_if_changed();
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Input(Input::Key(key)) => self.key(key),
            Event::Input(Input::Mouse(mouse)) => self.mouse(mouse),
            Event::Input(Input::Resize(cols, rows)) => self.resize(cols, rows),
            Event::Input(_) => {}
            #[cfg(unix)]
            Event::Ask(question) => {
                self.questions.push_back(question);
                self.dirty = true;
            }
            Event::Changed(bits) => {
                // Activity の Events に、何が変わったと知らされたかと、それで読み直すもの。
                let mut changed = Vec::new();
                for (bit, name) in [
                    (CHANGED_REFS, tr!("refs")),
                    (CHANGED_INDEX, tr!("the index")),
                    (CHANGED_WORKTREE, tr!("the working tree")),
                ] {
                    if bits & bit != 0 {
                        changed.push(name);
                    }
                }
                if !changed.is_empty() {
                    let mut text = tr!("Changed: {what}", what = changed.join(", "));
                    if bits & (CHANGED_INDEX | CHANGED_WORKTREE) != 0 {
                        text.push_str(&tr!(" → read the status again"));
                    }
                    if bits & CHANGED_REFS != 0 {
                        text.push_str(&tr!(" → read History and the refs again"));
                    }
                    self.event(text);
                }
                if bits & (CHANGED_INDEX | CHANGED_WORKTREE) != 0 {
                    self.changes.refresh();
                }
                if bits & CHANGED_REFS != 0 {
                    self.open();
                }
            }
            Event::Loaded(loaded) => {
                self.pending = self.pending.saturating_sub(1);
                let metric = match &loaded {
                    Loaded::Opened { .. } => Some(PERF_HISTORY),
                    Loaded::Status(_) => Some(PERF_STATUS),
                    Loaded::Commit { .. } => Some(PERF_DIFF),
                    Loaded::WorkingDiff { .. } => Some(PERF_WORKING_DIFF),
                    _ => None,
                };
                if let Some(metric) = metric {
                    self.record_perf(metric);
                }
                // 行と祖先は History の中だけ、コミットと構文の色は詳細・差分の中だけが変わる。
                match &loaded {
                    Loaded::Rows { .. } | Loaded::Lineage { .. } => self.damage |= Damage::MAIN,
                    Loaded::Commit { .. } | Loaded::Syntax { .. } => self.damage |= Damage::SIDE,
                    _ => self.dirty = true,
                }
                match loaded {
                    Loaded::Opened {
                        workdir,
                        refs,
                        history,
                    } => {
                        self.workdir = workdir.clone();
                        self.name = workdir
                            .as_deref()
                            .and_then(|w| std::path::Path::new(w).file_name())
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.branch = refs["headBranch"].as_str().map(str::to_owned);
                        self.head = refs["head"].as_str().map(str::to_owned);
                        // 読み直したら、選んでいたコミットを選び直す。
                        let selected = self.history.selected_id();
                        self.history.opened(&refs, &history);
                        self.sidebar.set_refs(&refs);
                        self.worktrees_opened(&refs);
                        // タグの署名の印（アプリのタグの行の印）。タグが変わったときだけ読む（検証の結果はコアが覚える）。
                        // ブランチの分け方（お気に入り・積んだもの・Inactive・Archived）。
                        self.send(Request::Run {
                            name: "branchGroups",
                            command: json!({"op": "branchGroups"}),
                        });
                        // subtree の一覧（git subtree があれば、サイドバーに Subtrees の節）。
                        self.send(Request::Run {
                            name: "subtreeList",
                            command: json!({"op": "subtrees"}),
                        });
                        // リポジトリの形（shallow・partial clone、sparse checkout）の印。
                        self.send(Request::Run {
                            name: "repositoryForm",
                            command: json!({"op": "repositoryForm"}),
                        });
                        // submodule の様子の印（取り出していない・別のコミット・中に変更・コンフリクト）。
                        if !self.sidebar.submodule_paths().is_empty() {
                            self.send(Request::Run {
                                name: "submoduleMarks",
                                command: json!({"op": "submodules"}),
                            });
                        }
                        let tags = self.sidebar.tag_names();
                        // タグのリモートでの様子（fetch のときに読んで覚えたもの）。
                        if !tags.is_empty() {
                            self.send(Request::Run {
                                name: "tagRemoteMarks",
                                command: json!({"op": "tagRemotes"}),
                            });
                        }
                        if !tags.is_empty() && tags != self.tag_signature_names {
                            self.tag_signature_names = tags.clone();
                            self.send(Request::Run {
                                name: "tagSignatures",
                                command: json!({"op": "tagSignatures", "names": tags}),
                            });
                        }
                        self.remotes = refs["remotes"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|r| r["name"].as_str().or(r.as_str()))
                            .map(str::to_owned)
                            .collect();
                        self.stash = refs["entries"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|e| e["kind"] == "stash")
                            .and_then(|e| e["name"].as_str())
                            .map(str::to_owned);
                        // 合うものだけにしていれば、すべての行が合った行。でなければ、検索していたら新しい History で
                        // 検索し直す。
                        if let Some(query) = self.matching.clone() {
                            self.cancel_search();
                            self.search = Some((query, None));
                            self.history
                                .set_matches((0..self.history.total()).collect());
                        } else if let Some((query, _)) = self.search.clone() {
                            self.start_search(query);
                        }
                        if let Some(id) = selected {
                            self.send(Request::RowOf {
                                history: self.history.id().clone(),
                                id: id.to_string(),
                            });
                        }
                        // 印も、同じコミットにつけ直す。
                        let marks = self.history.take_remap();
                        if !marks.is_empty() {
                            self.send(Request::MarksOf {
                                history: self.history.id().clone(),
                                ids: marks.iter().map(|m| m.id.to_string()).collect(),
                            });
                            self.remapping = marks;
                        }
                        self.opening = false;
                        if std::mem::take(&mut self.rebuilding) {
                            self.notice = Some(tr!("Rebuilt History"));
                        }
                        if std::mem::take(&mut self.reopen) {
                            self.open();
                        }
                    }
                    Loaded::Rows {
                        history,
                        start,
                        reply,
                    } => {
                        self.history.loaded(&history, start, &reply);
                        self.verify_signatures();
                    }
                    Loaded::Search { query, result } => {
                        // 取り消した検索（語を替えた・やめた）の答えは捨てる。
                        if let Some((current, running)) = &mut self.search
                            && *current == query
                            && running.take().is_some()
                        {
                            match result {
                                Ok(rows) => self.history.set_matches(rows),
                                Err(error) => {
                                    self.search = None;
                                    self.error = Some(error);
                                }
                            }
                        }
                    }
                    Loaded::File {
                        path,
                        blame,
                        result,
                    } => {
                        if let Some(error) = self.file.set(&path, blame, result) {
                            self.error = Some(error);
                        }
                    }
                    Loaded::Reflog(name, reply) => {
                        if let Some(error) = self.reflog.set(name, reply) {
                            self.error = Some(error);
                        }
                    }
                    Loaded::Lineage { row, bits } => self.history.set_lineage(row, bits),
                    Loaded::MarksOf(rows) => {
                        let marks = std::mem::take(&mut self.remapping);
                        self.history.remapped(marks, rows);
                    }
                    Loaded::RowOf(row) => {
                        if let Some(row) = row {
                            self.history.select(row);
                        }
                    }
                    Loaded::Commit { id, result } => {
                        if let Some(request) = self.details.loaded(&id, result, &self.clock) {
                            self.send(request);
                        }
                    }
                    Loaded::Syntax { id, reply } => {
                        if let Some(reply) = &reply {
                            self.changes.syntax(&id, reply);
                        }
                        self.details.syntax(&id, reply.as_ref());
                    }
                    Loaded::Status(Ok(status)) => {
                        self.upstream = status["upstream"].as_str().map(str::to_owned);
                        self.ahead = status["ahead"].as_u64().unwrap_or(0);
                        self.behind = status["behind"].as_u64().unwrap_or(0);
                        let sequence = &status["sequence"];
                        self.sequence = sequence["kind"].as_str().map(|kind| {
                            let progress =
                                sequence["current"].as_u64().zip(sequence["total"].as_u64());
                            (kind.to_owned(), progress)
                        });
                        self.retry_three_way = sequence["kind"] == "am"
                            && sequence["threeWay"] == false
                            && sequence["empty"] != true
                            && sequence["retried"] != true;
                        self.changes.status(&status);
                        // 行ごとの行数は、一覧を出してから裏で（アプリと同じ）。
                        self.send(Request::Background {
                            name: "workingStats",
                            command: json!({"op": "workingStats"}),
                        });
                        self.count_open_worktree();
                        self.sidebar.set_conflicts(self.changes.conflicts());
                        // Commit and Push: コミットの後の状態（上流より進んだ数）が届いたら、push を確かめる。
                        if !self.committing && std::mem::take(&mut self.push_after_commit) {
                            self.ask_push();
                        }
                    }
                    Loaded::Ran { name, result } => self.ran(name, result),
                    Loaded::Status(Err(error)) => {
                        self.changes.status_failed();
                        self.error = Some(error);
                    }
                    Loaded::WorkingDiff {
                        path,
                        serial,
                        result,
                    } => {
                        if let Some(request) = self.changes.working_diff(path, serial, result) {
                            self.send(request);
                        }
                    }
                    Loaded::Committed(result) => {
                        self.committing = false;
                        match result {
                            Ok(output) => {
                                self.changes.message.clear();
                                self.changes.amend = false;
                                // 最近のメッセージは、次に使うときに読み直す（今のコミットも入る）。
                                self.recent_messages = None;
                                self.recalled = None;
                                // コミットしたので、書きかけのメッセージを消す。
                                if self.draft_saved.as_deref().is_some_and(|d| !d.is_empty()) {
                                    self.draft_saved = Some(String::new());
                                    self.send(Request::Run {
                                        name: "saveCommitDraft",
                                        command: json!({"op": "saveCommitDraft", "text": ""}),
                                    });
                                }
                                self.focus = Focus::Main;
                                // hooks が何か書いていれば、その 1 行目を出す。2 行以上なら、`!` で全部を
                                // 出せる（アプリの hooks の出力のシートの代わり）。
                                let more = self.remember_hook_output(&output);
                                self.notice =
                                    Some(match output.lines().find(|l| !l.trim().is_empty()) {
                                        Some(line) => {
                                            tr!("Committed: {output}", output = line.trim()) + &more
                                        }
                                        None => tr!("Committed"),
                                    });
                                self.changes.refresh();
                                self.open();
                            }
                            Err(error) => {
                                self.push_after_commit = false;
                                // hooks に断られたなら、その出力を全部見られるように。
                                let more = self.remember_hook_output(&error);
                                let first = error
                                    .lines()
                                    .find(|l| !l.trim().is_empty())
                                    .unwrap_or("")
                                    .trim()
                                    .to_owned();
                                self.error = Some(first + &more);
                            }
                        }
                    }
                    Loaded::Message(message) if self.rewording.is_some() => {
                        // 書き換えるコミットの今のメッセージ: エディタで開く（`take_edit_message`）。
                        match message {
                            Some(message) => {
                                if let Some(rewording) = &mut self.rewording {
                                    rewording.1 = Some(message);
                                }
                            }
                            None => {
                                self.rewording = None;
                                self.error = Some(tr!("Can’t read the commit’s message"));
                            }
                        }
                    }
                    Loaded::Message(message) => {
                        // 待つ間に書き始めていたら、そのままにする。
                        if let Some(message) = message
                            && self.changes.amend
                            && self.changes.message.is_empty()
                        {
                            self.changes.message.set_text(&message);
                        }
                    }
                    Loaded::Staged(result) => {
                        self.changes.clear_mark();
                        if let Err(error) = result {
                            self.error = Some(error);
                        }
                        // 変化の知らせを待たずに読み直す。
                        self.changes.refresh();
                    }
                }
            }
            // 取り消した検索（語を替えた・やめた）のものは捨てる。
            Event::Partial { query, rows, done } => {
                if let Some((current, Some(_))) = &self.search
                    && *current == query
                {
                    self.search_done = done;
                    self.history.set_partial_matches(rows);
                    self.dirty = true;
                }
            }
            // clone できたら、そのリポジトリを開く（Quick Open で選んだときと同じ）。
            Event::CountWorktrees => self.read_worktree_counts(),
            Event::Cloned(result) => {
                self.pending = self.pending.saturating_sub(1);
                self.running = None;
                match result {
                    Ok(dest) => {
                        self.open_location = Some(dest);
                        self.quit = true;
                    }
                    Err(error) => self.error = Some(error),
                }
                self.dirty = true;
            }
            Event::Scanned(paths) => {
                if let Some(quick) = &mut self.quick_open {
                    quick.add(paths);
                    self.dirty = true;
                }
            }
            Event::Failed(error) => {
                self.pending = self.pending.saturating_sub(1);
                self.history.failed();
                self.opening = false;
                self.rebuilding = false;
                self.error = Some(error);
                self.dirty = true;
            }
        }
        self.request();
    }

    /// 画面に出すのに足りないものを頼む。
    fn request(&mut self) {
        // 見える範囲が動いたら、足りない行を頼む。
        if let Some(request) = self.history.request() {
            self.send(request);
        }
        if let Some(request) = self.history.lineage_request() {
            self.send(request);
        }
        let side = self.layout().side.is_some();
        // History で 2 つ以上に印をつけていれば、その差分を出す（アプリの複数選択）。
        let selection = (side
            && self.view == View::History
            && self.pinned.is_none()
            && self.history.mark_count() >= 2)
            .then(|| Want::Selection {
                history: self.history.id().clone(),
                rows: self.history.marks().map(|(row, _)| row).collect(),
                pair: self.pair,
                commits: self.history.marks().map(|(_, m)| m.clone()).collect(),
            });
        if let Some(want) = selection {
            if self.details.want(want) {
                self.damage |= Damage::SIDE;
            }
        } else if side
            && let Some(id) = match self.view {
                View::History => self.pinned.or(self.history.selected_id()),
                View::Reflog => self.reflog.selected_id(),
                View::File => self.file.selected_id(),
                View::Changes => None,
            }
        {
            let stash = self.view == View::History && self.pinned == Some(id);
            // 詳細の中だけが変わる。
            if self.details.want(Want::Commit { id, stash }) {
                self.damage |= Damage::SIDE;
            }
        }
        if let Some(request) = self.file.request() {
            self.send(request);
        }
        if let Some(request) = self.reflog.request(self.view == View::Reflog) {
            self.send(request);
        }
        if let Some(request) = self.details.request() {
            self.send(request);
        }
        while let Some(request) = self.changes.request(side && self.view == View::Changes) {
            self.send(request);
        }
    }

    fn key(&mut self, key: KeyEvent) {
        // Windows は離したときも知らせる。
        if key.kind == KeyEventKind::Release {
            return;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        // 画面が乱れたときに、どこからでもすべて書き直す（中身は読み直さない）。
        if control && key.code == KeyCode::Char('l') {
            self.redraw = true;
            self.dirty = true;
            return;
        }
        // アプリの Switch Branch…: 打った語で絞って切り替える（最近のものを先に）。
        if control && key.code == KeyCode::Char('b') {
            self.open_branch_pick();
            return;
        }
        // アプリの Refresh: Working Copy・ref・History を読み直す（ふだんは変わったら読み直している）。
        if control && key.code == KeyCode::Char('r') {
            self.refresh();
            return;
        }
        // 詳細・差分が、一覧の所に開いている（並んでいない）。
        let covering = self.focus == Focus::Side && self.layout().main.is_none();
        // エラーは、次のキーまで出しておく。
        self.dirty |= self.error.take().is_some() | self.notice.take().is_some();
        // push を確かめている間の `t`: 送り先のブランチを打つ。
        if std::mem::take(&mut self.push_to_offered) && key.code == KeyCode::Char('t') {
            self.confirm = None;
            self.choices = None;
            self.ask_push_to();
            self.dirty = true;
            return;
        }
        // パッチを書く前の `v`: 版の番号を打つ。ほかのキーなら、聞いていたことは忘れる。
        if self.choices.is_some()
            && let Some(pending) = self.patch_version.take()
            && key.code == KeyCode::Char('v')
        {
            self.choices = None;
            self.patch_version = Some(pending);
            self.naming = Naming::PatchVersion;
            self.typing_branch = Some((String::new(), String::new(), "2".to_owned()));
            self.dirty = true;
            return;
        }
        // octopus merge を確かめている間の `l`: コンフリクトしそうなものを最後に回して見直す（アプリの Put Last）。
        if self.choices.is_some()
            && let Some(order) = self.octopus_last.take()
            && key.code == KeyCode::Char('l')
        {
            self.choices = None;
            self.preview_octopus(order);
            self.dirty = true;
            return;
        }
        // clone の取り方・一緒にすることを選んでいる間（`ask_clone_download` と `ask_clone_extras`）。
        if self.choices.is_some()
            && let Some(mut plan) = self.clone_plan.take()
        {
            match (plan.stage, key.code) {
                (0, KeyCode::Char(ch @ ('y' | 'c' | 'r'))) => {
                    plan.download = ch;
                    plan.stage = 1;
                    self.choices = None;
                    self.ask_clone_extras(plan);
                }
                (1, KeyCode::Char(ch @ ('y' | 's' | 'f' | 'a'))) => {
                    self.choices = None;
                    self.start_clone(plan, ch);
                }
                _ => self.choices = None,
            }
            self.dirty = true;
            return;
        }
        // shallow clone を取り足す前の `d`: 日付を打つ。
        if self.choices.is_some()
            && std::mem::take(&mut self.deepen_date_offered)
            && key.code == KeyCode::Char('d')
        {
            self.choices = None;
            self.naming = Naming::DeepenSince;
            self.typing_branch = Some((String::new(), String::new(), String::new()));
            self.dirty = true;
            return;
        }
        // worktree をロックする前の `r`: 理由を打つ。
        if self.choices.is_some()
            && let Some(path) = self.worktree_lock.take()
            && key.code == KeyCode::Char('r')
        {
            self.choices = None;
            self.naming = Naming::WorktreeLockReason;
            self.typing_branch = Some((path, String::new(), String::new()));
            self.dirty = true;
            return;
        }
        // bundle を書く前の `a`: 入れない基点を打つ（初めは今のブランチの上流）。
        if self.choices.is_some()
            && let Some(file) = self.bundle_save.take()
            && key.code == KeyCode::Char('a')
        {
            self.choices = None;
            self.bundle_save = Some(file);
            self.naming = Naming::BundleBase;
            let start = self.upstream.clone().unwrap_or_default();
            self.typing_branch = Some((String::new(), String::new(), start));
            self.dirty = true;
            return;
        }
        // アーカイブを書く前の `o`: 入れるフォルダを打つ。
        if self.choices.is_some()
            && let Some(pending) = self.archive_folder.take()
            && key.code == KeyCode::Char('o')
        {
            self.choices = None;
            self.archive_folder = Some(pending);
            self.naming = Naming::ArchiveFolder;
            self.typing_branch = Some((String::new(), String::new(), String::new()));
            self.dirty = true;
            return;
        }
        // 確かめている間は、`y`（か Enter）で行い、ほかのキーでやめる。答えたら、重ねていた絵も消す。
        if self.choices.is_some() || self.confirm.is_some() {
            self.question_drawing = None;
        }
        if let Some((_, options)) = self.choices.take() {
            if let KeyCode::Char(pressed) = key.code
                && let Some((_, _, request)) = options.into_iter().find(|o| o.0 == pressed)
            {
                self.send(request);
            }
            self.dirty = true;
            return;
        }
        if let Some((_, _, request)) = self.confirm.take() {
            if matches!(key.code, KeyCode::Char('y' | 'Y') | KeyCode::Enter) {
                self.send(request);
            }
            self.dirty = true;
            return;
        }
        // 尋ねられている間は、Enter で答える、Esc でやめる（尋ねた操作は失敗する）。
        #[cfg(unix)]
        if let Some(question) = self.questions.front() {
            self.dirty = true;
            match key.code {
                KeyCode::Enter => {
                    let _ = question.reply.send(Some(std::mem::take(&mut self.answer)));
                    self.questions.pop_front();
                }
                KeyCode::Esc => {
                    let _ = question.reply.send(None);
                    self.answer.clear();
                    self.questions.pop_front();
                }
                KeyCode::Backspace => {
                    self.answer.pop();
                }
                KeyCode::Char(ch) if !control => self.answer.push(ch),
                _ => {}
            }
            return;
        }
        // hooks の出力の枠も、`j` `k` で動かし、ほかのキーで閉じる。
        if let Some(top) = self.showing_hooks {
            self.dirty = true;
            self.showing_hooks = match key.code {
                KeyCode::Char('j') | KeyCode::Down => Some(top + 1),
                KeyCode::Char('k') | KeyCode::Up => Some(top.saturating_sub(1)),
                _ => None,
            };
            return;
        }
        // キーの一覧は、`j` `k`（と矢印）で動かせる。ほかのキーで閉じる。
        if self.help {
            self.dirty = true;
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => self.help_top += 1,
                KeyCode::Char('k') | KeyCode::Up => self.help_top = self.help_top.saturating_sub(1),
                _ => self.help = false,
            }
            return;
        }
        // Daemon: `S` で起動、`X` で止める、`r` で読み直す、`Tab` で操作ログへ。ほかのキーで閉じる。
        if self.daemon.is_some() {
            self.dirty = true;
            match key.code {
                KeyCode::Char('r') => self.read_daemon(),
                KeyCode::Char('S') => {
                    match crate::daemon_program() {
                        Some(program) => match amonhen_remote::local::start(program) {
                            Ok(()) => self.notice = Some(tr!("Started the shared daemon")),
                            Err(e) => self.error = Some(e.to_string()),
                        },
                        None => self.error = Some(tr!("Start it with amonhen daemon start")),
                    }
                    self.read_daemon();
                }
                KeyCode::Char('X') => {
                    match amonhen_remote::local::control("stop") {
                        Ok(Some(_)) => self.notice = Some(tr!("Stopped the shared daemon")),
                        Ok(None) => self.notice = Some(tr!("The shared daemon isn’t running")),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                    self.read_daemon();
                }
                KeyCode::Tab => {
                    self.daemon = None;
                    self.run(
                        "undoLog",
                        tr!("Reading the operation log…"),
                        json!({"op": "undoLog"}),
                    );
                }
                _ => self.daemon = None,
            }
            return;
        }
        // Performance: `Tab` で Events へ。ほかのキーで閉じる。
        if self.showing_performance {
            self.dirty = true;
            self.showing_performance = false;
            if key.code == KeyCode::Tab {
                self.showing_events = Some(0);
            }
            return;
        }
        // Events: j/k で動かす、`Tab` で Daemon へ。ほかのキーで閉じる。
        if let Some(top) = self.showing_events {
            self.dirty = true;
            let last = self.event_log.len().saturating_sub(1);
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.showing_events = Some((top + 1).min(last))
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.showing_events = Some(top.saturating_sub(1))
                }
                KeyCode::Tab => {
                    self.showing_events = None;
                    self.read_daemon();
                }
                _ => self.showing_events = None,
            }
            return;
        }
        // History Cache: `r` で読み直す、`Tab` で Events へ。ほかのキーで閉じる。
        if self.history_cache.is_some() {
            self.dirty = true;
            match key.code {
                KeyCode::Char('r') => {
                    self.send(Request::Run {
                        name: "historyCache",
                        command: json!({"op": "historyCache"}),
                    });
                }
                KeyCode::Tab => {
                    self.history_cache = None;
                    self.showing_performance = true;
                }
                _ => self.history_cache = None,
            }
            return;
        }
        // Operations: 選んでいるものの出力を下に出す。`x` で取り消す、`r` で読み直す、`Tab` で操作ログへ。
        if let Some(mut ops) = self.operations.take() {
            self.dirty = true;
            let last = ops.entries.len().saturating_sub(1);
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => ops.selected = (ops.selected + 1).min(last),
                KeyCode::Char('k') | KeyCode::Up => ops.selected = ops.selected.saturating_sub(1),
                KeyCode::Char('g') | KeyCode::Home => ops.selected = 0,
                KeyCode::Char('G') | KeyCode::End => ops.selected = last,
                KeyCode::Char('r') => {
                    self.read_operations();
                    return;
                }
                // 記録を JSON のファイルに書き出す（この機械に。アプリの Export JSON…）。
                KeyCode::Char('s') => {
                    self.export_records = Some(ops.records.clone());
                    self.naming = Naming::ActivityExport;
                    self.typing_branch = Some((
                        String::new(),
                        String::new(),
                        "amonhen-activity.json".to_owned(),
                    ));
                    return;
                }
                KeyCode::Tab => {
                    self.send(Request::Run {
                        name: "historyCache",
                        command: json!({"op": "historyCache"}),
                    });
                    return;
                }
                KeyCode::Char('x') => match ops.entries.get(ops.selected) {
                    Some(op) if op.ok.is_none() && op.cancellable => {
                        self.send(Request::Run {
                                name: "cancelActivity",
                                command: json!({"op": "cancelActivity", "seq": op.seq, "remote": op.remote}),
                            });
                    }
                    Some(_) => {
                        self.notice =
                            Some(tr!("Only a running operation that can stop is cancelled"))
                    }
                    None => {}
                },
                _ => return,
            }
            self.operations = Some(ops);
            return;
        }
        // 操作ログ: 選んでいる記録まで（新しいものから順に）戻す。確かめてから。ほかのキーで閉じる。
        if let Some(mut log) = self.operation_log.take() {
            self.dirty = true;
            if key.code == KeyCode::Tab {
                self.read_operations();
                return;
            }
            if log.entries.is_empty() {
                return;
            }
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    log.selected = (log.selected + 1).min(log.entries.len().saturating_sub(1));
                }
                KeyCode::Char('k') | KeyCode::Up => log.selected = log.selected.saturating_sub(1),
                KeyCode::Char('n') if control => {
                    log.selected = (log.selected + 1).min(log.entries.len().saturating_sub(1));
                }
                KeyCode::Char('p') if control => log.selected = log.selected.saturating_sub(1),
                KeyCode::Enter => {
                    let count = log.selected + 1;
                    let ask = match count {
                        1 => tr!("Undo “{label}”?", label = log.entries[0].0),
                        _ => tr!("Undo the last {count} operations?", count = count),
                    };
                    self.confirm = Some((
                        ask,
                        tr!("undo"),
                        Request::Run {
                            name: "undo",
                            command: Value::Array(vec![json!({"op": "undo"}); count]),
                        },
                    ));
                    return;
                }
                _ => return,
            }
            self.operation_log = Some(log);
            return;
        }
        if self.edit_mode.is_some() {
            self.edit_mode_key(key.code);
            return;
        }
        // Quick Open: 打った語で絞る。Enter で選んだリポジトリを開き直す、Esc でやめる。
        // Fixup・Squash Commit For と最近のメッセージ: 打った語で絞り、Enter で欄に入れる。Tab で fixup! と
        // squash! を切り替える。
        if let Some(mut pick) = self.fixup_pick.take() {
            self.dirty = true;
            match key.code {
                KeyCode::Esc => return,
                KeyCode::Enter => {
                    let chosen = (pick.shown().get(pick.selected))
                        .and_then(|(i, _)| pick.items.as_ref().map(|items| items[*i].clone()));
                    if let Some(chosen) = chosen {
                        match pick.kind {
                            PickKind::Recent => self.use_message(&chosen),
                            PickKind::Insert => self.insert_into_message(&chosen),
                            kind => self.use_fixup(&chosen, kind == PickKind::Squash),
                        }
                    }
                    return;
                }
                KeyCode::Tab => {
                    pick.kind = match pick.kind {
                        PickKind::Fixup => PickKind::Squash,
                        PickKind::Squash => PickKind::Fixup,
                        kind => kind,
                    }
                }
                KeyCode::Down => pick.selected += 1,
                KeyCode::Char('n') if control => pick.selected += 1,
                KeyCode::Up => pick.selected = pick.selected.saturating_sub(1),
                KeyCode::Char('p') if control => pick.selected = pick.selected.saturating_sub(1),
                KeyCode::Backspace => {
                    pick.query.pop();
                    pick.selected = 0;
                }
                KeyCode::Char(ch) if !control => {
                    pick.query.push(ch);
                    pick.selected = 0;
                }
                _ => {}
            }
            pick.selected = pick.selected.min(pick.shown().len().saturating_sub(1));
            self.fixup_pick = Some(pick);
            return;
        }
        if let Some(mut pick) = self.branch_pick.take() {
            self.dirty = true;
            match key.code {
                KeyCode::Esc => return,
                KeyCode::Enter => {
                    match pick.shown().get(pick.selected) {
                        Some(BranchChoice::Branch(name, false)) => {
                            let name = name.to_string();
                            self.run(
                                "checkout",
                                tr!("Checking out {name}…", name = name),
                                json!({"op": "checkout", "branch": name}),
                            );
                        }
                        Some(BranchChoice::Branch(name, true)) => {
                            let name = name.to_string();
                            self.run(
                                "checkout",
                                tr!("Checking out {name}…", name = name),
                                json!({"op": "track", "remoteBranch": name}),
                            );
                        }
                        Some(BranchChoice::New(name)) => {
                            let name = name.to_string();
                            self.run(
                                "createBranch",
                                tr!("Creating {name}…", name = name),
                                json!({"op": "createBranch", "name": name, "start": "HEAD",
                                    "checkout": true}),
                            );
                        }
                        None => {}
                    }
                    return;
                }
                KeyCode::Down => pick.selected += 1,
                KeyCode::Char('n') if control => pick.selected += 1,
                KeyCode::Up => pick.selected = pick.selected.saturating_sub(1),
                KeyCode::Char('p') if control => pick.selected = pick.selected.saturating_sub(1),
                KeyCode::Backspace => {
                    pick.query.pop();
                    pick.selected = 0;
                }
                KeyCode::Char(ch) if !control => {
                    pick.query.push(ch);
                    pick.selected = 0;
                }
                _ => {}
            }
            pick.selected = pick.selected.min(pick.shown().len().saturating_sub(1));
            self.branch_pick = Some(pick);
            return;
        }
        if let Some(mut quick) = self.quick_open.take() {
            self.dirty = true;
            match key.code {
                KeyCode::Esc => return,
                KeyCode::Enter => {
                    // URL（か bundle のファイル）を打っていれば、それを clone する（アプリの Clone…）。
                    if clonable(quick.query.trim()) {
                        self.ask_clone_destination(quick.query.trim());
                        return;
                    }
                    if let Some(candidate) = quick.shown().get(quick.selected) {
                        self.open_location = Some(candidate.path.clone());
                        self.quit = true;
                    }
                    return;
                }
                KeyCode::Down => quick.selected += 1,
                KeyCode::Char('n') if control => quick.selected += 1,
                KeyCode::Up => quick.selected = quick.selected.saturating_sub(1),
                KeyCode::Char('p') if control => quick.selected = quick.selected.saturating_sub(1),
                KeyCode::Backspace => {
                    quick.query.pop();
                    quick.selected = 0;
                }
                KeyCode::Char(ch) if !control => {
                    quick.query.push(ch);
                    quick.selected = 0;
                }
                _ => {}
            }
            quick.selected = quick.selected.min(quick.shown().len().saturating_sub(1));
            self.quick_open = Some(quick);
            return;
        }
        // コマンドの一覧は、打った語で絞る。Enter で選んでいる行を動かす、Esc でやめる。
        if let Some(mut palette) = self.palette.take() {
            self.dirty = true;
            match key.code {
                KeyCode::Esc => return,
                KeyCode::Enter => {
                    let entries = self.palette_entries(&palette.query);
                    if let Some((entry, _)) = entries.get(palette.selected) {
                        self.run_entry(entry);
                    }
                    return;
                }
                KeyCode::Down => palette.selected += 1,
                KeyCode::Char('n') if control => palette.selected += 1,
                KeyCode::Up => palette.selected = palette.selected.saturating_sub(1),
                KeyCode::Char('p') if control => {
                    palette.selected = palette.selected.saturating_sub(1);
                }
                KeyCode::Backspace => {
                    palette.query.pop();
                    palette.selected = 0;
                }
                KeyCode::Char(ch) if !control => {
                    palette.query.push(ch);
                    palette.selected = 0;
                }
                _ => {}
            }
            let count = self.palette_entries(&palette.query).len();
            palette.selected = palette.selected.min(count.saturating_sub(1));
            self.palette = Some(palette);
            return;
        }
        // 絞り込みの語は、打つたびに効く。Enter で確定、Esc で絞り込みをやめる。
        if let Some(filter) = &mut self.typing_filter {
            self.dirty = true;
            let changes = self.filtering_changes;
            let mut apply = |filter: &str| match changes {
                true => self.changes.set_filter(filter),
                false => self.sidebar.set_filter(filter),
            };
            match key.code {
                KeyCode::Enter => self.typing_filter = None,
                KeyCode::Esc => {
                    apply("");
                    self.typing_filter = None;
                }
                KeyCode::Backspace => {
                    filter.pop();
                    let filter = filter.clone();
                    apply(&filter);
                }
                KeyCode::Char(ch) if !control => {
                    filter.push(ch);
                    let filter = filter.clone();
                    apply(&filter);
                }
                _ => {}
            }
            return;
        }
        // 新しいブランチの名前を打っている間は、Enter で作って切り替える、Esc でやめる。
        if let Some((_, _, name)) = &mut self.typing_branch {
            self.dirty = true;
            // 候補は、次に補うまで。
            self.path_choices.clear();
            match key.code {
                // 追うブランチ: 空なら既定に戻す。
                KeyCode::Enter if matches!(self.naming, Naming::SubmoduleBranch) => {
                    if let Some((path, _, branch)) = self.typing_branch.take() {
                        let branch = branch.trim();
                        self.run(
                            "submoduleChanged",
                            tr!("Changing the branch to follow…"),
                            json!({"op": "changeSubmodule", "change": {"kind": "setBranch", "path": path,
                                "branch": (!branch.is_empty()).then_some(branch)}}),
                        );
                    }
                }
                // 取り出すフォルダ: 空ならすべて（sparse checkout をやめる）。
                KeyCode::Enter if matches!(self.naming, Naming::SparseFolders) => {
                    if let Some((_, _, typed)) = self.typing_branch.take() {
                        self.ask_sparse_folders(&typed);
                    }
                }
                KeyCode::Enter if matches!(self.naming, Naming::Stash) => {
                    if let Some((_, _, message)) = self.typing_branch.take() {
                        self.ask_stash(message.trim());
                    }
                }
                KeyCode::Enter => {
                    if let Some((start, what, name)) = self.typing_branch.take()
                        && !name.trim().is_empty()
                    {
                        let name = name.trim();
                        let (op, label, command) = match self.naming {
                            Naming::Stash | Naming::SparseFolders | Naming::SubmoduleBranch => {
                                return;
                            }
                            Naming::CloneDest => {
                                self.ask_clone_download(&start, &local_path(name));
                                return;
                            }
                            // アプリと同じ項目だけを、読みやすい JSON で。
                            Naming::ActivityExport => {
                                let records = self.export_records.take().unwrap_or_default();
                                let keep = [
                                    "seq", "kind", "repo", "label", "started", "duration", "ok",
                                    "error",
                                ];
                                let records: Vec<Value> =
                                    (records.as_array().into_iter().flatten())
                                        .map(|r| {
                                            keep.iter()
                                                .map(|k| ((*k).to_owned(), r[*k].clone()))
                                                .collect::<serde_json::Map<_, _>>()
                                                .into()
                                        })
                                        .collect();
                                let path = local_path(name);
                                let text =
                                    serde_json::to_string_pretty(&records).unwrap_or_default();
                                match std::fs::write(&path, text) {
                                    Ok(()) => {
                                        self.notice = Some(tr!(
                                            "Wrote {count} records to {path}",
                                            count = records.len(),
                                            path = path
                                        ))
                                    }
                                    Err(e) => self.error = Some(e.to_string()),
                                }
                                return;
                            }
                            Naming::SubmoduleUrl => (
                                "submoduleChanged",
                                tr!("Changing the URL…"),
                                json!({"op": "changeSubmodule", "change": {"kind": "setUrl", "path": start, "url": name}}),
                            ),
                            Naming::SubmoduleForeach => (
                                "submoduleForeach",
                                tr!("Running in each submodule…"),
                                json!({"op": "changeSubmodule", "change": {"kind": "foreach", "command": name, "recursive": false}}),
                            ),
                            // URL の次に、場所を聞く（初めは URL の最後の名前）。
                            Naming::SubmoduleAddUrl => {
                                let base = name
                                    .trim_end_matches('/')
                                    .rsplit(['/', '\\', ':'])
                                    .next()
                                    .unwrap_or(name);
                                let base = base.strip_suffix(".git").unwrap_or(base).to_owned();
                                self.naming = Naming::SubmoduleAddPath;
                                self.typing_branch = Some((name.to_owned(), name.to_owned(), base));
                                self.dirty = true;
                                return;
                            }
                            Naming::SubmoduleAddPath => (
                                "submoduleChanged",
                                tr!("Adding the submodule…"),
                                json!({"op": "changeSubmodule", "change": {"kind": "add", "url": start, "path": name, "branch": null}}),
                            ),
                            // 範囲は 1 つ（A...B）、2 つ（A..B C..D）か 3 つ（base A B）。
                            Naming::RangeDiffRanges => {
                                let ranges: Vec<String> =
                                    name.split_whitespace().map(str::to_owned).collect();
                                if !(1..=3).contains(&ranges.len()) {
                                    self.error = Some(tr!(
                                        "Write the ranges as git range-diff takes them: A...B, A..B C..D, or base A B"
                                    ));
                                    return;
                                }
                                let title =
                                    tr!("Range Diff of {ranges}", ranges = ranges.join(" "));
                                self.compare_ranges(title, ranges);
                                return;
                            }
                            Naming::SubtreeDir => {
                                let dir = name.trim_matches('/').to_owned();
                                self.subtree_source(&dir, "add");
                                return;
                            }
                            Naming::SubtreeSource => {
                                let mut parts = name.split_whitespace();
                                let (Some(url), Some(r), None) =
                                    (parts.next(), parts.next(), parts.next())
                                else {
                                    self.error = Some(tr!(
                                        "Write the URL and the branch or tag, like https://example.com/lib.git main"
                                    ));
                                    return;
                                };
                                self.ask_subtree(&start, &what, url, r);
                                return;
                            }
                            Naming::SubtreeSplit => (
                                "subtreeChanged",
                                tr!("Splitting the subtree…"),
                                json!({"op": "subtree", "action": {"kind": "split", "dir": start, "branch": name}}),
                            ),
                            Naming::PushTo => {
                                self.push_to(&start, name);
                                return;
                            }
                            Naming::Orphan => {
                                self.ask_orphan(name);
                                return;
                            }
                            Naming::RemoteRename => {
                                if name != start && self.remotes.iter().any(|r| r == name) {
                                    self.error =
                                        Some(tr!("There is a remote named {name}", name = name));
                                    return;
                                }
                                let url = self
                                    .editing_remote
                                    .as_ref()
                                    .and_then(|(_, url)| url.clone())
                                    .unwrap_or_default();
                                self.naming = Naming::RemoteEditUrl;
                                self.typing_branch = Some((start.clone(), name.to_owned(), url));
                                return;
                            }
                            Naming::RemoteEditUrl => {
                                let old_url = self
                                    .editing_remote
                                    .take()
                                    .and_then(|(_, url)| url)
                                    .unwrap_or_default();
                                let mut changes = Vec::new();
                                if what != start {
                                    changes.push(json!({"op": "changeRemote",
                                        "change": {"kind": "rename", "from": start, "to": what}}));
                                }
                                if name != old_url {
                                    changes.push(json!({"op": "changeRemote",
                                        "change": {"kind": "setUrl", "name": what, "url": name}}));
                                }
                                if changes.is_empty() {
                                    self.notice = Some(tr!("Nothing changed"));
                                    return;
                                }
                                (
                                    "editRemote",
                                    tr!("Changing {name}…", name = start),
                                    Value::Array(changes),
                                )
                            }
                            Naming::RemoteName => {
                                if self.remotes.iter().any(|r| r == name) {
                                    self.error =
                                        Some(tr!("There is a remote named {name}", name = name));
                                    return;
                                }
                                self.naming = Naming::RemoteUrl;
                                self.typing_branch =
                                    Some((name.to_owned(), name.to_owned(), String::new()));
                                return;
                            }
                            Naming::Archive => {
                                self.export_archive(&start, name, None);
                                return;
                            }
                            Naming::BundleFile => {
                                let file = local_path(name);
                                self.bundle_file = Some(file.clone());
                                (
                                    "bundleCheck",
                                    tr!("Reading the bundle…"),
                                    json!({"op": "bundleCheck", "file": file}),
                                )
                            }
                            Naming::OpenFile => {
                                self.open_file_view(name.trim_end_matches('/'), false);
                                return;
                            }
                            // 場所の次に、アプリの Save as Patch のシートの選択肢を聞く: そのまま、送り状を付けて、
                            // 版の番号を付けて（`-v`）。`.mbox` で終わる名前なら、1 つのファイルにまとめる。
                            Naming::SavePatch => {
                                self.ask_save_patch(&start, &local_path(name), None);
                                return;
                            }
                            Naming::ArchiveFolder => {
                                if let Some((rev, file)) = self.archive_folder.take() {
                                    self.export_archive(&rev, &file, Some(name));
                                }
                                return;
                            }
                            Naming::DeepenSince => {
                                self.send(Request::Run {
                                    name: "deepen",
                                    command: json!({"op": "deepen", "since": name.trim()}),
                                });
                                return;
                            }
                            Naming::PatchVersion => {
                                let Some((oids, place)) = self.patch_version.take() else {
                                    return;
                                };
                                match name.trim().parse::<u32>() {
                                    Ok(version) if version >= 1 => {
                                        self.ask_save_patch(&oids, &place, Some(version))
                                    }
                                    _ => {
                                        self.error =
                                            Some(tr!("Write the version as a number, like 2"))
                                    }
                                }
                                return;
                            }
                            Naming::Patch => {
                                let file = local_path(name);
                                self.patch_file = Some(file.clone());
                                (
                                    "readPatch",
                                    tr!("Reading the patch…"),
                                    json!({"op": "readPatch", "paths": [file]}),
                                )
                            }
                            Naming::BundleName => (
                                "fetchBundle",
                                tr!("Fetching from the bundle…"),
                                json!({"op": "fetchBundle", "file": start, "name": name}),
                            ),
                            // 場所の次に、アプリのシートと同じく何を入れるかを選ぶ。
                            Naming::Bundle => {
                                self.ask_bundle(&local_path(name));
                                return;
                            }
                            Naming::WorktreeLockReason => (
                                "lockWorktree",
                                tr!("Locking the worktree…"),
                                json!({"op": "worktreeLock", "path": start, "reason": name}),
                            ),
                            Naming::WorktreeMove => (
                                "moveWorktree",
                                tr!("Moving the worktree…"),
                                json!({"op": "worktreeMove", "path": start, "to": local_path(name)}),
                            ),
                            Naming::WorktreeLocate => (
                                "repairWorktree",
                                tr!("Reconnecting the worktree…"),
                                json!({"op": "worktreeRepair", "paths": [local_path(name)]}),
                            ),
                            Naming::BundleBase => {
                                let Some(file) = self.bundle_save.take() else {
                                    return;
                                };
                                let Some(branch) = self.branch.clone() else {
                                    return;
                                };
                                (
                                    "bundle",
                                    tr!("Saving the bundle…"),
                                    json!({"op": "bundle", "refs": [format!("refs/heads/{branch}")],
                                        "bases": [name], "file": file}),
                                )
                            }
                            Naming::RemoteUrl => (
                                "addRemote",
                                tr!("Adding {name}…", name = start),
                                json!({
                                    "op": "changeRemote",
                                    "change": {"kind": "add", "name": start, "url": name},
                                }),
                            ),
                            Naming::Worktree => (
                                "addWorktree",
                                tr!("Adding the worktree…"),
                                json!({"op": "worktreeAdd", "path": name, "start": start,
                                    "newBranch": self.worktree_branch.take()}),
                            ),
                            // History のコミットから: ブランチの名前の次に、場所を聞く。
                            Naming::WorktreeBranch => {
                                let path =
                                    self.workdir.as_deref().map_or(String::new(), |workdir| {
                                        let workdir = workdir.trim_end_matches(['/', '\\']);
                                        format!("{workdir}-{}", name.replace(['/', '\\'], "-"))
                                    });
                                self.worktree_branch = Some(name.to_owned());
                                self.naming = Naming::Worktree;
                                self.typing_branch = Some((start.clone(), name.to_owned(), path));
                                return;
                            }
                            Naming::StashBranch => (
                                "stashBranch",
                                tr!("Creating {name}…", name = name),
                                json!({"op": "stashBranch", "name": start, "branch": name}),
                            ),
                            Naming::Branch => (
                                "createBranch",
                                tr!("Creating {name}…", name = name),
                                json!({
                                    "op": "createBranch", "name": name, "start": start,
                                    "checkout": true,
                                }),
                            ),
                            Naming::Tag => (
                                "createTag",
                                tr!("Tagging {name}…", name = name),
                                json!({
                                    "op": "createTag", "name": name, "target": start,
                                    "message": null,
                                }),
                            ),
                            Naming::Rename => (
                                "renameBranch",
                                tr!("Renaming to {name}…", name = name),
                                json!({"op": "renameBranch", "from": start, "to": name}),
                            ),
                        };
                        self.run(op, label, command);
                    }
                }
                KeyCode::Esc => self.typing_branch = None,
                // ファイルのパスは、Tab でフォルダの中身から補う（git を動かす機械のもの）。
                KeyCode::Tab if matches!(self.naming, Naming::OpenFile) => {
                    let typed = name.clone();
                    self.complete_path(&typed);
                }
                // 取り出すフォルダは、最後の語を HEAD の木のフォルダから補う。
                KeyCode::Tab if matches!(self.naming, Naming::SparseFolders) => {
                    let last = name.rsplit(' ').next().unwrap_or("");
                    let folder = last.rsplit_once('/').map_or("", |(dir, _)| dir).to_owned();
                    self.send(Request::Call {
                        name: "completePath",
                        request: json!({"op": "run", "command": {"op": "treeFolders", "path": folder, "rev": "HEAD"}}),
                    });
                }
                KeyCode::Backspace => {
                    name.pop();
                }
                // 端末の行と同じく、Ctrl-u で打ったものを消す（初めに入れてあるパスを打ち直すときなど）。
                KeyCode::Char('u') if control => name.clear(),
                KeyCode::Char(ch) if !control => name.push(ch),
                _ => {}
            }
            return;
        }
        // 検索の語を打っている間は、Enter で検索、Esc でやめる。
        if let Some(query) = &mut self.typing_query {
            self.dirty = true;
            match key.code {
                KeyCode::Enter if std::mem::take(&mut self.typing_in_details) => {
                    let query = self.typing_query.take().unwrap_or_default();
                    if query.trim().is_empty() {
                        self.details.clear_found();
                    } else {
                        self.find_in_details(&query, true);
                    }
                }
                KeyCode::Enter => {
                    let query = self.typing_query.take().unwrap_or_default();
                    if query.trim().is_empty() {
                        self.clear_search();
                    } else {
                        self.start_search(query);
                    }
                }
                KeyCode::Esc => {
                    self.typing_query = None;
                    self.typing_in_details = false;
                }
                KeyCode::Backspace => {
                    query.pop();
                }
                // 打ったものを消す（シェルと同じ。ほかの欄と同じく）。
                KeyCode::Char('u') if control => query.clear(),
                KeyCode::Char(ch) if !control => query.push(ch),
                _ => {}
            }
            return;
        }
        // メッセージを書いている間は、文字のキーはすべて入力。
        if self.focus == Focus::Message && !(control && key.code == KeyCode::Char('c')) {
            self.message_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('c') if control => self.quit = true,
            #[cfg(unix)]
            KeyCode::Char('z') if control => self.suspend = true,
            // 変更のないファイルも、パスを打って File History を開く（Tab で補う）。
            KeyCode::Char('O') => {
                self.naming = Naming::OpenFile;
                self.typing_branch = Some((String::new(), String::new(), String::new()));
                self.dirty = true;
            }
            KeyCode::Char('M') => {
                self.mouse = !self.mouse;
                self.mouse_changed = true;
                self.notice = Some(match self.mouse {
                    true => tr!("Mouse on"),
                    false => tr!("Mouse off: the terminal selects text"),
                });
                self.dirty = true;
            }
            KeyCode::Char('q') if !covering => self.quit = true,
            KeyCode::Tab | KeyCode::BackTab => {
                // サイドバーが出ていれば、そこも回る。
                let sidebar = self.size.0 >= SIDEBAR_FROM;
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::Main,
                    Focus::Main | Focus::Message => Focus::Side,
                    Focus::Side if sidebar => Focus::Sidebar,
                    Focus::Side => Focus::Main,
                };
                self.dirty = true;
            }
            // アプリの Fetch のシートと同じく、どこから取るかと、片づけるか（prune の設定）を見せて確かめる。
            KeyCode::Char('f') if !control => {
                if self.remotes.is_empty() {
                    self.error = Some(tr!("There is no remote to fetch from"));
                } else {
                    self.send(Request::Run {
                        name: "fetchConfig",
                        command: json!({"op": "configList"}),
                    });
                }
            }
            // サイドバーでチェックアウトしていないブランチを選んでいれば、そのブランチを上流に合わせる（アプリの
            // ブランチのメニューの Fast-Forward・Push・Publish）。
            // サイドバーで subtree を選んでいれば、その Pull・Push（アプリの subtree のメニュー）。
            KeyCode::Char('p' | 'P') if !control && self.picked_subtree().is_some() => {
                let dir = self.picked_subtree().unwrap_or_default();
                self.subtree_source(
                    &dir,
                    if key.code == KeyCode::Char('p') {
                        "pull"
                    } else {
                        "push"
                    },
                );
            }
            KeyCode::Char('p') if !control => match self.picked_other_branch() {
                Some(branch) => self.ask_fast_forward(branch),
                None => self.ask_pull(),
            },
            KeyCode::Char('P') if !control => match self.picked_other_branch() {
                Some(branch) => {
                    let upstream = self.sidebar.upstream_of(&branch);
                    let (ahead, behind) = self.sidebar.picked_counts();
                    self.ask_push_of(branch, upstream, ahead, behind, false);
                }
                None => self.ask_push(),
            },
            // 次に戻す操作の名前を聞いてから、確かめる。
            KeyCode::Char('u') if !control => {
                self.run("undoPeek", tr!("Undo…"), json!({"op": "undoPeek"}))
            }
            // サイドバーの stash なら、そこにブランチを作って戻す（`stash_key`）。
            KeyCode::Char('b') if !control => {
                if !(self.focus == Focus::Sidebar && self.stash_key(KeyCode::Char('b'))) {
                    self.ask_branch();
                }
            }
            KeyCode::Char('T') => self.ask_name(Naming::Tag),
            // 止まった git am のパッチを 3-way merge で当て直す（アプリの Retry with 3-Way）。
            KeyCode::Char('R') if self.retry_three_way => {
                self.run(
                    "amThreeWay",
                    tr!("Retrying with a 3-way merge…"),
                    json!({"op": "amThreeWay"}),
                );
            }
            KeyCode::Char('R') if self.view != View::Changes && self.focus == Focus::Main => {
                self.ask_reset();
            }
            // stash する前に、メッセージ（なくてもよい）を聞き、何を含めるかを選ばせる。
            KeyCode::Char('s') if !control && self.focus != Focus::Side => {
                self.naming = Naming::Stash;
                self.typing_branch = Some((String::new(), String::new(), String::new()));
                self.dirty = true;
            }
            KeyCode::Char('S') if self.focus != Focus::Side => match self.stash.clone() {
                Some(name) => self.ask_stash_apply(name, true),
                None => {
                    self.error = Some(tr!("No stash to pop"));
                    self.dirty = true;
                }
            },
            // 途中の操作があるときだけ: 続ける、飛ばす、やめる（やめる前に確かめる）。
            KeyCode::Char('C') if self.sequence.is_some() => {
                self.run("continue", tr!("Continuing…"), json!({"op": "continue"}));
            }
            KeyCode::Char('K') if self.sequence.is_some() => {
                self.run("skip", tr!("Skipping…"), json!({"op": "skip"}));
            }
            KeyCode::Char('A') if self.sequence.is_some() => {
                let kind = self.sequence.as_ref().map_or("", |s| s.0.as_str());
                self.confirm = Some((
                    tr!("Abort the {kind}?", kind = kind),
                    tr!("abort"),
                    Request::Run {
                        name: "abort",
                        command: json!({"op": "abort"}),
                    },
                ));
                self.dirty = true;
            }
            // 最後のコミットで hooks が書いたものを、全部出す。
            KeyCode::Char('!') => {
                // 最後の失敗の全部があれば、それ（新しいほう）。なければ hooks の出力。
                if self.failure_details.is_some() {
                    self.showing_failure = true;
                    self.showing_hooks = Some(0);
                } else {
                    self.showing_failure = false;
                    match self.hook_output {
                        Some(_) => self.showing_hooks = Some(0),
                        None => self.notice = Some(tr!("No output from the hooks yet")),
                    }
                }
                self.dirty = true;
            }
            KeyCode::Char('?') => {
                self.help = true;
                self.help_top = 0;
                self.dirty = true;
            }
            KeyCode::Char(':') => {
                self.palette = Some(Palette::default());
                self.dirty = true;
            }
            KeyCode::Char('0') => {
                self.dirty |= self.focus != Focus::Sidebar;
                self.focus = Focus::Sidebar;
            }
            KeyCode::Char('1') => self.show(View::Changes),
            KeyCode::Char('2') => self.show(View::History),
            KeyCode::Char('3') => self.show(View::Reflog),
            _ => {
                self.dirty |= match (self.view, self.focus) {
                    (View::History, Focus::Main) => self.history_key(
                        key.code,
                        control,
                        key.modifiers.contains(KeyModifiers::SHIFT),
                    ),
                    (View::History | View::Reflog | View::File, Focus::Side) => {
                        self.details_key(key.code, control)
                    }
                    (View::File, Focus::Main) => self.file_key(key.code, control),
                    (View::Reflog, Focus::Main) => self.reflog_key(key.code, control),
                    (View::Changes, Focus::Main) => self.changes_key(key.code, control),
                    (View::Changes, Focus::Side) => self.changes_diff_key(key.code, control),
                    (_, Focus::Message) => false,
                    (_, Focus::Sidebar) => self.sidebar_key(key.code, control),
                }
            }
        }
    }

    /// コマンドの一覧に出す操作: 今いる所で動かせて、名前が `query` に合うもの。
    fn palette_entries(&self, query: &str) -> Vec<(&'static commands::Entry, String)> {
        commands::ENTRIES
            .iter()
            .filter(|e| match e.place {
                Place::Anywhere | Place::History => true,
                Place::NotSide => self.focus != Focus::Side,
                Place::Sidebar => self.focus == Focus::Sidebar,
                Place::Changes => self.view == View::Changes,
                Place::Sequence => self.sequence.is_some(),
            })
            .map(|e| (e, e.title()))
            .filter(|(_, title)| commands::matches(title, query))
            .collect()
    }

    /// コマンドの一覧で選んだ操作を動かす: その操作のキーを押したのと同じ。
    fn run_entry(&mut self, entry: &commands::Entry) {
        match entry.place {
            Place::History => self.show(View::History),
            Place::Changes if self.focus == Focus::Side => self.focus = Focus::Main,
            _ => {}
        }
        if entry.key == KeyCode::Null {
            self.run_keyless(entry.id);
            return;
        }
        let modifiers = match entry.control {
            true => KeyModifiers::CONTROL,
            false => KeyModifiers::NONE,
        };
        self.key(KeyEvent::new(entry.key, modifiers));
    }

    /// Working Copy で選んでいるコンフリクトのファイルに、Magic Merge を当てる（決まった規則で解けるものを解く。
    /// Undo できる）。差分で開いていれば、そのファイルに。
    fn magic_merge(&mut self) {
        match self.changes.conflicted_path().map(str::to_owned) {
            Some(path) => self.run(
                "magicMerge",
                tr!("Magic Merge…"),
                json!({"op": "magicMergeFile", "path": path}),
            ),
            None => {
                self.error = Some(tr!("Select a conflicted file in the Working Copy"));
                self.dirty = true;
            }
        }
    }

    /// Quick Open を出す。候補はアプリと同じ（この機械の bookmarks.json・recent.json）: お気に入り、ブックマーク、
    /// 最近開いたもの、探すフォルダの下で見つけたもの。探すフォルダは手元のものを裏で探し、見つかったら足す（SSH 先の
    /// フォルダは探さない）。
    fn open_quick(&mut self) {
        let (bookmarks, recents, roots) = match (
            amonhen_model::bookmarks::read(),
            amonhen_model::recent::read(),
            amonhen_model::bookmarks::scan_roots(),
        ) {
            (Ok(b), Ok(r), Ok(s)) => (b, r, s),
            (Err(reason), _, _) | (_, Err(reason), _) | (_, _, Err(reason)) => {
                self.error = Some(reason);
                return;
            }
        };
        // 何もなくても開く（URL を打てば clone できる）。
        let roots: Vec<_> = roots.into_iter().filter(|r| r.is_local()).collect();
        let mut quick = QuickOpen {
            query: String::new(),
            selected: 0,
            items: bookmarks
                .iter()
                .map(|b| Candidate {
                    path: b.path.clone(),
                    name: b.display_name(),
                    favorite: b.favorite,
                })
                .collect(),
        };
        quick.add(recents.into_iter().map(|r| r.path));
        self.quick_open = Some(quick);
        if !roots.is_empty() {
            let events = self.events.clone();
            std::thread::spawn(move || {
                for root in roots {
                    let found = root.find_local();
                    if !found.is_empty() && events.send(Event::Scanned(found)).is_err() {
                        return;
                    }
                }
            });
        }
    }

    /// コマンドの一覧からだけ動かす操作（キーを持たない）。
    fn run_keyless(&mut self, id: &str) {
        if id == "quick-open" {
            self.open_quick();
            return;
        }
        if id == "rebuild-history" {
            self.rebuild_next = true;
            self.notice = Some(tr!("Rebuilding History…"));
            self.refresh();
            return;
        }
        if id == "activity" {
            // アプリの Activity のうち、このリポジトリの操作ログ（git の記録・画面の速さは端末では出さない）。
            self.run(
                "undoLog",
                tr!("Reading the operation log…"),
                json!({"op": "undoLog"}),
            );
            return;
        }
        if id == "compare-with-previous" {
            self.run(
                "previousVersion",
                tr!("Finding the previous version…"),
                json!({"op": "previousVersion"}),
            );
        }
        if id == "save-patch" {
            // History で選んでいるコミット（印があれば、印をつけたものを古い順に）を、git の名前のファイルにして、今の
            // フォルダ（この機械）に。
            if self.history.mark_count() >= 2 {
                if self.history.marks().any(|(_, m)| m.merge) {
                    self.error = Some(tr!("A merge can’t be saved as a patch"));
                    self.dirty = true;
                    return;
                }
                let mut ids: Vec<String> = self
                    .history
                    .marks()
                    .map(|(_, m)| m.id.to_string())
                    .collect();
                ids.reverse();
                let what = tr!("{count} commits", count = ids.len());
                self.naming = Naming::SavePatch;
                self.typing_branch = Some((ids.join(" "), what, local_path("")));
                self.dirty = true;
                return;
            }
            let Some(id) = self.history.selected_id() else {
                return;
            };
            let id = id.to_string();
            let short = id[..id.len().min(8)].to_owned();
            self.naming = Naming::SavePatch;
            self.typing_branch = Some((id, short, local_path("")));
            self.dirty = true;
        }
        if id == "apply-patch" {
            self.naming = Naming::Patch;
            self.typing_branch = Some((String::new(), String::new(), local_path("")));
            self.dirty = true;
        }
        if id == "fetch-bundle" {
            self.naming = Naming::BundleFile;
            self.typing_branch = Some((String::new(), String::new(), local_path("")));
            self.dirty = true;
        }
        if id == "save-bundle" {
            // HEAD とすべてのブランチ・タグを、今のフォルダ（この機械）の 1 つのファイルに。
            let name = match self.name.is_empty() {
                true => "repository".to_owned(),
                false => self.name.clone(),
            };
            self.naming = Naming::Bundle;
            let file = local_path(&format!("{name}.bundle"));
            self.typing_branch = Some((String::new(), String::new(), file));
            self.dirty = true;
        }
        if id == "export-archive" {
            // History で選んでいるコミットを、今のフォルダ（TUI を始めた所。SSH 先でも、この機械）に書き出す。
            let Some(id) = self.history.selected_id() else {
                return;
            };
            let id = id.to_string();
            let short = id[..id.len().min(8)].to_owned();
            let name = match self.name.is_empty() {
                true => short.clone(),
                false => format!("{}-{short}", self.name),
            };
            let file = local_path(&format!("{name}.zip"));
            self.naming = Naming::Archive;
            self.typing_branch = Some((id, short, file));
            self.dirty = true;
        }
        if id == "delete-ignored-files" {
            self.run(
                "ignoredFiles",
                tr!("Finding ignored files…"),
                json!({"op": "ignoredFiles"}),
            );
        }
        if id == "add-submodule" {
            self.naming = Naming::SubmoduleAddUrl;
            self.typing_branch = Some((String::new(), String::new(), String::new()));
            self.dirty = true;
            return;
        }
        if id == "add-subtree" {
            if self.sidebar.subtrees_available() == Some(false) {
                self.error = Some(tr!("This machine's git has no git subtree"));
                return;
            }
            self.naming = Naming::SubtreeDir;
            self.typing_branch = Some((String::new(), String::new(), String::new()));
            self.dirty = true;
            return;
        }
        if id == "update-submodules" {
            self.run(
                "submodules",
                tr!("Reading the submodules…"),
                json!({"op": "submodules"}),
            );
        }
        if id == "add-remote" {
            self.naming = Naming::RemoteName;
            // 初めは、まだリモートがなければ origin。
            let name = match self.remotes.is_empty() {
                true => "origin".to_owned(),
                false => String::new(),
            };
            self.typing_branch = Some((String::new(), String::new(), name));
            self.dirty = true;
        }
        if id == "clean-up-branches" {
            self.run(
                "cleanupList",
                tr!("Finding branches to clean up…"),
                json!({"op": "branchCleanup"}),
            );
        }
        if id == "new-orphan-branch" {
            self.naming = Naming::Orphan;
            self.typing_branch = Some((String::new(), String::new(), String::new()));
            self.dirty = true;
        }
        if id == "prune-worktrees" {
            // フォルダのなくなった worktree の記録を片付ける（フォルダには触らない）。
            self.run(
                "pruneWorktrees",
                tr!("Pruning worktrees…"),
                json!({"op": "worktreePrune"}),
            );
        }
    }

    /// メインの画面を替える。
    fn show(&mut self, view: View) {
        // Reflog を開き直すと、HEAD のものに戻る（ブランチのものは、サイドバーの `L` で）。
        if view == View::Reflog {
            self.dirty |= self.reflog.name().is_some();
            self.reflog.show(None);
        }
        self.dirty |= self.view != view || self.focus != Focus::Main;
        self.pinned = None;
        self.view = view;
        self.focus = Focus::Main;
        self.sidebar.show_view(self.picked_view());
    }

    /// サイドバーで選ぶ、今の画面の行（Range Diff を出しているなら、その行）。
    fn picked_view(&self) -> Picked {
        match self.view {
            View::File if self.file.is_range_diff() => Picked::RangeDiff,
            view => view.picked(),
        }
    }

    /// `ranges`（git range-diff の範囲）を Range Diff で比べる。答えは Compare with Previous Version と同じ画面に。
    fn compare_ranges(&mut self, title: String, ranges: Vec<String>) {
        self.range_title = Some(title);
        self.range_ranges = ranges.clone();
        self.run(
            "rangeDiff",
            tr!("Comparing…"),
            json!({"op": "rangeDiff", "ranges": ranges, "creationFactor": null}),
        );
    }

    /// コアの命令を動かす。1 つずつ（動いている間は、次を受けない）。
    fn run(&mut self, name: &'static str, label: String, command: Value) {
        self.dirty = true;
        if self.running.is_some() {
            self.error = Some(tr!("Another operation is still running"));
            return;
        }
        self.running = Some(label);
        self.send(Request::Run { name, command });
    }

    /// History の並び順を、日付順とトポロジー順で替える（この TUI の間だけ）。
    fn toggle_history_order(&mut self) {
        self.dirty = true;
        if self.running.is_some() {
            self.error = Some(tr!("Another operation is still running"));
            return;
        }
        let order = match self.topo_order {
            true => "date",
            false => "topo",
        };
        self.running = Some(tr!("Reordering History…"));
        self.send(Request::Call {
            name: "historyOrder",
            request: json!({"op": "historyOrder", "order": order}),
        });
    }

    /// 命令が済んだ。だめなら理由を出し、どちらでも状態・ref・History を読み直す。
    /// タグ `name` のリモートごとの様子（いつ読んだかも）。push・置き換えは `t` で。
    fn tag_remote_text(&self, name: &str) -> String {
        let info = &self.tag_remotes;
        let remotes: Vec<String> = (info["remotes"].as_array().into_iter().flatten())
            .filter_map(|r| {
                let remote = r["name"].as_str()?;
                let state = match info["tags"][name][remote].as_str() {
                    Some("same") => tr!("has it"),
                    Some("different") => tr!("has a different tag with this name (t replaces it)"),
                    Some(_) => tr!("doesn’t have it (t pushes it)"),
                    None => tr!("not checked yet (Fetch reads it)"),
                };
                let when = r["checked"]
                    .as_i64()
                    .map(|ms| {
                        format!(
                            " · {}",
                            tr!(
                                "read {time}",
                                time = crate::history::ago(self.clock.now, ms / 1000)
                            )
                        )
                    })
                    .unwrap_or_default();
                Some(format!("{remote}: {state}{when}"))
            })
            .collect();
        match remotes.is_empty() {
            true => tr!("{name}: no remotes", name = name),
            false => format!("{name} — {}", remotes.join("; ")),
        }
    }

    /// サイドバーで選んでいる submodule のパス。
    fn picked_submodule(&self) -> Option<String> {
        self.sidebar
            .picked_ref()
            .filter(|r| r.0 == "submodule")
            .map(|r| r.1.to_owned())
    }

    /// submodule の行のキー（`picked_submodule` のもの）。
    fn submodule_key(&mut self, path: &str, code: KeyCode) {
        let change = |change: Value| json!({"op": "changeSubmodule", "change": change});
        match code {
            KeyCode::Char('e') => {
                self.naming = Naming::SubmoduleUrl;
                self.typing_branch = Some((path.to_owned(), path.to_owned(), String::new()));
            }
            KeyCode::Char('B') => {
                self.naming = Naming::SubmoduleBranch;
                self.typing_branch = Some((path.to_owned(), path.to_owned(), String::new()));
            }
            KeyCode::Char('F') => {
                self.naming = Naming::SubmoduleForeach;
                self.typing_branch = Some((String::new(), String::new(), String::new()));
            }
            KeyCode::Char('x') => {
                let remove = |delete: bool| Request::Run {
                    name: "submoduleChanged",
                    command: change(
                        json!({"kind": "remove", "path": path, "deleteGitDir": delete}),
                    ),
                };
                self.choices = Some((
                    tr!(
                        "Remove the submodule {path}? Its folder goes; .git/modules keeps its repository unless deleted too.",
                        path = path
                    ),
                    vec![
                        ('y', tr!("remove"), remove(false)),
                        ('d', tr!("remove and delete its .git/modules"), remove(true)),
                    ],
                ));
            }
            KeyCode::Char('a') => {
                self.confirm = Some((
                    tr!(
                        "Move the repository data of {path} into .git/modules (git submodule absorbgitdirs)?",
                        path = path
                    ),
                    tr!("move"),
                    Request::Run {
                        name: "submoduleChanged",
                        command: change(json!({"kind": "absorb", "paths": [path]})),
                    },
                ));
            }
            KeyCode::Char('y') => {
                self.confirm = Some((
                    tr!(
                        "Copy the submodules' URLs from .gitmodules into .git/config (git submodule sync)?"
                    ),
                    tr!("sync"),
                    Request::Run {
                        name: "submoduleChanged",
                        command: change(json!({"kind": "sync", "recursive": false})),
                    },
                ));
            }
            _ => {}
        }
        self.dirty = true;
    }

    /// サイドバーで印をつけたローカルブランチ（印をつけた順）。
    fn marked_branches(&self) -> Vec<String> {
        let locals: Vec<String> = (self.sidebar.branches().into_iter())
            .filter(|(_, remote)| !remote)
            .map(|(name, _)| name)
            .collect();
        (self.sidebar.marked_names().into_iter())
            .filter(|n| locals.contains(n))
            .collect()
    }

    /// 印をつけたブランチをまとめて: `*` はすべてお気に入りならすべて外し、そうでなければ足りないものを足す。
    /// `z` はすべてアーカイブ済みならすべて戻し、そうでなければすべてアーカイブする。
    fn change_branches_place(&mut self, names: &[String], code: KeyCode) {
        let favorites = self.sidebar.favorites();
        let key = |what: &str, name: &str| format!("branch.{name}.amonhen{what}");
        let set = |k: String, value: Value| json!({"op": "configSet", "key": k, "value": value});
        let list = names.join(", ");
        let (commands, notice) = if code == KeyCode::Char('*') {
            if names.iter().all(|n| favorites.contains(n)) {
                let rest: Vec<&String> = favorites.iter().filter(|n| !names.contains(n)).collect();
                let mut commands: Vec<Value> = (rest.iter().enumerate())
                    .map(|(i, n)| set(key("Pinned", n), json!((i + 1).to_string())))
                    .collect();
                commands.extend(names.iter().map(|n| set(key("Pinned", n), Value::Null)));
                (
                    commands,
                    tr!("Removed {names} from Favorites", names = list),
                )
            } else {
                let mut all = favorites.clone();
                all.extend(names.iter().filter(|n| !favorites.contains(n)).cloned());
                let commands = (all.iter().enumerate())
                    .map(|(i, n)| set(key("Pinned", n), json!((i + 1).to_string())))
                    .collect();
                (commands, tr!("Added {names} to Favorites", names = list))
            }
        } else {
            let back = names.iter().all(|n| self.sidebar.is_archived(n));
            let value = if back { Value::Null } else { json!("true") };
            let commands = (names.iter())
                .map(|n| set(key("Archived", n), value.clone()))
                .collect();
            let notice = match back {
                true => tr!("Brought {names} back from Archived", names = list),
                false => tr!("Archived {names}", names = list),
            };
            (commands, notice)
        };
        self.send_branch_config(commands);
        self.notice = Some(notice);
    }

    /// お気に入り・アーカイブの設定を書く（すべて済んだら読み直す）。
    fn send_branch_config(&mut self, commands: Vec<Value>) {
        self.branch_config_pending += commands.len();
        for command in commands {
            self.send(Request::Run {
                name: "branchConfig",
                command,
            });
        }
    }

    /// Activity の Operations を読む: この TUI が動かしたもの（同じプロセスの記録）と、daemon のもの。
    fn read_operations(&mut self) {
        self.send(Request::Call {
            name: "activity",
            request: json!({"op": "activity"}),
        });
    }

    /// 頼んでから答えが届くまでの時間を、Performance に 1 つ記録する（200 まで）。
    fn record_perf(&mut self, metric: &'static str) {
        let Some(started) = self.perf_started.remove(metric) else {
            return;
        };
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let values = match self.perf.iter().position(|(m, _)| *m == metric) {
            Some(i) => &mut self.perf[i].1,
            None => {
                self.perf.push((metric, Default::default()));
                &mut self.perf.last_mut().expect("just pushed").1
            }
        };
        values.push_back(ms);
        if values.len() > 200 {
            values.pop_front();
        }
    }

    /// clone する場所を聞く（今のフォルダの、URL の最後の名前）。
    fn ask_clone_destination(&mut self, url: &str) {
        self.quick_open = None;
        let name = url
            .trim_end_matches('/')
            .rsplit(['/', '\\', ':'])
            .next()
            .unwrap_or("repo");
        let name = name.strip_suffix(".git").unwrap_or(name);
        let name = name.strip_suffix(".bundle").unwrap_or(name);
        self.naming = Naming::CloneDest;
        self.typing_branch = Some((url.to_owned(), url.to_owned(), local_path(name)));
        self.dirty = true;
    }

    /// 何を取ってくるか（アプリの Clone のシートと同じ: すべて、中身は必要なときに、最近の履歴だけ）。
    fn ask_clone_download(&mut self, url: &str, dest: &str) {
        self.clone_plan = Some(ClonePlan {
            url: url.to_owned(),
            dest: dest.to_owned(),
            stage: 0,
            download: 'y',
        });
        self.choices = Some((
            tr!(
                "Clone {url} into {dest}: download what?",
                url = url,
                dest = dest
            ),
            vec![
                ('y', tr!("everything"), Request::Status),
                (
                    'c',
                    tr!("contents when needed (partial clone)"),
                    Request::Status,
                ),
                (
                    'r',
                    tr!("recent history only (shallow clone)"),
                    Request::Status,
                ),
            ],
        ));
    }

    /// 一緒にすること: submodule も取り出す、一番上のファイルだけを取り出す（sparse checkout で始める）。
    fn ask_clone_extras(&mut self, plan: ClonePlan) {
        self.clone_plan = Some(plan);
        self.choices = Some((
            tr!("Clone it now?"),
            vec![
                ('y', tr!("clone"), Request::Status),
                ('s', tr!("with its submodules"), Request::Status),
                (
                    'f',
                    tr!("only the files at the top (pick folders later)"),
                    Request::Status,
                ),
                ('a', tr!("both"), Request::Status),
            ],
        ));
    }

    /// clone を裏で動かす（終わったら `Event::Cloned`）。git を動かすのはこの機械。
    fn start_clone(&mut self, plan: ClonePlan, extras: char) {
        let options = amonhen_backend_cli::CloneOptions {
            filter: (plan.download == 'c').then(|| "blob:none".to_owned()),
            depth: (plan.download == 'r').then_some(1),
            sparse: matches!(extras, 'f' | 'a'),
            branch: None,
            recurse_submodules: matches!(extras, 's' | 'a'),
        };
        self.running = Some(tr!("Cloning {url}…", url = plan.url));
        self.pending += 1;
        let events = self.events.clone();
        std::thread::spawn(move || {
            let result =
                amonhen_backend_cli::clone(&plan.url, std::path::Path::new(&plan.dest), &options)
                    .map(|()| plan.dest.clone())
                    .map_err(|e| e.to_string());
            let _ = events.send(Event::Cloned(result));
        });
        self.dirty = true;
    }

    /// Activity の Events に 1 つ記録する（500 まで）。
    fn event(&mut self, text: String) {
        self.event_log.push_back((self.clock_now(), text));
        if self.event_log.len() > 500 {
            self.event_log.pop_front();
        }
    }

    /// 今の時刻（UNIX 時刻の秒）。
    fn clock_now(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(self.clock.now, |d| d.as_secs() as i64)
    }

    /// 共有の daemon の様子を読む（`amonhen daemon status` と同じ。手元のソケットに聞くので、すぐ返る）。
    fn read_daemon(&mut self) {
        self.daemon = Some(
            amonhen_remote::local::control("status")
                .ok()
                .flatten()
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or(Value::Null),
        );
        self.dirty = true;
    }

    /// サイドバーで選んでいるローカルブランチの名前。
    fn picked_branch(&self) -> Option<String> {
        self.sidebar
            .picked_ref()
            .filter(|r| r.0 == "branch")
            .map(|r| r.1.to_owned())
    }

    /// ブランチのお気に入り・アーカイブ・お気に入りの順を変える（git の設定 `branch.<name>.amonhenPinned`・
    /// `amonhenArchived` に書く。アプリと同じなので、両方で同じに見える）。
    fn change_branch_place(&mut self, name: &str, code: KeyCode) {
        let favorites = self.sidebar.favorites();
        let key = |what: &str, name: &str| format!("branch.{name}.amonhen{what}");
        let order = |names: &[String]| -> Vec<Value> {
            names
                .iter()
                .enumerate()
                .map(|(i, n)| json!({"op": "configSet", "key": key("Pinned", n), "value": (i + 1).to_string()}))
                .collect()
        };
        let pinned = favorites.iter().position(|n| n == name);
        let (commands, notice) = match (code, pinned) {
            (KeyCode::Char('*'), Some(i)) => {
                let mut rest = favorites.clone();
                rest.remove(i);
                let mut commands = order(&rest);
                commands
                    .push(json!({"op": "configSet", "key": key("Pinned", name), "value": null}));
                (commands, tr!("Removed {name} from Favorites", name = name))
            }
            (KeyCode::Char('*'), None) => {
                let mut all = favorites.clone();
                all.push(name.to_owned());
                (order(&all), tr!("Added {name} to Favorites", name = name))
            }
            (KeyCode::Char('z'), _) => {
                let archived = self.sidebar.is_archived(name);
                let value = if archived { Value::Null } else { json!("true") };
                (
                    vec![json!({"op": "configSet", "key": key("Archived", name), "value": value})],
                    match archived {
                        true => tr!("Brought {name} back from Archived", name = name),
                        false => tr!("Archived {name}", name = name),
                    },
                )
            }
            (KeyCode::Char('<' | '>'), Some(i)) => {
                let to = match code {
                    KeyCode::Char('<') => i.checked_sub(1),
                    _ => Some(i + 1).filter(|&j| j < favorites.len()),
                };
                let Some(to) = to else {
                    return;
                };
                let mut moved = favorites.clone();
                moved.swap(i, to);
                (order(&moved), String::new())
            }
            (KeyCode::Char('<' | '>'), None) => {
                self.error = Some(tr!("Only a favorite branch moves up or down (* adds it)"));
                return;
            }
            _ => return,
        };
        self.send_branch_config(commands);
        if !notice.is_empty() {
            self.notice = Some(notice);
        }
    }

    /// リポジトリの形（`repositoryForm` の info）から、一番上の行の印と Working Copy の知らせを決める。
    fn set_form(&mut self, info: &Value) {
        let mut marks = Vec::new();
        if info["shallow"] == true {
            marks.push(tr!("shallow"));
        }
        if info["partial"].is_object() {
            marks.push(tr!("partial"));
        }
        self.form_marks = marks;
        let sparse = &info["sparse"];
        let note = sparse.is_object().then(|| {
            let folders = sparse["folders"].as_array().map_or(0, Vec::len);
            match (sparse["cone"] == true, folders) {
                (false, _) => {
                    tr!("Sparse checkout: only the files matching its patterns are checked out.")
                }
                (true, 0) => tr!("Sparse checkout: only the files at the top are checked out."),
                (true, 1) => tr!("Sparse checkout: only 1 folder is checked out."),
                (true, n) => tr!(
                    "Sparse checkout: only {count} folders are checked out.",
                    count = n
                ),
            }
        });
        self.changes.set_sparse_note(note);
        self.form = info.clone();
        self.dirty = true;
    }

    /// sparse checkout で取り出すフォルダを打たせる（今のフォルダを初めに入れておく）。git 2.25 から。
    fn type_sparse_folders(&mut self) {
        if self.form["canSparseCheckout"] == false {
            self.error = Some(tr!(
                "Checking out only some folders needs git 2.25 or later"
            ));
            return;
        }
        let sparse = &self.form["sparse"];
        if sparse.is_object() && sparse["cone"] != true {
            self.error = Some(tr!(
                "This sparse checkout uses patterns; change it with git sparse-checkout"
            ));
            return;
        }
        let folders: Vec<&str> = (sparse["folders"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .collect();
        self.naming = Naming::SparseFolders;
        self.typing_branch = Some((String::new(), String::new(), folders.join(" ")));
    }

    /// 打ったフォルダだけを取り出す（空なら、すべて取り出す）。先に確かめる。
    fn ask_sparse_folders(&mut self, typed: &str) {
        let folders: Vec<String> = typed
            .split_whitespace()
            .map(|f| f.trim_matches('/').to_owned())
            .filter(|f| !f.is_empty())
            .collect();
        if folders.is_empty() {
            if !self.form["sparse"].is_object() {
                return;
            }
            self.confirm = Some((
                tr!("Check out all folders (stop the sparse checkout)?"),
                tr!("check out all"),
                Request::Run {
                    name: "sparseCheckout",
                    command: json!({"op": "disableSparseCheckout"}),
                },
            ));
            return;
        }
        self.confirm = Some((
            tr!(
                "Check out only {folders} (and the files at the top)?",
                folders = folders.join(", ")
            ),
            tr!("check out"),
            Request::Run {
                name: "sparseCheckout",
                command: json!({"op": "sparseCheckout", "folders": folders}),
            },
        ));
    }

    /// 何を取り足すかを選ばせる: shallow clone なら履歴（数・日付まで・すべて）、partial clone なら中身のすべて
    /// （git 2.36 から）。
    fn ask_fetch_more(&mut self) {
        let mut options = Vec::new();
        let shallow = self.history.is_shallow();
        if shallow {
            let deepen = |commits: u32| Request::Run {
                name: "deepen",
                command: json!({"op": "deepen", "commits": commits}),
            };
            options.push(('1', tr!("100 commits"), deepen(100)));
            options.push(('2', tr!("1,000 commits"), deepen(1_000)));
            options.push(('3', tr!("10,000 commits"), deepen(10_000)));
            // 選ぶ前に横取りして、日付を聞く（中身は送らない）。
            options.push(('d', tr!("up to a date…"), Request::Status));
            options.push((
                'a',
                tr!("all of it"),
                Request::Run {
                    name: "deepen",
                    command: json!({"op": "unshallow"}),
                },
            ));
            self.deepen_date_offered = true;
        }
        let partial = self.form["partial"].is_object();
        if partial {
            if self.form["canFetchAllContents"] == false {
                if !shallow {
                    self.error = Some(tr!(
                        "Fetching all contents of a partial clone needs git 2.36 or later"
                    ));
                    return;
                }
            } else {
                options.push((
                    'c',
                    tr!("all contents (partial clone)"),
                    Request::Run {
                        name: "fetchAllContents",
                        command: json!({"op": "fetchAllContents"}),
                    },
                ));
            }
        }
        let ask = match (shallow, partial) {
            (true, false) => tr!("Fetch older history from the remote?"),
            (false, _) => tr!(
                "Fetch all contents of the partial clone? Every file’s contents are downloaded."
            ),
            (true, true) => tr!("Fetch more from the remote?"),
        };
        self.choices = Some((ask, options));
    }

    /// History の読んである行の、署名を検証する（答えが来たら、残りを続けて頼む）。
    fn verify_signatures(&mut self) {
        if let Some(oids) = self.history.signatures_to_verify() {
            self.send(Request::Run {
                name: "commitSignatures",
                command: json!({"op": "verifySignatures", "oids": oids}),
            });
        }
    }

    /// refs を読んだら、worktree の一覧を覚えて、ほかの worktree の変更の数を読む（2 つ以上あるとき）。
    fn worktrees_opened(&mut self, refs: &Value) {
        let worktrees: Vec<&Value> = refs["worktrees"].as_array().into_iter().flatten().collect();
        let open = worktrees
            .iter()
            .find(|w| w["current"] == true)
            .and_then(|w| w["path"].as_str());
        self.worktree_paths = match open {
            Some(open) if worktrees.len() > 1 => Some((
                open.to_owned(),
                (worktrees.iter())
                    .filter(|w| {
                        w["current"] != true && w["prunable"].is_null() && w["bare"] != true
                    })
                    .filter_map(|w| w["path"].as_str().map(str::to_owned))
                    .collect(),
            )),
            _ => None,
        };
        self.count_open_worktree();
        self.read_worktree_counts();
        // ほかの worktree の変化は知らされないので、10 秒ごとに読み直す（アプリと同じ）。
        if self.worktree_paths.is_some() && !self.counts_clock {
            self.counts_clock = true;
            let events = self.events.clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    if events.send(Event::CountWorktrees).is_err() {
                        break;
                    }
                }
            });
        }
    }

    /// 開いている worktree の変更の数は、Working Copy の数。
    fn count_open_worktree(&mut self) {
        if let Some((open, _)) = &self.worktree_paths {
            self.worktree_counts
                .insert(open.clone(), self.changes.count() as u64);
            self.show_worktree_counts();
        }
    }

    /// ほかの worktree の変更の数を裏で読む（5 分ごとに、指紋が同じでも数え直す）。
    fn read_worktree_counts(&mut self) {
        let Some((_, others)) = &self.worktree_paths else {
            return;
        };
        if others.is_empty() {
            return;
        }
        let fresh = self
            .counts_fresh_at
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(300));
        if fresh {
            self.counts_fresh_at = Some(std::time::Instant::now());
        }
        let paths = others.clone();
        self.send(Request::Background {
            name: "worktreeCounts",
            command: json!({"op": "worktreeChanges", "paths": paths, "fresh": fresh}),
        });
    }

    fn show_worktree_counts(&mut self) {
        let counts = if self.worktree_paths.is_some() {
            self.worktree_counts.clone()
        } else {
            Default::default()
        };
        self.sidebar.set_worktree_counts(&counts);
        self.history.set_worktree_counts(&counts);
        self.damage |= Damage::SIDEBAR | Damage::MAIN;
        self.dirty = true;
    }

    fn ran(&mut self, name: &'static str, result: std::result::Result<Value, String>) {
        // タグのリモートでの様子（裏で読む）。印: 同じ名前の別のタグがあるリモートがあれば ⚠、読んだリモートの
        // どれにもなければ ∅（アプリの印）。
        if name == "tagRemoteMarks" {
            self.tag_remotes = result.ok().map(|r| r["info"].clone()).unwrap_or_default();
            let info = &self.tag_remotes;
            let checked: Vec<&str> = (info["remotes"].as_array().into_iter().flatten())
                .filter(|r| !r["checked"].is_null())
                .filter_map(|r| r["name"].as_str())
                .collect();
            let marks = (info["tags"].as_object().into_iter().flatten())
                .filter(|_| !checked.is_empty())
                .filter_map(|(tag, states)| {
                    let state = |r: &&str| states[*r].as_str();
                    if checked.iter().any(|r| state(r) == Some("different")) {
                        Some((tag.clone(), ('⚠', Color::Yellow)))
                    } else if checked.iter().all(|r| state(r) == Some("missing")) {
                        Some((tag.clone(), ('∅', Color::Indexed(244))))
                    } else {
                        None
                    }
                })
                .collect();
            self.sidebar.set_tag_remote_marks(marks);
            self.dirty = true;
            return;
        }
        // 折りたたみを覚えた・読んだ（裏で）。読めたら、そのとおりに History を読み直す。
        if name == "saveFold" {
            return;
        }
        if name == "foldState" {
            if let Ok(reply) = result {
                let info = &reply["info"];
                let fold = info["folded"] == true;
                let except: Vec<String> = (info["except"].as_array().into_iter().flatten())
                    .filter_map(|e| e.as_str().map(str::to_owned))
                    .collect();
                if fold != self.fold || except != self.expanded {
                    self.fold = fold;
                    self.expanded = except;
                    self.open();
                }
            }
            return;
        }
        // ブランチの分け方（裏で読む）と、お気に入り・アーカイブを書いたもの。動いている操作の表示を消さない。
        if name == "branchGroups" {
            let info = result.ok().map(|r| r["info"].clone()).unwrap_or_default();
            self.sidebar.set_branch_groups(info, self.clock.now);
            self.dirty = true;
            return;
        }
        if name == "branchConfig" {
            self.branch_config_pending = self.branch_config_pending.saturating_sub(1);
            if let Err(error) = result {
                self.error = Some(error);
            }
            if self.branch_config_pending == 0 {
                self.open();
            }
            self.dirty = true;
            return;
        }
        // 失敗した git のコマンド: 同じエラーで失敗した、いちばん新しい git の記録。
        if name == "failureCommand" {
            let Some(error) = self.failure_seen.take() else {
                return;
            };
            let records = result.map(|r| r["records"].clone()).unwrap_or_default();
            let found = (records.as_array().into_iter().flatten())
                .filter(|r| r["kind"] == "git" && r["ok"] == false)
                .filter(|r| {
                    let theirs = r["error"].as_str().unwrap_or("").trim();
                    !theirs.is_empty() && (error.contains(theirs) || theirs.contains(&error))
                })
                .max_by_key(|r| r["seq"].as_u64().unwrap_or(0))
                .and_then(|r| r["label"].as_str().map(str::to_owned));
            if let Some(command) = found {
                let first = self.failure_details.is_none();
                self.failure_details = Some(format!("$ {command}\n\n{error}"));
                self.details_are_output = false;
                // 1 行の失敗にも、`!` でコマンドを出せると書く。
                if first && let Some(shown) = &mut self.error {
                    shown.push_str(&tr!(" (! shows all)"));
                }
                self.dirty = true;
            }
            return;
        }
        // Operations: 手元の記録と daemon の記録を合わせて、新しい順に。選んでいたものは番号で引き継ぐ。
        if name == "activity" {
            let reply = result.unwrap_or_default();
            let remote = reply["remote"] == true;
            let mut entries: Vec<Operation> = (reply["records"].as_array().into_iter().flatten())
                .filter_map(|e| Operation::from(e, remote))
                .collect();
            entries.sort_by_key(|o| std::cmp::Reverse((o.started, o.seq)));
            let previous = self.operations.as_ref().and_then(|o| {
                let op = o.entries.get(o.selected)?;
                Some((op.seq, op.remote))
            });
            let selected = previous
                .and_then(|(seq, remote)| {
                    entries
                        .iter()
                        .position(|o| o.seq == seq && o.remote == remote)
                })
                .unwrap_or(0);
            self.operation_log = None;
            self.operations = Some(Operations {
                entries,
                selected,
                records: reply["records"].clone(),
            });
            self.dirty = true;
            return;
        }
        // History Cache の画面（Operations から `Tab`）。
        if name == "historyCache" {
            self.operations = None;
            self.history_cache = Some(result.map(|r| r["info"].clone()).unwrap_or_default());
            self.dirty = true;
            return;
        }
        if name == "cancelActivity" {
            match result {
                Ok(_) => self.notice = Some(tr!("Cancelled")),
                Err(error) => self.error = Some(error),
            }
            self.read_operations();
            return;
        }
        // Working Copy の行ごとの行数（裏で読む。読めなければ前の数のまま）。
        if name == "workingStats" {
            if let Ok(reply) = result
                && self.changes.set_stats(&reply["info"]["stats"])
            {
                self.damage |= Damage::MAIN;
                self.dirty = true;
            }
            return;
        }
        // ほかの worktree の変更の数（裏で読む。読めなければ前の数のまま）。
        if name == "worktreeCounts" {
            for w in (result.ok().as_ref())
                .and_then(|r| r["info"]["worktrees"].as_array().cloned())
                .unwrap_or_default()
            {
                if let Some(path) = w["path"].as_str() {
                    self.worktree_counts
                        .insert(path.to_owned(), w["count"].as_u64().unwrap_or(0));
                }
            }
            self.show_worktree_counts();
            return;
        }
        // subtree の一覧（裏で読む）。
        if name == "subtreeList" {
            let info = result.ok().map(|r| r["info"].clone()).unwrap_or_default();
            self.sidebar.set_subtrees(info);
            self.dirty = true;
            return;
        }
        // 裏で読むリポジトリの形も、動いている操作の表示を消さない。読めなければ印を出さない。
        if name == "repositoryForm" {
            let info = result.ok().map(|r| r["info"].clone()).unwrap_or_default();
            self.set_form(&info);
            return;
        }
        // 裏で読む署名は、動いている操作の表示を消さない。
        if name == "commitSignatures" {
            let reply = result.ok();
            self.history
                .set_signatures(reply.as_ref().map(|r| &r["info"]["checks"]));
            self.damage |= Damage::MAIN;
            self.verify_signatures();
            return;
        }
        self.running = None;
        let reply = match result {
            Ok(reply) => reply,
            // 絵が読めなくても、cherry-pick・revert は確かめる（絵なしで）。
            Err(_) if name == "pickPreview" => {
                if let Some((ask, yes, request, _)) = self.pick_question.take() {
                    self.confirm = Some((ask, yes, request));
                    self.dirty = true;
                }
                return;
            }
            // 下書きが読めなくても、欄はそのまま使える（覚えるのをやめる）。
            Err(_) if name == "draftOrTemplate" => return,
            // 変更の数が読めなくても、消すかは聞く（変更があれば git が断る）。
            // コンフリクトの見込みが読めなくても、octopus merge は確かめる（見込みなしで）。
            Err(_) if name == "octopusConflicts" => {
                if let Some((order, info)) = self.octopus.take() {
                    self.ask_octopus_with(&order, &info, None);
                }
                return;
            }
            Err(_) if name == "worktreeChangesForRemove" => {
                self.ask_remove_worktree(0);
                return;
            }
            // 絵が読めなくても、fast-forward は確かめる（絵なしで）。
            Err(_) if name == "fastForwardPreview" => {
                if let Some((branch, upstream, behind)) = self.fast_forward.take() {
                    self.confirm = Some((
                        tr!(
                            "Fast-forward {branch} to {upstream} (+{count} commits)? Undo brings it back.",
                            branch = branch,
                            upstream = upstream,
                            count = behind
                        ),
                        tr!("fast-forward"),
                        Request::Run {
                            name: "fastForward",
                            command: json!({"op": "fastForward", "branch": branch}),
                        },
                    ));
                    self.dirty = true;
                }
                return;
            }
            // 設定が読めなくても、どこから取るかは聞く（片づけるかは書かない）。
            Err(_) if name == "fetchConfig" => {
                self.ask_fetch(&Value::Null);
                return;
            }
            // 見通しが読めなくても（History にないなど）、消すかは聞く（断られれば git の理由が出る）。
            Err(_) if name == "deletePreview" => {
                self.ask_delete_with(None);
                return;
            }
            Err(error) => {
                if name == "recentMessages" {
                    self.fixup_pick = None;
                }
                if name == "editRows" {
                    self.combining.clear();
                }
                // 長い出力は、最初の行だけ。2 行以上なら、全部を `!` で出せる（アプリの Details）。
                let line = error.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                let lines = error.lines().filter(|l| !l.trim().is_empty()).count();
                self.failure_details = (lines > 1).then(|| error.trim_end().to_owned());
                self.details_are_output = false;
                // 失敗した git のコマンドを Activity の記録から探す（アプリの Details と同じく、詳細の先頭に）。
                self.failure_seen = Some(error.trim_end().to_owned());
                self.send(Request::Call {
                    name: "failureCommand",
                    request: json!({"op": "activity"}),
                });
                self.error = Some(match lines > 1 {
                    true => line.trim().to_owned() + &tr!(" (! shows all)"),
                    false => line.trim().to_owned(),
                });
                self.changes.refresh();
                self.open();
                return;
            }
        };
        match name {
            "completePath" => {
                let entries = reply["info"]["entries"].clone();
                self.complete_with(&entries);
                return;
            }
            // 置き換える push の絵: まだ選んでいる間だけ重ねる。
            "pushPreview" => {
                let target = self.pushing_to.clone();
                if self.choices.is_some() {
                    self.question_drawing = Some(&reply["info"]["drawing"])
                        .filter(|d| d.is_object())
                        .map(|d| {
                            (
                                tr!(" If you force push to {target} ", target = target),
                                crate::drawing::from_move(d, &target),
                            )
                        });
                    self.dirty = true;
                }
                return;
            }
            "pickPreview" => {
                if let Some((ask, yes, request, title)) = self.pick_question.take() {
                    let branch = self.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
                    self.question_drawing = Some(&reply["info"]["drawing"])
                        .filter(|d| d.is_object())
                        .map(|d| (title, crate::drawing::from_pick(d, &branch)));
                    self.confirm = Some((ask, yes, request));
                    self.dirty = true;
                }
                return;
            }
            "undoLog" => {
                let entries: Vec<(String, i64)> = reply["info"]["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|e| {
                        let label = e["label"].as_str().unwrap_or_default().to_owned();
                        (label, e["time"].as_f64().unwrap_or(0.0) as i64 / 1000)
                    })
                    .collect();
                // 戻すものがなくても枠は出す（`Tab` で Operations へ）。
                self.operation_log = Some(OperationLog {
                    entries,
                    selected: 0,
                });
                return;
            }
            // 戻す操作があれば、名前を出して確かめる。
            "undoPeek" => {
                match reply["label"].as_str() {
                    Some(label) => {
                        self.confirm = Some((
                            tr!("Undo “{label}”?", label = label),
                            tr!("undo"),
                            Request::Run {
                                name: "undo",
                                command: json!({"op": "undo"}),
                            },
                        ));
                    }
                    None => self.error = Some(tr!("Nothing to undo")),
                }
                return;
            }
            "fetch" => self.notice = Some(tr!("Fetched")),
            "deepen" => self.notice = Some(tr!("Fetched older history")),
            "fetchAllContents" => self.notice = Some(tr!("Fetched all contents")),
            "sparseCheckout" => self.notice = Some(tr!("Changed the folders checked out")),
            "pull" => self.notice = Some(tr!("Pulled")),
            "push" => self.notice = Some(tr!("Pushed")),
            "undo" => self.notice = Some(tr!("Undone")),
            "checkout" => self.notice = Some(tr!("Checked out")),
            "createBranch" => self.notice = Some(tr!("Created the branch")),
            "createTag" => self.notice = Some(tr!("Created the tag")),
            "renameBranch" => self.notice = Some(tr!("Renamed")),
            "deleteBranch" | "deleteTag" => self.notice = Some(tr!("Deleted")),
            "stash" => self.notice = Some(tr!("Stashed")),
            "pop" => self.notice = Some(tr!("Popped the stash")),
            "abort" => self.notice = Some(tr!("Aborted")),
            "resolve" => self.notice = Some(tr!("Resolved")),
            "reset" => {
                // hard で作業中の変更があれば、消す前に stash に残っている。
                self.notice = Some(match reply["info"]["stashed"] == true {
                    true => tr!("Reset (the working changes are in a stash)"),
                    false => tr!("Reset"),
                });
            }
            "apply" => self.notice = Some(tr!("Applied the stash")),
            "drop" => self.notice = Some(tr!("Dropped the stash")),
            "stashBranch" => self.notice = Some(tr!("Applied the stash on the new branch")),
            "removeWorktree" => self.notice = Some(tr!("Removed the worktree")),
            "restoreFile" => self.notice = Some(tr!("Restored the version")),
            "addWorktree" => self.notice = Some(tr!("Added the worktree")),
            "setUpstream" => self.notice = Some(tr!("Set the upstream")),
            "pruneWorktrees" => self.notice = Some(tr!("Pruned the worktrees")),
            "orphan" => self.notice = Some(tr!("Created the branch")),
            // 片付けてよいブランチが分かった: 選んでおくもの（取り込み済み）を消すか、上流の消えたものも含めるか。
            "cleanupList" => {
                self.ask_clean_up(&reply["info"]);
                return;
            }
            "cleanUp" => self.notice = Some(tr!("Deleted the branches")),
            "addRemote" => self.notice = Some(tr!("Added the remote")),
            "lockWorktree" => self.notice = Some(tr!("Locked the worktree")),
            "unlockWorktree" => self.notice = Some(tr!("Unlocked the worktree")),
            "moveWorktree" => self.notice = Some(tr!("Moved the worktree")),
            "repairWorktree" => self.notice = Some(tr!("Reconnected the worktree")),
            "fastForward" => self.notice = Some(tr!("Fast-forwarded")),
            "editRemote" => self.notice = Some(tr!("Changed the remote")),
            "pushTag" => self.notice = Some(tr!("Pushed the tag")),
            "deleteRemoteTag" => self.notice = Some(tr!("Deleted the tag from the remote")),
            "removeRemote" => self.notice = Some(tr!("Removed the remote")),
            "pruneRemote" => self.notice = Some(tr!("Pruned the remote-tracking branches")),
            "submodules" => {
                self.ask_update_submodules(&reply["info"]);
                return;
            }
            "updateSubmodules" => self.notice = Some(tr!("Updated the submodules")),
            "submoduleChanged" => self.notice = Some(tr!("Changed the submodule")),
            // それぞれで動かした命令の出力は、`!` で全部を出す。
            "submoduleForeach" => {
                let output = reply["info"]["output"]
                    .as_str()
                    .unwrap_or("")
                    .trim_end()
                    .to_owned();
                match output.is_empty() {
                    true => self.notice = Some(tr!("Ran in each submodule (no output)")),
                    false => {
                        self.notice = Some(tr!("Ran in each submodule (! shows the output)"));
                        self.failure_details = Some(output);
                        self.details_are_output = true;
                    }
                }
            }
            "subtreeChanged" => self.notice = Some(tr!("Done")),
            // ignore されたものが分かった: 数と最初のいくつかを出して、確かめてから消す（Undo では戻らない）。
            "ignoredFiles" => {
                let paths: Vec<String> = (reply["info"]["paths"].as_array().into_iter().flatten())
                    .filter_map(|p| p.as_str().map(str::to_owned))
                    .collect();
                self.dirty = true;
                if paths.is_empty() {
                    self.notice = Some(tr!(
                        "No ignored files: nothing in the working tree matches .gitignore"
                    ));
                    return;
                }
                const SHOWN: usize = 3;
                let mut names = amonhen_model::i18n::list(&paths[..paths.len().min(SHOWN)]);
                if paths.len() > SHOWN {
                    names = tr!(
                        "{names} and {count} more",
                        names = names,
                        count = paths.len() - SHOWN
                    );
                }
                self.confirm = Some((
                    tr!(
                        "Delete {count} ignored items ({names})? Undo can’t bring them back.",
                        count = paths.len(),
                        names = names
                    ),
                    tr!("delete"),
                    Request::Run {
                        name: "deleteIgnored",
                        command: json!({"op": "deleteIgnored", "paths": paths}),
                    },
                ));
                return;
            }
            "deleteIgnored" => self.notice = Some(tr!("Deleted the ignored files")),
            "addToGitignore" => {
                self.notice = Some(match reply["added"] == true {
                    true => tr!("Added to .gitignore"),
                    false => tr!("Already in .gitignore"),
                })
            }
            // パッチの中身が分かった: 当たらなければ git の答えを出す。メールの形ならコミットとして（git am）、
            // ただの diff なら作業ツリーに（git apply）当てるかを確かめる。
            "readPatch" => {
                self.dirty = true;
                let Some(file) = self.patch_file.take() else {
                    return;
                };
                let info = &reply["info"];
                let mailbox = info["kind"] == "mailbox";
                let preview = patch_preview(info);
                // 当たらなければ、git の答えを出して、3-way merge で当てるかを聞く（当たらないところはコンフリクト
                // として残る。アプリの Apply Patch の 3-way）。
                if info["applies"] != true {
                    let error = info["error"].as_str().unwrap_or("");
                    let line = error
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or(error);
                    let request = match mailbox {
                        true => Request::Run {
                            name: "am",
                            command: json!({"op": "am", "paths": [file], "threeWay": true}),
                        },
                        false => Request::Run {
                            name: "applyPatch",
                            command: json!({"op": "applyPatch", "paths": [file], "threeWay": true}),
                        },
                    };
                    self.confirm = Some((
                        tr!(
                            "The patch doesn’t apply: {reason}. Apply it with a 3-way merge, leaving conflicts where it still doesn’t?{preview}",
                            reason = line.trim().trim_end_matches('.'),
                            preview = preview
                        ),
                        tr!("apply 3-way"),
                        request,
                    ));
                    return;
                }
                let patches = info["patches"].as_array().map_or(0, Vec::len);
                // ただの diff は、作業ツリーだけか、index にも当てるか（`--index`）を選ぶ。
                if !mailbox {
                    let apply = |index: bool| Request::Run {
                        name: "applyPatch",
                        command: json!({"op": "applyPatch", "paths": [file], "index": index}),
                    };
                    self.choices = Some((
                        tr!(
                            "Apply the diff? Undo brings the files back.{preview}",
                            preview = preview
                        ),
                        vec![
                            ('y', tr!("to the working tree"), apply(false)),
                            ('i', tr!("to the index too (staged)"), apply(true)),
                        ],
                    ));
                    return;
                }
                let ask = match patches {
                    1 => tr!("Apply the patch to the current branch as a commit?"),
                    count => tr!(
                        "Apply {count} patches to the current branch as commits?",
                        count = count
                    ),
                };
                let request = Request::Run {
                    name: "am",
                    command: json!({"op": "am", "paths": [file], "threeWay": null}),
                };
                self.confirm = Some((format!("{ask}{preview}"), tr!("apply"), request));
                return;
            }
            "applyPatch" => self.notice = Some(tr!("Applied the patch")),
            "magicMerge" => {
                let resolved = reply["info"]["resolved"].as_u64().unwrap_or(0);
                let remaining = reply["info"]["remaining"].as_u64().unwrap_or(0);
                self.notice = Some(match (resolved, remaining) {
                    (0, _) => tr!("Magic Merge found nothing it can resolve on its own"),
                    (_, 0) => tr!("Magic Merge resolved {count}; none left", count = resolved),
                    _ => tr!(
                        "Magic Merge resolved {count}; {left} left",
                        count = resolved,
                        left = remaining
                    ),
                });
            }
            // 今のブランチの、最後に書き換える前の版が分かった: その範囲で range-diff を頼む。
            "previousVersion" => {
                let previous = &reply["info"]["previous"];
                if previous.is_null() {
                    self.notice = Some(tr!(
                        "The branch hasn’t been rewritten (its reflog has no previous version)"
                    ));
                    self.dirty = true;
                    return;
                }
                let title = tr!(
                    "{branch} compared with {selector}",
                    branch = previous["branch"].as_str().unwrap_or(""),
                    selector = previous["selector"].as_str().unwrap_or("")
                );
                let ranges = previous["ranges"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r.as_str().map(str::to_owned))
                    .collect();
                self.compare_ranges(title, ranges);
                return;
            }
            // 比べた結果は、Blame・File History と同じ画面に（選んだ組のコミットの詳細も同じ）。
            "rangeDiff" => {
                let title = self.range_title.take().unwrap_or_default();
                self.file.show_range_diff(title, &reply["info"]["pairs"]);
                // 開いている間は、サイドバーの Workspace に Range Diff の行（アプリと同じ）。
                self.sidebar.set_range_diff(true);
                if self.view != View::File {
                    self.file_back = self.view;
                }
                self.view = View::File;
                self.focus = Focus::Main;
                self.sidebar.show_view(Picked::RangeDiff);
                self.dirty = true;
                return;
            }
            // merge の見通しが分かった: コンフリクトの見込みも読んでから確かめる（fast-forward・取り込むものなしは
            // コンフリクトしない）。
            "mergePreview" => {
                let Some(preview) = self.preview.as_mut() else {
                    return;
                };
                preview.3 = reply["info"].clone();
                if reply["info"]["fastForward"] == true || reply["info"]["upToDate"] == true {
                    self.ask_with_preview(None);
                    return;
                }
                let name = preview.0.clone();
                self.run(
                    "mergeConflicts",
                    tr!("Looking ahead…"),
                    json!({"op": "predictMerge", "ours": null, "theirs": name}),
                );
                return;
            }
            "mergeConflicts" => {
                let conflicts = conflict_files(&reply["info"]["conflicts"]);
                self.ask_with_preview(conflicts);
                return;
            }
            "pullConflicts" => {
                let conflicts = conflict_files(&reply["info"]["conflicts"]);
                self.ask_pull_with(conflicts);
                return;
            }
            "resetPreview" => {
                let info = reply["info"].clone();
                self.ask_reset_with(&info);
                return;
            }
            // 直すリモートの今の URL を読んだら、名前から聞く（アプリの Edit… のシートの 2 つの欄）。
            "remotesForEdit" => {
                let Some((name, _)) = self.editing_remote.take() else {
                    return;
                };
                let url = (reply["info"]["remotes"].as_array().into_iter().flatten())
                    .find(|r| r["name"] == name.as_str())
                    .and_then(|r| r["fetchUrl"].as_str())
                    .map(str::to_owned);
                self.editing_remote = Some((name.clone(), url));
                self.naming = Naming::RemoteRename;
                self.typing_branch = Some((name.clone(), name.clone(), name));
                self.dirty = true;
                return;
            }
            "tagRemotes" => {
                let info = reply["info"].clone();
                self.ask_tag_remote(&info);
                return;
            }
            // チェックアウトしていないブランチの fast-forward: 前と後の絵を重ねて確かめる。
            "fastForwardPreview" => {
                let Some((branch, upstream, behind)) = self.fast_forward.take() else {
                    return;
                };
                self.question_drawing = Some(&reply["info"]["drawing"])
                    .filter(|d| d.is_object())
                    .map(|d| {
                        (
                            tr!(" Fast-forward {branch} ", branch = branch),
                            crate::drawing::from_move(d, &branch),
                        )
                    });
                self.confirm = Some((
                    tr!(
                        "Fast-forward {branch} to {upstream} (+{count} commits)? Undo brings it back.",
                        branch = branch,
                        upstream = upstream,
                        count = behind
                    ),
                    tr!("fast-forward"),
                    Request::Run {
                        name: "fastForward",
                        command: json!({"op": "fastForward", "branch": branch}),
                    },
                ));
                self.dirty = true;
                return;
            }
            "worktreeChangesForRemove" => {
                let count = reply["info"]["worktrees"][0]["count"].as_u64().unwrap_or(0);
                self.ask_remove_worktree(count);
                return;
            }
            "stashBases" => {
                let Some(name) = self.stash_about.take() else {
                    return;
                };
                let found = (reply["info"]["stashes"].as_array().into_iter().flatten())
                    .find(|s| s["name"] == name.as_str());
                let message = self.sidebar.detail_of("stash", &name).unwrap_or_default();
                let mut parts: Vec<String> = Vec::new();
                // メッセージの「On main: …」「WIP on main: …」から、作ったブランチ。
                if let Some(branch) = message
                    .strip_prefix("WIP on ")
                    .or_else(|| message.strip_prefix("On "))
                    .and_then(|m| m.split_once(':'))
                    .map(|(b, _)| b)
                {
                    parts.push(tr!("made on {branch}", branch = branch));
                }
                if let Some(s) = found {
                    let ahead = s["ahead"].as_u64().unwrap_or(0);
                    parts.push(match (s["behind"].as_u64().unwrap_or(0), ahead) {
                        (1.., _) => tr!("its base is not in HEAD’s history"),
                        (0, 0) => tr!("at HEAD"),
                        (0, n) => tr!("{count} behind HEAD", count = n),
                    });
                    if s["untracked"] == true {
                        parts.push(tr!("with untracked files"));
                    }
                    if let Some(base) = s["base"].as_str() {
                        parts.push(tr!("base {commit}", commit = &base[..base.len().min(8)]));
                    }
                }
                if !parts.is_empty() {
                    self.notice = Some(format!("{name}: {}", parts.join(" · ")));
                    self.dirty = true;
                }
                return;
            }
            // push・pull で送る・取り込むコミット: まだ聞いていれば、件名を確かめる文に足す。
            "syncCommits" => {
                let info = &reply["info"];
                let subjects: Vec<String> = (info["commits"].as_array().into_iter().flatten())
                    .filter_map(|c| c["subject"].as_str())
                    .map(|s| format!("“{s}”"))
                    .collect();
                let count = info["count"].as_u64().unwrap_or(0) as usize;
                let more = count.saturating_sub(subjects.len());
                if subjects.is_empty() {
                    return;
                }
                let mut list = subjects.join(", ");
                if more > 0 {
                    list.push_str(&tr!(", {count} more", count = more));
                }
                // まだ push・pull を聞いているときだけ（答えた後に届いたら捨てる）。
                let add = |ask: &mut String| {
                    let syncing = [tr!("Push "), tr!("Pull ")]
                        .iter()
                        .any(|p| ask.starts_with(p.as_str()));
                    if syncing && !ask.contains(&list) {
                        *ask = format!("{ask} ({list})");
                    }
                };
                if let Some((ask, _, _)) = &mut self.confirm {
                    add(ask);
                } else if let Some((ask, _)) = &mut self.choices {
                    add(ask);
                }
                self.dirty = true;
                return;
            }
            // HEAD の reflog の「checkout: moving from A to B」から、最近切り替えたブランチ（新しい順）。
            "recentBranches" => {
                if let Some(pick) = &mut self.branch_pick {
                    let mut recent: Vec<String> = Vec::new();
                    for entry in reply["info"]["entries"].as_array().into_iter().flatten() {
                        let Some(message) = entry["message"].as_str() else {
                            continue;
                        };
                        let Some(moving) = message.strip_prefix("checkout: moving from ") else {
                            continue;
                        };
                        if let Some((_, to)) = moving.split_once(" to ")
                            && !recent.iter().any(|r| r == to)
                        {
                            recent.push(to.to_owned());
                        }
                    }
                    pick.recent = recent;
                    pick.selected = 0;
                    self.dirty = true;
                }
                return;
            }
            // アプリの submodule の印の代わりに、1 文字: コンフリクト !、取り出していない ○、記録と違うコミット ≠、
            // 中に変更 ✎。
            "submoduleMarks" => {
                let marks = (reply["info"]["submodules"].as_array().into_iter().flatten())
                    .filter_map(|s| {
                        let path = s["path"].as_str()?.to_owned();
                        let mark = if s["conflict"] == true {
                            ('!', Color::Red)
                        } else if s["initialized"] == false {
                            ('○', Color::Indexed(244))
                        } else if s["moved"] == true {
                            ('≠', Color::Yellow)
                        } else if s["changed"] == true {
                            ('✎', Color::Yellow)
                        } else {
                            return None;
                        };
                        Some((path, mark))
                    })
                    .collect();
                self.sidebar.set_submodule_marks(marks);
                self.dirty = true;
                return;
            }
            "tagSignatures" => {
                let marks = (reply["info"]["tags"].as_array().into_iter().flatten())
                    .filter_map(|t| {
                        t["signed"].as_str()?;
                        let name = t["name"].as_str()?.to_owned();
                        let status = t["signature"]["status"].as_str().unwrap_or("");
                        Some((name, crate::details::signature_mark(status)))
                    })
                    .collect();
                self.sidebar.set_tag_marks(marks);
                self.dirty = true;
                return;
            }
            // octopus merge の見通しが分かった: 取り込むものがあれば、コンフリクトの見込みも（同じ順に重ねて）読む。
            "octopusPreview" => {
                let info = reply["info"].clone();
                let Some((order, _)) = self.octopus.take() else {
                    return;
                };
                if info["upToDate"] == true {
                    let branch = self.branch.clone().unwrap_or_default();
                    self.notice = Some(tr!("{branch} already has all of them", branch = branch));
                    return;
                }
                self.octopus = Some((order.clone(), info));
                self.run(
                    "octopusConflicts",
                    tr!("Looking ahead…"),
                    json!({"op": "predictMerge", "ours": null, "theirs": order}),
                );
                return;
            }
            "octopusConflicts" => {
                if let Some((order, info)) = self.octopus.take() {
                    let steps = reply["info"]["steps"].as_array().cloned();
                    self.ask_octopus_with(&order, &info, steps.as_deref());
                }
                return;
            }
            "fetchConfig" => {
                let entries = reply["info"]["entries"].clone();
                self.ask_fetch(&entries);
                return;
            }
            "deletePreview" => {
                let info = reply["info"].clone();
                self.ask_delete_with(Some(info));
                return;
            }
            // 書きかけのメッセージかテンプレート: 欄がまだ空なら入れる。テンプレートは覚えたものとして扱い、
            // 書き足したときだけ下書きになる。
            "draftOrTemplate" => {
                let draft = reply["draft"].as_str().map(str::to_owned);
                let template = reply["template"].as_str().map(str::to_owned);
                let free = self.changes.message.is_empty() && !self.changes.amend;
                let baseline = match (free, draft, template) {
                    (true, Some(draft), _) => {
                        self.changes.message.set_text(&draft);
                        draft.trim_end().to_owned()
                    }
                    (true, None, Some(template)) => {
                        self.changes.message.set_text(&template);
                        self.changes.message.text()
                    }
                    (true, None, None) => String::new(),
                    // 待つ間に書き始めていたら、それを下書きにする。
                    (false, Some(draft), _) => draft.trim_end().to_owned(),
                    (false, None, _) => String::new(),
                };
                self.draft_saved = Some(baseline);
                self.dirty = true;
                return;
            }
            "saveCommitDraft" => return,
            "recentMessages" => {
                let messages: Vec<String> = (reply["messages"].as_array().into_iter().flatten())
                    .filter_map(|m| m.as_str().map(|m| m.trim_end().to_owned()))
                    .collect();
                let pair = |v: &Value| {
                    let name = v[0].as_str().filter(|s| !s.is_empty())?;
                    let email = v[1].as_str().filter(|s| !s.is_empty())?;
                    Some((name.to_owned(), email.to_owned()))
                };
                let mut authors: Vec<(String, String)> = Vec::new();
                for (name, email) in
                    (reply["authors"].as_array().into_iter().flatten()).filter_map(pair)
                {
                    if !authors.iter().any(|(_, e)| e.eq_ignore_ascii_case(&email)) {
                        authors.push((name, email));
                    }
                }
                let recent = Recent {
                    messages,
                    authors,
                    me: pair(&reply["me"]),
                };
                if let Some(pick) = &mut self.fixup_pick {
                    pick.fill(&recent);
                }
                self.recent_messages = Some(recent);
                if std::mem::take(&mut self.recall_when_read) {
                    self.recall(true);
                }
                self.dirty = true;
                return;
            }
            "editRows" => {
                let rows = (reply["rows"].as_array().into_iter().flatten())
                    .map(|r| {
                        crate::editmode::Row::new(
                            r["oid"].as_str().unwrap_or("").to_owned(),
                            r["merge"] == true,
                            r["message"].as_str().unwrap_or("").to_owned(),
                        )
                    })
                    .collect();
                let mut mode = crate::editmode::EditMode::new(rows);
                let combining = std::mem::take(&mut self.combining);
                if !combining.is_empty() && !mode.combine_last(&combining) {
                    self.error = Some(tr!(
                        "Only commits next to each other on the current branch can be combined"
                    ));
                    self.dirty = true;
                    return;
                }
                self.edit_mode = Some(mode);
                self.edit_discard = false;
                self.dirty = true;
                return;
            }
            "editPreview" => {
                let Some(mode) = &self.edit_mode else {
                    return;
                };
                let info = &reply["info"];
                // merge を越える編集では描けない（アプリと同じく、そのときは文だけ）。
                self.question_drawing = Some(info["editDrawing"].clone())
                    .filter(Value::is_object)
                    .map(|d| (tr!(" Apply These Changes? "), d));
                let ask = tr!(
                    "Apply these changes? {summary}{risk}{notes} Undo brings it all back.",
                    summary = mode.summary(),
                    risk = risk_text(rewrite_conflicts(info)),
                    notes = rewrite_notes(info)
                );
                let label = format!("Edit {} Commits", mode.rows.len());
                self.confirm = Some((
                    ask,
                    tr!("apply"),
                    Request::Run {
                        name: "rewrite",
                        command: json!({"op": "rewrite", "label": label, "edit": mode.plan()}),
                    },
                ));
                self.dirty = true;
                return;
            }
            "rebasePreview" => {
                let info = reply["info"].clone();
                let conflicts = rewrite_conflicts(&info);
                if let Some(preview) = self.preview.as_mut() {
                    preview.3 = info;
                }
                self.ask_with_preview(conflicts);
                return;
            }
            // 並び順が替わった: History を読み直す（ref と状態も、ほかの操作と同じく下で読み直す）。
            "historyOrder" => {
                self.topo_order = reply["order"] == "topo";
                self.notice = Some(match self.topo_order {
                    true => tr!(
                        "History in topological order: a branch's commits stay together under its merge"
                    ),
                    false => tr!("History in date order"),
                });
            }
            "savePatch" => {
                let files = reply["info"]["files"].as_array();
                let file = files
                    .and_then(|f| f.first())
                    .and_then(Value::as_str)
                    .unwrap_or("");
                self.notice = Some(match files.map_or(0, Vec::len) {
                    0 | 1 => tr!("Saved {file}", file = file),
                    n => tr!(
                        "Saved {count} patches in {folder}",
                        count = n,
                        folder = std::path::Path::new(file)
                            .parent()
                            .map_or(String::new(), |p| p.display().to_string())
                    ),
                });
            }
            // bundle の中身が分かった: 足りないコミットがあれば断り、なければ取り込む名前を聞く。
            "bundleCheck" => {
                let info = &reply["info"];
                let missing = info["missing"].as_u64().unwrap_or(0);
                self.dirty = true;
                if missing > 0 {
                    self.error = Some(tr!(
                        "The bundle needs {count} commits this repository doesn’t have",
                        count = missing
                    ));
                    return;
                }
                let Some(file) = self.bundle_file.take() else {
                    return;
                };
                let name = info["name"].as_str().unwrap_or("bundle").to_owned();
                self.naming = Naming::BundleName;
                self.typing_branch = Some((file.clone(), file, name));
                return;
            }
            "fetchBundle" => {
                let count = reply["info"]["refs"].as_array().map_or(0, Vec::len);
                self.notice = Some(tr!(
                    "Fetched {count} branches from the bundle",
                    count = count
                ));
            }
            "bundle" => {
                let file = reply["info"]["file"].as_str().unwrap_or("");
                self.notice = Some(tr!("Saved the bundle to {file}", file = file));
            }
            "archive" => {
                let file = reply["info"]["file"].as_str().unwrap_or("");
                self.notice = Some(tr!("Exported to {file}", file = file));
            }
            // git am も、当たらなければ merge と同じく止まる（Continue・Skip・Abort）。
            // 分けるコミットで止まったら、Working Copy で分けてコミットしてから Continue。
            "split" if reply["outcome"] == "stopped" => {
                self.notice = Some(tr!(
                    "Stopped at the commit to split: commit its changes in parts, then continue (C)"
                ));
                self.view = View::Changes;
                self.focus = Focus::Main;
                self.sidebar.show_view(Picked::Changes);
            }
            "merge" | "rebase" | "continue" | "skip" | "cherryPick" | "revert" | "rewrite"
            | "split" | "am" | "amThreeWay" => {
                // 編集モードは、書き換えたら閉じる（書き換えられなければ、選んだものごと残る）。
                if name == "rewrite" {
                    self.edit_mode = None;
                }
                // コンフリクトで止まったら、Working Copy で解決する。
                match reply["outcome"].as_str() {
                    Some("conflict") => {
                        self.error = Some(tr!("Stopped at conflicts: resolve them, then continue"));
                        self.view = View::Changes;
                        self.focus = Focus::Main;
                        self.sidebar.show_view(Picked::Changes);
                    }
                    Some("stopped") => self.notice = Some(tr!("Stopped")),
                    _ => self.notice = Some(tr!("Done")),
                }
            }
            _ => {}
        }
        self.changes.refresh();
        self.open();
    }

    /// pull する。上流と分かれているときは、コンフリクトの見込みを読んでから、どう取り込むかを選ばせる（ほかは
    /// git の設定のとおり）。
    fn ask_pull(&mut self) {
        if self.ahead > 0 && self.behind > 0 {
            if let Some(upstream) = self.upstream.clone() {
                self.run(
                    "pullConflicts",
                    tr!("Looking ahead…"),
                    json!({"op": "predictMerge", "ours": null, "theirs": upstream}),
                );
            } else {
                self.ask_pull_with(None);
            }
            return;
        }
        // 上流に取り込むものがあれば、その数と件名を見せて確かめる（アプリの Pull のシート）。なければ、そのまま
        // （fetch して、新しく来たものを取り込む）。
        match (self.behind, self.upstream.clone()) {
            (1.., Some(upstream)) => {
                self.confirm = Some((
                    match self.behind {
                        1 => tr!("Pull 1 commit from {upstream}?", upstream = upstream),
                        n => tr!(
                            "Pull {count} commits from {upstream}?",
                            count = n,
                            upstream = upstream
                        ),
                    },
                    tr!("pull"),
                    Request::Run {
                        name: "pull",
                        command: json!({"op": "pull"}),
                    },
                ));
                self.send(Request::Run {
                    name: "syncCommits",
                    command: json!({"op": "between", "base": "HEAD", "tip": upstream,
                        "limit": 3}),
                });
                self.dirty = true;
            }
            _ => self.run("pull", tr!("Pulling…"), json!({"op": "pull"})),
        }
    }

    /// 分かれているときの pull の選択。`conflicts` は上流を merge したときにコンフリクトしそうなファイル（分から
    /// なければ `None`。rebase でも最後の結果は同じなので目安になる）。
    fn ask_pull_with(&mut self, conflicts: Option<Vec<String>>) {
        let pull = |mode: &str| Request::Run {
            name: "pull",
            command: json!({"op": "pull", "mode": mode}),
        };
        self.choices = Some((
            tr!(
                "Pull: the branch and its upstream have diverged.{risk} How?",
                risk = risk_text(conflicts)
            ),
            vec![
                ('y', tr!("as git is set"), pull("git")),
                ('m', tr!("merge"), pull("merge")),
                ('r', tr!("rebase"), pull("rebase")),
            ],
        ));
        self.dirty = true;
    }

    /// 今のブランチを上流へ push する前に確かめる。
    fn ask_push(&mut self) {
        let Some(branch) = self.branch.clone() else {
            self.error = Some(tr!("No branch is checked out"));
            self.dirty = true;
            return;
        };
        let upstream = self.upstream.clone();
        self.ask_push_of(branch, upstream, self.ahead, self.behind, true);
    }

    /// サイドバーで選んでいる、チェックアウトしていないローカルのブランチ。
    /// サイドバーで選んでいる subtree のフォルダ。
    fn picked_subtree(&self) -> Option<String> {
        if self.focus != Focus::Sidebar {
            return None;
        }
        match self.sidebar.picked_ref()? {
            ("subtree", dir, _) => Some(dir.to_owned()),
            _ => None,
        }
    }

    /// subtree の相手（URL と ref）を打たせる。覚えている相手を初めに入れておく（Push は Push の先の ref）。
    /// `what` は add・pull・push。
    fn subtree_source(&mut self, dir: &str, what: &str) {
        let remembered = self.sidebar.subtree(dir).cloned().unwrap_or_default();
        let url = remembered["url"].as_str().unwrap_or("");
        let r = match what {
            "push" => remembered["pushRef"]
                .as_str()
                .or(remembered["ref"].as_str()),
            _ => remembered["ref"].as_str(),
        }
        .unwrap_or("");
        let filled = match (url.is_empty(), r.is_empty()) {
            (true, _) => String::new(),
            (false, true) => format!("{url} "),
            (false, false) => format!("{url} {r}"),
        };
        self.naming = Naming::SubtreeSource;
        self.typing_branch = Some((dir.to_owned(), what.to_owned(), filled));
        self.dirty = true;
    }

    /// subtree の相手が決まった: Add・Pull は squash するかを選ばせ、Push は確かめる。
    fn ask_subtree(&mut self, dir: &str, what: &str, url: &str, r: &str) {
        let op = |kind: &str, squash: Option<bool>| {
            let mut action = json!({"kind": kind, "dir": dir, "url": url, "ref": r});
            if let Some(squash) = squash {
                action["squash"] = json!(squash);
            }
            Request::Run {
                name: "subtreeChanged",
                command: json!({"op": "subtree", "action": action}),
            }
        };
        if what == "push" {
            self.confirm = Some((
                tr!(
                    "Push the history of {dir} to {url} {r} (git subtree push)?",
                    dir = dir,
                    url = url,
                    r = r
                ),
                tr!("push"),
                op("push", None),
            ));
            return;
        }
        // 前に選んだ squash を先に（なければ git の既定の、squash しない）。
        let squashed = self
            .sidebar
            .subtree(dir)
            .and_then(|s| s["squash"].as_bool())
            == Some(true);
        let ask = match what {
            "add" => tr!("Add {url} {r} as {dir}?", url = url, r = r, dir = dir),
            _ => tr!(
                "Pull {url} {r} into {dir}? Undo brings it back.",
                url = url,
                r = r,
                dir = dir
            ),
        };
        let mut options = vec![
            ('y', tr!("with its whole history"), op(what, Some(false))),
            ('s', tr!("squashed into one commit"), op(what, Some(true))),
        ];
        if squashed {
            options.swap(0, 1);
        }
        self.choices = Some((ask, options));
    }

    fn picked_other_branch(&self) -> Option<String> {
        if self.focus != Focus::Sidebar || self.sidebar.picked_is_current() {
            return None;
        }
        match self.sidebar.picked_ref()? {
            ("branch", name, _) => Some(name.to_owned()),
            _ => None,
        }
    }

    /// チェックアウトしていないブランチ `branch` を上流まで進める前に、前と後の絵を読んで確かめる（アプリの
    /// Fast-Forward）。上流になく分かれていれば、理由を出す。
    fn ask_fast_forward(&mut self, branch: String) {
        self.dirty = true;
        let Some(upstream) = self.sidebar.upstream_of(&branch) else {
            self.error = Some(tr!("{branch} has no upstream", branch = branch));
            return;
        };
        match self.sidebar.picked_counts() {
            (_, 0) => {
                self.notice = Some(tr!(
                    "{branch} has everything in {upstream}",
                    branch = branch,
                    upstream = upstream
                ))
            }
            (1.., _) => {
                self.error = Some(tr!(
                    "{branch} and {upstream} have diverged, so it can’t be fast-forwarded",
                    branch = branch,
                    upstream = upstream
                ))
            }
            (0, behind) => {
                self.fast_forward = Some((branch.clone(), upstream.clone(), behind));
                self.send(Request::Run {
                    name: "fastForwardPreview",
                    command: json!({"op": "movePreview", "from": branch, "to": upstream,
                        "branch": branch}),
                });
            }
        }
    }

    /// 上流 `upstream` へ push する（チェックアウトしているブランチなら `current`）。上流がなければ publish する。
    fn ask_push_of(
        &mut self,
        branch: String,
        upstream: Option<String>,
        ahead: u64,
        behind: u64,
        current: bool,
    ) {
        self.dirty = true;
        let push =
            |remote: &str, remote_branch: &str, force: bool, set_upstream: bool| Request::Run {
                name: "push",
                command: json!({
                    "op": "push",
                    "remote": remote,
                    "branch": branch,
                    "remoteBranch": remote_branch,
                    "force": force,
                    "setUpstream": set_upstream,
                }),
            };
        let Some((remote, remote_branch)) = upstream.as_deref().and_then(|u| u.split_once('/'))
        else {
            // 上流がなければ、リモートを選んで publish する（同じ名前のブランチを作り、上流にする）。
            match self.remotes.as_slice() {
                [] => {
                    self.error = Some(tr!(
                        "There is no remote to publish {branch} to",
                        branch = branch
                    ))
                }
                [remote] => {
                    self.confirm = Some((
                        tr!(
                            "Publish {branch} to {remote}?",
                            branch = branch,
                            remote = remote
                        ),
                        tr!("publish"),
                        push(remote, &branch, false, true),
                    ));
                }
                remotes => {
                    let options = remotes
                        .iter()
                        .zip('1'..='9')
                        .map(|(remote, key)| {
                            (key, remote.clone(), push(remote, &branch, false, true))
                        })
                        .collect();
                    self.choices = Some((
                        tr!("Publish {branch} to which remote?", branch = branch),
                        options,
                    ));
                }
            }
            return;
        };
        if ahead == 0 && !current {
            self.notice = Some(tr!(
                "{remote}/{remote_branch} has everything in {branch}",
                remote = remote,
                remote_branch = remote_branch,
                branch = branch
            ));
            return;
        }
        let ask = match ahead {
            1 => tr!(
                "Push {branch} to {remote}/{remote_branch} (1 commit)?",
                branch = branch,
                remote = remote,
                remote_branch = remote_branch
            ),
            count => tr!(
                "Push {branch} to {remote}/{remote_branch} ({count} commits)?",
                branch = branch,
                remote = remote,
                remote_branch = remote_branch,
                count = count
            ),
        };
        // 上流に手元にないコミットがあれば、ふつうの push は断られる。置き換える push（上流が最後に見たときの
        // ままのときだけ）も選べるようにする。
        // `t` で、ほかのブランチへ送れる（`ask_push_to`。選ぶ前に横取りするので、中身は送らない）。今のブランチだけ。
        self.push_to_offered = current;
        self.pushing_to = format!("{remote}/{remote_branch}");
        // 送るコミットの件名を、裏で読んで確かめる文に足す（アプリの Push のシートのコミットの一覧の代わり）。
        self.send(Request::Run {
            name: "syncCommits",
            command: json!({"op": "between", "base": format!("{remote}/{remote_branch}"),
                "tip": branch, "limit": 3}),
        });
        if behind > 0 {
            let mut options = vec![
                ('y', tr!("push"), push(remote, remote_branch, false, false)),
                (
                    'f',
                    tr!(
                        "force push (with lease), replacing {count} on the remote",
                        count = behind
                    ),
                    push(remote, remote_branch, true, false),
                ),
            ];
            if current {
                options.push(('t', tr!("another branch…"), Request::Status));
            }
            self.choices = Some((ask, options));
            // 置き換える push の前と後の絵を、裏で読んで重ねる（アプリの Force Push のシートと同じ）。
            self.send(Request::Call {
                name: "pushPreview",
                request: json!({"op": "run", "command": {
                    "op": "movePreview", "from": format!("{remote}/{remote_branch}"),
                    "to": branch, "branch": null,
                }}),
            });
            return;
        }
        self.confirm = Some((ask, tr!("push"), push(remote, remote_branch, false, false)));
    }

    /// サイドバーで選んでいるリモートのブランチを、今のブランチの上流にする（確かめてから）。
    fn ask_set_upstream(&mut self) {
        let Some(branch) = self.branch.clone() else {
            self.error = Some(tr!("No branch is checked out"));
            return;
        };
        let picked = (self.sidebar.picked_ref())
            .filter(|r| r.0 == "remote")
            .map(|r| r.1.to_owned());
        // リモートの名前は、知っているもののうち合うもの（名前に `/` があってもよいよう、長いほうから）。
        let split = picked.as_deref().and_then(|name| {
            let mut remotes: Vec<&String> = self.remotes.iter().collect();
            remotes.sort_by_key(|r| std::cmp::Reverse(r.len()));
            remotes.into_iter().find_map(|r| {
                let rest = name.strip_prefix(r.as_str())?.strip_prefix('/')?;
                (!rest.is_empty()).then(|| (r.clone(), rest.to_owned()))
            })
        });
        let Some((remote, remote_branch)) = split else {
            self.error = Some(tr!("Select a remote branch to be the upstream"));
            return;
        };
        let set = |key: &str, value: String| json!({"op": "configSet", "key": key, "value": value});
        self.confirm = Some((
            tr!(
                "Set the upstream of {branch} to {remote}/{remote_branch}?",
                branch = branch,
                remote = remote,
                remote_branch = remote_branch
            ),
            tr!("set"),
            Request::Run {
                name: "setUpstream",
                command: json!([
                    set(&format!("branch.{branch}.remote"), remote),
                    set(
                        &format!("branch.{branch}.merge"),
                        format!("refs/heads/{remote_branch}")
                    ),
                ]),
            },
        ));
    }

    /// `rev` の時点のファイルを `file` に書き出す（`git archive`）。形はファイルの拡張子で決め、中身はファイルの名前の
    /// フォルダに入れる（アプリと同じ）。相対のパスは、TUI を始めたフォルダから。
    fn export_archive(&mut self, rev: &str, file: &str, folder: Option<&str>) {
        let format = [
            (".zip", "zip"),
            (".tar.gz", "tar.gz"),
            (".tgz", "tar.gz"),
            (".tar", "tar"),
        ]
        .into_iter()
        .find(|(ext, _)| file.ends_with(ext));
        let Some((ext, format)) = format else {
            self.error = Some(tr!("End the file name with .zip, .tar.gz or .tar"));
            return;
        };
        let path = std::path::PathBuf::from(local_path(file));
        let stem = path
            .file_name()
            .map(|n| n.to_string_lossy().trim_end_matches(ext).to_owned())
            .unwrap_or_default();
        let prefix = match stem.is_empty() {
            true => String::new(),
            false => format!("{stem}/"),
        };
        let file = path.to_string_lossy().into_owned();
        let export = |prefix: &str, paths: &[&str]| Request::Run {
            name: "archive",
            command: json!({
                "op": "archive", "rev": rev, "format": format, "prefix": prefix, "paths": paths,
                "file": file,
            }),
        };
        // アプリのシートと同じく、中身をフォルダに入れるか、先頭に置くか、1 つのフォルダだけにするかを選ぶ。
        match folder {
            Some(folder) => {
                let folder = folder.trim_matches('/');
                self.confirm = Some((
                    tr!(
                        "Export only {folder} to {file}?",
                        folder = folder,
                        file = file
                    ),
                    tr!("export"),
                    export(&prefix, &[folder]),
                ));
            }
            None => {
                let mut options: Vec<Choice> = vec![
                    (
                        'y',
                        tr!("in a folder {folder}", folder = prefix),
                        export(&prefix, &[]),
                    ),
                    ('t', tr!("at the top, without a folder"), export("", &[])),
                    // 選ぶ前に横取りして、フォルダを聞く（中身は送らない）。
                    ('o', tr!("only one folder…"), Request::Status),
                ];
                if prefix.is_empty() {
                    options.remove(0);
                }
                self.archive_folder = Some((rev.to_owned(), file.clone()));
                self.choices = Some((
                    tr!(
                        "Export the files of {rev} to {file}?",
                        rev = &rev[..rev.len().min(8)],
                        file = file
                    ),
                    options,
                ));
            }
        }
        self.dirty = true;
    }

    /// Update Submodules: `submodules` の答えから、すべての submodule を取り出して記録したコミットにするか、中の
    /// submodule までか、追うブランチの最新にするかを選ばせる（アプリのシートの選び方と同じ）。
    fn ask_update_submodules(&mut self, info: &Value) {
        let paths: Vec<String> = (info["submodules"].as_array().into_iter().flatten())
            .filter_map(|s| s["path"].as_str().map(str::to_owned))
            .collect();
        self.dirty = true;
        if paths.is_empty() {
            self.notice = Some(tr!("There are no submodules"));
            return;
        }
        let update = |recursive: bool, remote: bool| Request::Run {
            name: "updateSubmodules",
            command: json!({
                "op": "updateSubmodules", "paths": paths, "recursive": recursive, "remote": remote,
            }),
        };
        self.choices = Some((
            tr!("Update {names}?", names = amonhen_model::i18n::list(&paths)),
            vec![
                ('y', tr!("to the recorded commits"), update(false, false)),
                ('r', tr!("recursively"), update(true, false)),
                ('t', tr!("to their branches' latest"), update(false, true)),
            ],
        ));
    }

    /// Clean Up Branches: `branchCleanup` の答えから、消すブランチを確かめる。最初に選んでおくもの（既定の
    /// ブランチに取り込み済み）を消すか、上流の消えたものも含めるかを選ばせる（アプリのシートと同じく、上流が消えた
    /// だけのものは最初は選ばない）。
    fn ask_clean_up(&mut self, info: &Value) {
        let branches: Vec<&Value> = info["branches"].as_array().into_iter().flatten().collect();
        let pick = |all: bool| -> Vec<Value> {
            (branches.iter())
                .filter(|b| all || b["checked"] == true)
                .map(|b| json!({"name": b["name"], "force": b["force"]}))
                .collect()
        };
        let names = |picked: &[Value]| -> String {
            let names: Vec<&str> = picked.iter().filter_map(|b| b["name"].as_str()).collect();
            amonhen_model::i18n::list(&names)
        };
        let (checked, all) = (pick(false), pick(true));
        let delete = |branches: Vec<Value>| Request::Run {
            name: "cleanUp",
            command: json!({"op": "deleteBranches", "branches": branches}),
        };
        self.dirty = true;
        if all.is_empty() {
            self.notice = Some(tr!("No branches to clean up"));
            return;
        }
        let base = info["base"].as_str().unwrap_or("");
        let mut options = Vec::new();
        if !checked.is_empty() {
            options.push((
                'y',
                tr!("delete those merged into {base}", base = base),
                delete(checked.clone()),
            ));
        }
        if all.len() > checked.len() {
            let gone = all.len() - checked.len();
            let label = match checked.is_empty() {
                true => tr!(
                    "delete those whose upstream is gone ({count})",
                    count = gone
                ),
                false => tr!("also those whose upstream is gone ({count})", count = gone),
            };
            options.push(('a', label, delete(all.clone())));
        }
        let shown = match checked.is_empty() {
            true => names(&all),
            false => names(&checked),
        };
        self.choices = Some((
            tr!("Clean up {names}? Undo brings them back.", names = shown),
            options,
        ));
    }

    /// 履歴のない新しいブランチ `name` を、空から作るか、今のファイルを stage したまま作るかを選ばせる。
    fn ask_orphan(&mut self, name: &str) {
        let orphan = |keep_files: bool| Request::Run {
            name: "orphan",
            command: json!({"op": "createOrphanBranch", "name": name, "keepFiles": keep_files}),
        };
        self.choices = Some((
            tr!("Start {name} with no history?", name = name),
            vec![
                ('e', tr!("empty"), orphan(false)),
                ('k', tr!("keeping the current files staged"), orphan(true)),
            ],
        ));
        self.dirty = true;
    }

    /// push の送り先を打たせる（初めは上流）。
    fn ask_push_to(&mut self) {
        let Some(branch) = self.branch.clone() else {
            return;
        };
        let start = self.upstream.clone().unwrap_or_default();
        self.naming = Naming::PushTo;
        self.typing_branch = Some((branch.clone(), branch, start));
    }

    /// `branch` を `target`（`<remote>/<branch>`）へ push する。上流は変えない。
    fn push_to(&mut self, branch: &str, target: &str) {
        let parsed = target
            .split_once('/')
            .filter(|(remote, to)| !to.is_empty() && self.remotes.iter().any(|r| r == remote));
        let Some((remote, to)) = parsed else {
            self.error = Some(tr!(
                "Write the remote and the branch, like {example}",
                example = format!("{}/{branch}", self.remotes.first().map_or("origin", |r| r))
            ));
            return;
        };
        self.run(
            "push",
            tr!("Pushing to {target}…", target = target),
            json!({
                "op": "push", "remote": remote, "branch": branch, "remoteBranch": to,
                "force": false, "setUpstream": false,
            }),
        );
    }

    /// 編集モードのキー。`j` `k` で選び、`J` `K` で動かし、行ですることを選ぶ。Enter で見通しを読んでから確かめて
    /// 当てる。Esc でやめる（変更があれば、もう 1 度押してから）。
    fn edit_mode_key(&mut self, code: KeyCode) {
        use crate::editmode::Action;
        let Some(mode) = self.edit_mode.as_mut() else {
            return;
        };
        self.dirty = true;
        let last = mode.rows.len().saturating_sub(1);
        let result = match code {
            KeyCode::Char('j') | KeyCode::Down => {
                mode.selected = (mode.selected + 1).min(last);
                Ok(())
            }
            KeyCode::Char('k') | KeyCode::Up => {
                mode.selected = mode.selected.saturating_sub(1);
                Ok(())
            }
            KeyCode::Char('J') => mode.move_selected(1),
            KeyCode::Char('K') => mode.move_selected(-1),
            KeyCode::Char('p') => mode.set(Action::Pick),
            KeyCode::Char('s') => mode.set(Action::Squash),
            KeyCode::Char('f') => mode.set(Action::Fixup),
            KeyCode::Char('d') => mode.set(Action::Drop),
            KeyCode::Char('e') => mode.set(Action::Edit),
            KeyCode::Char('r') => match mode.message_to_edit() {
                Some(text) => {
                    self.plan_message = Some(text);
                    Ok(())
                }
                None => Err(tr!("A merge is recreated as it is")),
            },
            KeyCode::Enter => {
                if !mode.has_changes() {
                    self.edit_mode = None;
                    self.notice = Some(tr!("Nothing was changed"));
                    return;
                }
                let edit = mode.plan();
                self.run(
                    "editPreview",
                    tr!("Looking ahead…"),
                    json!({"op": "rewritePreview", "edit": edit, "onto": null}),
                );
                Ok(())
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if mode.has_changes() && !self.edit_discard {
                    self.edit_discard = true;
                    self.notice = Some(tr!("Press Esc again to discard these changes"));
                    return;
                }
                self.edit_mode = None;
                Ok(())
            }
            _ => Ok(()),
        };
        self.edit_discard = false;
        if let Err(reason) = result {
            self.error = Some(reason);
        }
    }

    /// ファイルの Blame か File History を開く（戻るところは今の画面）。
    fn open_file_view(&mut self, path: &str, blame: bool) {
        self.file.open(path, blame, None);
        self.sidebar.set_range_diff(false);
        if self.view != View::File {
            self.file_back = self.view;
        }
        self.view = View::File;
        self.focus = Focus::Main;
        self.dirty = true;
    }

    /// 打っているパスの最後の部分を、そのフォルダの中身から補う（`completePath` の答えを `complete_with` で）。
    fn complete_path(&mut self, typed: &str) {
        let Some(workdir) = self.workdir.clone() else {
            return;
        };
        let folder = typed.rsplit_once('/').map_or("", |(dir, _)| dir);
        let path = match folder.is_empty() {
            true => workdir,
            false => format!("{}/{folder}", workdir.trim_end_matches('/')),
        };
        self.send(Request::Call {
            name: "completePath",
            request: json!({"op": "run", "command": {"op": "listDirectory", "path": path}}),
        });
    }

    /// フォルダの中身で、打っているパスを補う。1 つに決まれば埋め（フォルダなら `/` を足す）、いくつもあれば共通の
    /// ところまで埋めて、候補を一番下の行に出す。
    fn complete_with(&mut self, entries: &Value) {
        let Some((_, _, name)) = self.typing_branch.as_mut() else {
            return;
        };
        // 取り出すフォルダは空白で区切って並べるので、最後の語を補う（フォルダだけ）。
        let folders_only = matches!(self.naming, Naming::SparseFolders);
        let (before, word) = match name.rsplit_once(' ').filter(|_| folders_only) {
            Some((before, word)) => (format!("{before} "), word.to_owned()),
            None => (String::new(), name.clone()),
        };
        let (folder, part) = match word.rsplit_once('/') {
            Some((dir, part)) => (format!("{before}{dir}/"), part.to_owned()),
            None => (before, word.clone()),
        };
        let mut matches: Vec<(String, bool)> = (entries.as_array().into_iter().flatten())
            .filter_map(|e| {
                let entry = e["name"].as_str()?;
                let is_folder = e["kind"] == "folder";
                (entry.starts_with(&part) && entry != ".git" && (is_folder || !folders_only))
                    .then(|| (entry.to_owned(), is_folder))
            })
            .collect();
        matches.sort();
        match matches.as_slice() {
            [] => self.path_choices = tr!("(no match)"),
            [(only, folder_kind)] => {
                *name = format!("{folder}{only}{}", if *folder_kind { "/" } else { "" });
            }
            several => {
                let first = &several[0].0;
                let common = (part.len()..=first.len())
                    .rev()
                    .find(|&n| {
                        first.is_char_boundary(n)
                            && several.iter().all(|(m, _)| m.starts_with(&first[..n]))
                    })
                    .unwrap_or(part.len());
                *name = format!("{folder}{}", &first[..common]);
                let shown: Vec<String> = several
                    .iter()
                    .take(8)
                    .map(|(m, f)| format!("{m}{}", if *f { "/" } else { "" }))
                    .collect();
                let more = several.len().saturating_sub(8);
                self.path_choices = match more {
                    0 => shown.join("  "),
                    _ => format!(
                        "{}  {}",
                        shown.join("  "),
                        tr!("+{count} more", count = more)
                    ),
                };
            }
        }
        self.dirty = true;
    }

    /// History で選んでいるコミットへの操作: `y` cherry-pick、`v` revert、`c` そのコミットをチェックアウト
    /// （detached HEAD）。
    /// 印をつけたコミットの ID（印がなければ選んでいるコミットの）を、端末のクリップボードに入れる。
    fn copy_ids(&mut self) -> bool {
        let ids: Vec<String> = match self.history.mark_count() {
            0 => self
                .history
                .selected_id()
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            _ => self
                .history
                .marks()
                .map(|(_, m)| m.id.to_string())
                .collect(),
        };
        self.copy_commit_ids(ids)
    }

    /// コミットの ID を端末のクリップボードに（OSC 52）。
    fn copy_commit_ids(&mut self, ids: Vec<String>) -> bool {
        if ids.is_empty() {
            return false;
        }
        self.notice = Some(match ids.len() {
            1 => tr!(
                "Copied {commit} (to the terminal’s clipboard)",
                commit = &ids[0][..8]
            ),
            n => tr!(
                "Copied {count} commit IDs (to the terminal’s clipboard)",
                count = n
            ),
        });
        self.copy = Some(ids.join("\n"));
        true
    }

    /// 印をつけたコミットを、今のブランチに cherry-pick（古い順）・revert（新しい順）する前に、前と後の絵を読んで
    /// 確かめる（アプリと同じく 1 つの命令で。コンフリクトで止まったら、残りは git が覚えていて Continue・Skip で続く）。
    fn ask_pick_marked(&mut self, revert: bool) -> bool {
        let mut marks: Vec<&Marked> = self.history.marks().map(|(_, m)| m).collect();
        if !revert {
            marks.reverse();
        }
        let oids: Vec<String> = marks.iter().map(|m| m.id.to_string()).collect();
        let is_merge = marks.iter().any(|m| m.merge);
        let count = oids.len();
        let onto = self.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
        let (ask, yes, name, title) = match revert {
            false => (
                tr!(
                    "Cherry-pick {count} commits onto {branch}, oldest first?",
                    count = count,
                    branch = onto
                ),
                tr!("cherry-pick"),
                "cherryPick",
                tr!(" Cherry-pick {count} commits ", count = count),
            ),
            true => (
                tr!(
                    "Revert {count} commits on {branch}, newest first?",
                    count = count,
                    branch = onto
                ),
                tr!("revert"),
                "revert",
                tr!(" Revert {count} commits ", count = count),
            ),
        };
        let request = Request::Run {
            name,
            command: json!({"op": name, "oids": oids, "isMerge": is_merge}),
        };
        self.pick_question = Some((ask, yes, request, title));
        self.run(
            "pickPreview",
            tr!("Looking ahead…"),
            json!({"op": "pickPreview", "oids": oids, "revert": revert}),
        );
        true
    }

    /// 印をつけたコミット（first-parent の並びで続いたもの）を、編集モードで 1 つにまとめる（アプリの Combine）。
    /// 一番古いものから HEAD までを開き、新しい方をひとつ古いものとまとめる行にしておく。当てるのは編集モードで。
    fn combine_marked(&mut self) -> bool {
        let marks: Vec<&Marked> = self.history.marks().map(|(_, m)| m).collect();
        if marks.iter().any(|m| m.merge) {
            self.error = Some(tr!("A merge can’t be combined with other commits"));
            return true;
        }
        let Some(oldest) = marks.last() else {
            return false;
        };
        self.combining = marks.iter().map(|m| m.id.to_string()).collect();
        self.send(Request::EditRows {
            from: oldest.id.to_string(),
        });
        true
    }

    fn ask_commit_operation(&mut self, code: KeyCode) -> bool {
        let Some((id, is_merge, subject)) = self.history.selected_commit() else {
            return false;
        };
        let (id, subject) = (id.to_string(), subject.to_owned());
        let short = id[..id.len().min(8)].to_owned();
        // メッセージの書き換えは、今のメッセージを読んでからエディタで開く。
        if code == KeyCode::Char('e') {
            self.rewording = Some((id.clone(), None));
            self.send(Request::Message { id });
            return true;
        }
        let rewrite = |label: &str, kind: &str| Request::Run {
            name: "rewrite",
            command: json!({"op": "rewrite", "label": label, "edit": {"kind": kind, "oid": id}}),
        };
        if code == KeyCode::Char('F') {
            self.confirm = Some((
                tr!(
                    "Fix up {commit} “{subject}” into its parent? Undo brings it back.",
                    commit = short,
                    subject = subject
                ),
                tr!("fix up"),
                rewrite("Fixup into Parent", "fixup"),
            ));
            return true;
        }
        if code == KeyCode::Char('X') {
            self.confirm = Some((
                tr!(
                    "Drop {commit} “{subject}”? Undo brings it back.",
                    commit = short,
                    subject = subject
                ),
                tr!("drop"),
                rewrite("Drop", "drop"),
            ));
            return true;
        }
        // 分ける: edit で止めて変更を作業ツリーに戻す。続きは Working Copy で分けてコミットし、Continue。
        if code == KeyCode::Char('|') {
            if is_merge {
                self.error = Some(tr!("A merge can’t be split"));
                return true;
            }
            self.confirm = Some((
                tr!(
                    "Split {commit} “{subject}”? Its changes go back to the working tree; commit them in parts, then continue. Undo brings it back.",
                    commit = short,
                    subject = subject
                ),
                tr!("split"),
                Request::Run {
                    name: "split",
                    command: json!({"op": "rewrite", "label": format!("Split {short}"),
                        "edit": {"kind": "split", "oid": id}}),
                },
            ));
            return true;
        }
        // cherry-pick・revert は、前と後の絵を読んでから確かめる（アプリのシートと同じ）。
        if matches!(code, KeyCode::Char('y' | 'v')) && !is_merge {
            let revert = code == KeyCode::Char('v');
            let (ask, yes, name, op, title) = match revert {
                false => (
                    tr!(
                        "Cherry-pick {commit} “{subject}”?",
                        commit = short,
                        subject = subject
                    ),
                    tr!("cherry-pick"),
                    "cherryPick",
                    "cherryPick",
                    tr!(" Cherry-pick {commit} ", commit = short),
                ),
                true => (
                    tr!(
                        "Revert {commit} “{subject}”?",
                        commit = short,
                        subject = subject
                    ),
                    tr!("revert"),
                    "revert",
                    "revert",
                    tr!(" Revert {commit} ", commit = short),
                ),
            };
            let request = Request::Run {
                name,
                command: json!({"op": op, "oid": id, "isMerge": false}),
            };
            self.pick_question = Some((ask, yes, request, title));
            self.run(
                "pickPreview",
                tr!("Looking ahead…"),
                json!({"op": "pickPreview", "oids": [id], "revert": revert}),
            );
            return true;
        }
        self.confirm = Some(match code {
            KeyCode::Char('y') => (
                tr!(
                    "Cherry-pick {commit} “{subject}”?",
                    commit = short,
                    subject = subject
                ),
                tr!("cherry-pick"),
                Request::Run {
                    name: "cherryPick",
                    command: json!({"op": "cherryPick", "oid": id, "isMerge": is_merge}),
                },
            ),
            KeyCode::Char('v') => (
                tr!(
                    "Revert {commit} “{subject}”?",
                    commit = short,
                    subject = subject
                ),
                tr!("revert"),
                Request::Run {
                    name: "revert",
                    command: json!({"op": "revert", "oid": id, "isMerge": is_merge}),
                },
            ),
            _ => (
                tr!("Check out {commit} (detached HEAD)?", commit = short),
                tr!("check out"),
                Request::Run {
                    name: "checkout",
                    command: json!({"op": "detach", "oid": id}),
                },
            ),
        });
        true
    }

    /// 今のブランチを、選んでいるコミット（History・Reflog）に動かす前に、どう動かすかを選ばせる。
    fn ask_reset(&mut self) {
        let id = match self.view {
            View::History => self.history.selected_id(),
            View::Reflog => self.reflog.selected_id(),
            View::Changes | View::File => None,
        };
        let Some(id) = id.map(|id| id.to_string()) else {
            return;
        };
        // 先に見通し（外れるコミット・加わるコミット・上流が持っているか）を読み、確かめる文に書く。
        self.reset_to = Some(id.clone());
        self.run(
            "resetPreview",
            tr!("Looking ahead…"),
            json!({"op": "movePreview", "from": "HEAD", "to": id, "branch": self.branch}),
        );
    }

    /// reset の見通し（`movePreview` の `info`）から、どう reset するかを選ばせる。
    fn ask_reset_with(&mut self, preview: &Value) {
        let Some(id) = self.reset_to.take() else {
            return;
        };
        let branch = self.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
        // アプリのシートの前と後の絵（同じなら、絵はない）。
        self.question_drawing = preview["drawing"].as_object().map(|_| {
            (
                tr!(" Reset {branch} ", branch = branch),
                crate::drawing::from_move(&preview["drawing"], &branch),
            )
        });
        let lost = preview["lostCount"].as_u64().unwrap_or(0);
        let gained = preview["gainedCount"].as_u64().unwrap_or(0);
        let pushed = preview["upstreamHas"].as_u64().unwrap_or(0);
        let force = match (pushed, preview["upstream"].as_str()) {
            (1.., Some(upstream)) => tr!(
                " {upstream} has {count} of the commits it leaves, so pushing will need Force Push.",
                upstream = upstream,
                count = pushed
            ),
            _ => String::new(),
        };
        let option = |key: char, mode: &'static str, label: String| {
            (
                key,
                label,
                Request::Run {
                    name: "reset",
                    command: json!({"op": "reset", "oid": id, "mode": mode}),
                },
            )
        };
        self.choices = Some((
            tr!(
                "Reset {branch} to {commit} (−{lost} +{gained} commits)? Undo brings it back.{force}",
                branch = branch,
                commit = &id[..id.len().min(8)],
                lost = lost,
                gained = gained,
                force = force
            ),
            vec![
                option('s', "soft", tr!("soft (keep the index and files)")),
                option('m', "mixed", tr!("mixed (keep the files)")),
                option(
                    'h',
                    "hard",
                    tr!("hard (stash the changes, then discard them)"),
                ),
            ],
        ));
        self.dirty = true;
    }

    /// 新しいブランチの名前を聞く。始まりは、サイドバーなら選んでいる ref、ほかは History で選んでいるコミット
    /// （History を見ていなければ HEAD）。
    fn ask_branch(&mut self) {
        self.ask_name(Naming::Branch);
    }

    /// 新しいブランチかタグの名前を聞く（始まりは `ask_branch` のとおり）。
    /// stash に何を含めるかを選ばせる（メッセージは聞いてある。空なら付けない）。
    fn ask_stash(&mut self, message: &str) {
        let message = match message.is_empty() {
            true => Value::Null,
            false => json!(message),
        };
        let stash = |extra: Value| {
            let mut command = json!({"op": "stashPush", "message": message});
            if let (Some(command), Value::Object(extra)) = (command.as_object_mut(), extra) {
                command.extend(extra);
            }
            Request::Run {
                name: "stash",
                command,
            }
        };
        let mut options = vec![
            (
                'y',
                tr!("tracked files"),
                stash(json!({"includeUntracked": false})),
            ),
            (
                'u',
                tr!("with untracked files"),
                stash(json!({"includeUntracked": true})),
            ),
            (
                'k',
                tr!("keep what is staged"),
                stash(json!({"includeUntracked": false, "keepIndex": true})),
            ),
            // アプリの Stash のシートと同じく、stage したものだけ（git 2.35 から）と、選んでいるファイルだけ。
            (
                's',
                tr!("only what is staged"),
                stash(json!({"includeUntracked": false, "staged": true})),
            ),
        ];
        if self.view == View::Changes
            && let Some(path) = self.changes.selected_path()
        {
            let untracked = self.changes.untracked_path().is_some();
            options.push((
                'f',
                tr!("only {path}", path = path),
                stash(json!({"includeUntracked": untracked, "paths": [path]})),
            ));
        }
        self.choices = Some((tr!("Stash the working changes?"), options));
        self.dirty = true;
    }

    fn ask_name(&mut self, naming: Naming) {
        let picked = match self.focus {
            Focus::Sidebar => self
                .sidebar
                .picked_ref()
                .filter(|r| matches!(r.0, "branch" | "remote" | "tag"))
                .map(|(_, name, _)| (name.to_owned(), name.to_owned())),
            _ => None,
        };
        let start = picked.or_else(|| {
            let id = match self.view {
                View::History => self.history.selected_id()?.to_string(),
                View::Reflog => self.reflog.selected_id()?.to_string(),
                View::File => self.file.selected_id()?.to_string(),
                View::Changes => self.head.clone()?,
            };
            let short = id[..id.len().min(8)].to_owned();
            Some((id, short))
        });
        if let Some((start, shown)) = start {
            self.naming = naming;
            self.typing_branch = Some((start, shown, String::new()));
            self.dirty = true;
        }
    }

    /// サイドバーで選んでいるブランチかタグを消す前に確かめる（Undo で戻せる）。今のブランチは消せない。
    fn ask_delete(&mut self) -> bool {
        let Some((kind, name, _)) = self.sidebar.picked_ref() else {
            return false;
        };
        let (op, command) = match kind {
            "branch" if Some(name) == self.branch.as_deref() => {
                self.error = Some(tr!("The current branch can’t be deleted"));
                return true;
            }
            // ほかのどこにもないコミットがあれば git は `-d` を断るので、先に見通しを読む（アプリと同じく、今の
            // ブランチか上流のどちらかに取り込まれていれば消せる）。
            "branch" => {
                let name = name.to_owned();
                let upstream = self.sidebar.upstream_of(&name);
                self.run(
                    "deletePreview",
                    tr!("Looking ahead…"),
                    json!({"op": "deletePreview", "branch": name, "against": "HEAD"}),
                );
                self.deleting = Some((name, upstream, None));
                return true;
            }
            "tag" => ("deleteTag", json!({"op": "deleteTag", "name": name})),
            _ => return false,
        };
        self.confirm = Some((
            tr!("Delete {name}? Undo brings it back.", name = name),
            tr!("delete"),
            Request::Run { name: op, command },
        ));
        true
    }

    /// bundle を `file`（この機械の）に書く前に、何を入れるかを選ばせる: すべて（HEAD とブランチ・タグ）、今の
    /// ブランチだけ、今のブランチのうち基点より後だけ（アプリのシートの「一部の ref」「基点より後」）。
    fn ask_bundle(&mut self, file: &str) {
        let bundle = |refs: Vec<String>| Request::Run {
            name: "bundle",
            command: json!({"op": "bundle", "refs": refs, "bases": [], "file": file}),
        };
        let mut options: Vec<Choice> = vec![(
            'y',
            tr!("HEAD and every branch and tag"),
            bundle(Vec::new()),
        )];
        if let Some(branch) = self.branch.clone() {
            options.push((
                'b',
                tr!("only {branch}", branch = branch),
                bundle(vec![format!("refs/heads/{branch}")]),
            ));
            // 選ぶ前に横取りして、基点を聞く（中身は送らない）。
            options.push((
                'a',
                tr!("only {branch} after a base…", branch = branch),
                Request::Status,
            ));
            self.bundle_save = Some(file.to_owned());
        }
        self.choices = Some((
            tr!("Save a bundle at {file} with what?", file = file),
            options,
        ));
        self.dirty = true;
    }

    /// コミット `oids`（空白区切り、古い順）を `place`（フォルダ、`.mbox` なら 1 つのファイル）にパッチとして書く前に、
    /// 送り状（`--cover-letter`、2 つ以上のとき）と版の番号（`-v`）を選ばせる。`version` は聞いた後の版。
    fn ask_save_patch(&mut self, oids: &str, place: &str, version: Option<u32>) {
        let ids: Vec<&str> = oids.split(' ').collect();
        let one_file = place.to_lowercase().ends_with(".mbox");
        let save = |cover: bool| {
            let mut command = json!({"op": "formatPatch", "oids": ids,
                "coverLetter": cover, "reroll": version});
            command[if one_file { "file" } else { "dir" }] = json!(place);
            Request::Run {
                name: "savePatch",
                command,
            }
        };
        let mut options: Vec<Choice> = vec![('y', tr!("save"), save(false))];
        if ids.len() >= 2 {
            options.push(('c', tr!("with a cover letter"), save(true)));
        }
        if version.is_none() {
            // 選ぶ前に横取りして、版の番号を聞く（中身は送らない）。
            options.push(('v', tr!("as a new version (-v)…"), Request::Status));
            self.patch_version = Some((oids.to_owned(), place.to_owned()));
        }
        let what = match one_file {
            true => tr!("Save as one file {place}?", place = place),
            false => tr!("Save as patches in {place}?", place = place),
        };
        let ask = match version {
            Some(v) => format!("{what} (v{v})"),
            None => what,
        };
        self.choices = Some((ask, options));
        self.dirty = true;
    }

    /// サイドバーで選んでいるリモートのブランチ（`origin/main`）のリモート。
    fn picked_remote(&self) -> Option<String> {
        let Some(("remote", name, _)) = self.sidebar.picked_ref() else {
            return None;
        };
        self.remotes
            .iter()
            .filter(|r| name.starts_with(&format!("{r}/")))
            .max_by_key(|r| r.len())
            .cloned()
    }

    /// リモートの行での `e`（Edit…）・`x`（Remove…）・`X`（Prune…）。アプリの Remotes のメニューと同じ。
    fn remote_key(&mut self, code: KeyCode) -> bool {
        let Some(remote) = self.picked_remote() else {
            return false;
        };
        match code {
            KeyCode::Char('e') => {
                self.editing_remote = Some((remote, None));
                self.send(Request::Run {
                    name: "remotesForEdit",
                    command: json!({"op": "remotes"}),
                });
            }
            KeyCode::Char('x') => {
                self.confirm = Some((
                    tr!(
                        "Remove {name}? This removes the remote’s settings and its remote-tracking branches ({name}/…); the remote repository isn’t touched. Undo can’t bring it back.",
                        name = remote
                    ),
                    tr!("remove"),
                    Request::Run {
                        name: "removeRemote",
                        command: json!({"op": "changeRemote",
                            "change": {"kind": "remove", "name": remote}}),
                    },
                ));
            }
            _ => {
                self.confirm = Some((
                    tr!(
                        "Prune {name}? This removes the remote-tracking branches whose branches are gone from {name} (git remote prune). Undo can’t bring them back.",
                        name = remote
                    ),
                    tr!("prune"),
                    Request::Run {
                        name: "pruneRemote",
                        command: json!({"op": "changeRemote",
                            "change": {"kind": "prune", "name": remote}}),
                    },
                ));
            }
        }
        self.dirty = true;
        true
    }

    /// タグのリモートでの様子 `info`（`tagRemotes`）を受けて、どのリモートに push するか（置き換えるか）、どこから
    /// 消すかを選ばせる。アプリのタグのメニューと同じく、push はまだないリモートへ、置き換えは同じ名前の別のタグが
    /// あるリモートへ、削除はあるリモートから。fetch でまだ読んでいないリモートはどれにも出し、そう書く。
    fn ask_tag_remote(&mut self, info: &Value) {
        let Some((tag, delete)) = self.tag_remote.take() else {
            return;
        };
        let checked = |remote: &str| {
            (info["remotes"].as_array().into_iter().flatten())
                .any(|r| r["name"] == remote && !r["checked"].is_null())
        };
        let state = |remote: &str| info["tags"][tag.as_str()][remote].as_str();
        let unchecked = |remote: &str| match checked(remote) {
            true => String::new(),
            false => tr!(" (not checked yet)"),
        };
        let mut choices: Vec<Choice> = Vec::new();
        for remote in &self.remotes {
            let (label, force) = match (checked(remote), state(remote), delete) {
                (true, Some("missing"), true) | (true, Some("same"), false) => continue,
                (_, _, true) => (format!("{remote}{}", unchecked(remote)), false),
                (true, Some("different"), false) => {
                    (tr!("replace on {remote}", remote = remote), true)
                }
                _ => (format!("{remote}{}", unchecked(remote)), false),
            };
            let Some(key) = char::from_digit(choices.len() as u32 + 1, 10) else {
                break;
            };
            let command = json!({"op": "pushTags", "remote": remote, "names": [tag],
                "force": force, "delete": delete});
            let name = if delete { "deleteRemoteTag" } else { "pushTag" };
            choices.push((key, label, Request::Run { name, command }));
        }
        if choices.is_empty() {
            self.notice = Some(match delete {
                true => tr!("No remote has {tag}", tag = tag),
                false => tr!("Every remote has {tag}", tag = tag),
            });
            self.dirty = true;
            return;
        }
        let ask = match delete {
            true => tr!(
                "Delete the tag {tag} from which remote? Everyone who fetches from it loses the tag; yours stays. Undo can’t bring it back.",
                tag = tag
            ),
            false => tr!(
                "Push the tag {tag} to which remote? Everyone who fetches from it gets the tag. Undo can’t take it back.",
                tag = tag
            ),
        };
        self.choices = Some((ask, choices));
        self.dirty = true;
    }

    /// どこから fetch するかを聞く。リモートが 1 つなら確かめるだけ、いくつもあれば、すべて（`git fetch --all` と
    /// 同じ）か 1 つを選ぶ。git の設定 `entries`（`configList`）で片づける（prune）なら、そう書く。
    fn ask_fetch(&mut self, entries: &Value) {
        let value = |key: &str| -> Option<String> {
            (entries.as_array().into_iter().flatten())
                .rfind(|e| {
                    e["key"]
                        .as_str()
                        .is_some_and(|k| k.eq_ignore_ascii_case(key))
                })
                .and_then(|e| e["value"].as_str())
                .map(str::to_lowercase)
        };
        // git と同じく、リモートの remote.<name>.prune、なければ fetch.prune。効いている設定の名前（片づけないなら None）。
        let prune_key = |remote: &str| -> Option<String> {
            let own = format!("remote.{remote}.prune");
            let key = if value(&own).is_some() {
                own
            } else {
                "fetch.prune".to_owned()
            };
            (value(&key).as_deref() == Some("true")).then_some(key)
        };
        let remotes = self.remotes.clone();
        let pruning: Vec<&String> = remotes.iter().filter(|r| prune_key(r).is_some()).collect();
        let mut keys: Vec<String> = Vec::new();
        for key in remotes.iter().filter_map(|r| prune_key(r)) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        let prune = match pruning.len() {
            0 => String::new(),
            n if n == remotes.len() => tr!(
                " Remote branches deleted there are removed here too ({settings}).",
                settings = keys.join(", ")
            ),
            _ => tr!(
                " Remote branches deleted on {remotes} are removed here too ({settings}).",
                remotes = pruning
                    .iter()
                    .map(|r| r.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                settings = keys.join(", ")
            ),
        };
        let fetch = |remote: Option<&str>| Request::Run {
            name: "fetch",
            command: match remote {
                Some(remote) => json!({"op": "fetch", "remote": remote}),
                None => json!({"op": "fetch"}),
            },
        };
        if let [remote] = remotes.as_slice() {
            self.confirm = Some((
                tr!(
                    "Fetch from {remote}?{prune}",
                    remote = remote,
                    prune = prune
                ),
                tr!("fetch"),
                fetch(Some(remote)),
            ));
        } else {
            let mut choices: Vec<Choice> = vec![(
                'a',
                tr!("all remotes ({remotes})", remotes = remotes.join(", ")),
                fetch(None),
            )];
            for (i, remote) in remotes.iter().take(9).enumerate() {
                let key = char::from_digit(i as u32 + 1, 10).expect("one digit");
                choices.push((key, remote.clone(), fetch(Some(remote))));
            }
            self.choices = Some((tr!("Fetch from where?{prune}", prune = prune), choices));
        }
        self.dirty = true;
    }

    /// 消すブランチの見通し `preview` を受けて、上流とも比べるか、消すかを聞く。ほかのどこにもないコミットが
    /// あれば、それが外れる絵と数を出し、Delete Anyway（`-D`）で消す。
    fn ask_delete_with(&mut self, preview: Option<Value>) {
        let Some((name, upstream, best)) = self.deleting.take() else {
            return;
        };
        let lost = |p: &Value| p["lostCount"].as_u64().unwrap_or(0);
        let best = match (best, preview) {
            (Some(b), Some(p)) => Some(if lost(&p) < lost(&b) { p } else { b }),
            (b, p) => p.or(b),
        };
        if let (Some(upstream), true) = (&upstream, best.as_ref().is_some_and(|b| lost(b) > 0)) {
            self.run(
                "deletePreview",
                tr!("Looking ahead…"),
                json!({"op": "deletePreview", "branch": name, "against": upstream}),
            );
            self.deleting = Some((name, None, best));
            return;
        }
        let count = best.as_ref().map_or(0, lost);
        let (ask, yes) = if count == 0 {
            (
                tr!("Delete {name}? Undo brings it back.", name = name),
                tr!("delete"),
            )
        } else {
            self.question_drawing = best
                .as_ref()
                .map(|b| &b["drawing"])
                .filter(|d| d.is_object())
                .map(|d| {
                    (
                        tr!(" After deleting {name} ", name = name),
                        // 消した後に、そのブランチはない。
                        crate::drawing::from_move(d, ""),
                    )
                });
            (
                tr!(
                    "Delete {name}? Commits on no other branch ({count}) go with it. Undo brings it back.",
                    name = name,
                    count = count
                ),
                tr!("delete anyway"),
            )
        };
        self.confirm = Some((
            ask,
            yes,
            Request::Run {
                name: "deleteBranch",
                command: json!({"op": "deleteBranch", "name": name, "force": count > 0}),
            },
        ));
        self.dirty = true;
    }

    /// サイドバーで選んでいるブランチの、新しい名前を聞く。
    fn ask_rename(&mut self) -> bool {
        let Some(("branch", name, _)) = self.sidebar.picked_ref() else {
            return false;
        };
        let name = name.to_owned();
        self.naming = Naming::Rename;
        self.typing_branch = Some((name.clone(), name.clone(), name));
        true
    }

    /// サイドバーで選んでいる ref をチェックアウトする: ローカルのブランチはそのまま、リモートのブランチは追跡する
    /// ローカルのブランチを作って。
    fn checkout_picked(&mut self) -> bool {
        let Some((kind, name, _)) = self.sidebar.picked_ref() else {
            return false;
        };
        let command = match kind {
            "branch" => json!({"op": "checkout", "branch": name}),
            "remote" => json!({"op": "track", "remoteBranch": name}),
            _ => {
                self.error = Some(tr!("Only branches can be checked out here"));
                return true;
            }
        };
        let label = tr!("Checking out {name}…", name = name);
        self.run("checkout", label, command);
        true
    }

    /// サイドバーで選んでいる ref を今のブランチに merge する（`merge`）か、今のブランチをその上に rebase する
    /// 前に確かめる。
    fn ask_merge_or_rebase(&mut self, merge: bool) -> bool {
        let Some((kind, name, _)) = self.sidebar.picked_ref() else {
            return false;
        };
        if kind == "stash" || kind == "worktree" {
            return false;
        }
        let Some(branch) = self.branch.clone() else {
            self.error = Some(tr!("No branch is checked out"));
            return true;
        };
        // アプリが確かめるときに描く前と後の代わりに、エンジンの見通しを読んで、確かめる文に書く。
        let name = name.to_owned();
        self.preview = Some((name.clone(), branch, merge, Value::Null));
        match merge {
            true => self.run(
                "mergePreview",
                tr!("Looking ahead…"),
                json!({"op": "mergePreview", "theirs": name}),
            ),
            false => self.run(
                "rebasePreview",
                tr!("Looking ahead…"),
                json!({"op": "rewritePreview", "onto": name}),
            ),
        }
        true
    }

    /// 印をつけた ref を、今のブランチに 1 つの merge コミットで取り込む前に、見通し（`octopusPreview`）を読む。
    fn ask_octopus(&mut self) -> bool {
        if self.branch.is_none() {
            self.error = Some(tr!("No branch is checked out"));
            return true;
        }
        let theirs = self.sidebar.marked_names();
        self.preview_octopus(theirs);
        true
    }

    /// `order` の順に取り込む octopus merge の見通しを読む（答えは `octopusPreview`）。
    fn preview_octopus(&mut self, order: Vec<String>) {
        self.octopus = Some((order.clone(), Value::Null));
        self.run(
            "octopusPreview",
            tr!("Looking ahead…"),
            json!({"op": "octopusPreview", "theirs": order}),
        );
    }

    /// octopus merge の見通しから、それぞれが持ち込むコミットの数と、重ねたときにコンフリクトしそうなファイル
    /// （`steps`。分からなければ None）を書いて確かめる（アプリの octopus merge のシートの代わり）。コンフリクト
    /// しそうなものが最後でなければ、`l` で最後に回せる（アプリの Put Last）。
    fn ask_octopus_with(&mut self, order: &[String], info: &Value, steps: Option<&[Value]>) {
        let branch = self.branch.clone().unwrap_or_default();
        let conflicts_of = |rev: &str| -> Vec<String> {
            (steps.into_iter().flatten())
                .find(|s| s["rev"] == rev)
                .and_then(|s| conflict_files(&s["conflicts"]))
                .unwrap_or_default()
        };
        let sides: Vec<String> = (info["drawing"]["sides"].as_array().into_iter().flatten())
            .filter_map(|s| {
                let rev = s["rev"].as_str()?;
                let count = u64::from(!s["tip"].is_null()) + s["more"].as_u64().unwrap_or(0);
                let files = conflicts_of(rev);
                Some(match files.len() {
                    0 => format!("{rev} ({count})"),
                    _ => tr!(
                        "{rev} ({count}, conflicts in {files})",
                        rev = rev,
                        count = count,
                        files = amonhen_model::i18n::list(
                            &files.iter().map(String::as_str).collect::<Vec<_>>()
                        )
                    ),
                })
            })
            .collect();
        let expected = match steps {
            Some(_) if order.iter().all(|r| conflicts_of(r).is_empty()) => {
                tr!(" No conflicts are expected.")
            }
            _ => String::new(),
        };
        let ask = tr!(
            "Merge {sides} into {branch} as one merge commit (octopus)?{expected} Undo brings it back.",
            sides = sides.join(", "),
            branch = branch,
            expected = expected
        );
        let merge = Request::Run {
            name: "merge",
            command: json!({"op": "merge", "rev": order}),
        };
        // コンフリクトしそうなもののうち最初のものが最後でなければ、最後に回す順を勧める。
        let first = order.iter().position(|r| !conflicts_of(r).is_empty());
        match first {
            Some(at) if at + 1 < order.len() => {
                let mut last = order.to_vec();
                let moved = last.remove(at);
                last.push(moved.clone());
                self.octopus_last = Some(last);
                self.choices = Some((
                    ask,
                    vec![
                        ('y', tr!("merge"), merge),
                        ('l', tr!("put {rev} last", rev = moved), Request::Status),
                    ],
                ));
            }
            _ => self.confirm = Some((ask, tr!("merge"), merge)),
        }
        self.dirty = true;
    }

    /// 印をつけたブランチをまとめて消す前に確かめる。ほかのどこにもないコミットのあるブランチは、git が断る（そこ
    /// で止まる）。
    fn ask_delete_marked(&mut self) -> bool {
        let names = self.sidebar.marked_names();
        let branches = self.sidebar.branches();
        if names
            .iter()
            .any(|n| !branches.iter().any(|(b, remote)| b == n && !remote))
        {
            self.error = Some(tr!("Only local branches can be deleted together"));
            return true;
        }
        if self.branch.as_ref().is_some_and(|b| names.contains(b)) {
            self.error = Some(tr!("The current branch can’t be deleted"));
            return true;
        }
        let commands: Vec<Value> = names
            .iter()
            .map(|n| json!({"op": "deleteBranch", "name": n, "force": false}))
            .collect();
        self.confirm = Some((
            tr!(
                "Delete {count} branches ({names})? A branch with commits on no other branch is refused. Undo brings them back.",
                count = names.len(),
                names = names.join(", ")
            ),
            tr!("delete"),
            Request::Run {
                name: "deleteBranch",
                command: Value::Array(commands),
            },
        ));
        true
    }

    /// merge・rebase の見通しから、確かめる文を作る。`conflicts` はコンフリクトしそうなファイル（分からなければ
    /// `None`）。
    fn ask_with_preview(&mut self, conflicts: Option<Vec<String>>) {
        let Some((name, branch, merge, preview)) = self.preview.take() else {
            return;
        };
        self.dirty = true;
        let count = |shown: &Value, more: &Value| {
            shown.as_array().map_or(0, Vec::len) as u64 + more.as_u64().unwrap_or(0)
        };
        let risk = risk_text(conflicts);
        // アプリのシートと同じく、前と後の絵を重ねる（取り込むものがなければ、絵はない）。
        self.question_drawing = match (merge, preview["fastForward"] == true) {
            (true, true) => Some(&preview["move"]["drawing"])
                .filter(|d| d.is_object())
                .map(|d| {
                    (
                        tr!(" Fast-forward {branch} ", branch = branch),
                        crate::drawing::from_move(d, &branch),
                    )
                }),
            (true, false) => Some(&preview["drawing"])
                .filter(|d| d.is_object())
                .map(|d| {
                    (
                        tr!(" Merge {name} into {branch} ", name = name, branch = branch),
                        crate::drawing::from_merge(d, &branch, &name),
                    )
                }),
            (false, _) => Some(&preview["drawing"])
                .filter(|d| d.is_object())
                .map(|d| {
                    (
                        tr!(
                            " Rebase {branch} onto {name} ",
                            branch = branch,
                            name = name
                        ),
                        crate::drawing::from_rebase(d, &branch, &name),
                    )
                }),
        };
        let (ask, yes, request) = if merge {
            // 取り込み方（アプリの Merge のシートの選択。覚えない）。`y` は git の設定のとおり。
            let mode = |mode: &str| Request::Run {
                name: "merge",
                command: json!({"op": "merge", "rev": name, "mode": mode}),
            };
            if preview["upToDate"] == true {
                self.notice = Some(tr!(
                    "{branch} already has {name}",
                    branch = branch,
                    name = name
                ));
                return;
            }
            if preview["fastForward"] == true {
                let drawing = &preview["move"]["drawing"];
                let n = count(&drawing["gained"], &drawing["gainedMore"]);
                let ask = tr!(
                    "Fast-forward {branch} to {name} (+{count})?",
                    branch = branch,
                    name = name,
                    count = n
                );
                self.choices = Some((
                    ask,
                    vec![
                        ('y', tr!("merge"), mode("git")),
                        ('f', "fast-forward only".to_owned(), mode("fastForwardOnly")),
                        ('c', tr!("always a merge commit"), mode("noFastForward")),
                        ('s', tr!("squash into one commit"), mode("squash")),
                    ],
                ));
                return;
            } else {
                let drawing = &preview["drawing"];
                let theirs = count(&drawing["theirs"], &drawing["theirsMore"]);
                let ours = count(&drawing["ours"], &drawing["oursMore"]);
                let ask = tr!(
                    "Merge {name} into {branch} as “{subject}” (+{theirs} in, {ours} on {branch} since they split).{risk}",
                    name = name,
                    branch = branch,
                    theirs = theirs,
                    ours = ours,
                    subject = drawing["subject"].as_str().unwrap_or(""),
                    risk = risk
                );
                // 分かれていれば fast-forward はできない（merge コミットか、まとめて 1 つのコミットか）。
                self.choices = Some((
                    ask,
                    vec![
                        ('y', tr!("merge"), mode("git")),
                        ('s', tr!("squash into one commit"), mode("squash")),
                    ],
                ));
                return;
            }
        } else {
            let ask = tr!(
                "Rebase {branch} onto {name}?{risk}{notes}",
                branch = branch,
                name = name,
                risk = risk,
                notes = rewrite_notes(&preview)
            );
            let request = Request::Run {
                name: "rebase",
                command: json!({"op": "rebase", "onto": name}),
            };
            (ask, tr!("rebase"), request)
        };
        self.confirm = Some((ask, yes, request));
    }

    /// サイドバーで選んでいる stash を戻す（`pop` なら、戻して消す）か、消す（先に確かめる）。
    /// stash を作業ツリーに戻す前に、どの stash かを見せて確かめる（アプリと同じ）。`pop` なら一覧から消す
    /// （コンフリクトしたら残す）。
    fn ask_stash_apply(&mut self, name: String, pop: bool) {
        let message = self
            .sidebar
            .detail_of("stash", &name)
            .map_or(String::new(), |m| format!(" “{m}”"));
        let (ask, yes, op) = match pop {
            true => (
                tr!(
                    "Pop {name}{message}? It goes back to the working tree and leaves the list. Undo brings it back.",
                    name = name,
                    message = message
                ),
                tr!("pop"),
                "pop",
            ),
            false => (
                tr!(
                    "Apply {name}{message}? It goes back to the working tree and stays in the list. Undo brings it back.",
                    name = name,
                    message = message
                ),
                tr!("apply"),
                "apply",
            ),
        };
        self.confirm = Some((
            ask,
            yes,
            Request::Run {
                name: op,
                command: json!({"op": "stashApply", "name": name, "pop": pop}),
            },
        ));
        self.dirty = true;
    }

    /// 消す worktree に `changes` 個の変更があるのを受けて、確かめる。変更があれば、それごと消す（`--force`）。
    fn ask_remove_worktree(&mut self, changes: u64) {
        let Some(path) = self.removing_worktree.take() else {
            return;
        };
        let ask = match changes {
            0 => tr!(
                "Remove the worktree at {path}? The branch stays. Undo can’t bring it back.",
                path = path
            ),
            count => tr!(
                "Remove the worktree at {path}? Its uncommitted changes in {count} files are lost with the folder; the branch stays. Undo can’t bring them back.",
                path = path,
                count = count
            ),
        };
        self.confirm = Some((
            ask,
            tr!("remove"),
            Request::Run {
                name: "removeWorktree",
                command: json!({"op": "worktreeRemove", "path": path, "force": changes > 0}),
            },
        ));
        self.dirty = true;
    }

    /// worktree の行の `L`（ロックする・外す）と `n`（フォルダを移す。見つからなければ今の場所を聞いてつなぎ直す）。
    /// アプリの worktree のメニューの Lock・Unlock・Move…・Locate…。
    fn worktree_key(&mut self, code: KeyCode) -> bool {
        let Some(("worktree", path, _)) = self.sidebar.picked_ref() else {
            return false;
        };
        let path = path.to_owned();
        let (locked, missing, main) = self.sidebar.worktree_state(&path);
        match code {
            KeyCode::Char('L') if locked => {
                self.confirm = Some((
                    tr!("Unlock the worktree at {path}?", path = path),
                    tr!("unlock"),
                    Request::Run {
                        name: "unlockWorktree",
                        command: json!({"op": "worktreeUnlock", "path": path}),
                    },
                ));
            }
            KeyCode::Char('L') => {
                let lock = Request::Run {
                    name: "lockWorktree",
                    command: json!({"op": "worktreeLock", "path": path, "reason": null}),
                };
                // 選ぶ前に横取りして、理由を聞く（中身は送らない）。
                self.worktree_lock = Some(path.clone());
                self.choices = Some((
                    tr!(
                        "Lock the worktree at {path}? git then won’t prune, remove or move it.",
                        path = path
                    ),
                    vec![
                        ('y', tr!("lock"), lock),
                        ('r', tr!("with a reason…"), Request::Status),
                    ],
                ));
            }
            KeyCode::Char('n') if missing => {
                self.naming = Naming::WorktreeLocate;
                self.typing_branch = Some((path.clone(), path.clone(), path));
            }
            KeyCode::Char('n') if main => {
                self.error = Some(tr!("The main worktree can’t be moved"));
            }
            KeyCode::Char('n') if self.sidebar.picked_is_current() => {
                self.error = Some(tr!("This worktree is the one that is open"));
            }
            KeyCode::Char('n') if locked => {
                self.error = Some(tr!("The worktree is locked: unlock it (L) first"));
            }
            KeyCode::Char('n') => {
                self.naming = Naming::WorktreeMove;
                self.typing_branch = Some((path.clone(), path.clone(), path));
            }
            _ => return false,
        }
        self.dirty = true;
        true
    }

    fn stash_key(&mut self, code: KeyCode) -> bool {
        // worktree の行の `x` は、その worktree を消す。先に変更の数を読み、あればそれごと消える（`--force`）と書いて
        // 確かめる（アプリと同じ）。
        if let (KeyCode::Char('x'), Some(("worktree", path, _))) = (code, self.sidebar.picked_ref())
        {
            let path = path.to_owned();
            if self.sidebar.picked_is_current() {
                self.error = Some(tr!("This worktree is the one that is open"));
                return true;
            }
            self.removing_worktree = Some(path.clone());
            self.send(Request::Run {
                name: "worktreeChangesForRemove",
                command: json!({"op": "worktreeChanges", "paths": [path], "fresh": true}),
            });
            return true;
        }
        let Some(("stash", name, _)) = self.sidebar.picked_ref() else {
            return false;
        };
        let name = name.to_owned();
        match code {
            KeyCode::Char('a') => self.ask_stash_apply(name, false),
            KeyCode::Char('o') => self.ask_stash_apply(name, true),
            // stash を作ったところに新しいブランチを作って切り替え、そこに戻す（アプリの Branch…）。
            KeyCode::Char('b') => {
                self.naming = Naming::StashBranch;
                self.typing_branch = Some((name.clone(), name, String::new()));
                self.dirty = true;
            }
            KeyCode::Char('x') => {
                self.confirm = Some((
                    tr!("Drop {name}? Undo brings it back.", name = name),
                    tr!("drop"),
                    Request::Run {
                        name: "drop",
                        command: json!({"op": "stashDrop", "name": name}),
                    },
                ));
            }
            _ => return false,
        }
        true
    }

    /// History を検索する。動いている検索は取り消す。
    fn start_search(&mut self, query: String) {
        self.cancel_search();
        // 合うものだけにしていれば、新しい語で絞り直す。
        if self.matching.is_some() {
            self.matching = Some(query.clone());
            self.search = Some((query, None));
            self.open();
            return;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        self.send(Request::Search {
            history: self.history.id().clone(),
            query: query.clone(),
            cancelled: cancelled.clone(),
        });
        self.search = Some((query, Some(cancelled)));
        self.search_done = 0.0;
        self.history.begin_search();
    }

    fn cancel_search(&mut self) {
        if let Some((_, Some(running))) = &self.search {
            running.store(true, Ordering::Relaxed);
        }
    }

    fn clear_search(&mut self) -> bool {
        self.cancel_search();
        let had = self.search.take().is_some();
        // 合うものだけにしていたら、すべてに戻す。
        if self.matching.take().is_some() {
            self.open();
        }
        self.history.clear_matches() || had
    }

    fn file_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.file.page();
        match code {
            KeyCode::Char('d') if control => self.file.move_selection(page / 2),
            KeyCode::Char('u') if control => self.file.move_selection(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.file.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.file.move_selection(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.file.move_selection(page),
            KeyCode::PageUp => self.file.move_selection(-page),
            KeyCode::Char('g') | KeyCode::Home => self.file.select(0),
            KeyCode::Char('G') | KeyCode::End => self.file.select(usize::MAX),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                self.focus = Focus::Side;
                true
            }
            // range-diff で、変わらなかった組を出すか（アプリの Changes Only）。
            KeyCode::Char('c') => self.file.toggle_changes_only(),
            // range-diff の範囲を打ち直す（アプリの範囲の欄）。
            KeyCode::Char('r') if self.file.is_range_diff() => {
                self.naming = Naming::RangeDiffRanges;
                self.typing_branch =
                    Some((String::new(), String::new(), self.range_ranges.join(" ")));
                true
            }
            // File History で、作業ツリーのファイルをその版に戻す（確かめてから。Undo で元に戻る）。
            // 同じファイルの Blame（今の HEAD の）と File History を切り替える。
            KeyCode::Char('B' | 'L') if self.file.file_path().is_some() => {
                let path = self.file.file_path().unwrap_or("").to_owned();
                self.open_file_view(&path, code == KeyCode::Char('B'));
                true
            }
            KeyCode::Char('V') => {
                let version = (self.file.selected_version())
                    .map(|(id, path, then)| (id.to_string(), path.to_owned(), then.to_owned()));
                match version {
                    Some((_, path, then)) if path != then => {
                        self.error = Some(tr!(
                            "The file was {then} at that commit; a version from before a rename can’t be restored here",
                            then = then
                        ));
                    }
                    // アプリと同じく、その版か、その前の版（そのコミットの親の）に戻す。stage もするかを選べる。その版に
                    // ないファイルは消える。
                    Some((id, path, _)) => {
                        let restore = |rev: String, staged: bool| Request::Run {
                            name: "restoreFile",
                            command: json!({"op": "restoreFiles", "rev": rev, "paths": [path],
                                "staged": staged}),
                        };
                        let before = format!("{id}^");
                        self.choices = Some((
                            tr!(
                                "Restore {path} from {commit}? A file that version doesn’t have is deleted. Undo brings back what it has now.",
                                path = path,
                                commit = &id[..id.len().min(8)]
                            ),
                            vec![
                                ('y', tr!("this version"), restore(id.clone(), false)),
                                (
                                    's',
                                    tr!("this version, staged too"),
                                    restore(id.clone(), true),
                                ),
                                ('b', tr!("as it was before"), restore(before.clone(), false)),
                                (
                                    'B',
                                    tr!("as it was before, staged too"),
                                    restore(before, true),
                                ),
                            ],
                        ));
                    }
                    None => self.error = Some(tr!("Open File History (L) to restore a version")),
                }
                true
            }
            // その行のコミットを、History で選ぶ。
            KeyCode::Char('H') => {
                if let Some(id) = self.file.selected_id() {
                    self.view = View::History;
                    self.sidebar.show_view(Picked::History);
                    self.send(Request::RowOf {
                        history: self.history.id().clone(),
                        id: id.to_string(),
                    });
                }
                true
            }
            KeyCode::Esc | KeyCode::Backspace => {
                self.show(self.file_back);
                true
            }
            _ => false,
        }
    }

    fn reflog_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.reflog.page();
        match code {
            KeyCode::Char('d') if control => self.reflog.move_selection(page / 2),
            KeyCode::Char('u') if control => self.reflog.move_selection(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.reflog.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.reflog.move_selection(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.reflog.move_selection(page),
            KeyCode::PageUp => self.reflog.move_selection(-page),
            KeyCode::Char('g') | KeyCode::Home => self.reflog.select(0),
            KeyCode::Char('G') | KeyCode::End => self.reflog.select(usize::MAX),
            KeyCode::Char('h') | KeyCode::Left => {
                self.focus = Focus::Sidebar;
                true
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                self.focus = Focus::Side;
                true
            }
            // アプリの Reflog のメニューと同じ: そこをチェックアウト（detached HEAD）、History で選ぶ、ID をコピー。
            KeyCode::Char('c') => {
                let Some(id) = self.reflog.selected_id().map(|id| id.to_string()) else {
                    return false;
                };
                self.confirm = Some((
                    tr!("Check out {commit} (detached HEAD)?", commit = &id[..8]),
                    tr!("check out"),
                    Request::Run {
                        name: "checkout",
                        command: json!({"op": "detach", "oid": id}),
                    },
                ));
                true
            }
            // その行のコミットを、一番上の行（今の HEAD）と 2 つの版として比べる（アプリの Compare with Current HEAD）。
            KeyCode::Char('=') => {
                let (Some(id), Some((head, selector))) =
                    (self.reflog.selected_id(), self.reflog.newest_and_selector())
                else {
                    return false;
                };
                let title = tr!(
                    "{selector} compared with the current HEAD",
                    selector = selector
                );
                self.compare_ranges(title, vec![format!("{id}...{head}")]);
                true
            }
            KeyCode::Char('H') => {
                let Some(id) = self.reflog.selected_id() else {
                    return false;
                };
                if !self.reflog.selected_reachable() {
                    self.error = Some(tr!("This commit isn’t in History"));
                    return true;
                }
                self.view = View::History;
                self.focus = Focus::Main;
                self.sidebar.show_view(Picked::History);
                self.send(Request::RowOf {
                    history: self.history.id().clone(),
                    id: id.to_string(),
                });
                true
            }
            KeyCode::Char('Y') => {
                let ids = self.reflog.selected_id().map(|id| id.to_string());
                self.copy_commit_ids(ids.into_iter().collect())
            }
            _ => false,
        }
    }

    fn sidebar_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.sidebar.page();
        match code {
            KeyCode::Char('d') if control => self.sidebar.move_selection(page / 2),
            KeyCode::Char('u') if control => self.sidebar.move_selection(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.sidebar.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.sidebar.move_selection(-1),
            KeyCode::PageDown => self.sidebar.move_selection(page),
            KeyCode::PageUp => self.sidebar.move_selection(-page),
            KeyCode::Char('g') | KeyCode::Home => self.sidebar.move_selection(i64::MIN / 2),
            KeyCode::Char('G') | KeyCode::End => self.sidebar.move_selection(i64::MAX / 2),
            // Branches の Inactive・Archived のまとまりは、開く・畳む。
            KeyCode::Enter | KeyCode::Char(' ' | 'l') | KeyCode::Right
                if self.sidebar.toggle_group() =>
            {
                true
            }
            KeyCode::Enter | KeyCode::Char(' ' | 'l') | KeyCode::Right => {
                self.pick();
                true
            }
            // submodule の行で: URL を変える、追うブランチ、消す、中のデータを .git/modules へ、URL を写す、
            // それぞれで命令を動かす（アプリの submodule のメニュー）。
            KeyCode::Char('e' | 'B' | 'x' | 'a' | 'y' | 'F')
                if self.picked_submodule().is_some() =>
            {
                let path = self.picked_submodule().unwrap_or_default();
                self.submodule_key(&path, code);
                true
            }
            // subtree の行で: そのフォルダの履歴だけのブランチを作る（Split）。
            KeyCode::Char('t') if self.picked_subtree().is_some() => {
                let dir = self.picked_subtree().unwrap_or_default();
                let branch = format!("{}-split", dir.replace('/', "-"));
                self.naming = Naming::SubtreeSplit;
                self.typing_branch = Some((dir, String::new(), branch));
                true
            }
            // タグの行で: リモートごとの様子を一番下の行に（アプリの印の吹き出し）。
            KeyCode::Char('i') if self.sidebar.picked_ref().is_some_and(|r| r.0 == "tag") => {
                let name = self
                    .sidebar
                    .picked_ref()
                    .map(|r| r.1.to_owned())
                    .unwrap_or_default();
                self.notice = Some(self.tag_remote_text(&name));
                true
            }
            // ブランチの行で: お気に入りにする・外す（★）、アーカイブする・戻す、お気に入りの順を上・下へ。
            // 印をつけたブランチがあれば、そのすべてをお気に入りにする・外す、アーカイブする・戻す。
            KeyCode::Char('*' | 'z') if !self.marked_branches().is_empty() => {
                let names = self.marked_branches();
                self.change_branches_place(&names, code);
                // 印は外す（書き終えて読み直すと、History もすべてのブランチに戻る）。
                self.sidebar.clear_marks();
                true
            }
            KeyCode::Char('*' | 'z' | '<' | '>') if self.picked_branch().is_some() => {
                let name = self.picked_branch().unwrap_or_default();
                self.change_branch_place(&name, code);
                true
            }
            KeyCode::Char('/') => {
                self.typing_filter = Some(self.sidebar.filter().to_owned());
                self.filtering_changes = false;
                true
            }
            KeyCode::Char('c') => self.checkout_picked(),
            // リモートのブランチの行で: そのリモートを直す（名前と URL）、消す、消えたブランチを片づける。
            KeyCode::Char('e' | 'x' | 'X') if self.picked_remote().is_some() => {
                self.remote_key(code)
            }
            // タグの行で: リモートへ push（同じ名前の別のタグがあれば置き換え）、リモートから消す。
            KeyCode::Char('t' | 'X')
                if matches!(self.sidebar.picked_ref(), Some(("tag", _, _))) =>
            {
                let name = self.sidebar.picked_ref().map_or("", |r| r.1).to_owned();
                if self.remotes.is_empty() {
                    self.error = Some(tr!("There is no remote"));
                    return true;
                }
                self.tag_remote = Some((name, code == KeyCode::Char('X')));
                self.send(Request::Run {
                    name: "tagRemotes",
                    command: json!({"op": "tagRemotes"}),
                });
                true
            }
            KeyCode::Char('L' | 'n')
                if matches!(self.sidebar.picked_ref(), Some(("worktree", _, _))) =>
            {
                self.worktree_key(code)
            }
            KeyCode::Char('a' | 'o' | 'x') => self.stash_key(code),
            // 印をつけた ref がいくつもあれば、まとめて: 1 つの merge コミットで取り込む（octopus）、ブランチを消す。
            KeyCode::Char('m') if self.sidebar.marked_names().len() >= 2 => self.ask_octopus(),
            KeyCode::Char('D') if self.sidebar.marked_names().len() >= 2 => {
                self.ask_delete_marked()
            }
            KeyCode::Char('D') => self.ask_delete(),
            KeyCode::Char('n') => self.ask_rename(),
            KeyCode::Char('m') => self.ask_merge_or_rebase(true),
            KeyCode::Char('r') => self.ask_merge_or_rebase(false),
            // 今のブランチの上流を、選んでいるリモートのブランチにする（アプリの Set Upstream）。
            KeyCode::Char('U') => {
                self.ask_set_upstream();
                true
            }
            // ブランチを、新しい worktree に出す。場所を聞く（今の作業ツリーの隣を、初めに入れておく）。
            KeyCode::Char('w') => {
                let picked = (self.sidebar.picked_ref())
                    .filter(|r| r.0 == "branch")
                    .map(|r| r.1.to_owned());
                match picked {
                    Some(_) if self.sidebar.picked_is_current() => {
                        self.error = Some(tr!("This branch is checked out here"));
                    }
                    Some(branch) => {
                        let path = self.workdir.as_deref().map_or(String::new(), |workdir| {
                            let workdir = workdir.trim_end_matches(['/', '\\']);
                            format!("{workdir}-{}", branch.replace(['/', '\\'], "-"))
                        });
                        self.naming = Naming::Worktree;
                        self.typing_branch = Some((branch.clone(), branch, path));
                    }
                    None => self.error = Some(tr!("Select a local branch for the new worktree")),
                }
                true
            }
            // ブランチの reflog（そのブランチが動いた記録）を、Reflog の画面に出す。
            KeyCode::Char('L') => {
                let picked = (self.sidebar.picked_ref())
                    .filter(|r| matches!(r.0, "branch" | "remote"))
                    .map(|r| r.1.to_owned());
                match picked {
                    Some(name) => {
                        self.show(View::Reflog);
                        self.reflog.show(Some(name));
                    }
                    None => self.error = Some(tr!("Select a branch to see its reflog")),
                }
                true
            }
            // 絞り込んでいれば、まずそれをやめる。
            // ref に印をつけ・外す。印をつけた ref があれば、History はそれらから辿れるものだけ（アプリの、いくつも
            // 選んだ ref の History）。
            KeyCode::Char('v') => {
                if !self.sidebar.toggle_mark() {
                    self.error = Some(tr!("Select a branch, remote branch or tag to mark"));
                    return true;
                }
                self.notice = Some(match self.sidebar.marked_names().as_slice() {
                    [] => tr!("History of all branches"),
                    names => tr!(
                        "History of {refs} (Esc clears the marks)",
                        refs = names.join(", ")
                    ),
                });
                self.open();
                true
            }
            KeyCode::Esc if self.sidebar.has_marks() => {
                self.sidebar.clear_marks();
                self.notice = Some(tr!("History of all branches"));
                self.open();
                true
            }
            KeyCode::Esc if !self.sidebar.filter().is_empty() => {
                self.sidebar.set_filter("");
                true
            }
            KeyCode::Esc => {
                self.focus = Focus::Main;
                true
            }
            _ => false,
        }
    }

    /// 今の枠で使えるキー（`?` で出す一覧）。
    fn help_lines(&self) -> Vec<(&'static str, String)> {
        let mut lines = match (self.view, self.focus) {
            (_, Focus::Sidebar) => vec![
                ("j k", tr!("Move")),
                (
                    "Enter Space",
                    tr!("Open the view, or select the ref's commit in History"),
                ),
                ("c", tr!("Check out the branch")),
                ("m", tr!("Merge it into the current branch")),
                ("r", tr!("Rebase the current branch onto it")),
                ("a o x", tr!("On a stash: apply, pop, drop")),
                (
                    "b",
                    tr!("On a stash: a branch where it was made, with it applied"),
                ),
                (
                    "x L n",
                    tr!("On a worktree: remove, lock or unlock, move (or locate) it"),
                ),
                (
                    "e x X",
                    tr!("On a remote branch: edit its remote (name, URL), remove it, prune it"),
                ),
                (
                    "t X i",
                    tr!(
                        "On a tag: push it (or replace it) to a remote; delete it there; where it is"
                    ),
                ),
                (
                    "p P",
                    tr!(
                        "On a branch not checked out: fast-forward it to its upstream; push or publish it"
                    ),
                ),
                ("w", tr!("New worktree for the branch")),
                (
                    "e B x a y F",
                    tr!("On a submodule: URL, branch to follow, remove, absorb, sync, run in each"),
                ),
                (
                    "p P t",
                    tr!("On a subtree: pull, push; split its history into a branch"),
                ),
                (
                    "U",
                    tr!("On a remote branch: make it the upstream of the current branch"),
                ),
                ("n D", tr!("Rename a branch; delete a branch or a tag")),
                (
                    "* z < >",
                    tr!("On a branch: favorite or not; archive or not; move a favorite up, down"),
                ),
                ("L", tr!("The branch's reflog")),
                (
                    "v",
                    tr!("Mark refs: History of the marked ones; m and D act on all of them"),
                ),
                ("/", tr!("Filter branches and tags")),
                ("Esc", tr!("Clear the filter, or go back")),
            ],
            (View::History, Focus::Main) => vec![
                ("j k", tr!("Move")),
                ("g G", tr!("Top and bottom")),
                ("PgUp PgDn", tr!("Move by a page")),
                ("@", tr!("Go to HEAD")),
                (
                    "o +",
                    tr!("Date or topological order; fetch more of a shallow or partial clone"),
                ),
                (
                    "z Z",
                    tr!("Fold merged branches; open or close the selected merge"),
                ),
                ("] [", tr!("Go to the parent, the child")),
                (
                    "m Shift-↑↓",
                    tr!("Select several commits: one at a time, or a run"),
                ),
                (
                    "x . =",
                    tr!("With two: swap the direction; diff from the fork; Range Diff them"),
                ),
                (
                    "y v E",
                    tr!("With several selected: cherry-pick, revert them; combine them into one"),
                ),
                (
                    "Y w",
                    tr!("Copy the commit IDs; a new worktree with a branch at the commit"),
                ),
                ("Enter Space", tr!("Go to the details")),
                ("/", tr!("Search")),
                ("n N", tr!("Next and previous match")),
                ("y v", tr!("Cherry-pick, revert the selected commit")),
                ("c", tr!("Check out the selected commit (detached HEAD)")),
                ("e", tr!("Edit the commit's message in git's editor")),
                ("F X", tr!("Fix up into the parent; drop the commit")),
                (
                    "|",
                    tr!("Split the commit: its changes go back to commit in parts"),
                ),
                ("E", tr!("Edit the commits from here to HEAD at once")),
                ("Esc", tr!("Clear the selection, then the search")),
            ],
            (View::Reflog, Focus::Main) => vec![
                ("j k", tr!("Move")),
                ("g G", tr!("Top and bottom")),
                ("Enter", tr!("Go to the details")),
                ("c", tr!("Check out the entry's commit (detached HEAD)")),
                ("H", tr!("Select the entry's commit in History")),
                ("=", tr!("Compare with the current HEAD (Range Diff)")),
                ("Y", tr!("Copy the commit ID (the terminal’s clipboard)")),
            ],
            (View::File, Focus::Main) => vec![
                ("j k", tr!("Move")),
                ("g G", tr!("Top and bottom")),
                ("Enter", tr!("Go to the commit's details")),
                ("H", tr!("Select the commit in History")),
                ("B L", tr!("Switch to the Blame, File History of the file")),
                (
                    "V",
                    tr!("In File History: restore the file to that version"),
                ),
                (
                    "c r",
                    tr!("In a range diff: changes only, or every pair; edit the ranges"),
                ),
                ("Esc", tr!("Go back")),
            ],
            (View::History | View::Reflog | View::File, Focus::Side) => vec![
                ("j k", tr!("Scroll")),
                ("[ ]", tr!("Previous and next file")),
                (
                    "t z o",
                    tr!("Files as a tree or a list; fold its folders; fold the file in view"),
                ),
                ("B L", tr!("Blame, File History of the file in view")),
                (
                    "Y e",
                    tr!("Copy the path of the file in view; open it in git's editor"),
                ),
                ("/ n N", tr!("Find in the details; next and previous")),
                (
                    "w",
                    tr!("Ignore whitespace: at line ends, its amount, all, none"),
                ),
                (
                    "+ F",
                    tr!("Widen the context of the file in view; show the whole file"),
                ),
                ("h l", tr!("Pan long lines; h at the left edge goes back")),
                ("Esc", tr!("Go back to History")),
            ],
            (View::Changes, Focus::Main) | (_, Focus::Message) => vec![
                ("j k", tr!("Move")),
                ("Space", tr!("Stage or unstage the file")),
                ("+", tr!("Stage all (not conflicted files)")),
                ("d", tr!("Discard the file's changes")),
                ("B L", tr!("Blame, File History of the file")),
                ("e", tr!("Open the file in git's editor")),
                (
                    "o t",
                    tr!("Take the current or the incoming side of a conflicted file"),
                ),
                (
                    "m",
                    tr!("Magic Merge: resolve what rules can in a conflicted file"),
                ),
                ("Enter", tr!("Go to the diff")),
                (
                    "c",
                    tr!("Write the commit message (Ctrl-e there opens git's editor)"),
                ),
                ("a", tr!("Amend the last commit")),
                (
                    "F",
                    tr!(
                        "Fixup or Squash Commit For: a recent commit's subject as fixup! or squash!"
                    ),
                ),
                (
                    "v",
                    tr!("List paths, a folder tree (Enter folds) or file names first"),
                ),
                ("/", tr!("Filter the list by path")),
                ("R", tr!("Use a recent commit's message")),
                ("W", tr!("Check out only some folders (sparse checkout)")),
                (
                    "i",
                    tr!("Add an untracked file, or its extension, to .gitignore"),
                ),
                (
                    "I",
                    tr!("Insert a Conventional Commits type, Co-authored-by or Signed-off-by"),
                ),
                (
                    "Alt-↑ Alt-↓",
                    tr!(
                        "In the message field: recall recent messages, then back to what was written"
                    ),
                ),
                ("Ctrl-s", tr!("Commit")),
                ("Ctrl-p", tr!("Commit, then push")),
            ],
            (View::Changes, Focus::Side) => vec![
                ("j k", tr!("Move by line")),
                ("[ ]", tr!("Previous and next hunk")),
                (
                    "Space",
                    tr!("Stage or unstage the line, or the hunk on its header"),
                ),
                ("d", tr!("Discard the line or the hunk")),
                (
                    "v",
                    tr!("Start a range of lines; Space and d then act on all of it"),
                ),
                (
                    "o t b a",
                    tr!("In a conflict: take current, incoming, both, or the base"),
                ),
                ("m", tr!("Magic Merge on the whole file")),
                (
                    "s",
                    tr!("For a file shown one side at a time: the staged or the unstaged side"),
                ),
                (
                    "w",
                    tr!("Ignore whitespace: at line ends, its amount, all, none"),
                ),
                ("h l", tr!("Pan long lines; h at the left edge goes back")),
                ("Esc", tr!("Go back to the list")),
            ],
        };
        lines.extend([
            ("", String::new()),
            ("f p P", tr!("Fetch, pull, push")),
            (
                "b T Ctrl-b",
                tr!("New branch, new tag at the selected commit or ref; switch branch"),
            ),
            (
                "s S",
                tr!("Stash (asks for a message), and pop the newest stash"),
            ),
            (
                "C K A R",
                tr!("Continue, skip, abort a merge or rebase; retry a stopped git am 3-way"),
            ),
            (
                "u !",
                tr!("Undo the last operation; the hooks' output of the last commit"),
            ),
            (
                "R",
                tr!("In History or Reflog: reset the branch to the selected commit"),
            ),
            (
                "M",
                tr!("Mouse on or off (off lets the terminal select text)"),
            ),
            (":", tr!("Commands by name")),
            ("O", tr!("File History of any file, by its path")),
            ("Ctrl-l", tr!("Redraw the screen")),
            (
                "Ctrl-r",
                tr!("Refresh: read the working copy, refs and History again"),
            ),
            #[cfg(unix)]
            ("Ctrl-z", tr!("Suspend (fg in the shell comes back)")),
            ("1 2 3", tr!("Changes, History and Reflog")),
            ("0", tr!("Sidebar")),
            ("Tab", tr!("Next pane")),
            ("q", tr!("Quit")),
        ]);
        lines
    }

    /// サイドバーで選んでいる行を開く: メインの画面を替えるか、ref のコミットを History で選ぶ。
    fn pick(&mut self) {
        match self.sidebar.picked() {
            Some(Picked::Changes) => self.show(View::Changes),
            Some(Picked::History) => self.show(View::History),
            Some(Picked::Reflog) => self.show(View::Reflog),
            // 開いている Range Diff に戻る。
            Some(Picked::RangeDiff) => {
                if self.view != View::File {
                    self.file_back = self.view;
                }
                self.show(View::File);
            }
            // stash は History の行ではないので、その差分を詳細の枠に出す。
            Some(Picked::Commit(id))
                if self.sidebar.picked_ref().is_some_and(|r| r.0 == "stash") =>
            {
                self.view = View::History;
                self.focus = Focus::Side;
                self.pinned = Oid::from_hex(&id);
                // アプリの Stashes の行の 2 行目と同じこと（作ったブランチ・土台と HEAD・untracked）を、一番下の行に。
                let name = self.sidebar.picked_ref().map_or("", |r| r.1).to_owned();
                self.stash_about = Some(name);
                self.send(Request::Run {
                    name: "stashBases",
                    command: json!({"op": "stashBases"}),
                });
            }
            // submodule は、TUI をそこで開き直す（アプリの Open in New Window の代わり）。取り出していなければ、
            // そう言う。
            Some(Picked::Commit(_))
                if self
                    .sidebar
                    .picked_ref()
                    .is_some_and(|r| r.0 == "submodule") =>
            {
                let path = self.sidebar.picked_ref().map_or("", |r| r.1).to_owned();
                let full = self
                    .workdir
                    .as_deref()
                    .map(|w| format!("{}/{path}", w.trim_end_matches(['/', '\\'])));
                match full {
                    Some(full)
                        if std::path::Path::new(&full).join(".git").exists()
                            || !self.files_here =>
                    {
                        self.open_worktree = Some(full);
                        self.quit = true;
                    }
                    _ => {
                        self.notice = Some(tr!(
                            "{path} isn’t checked out: Update Submodules in the command list first",
                            path = path
                        ))
                    }
                }
            }
            // ほかの worktree は、TUI をそこで開き直す（アプリが新しい窓で開くのの代わり）。開いているものは、
            // その場所を知らせるだけ。
            Some(Picked::Commit(_))
                if self.sidebar.picked_ref().is_some_and(|r| r.0 == "worktree") =>
            {
                let path = self.sidebar.picked_ref().map_or("", |r| r.1).to_owned();
                if self.sidebar.picked_is_current() {
                    self.notice = Some(tr!(
                        "This is the worktree that is open, at {path}",
                        path = path
                    ));
                } else {
                    self.open_worktree = Some(path);
                    self.quit = true;
                }
            }
            Some(Picked::Commit(id)) => {
                self.pinned = None;
                self.view = View::History;
                self.focus = Focus::Main;
                self.send(Request::RowOf {
                    history: self.history.id().clone(),
                    id,
                });
            }
            None => {}
        }
        self.dirty = true;
    }

    /// メッセージの欄でのキー: Esc と Tab で一覧へ、`Ctrl-s`（か Alt-Enter）でコミット、ほかは入力。
    fn message_key(&mut self, key: KeyEvent) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        self.dirty = true;
        match key.code {
            KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => self.focus = Focus::Main,
            KeyCode::Char('s') if control => self.commit(),
            KeyCode::Char('p') if control => {
                self.push_after_commit = true;
                self.commit();
                self.push_after_commit &= self.committing;
            }
            KeyCode::Char('e') if control => self.edit_message = true,
            KeyCode::Enter if alt => self.commit(),
            // 最近のメッセージを順に呼び出す（Alt-↓ で戻ると、呼び出す前に書いていたもの）。
            KeyCode::Up | KeyCode::Down if alt => self.recall(key.code == KeyCode::Up),
            KeyCode::Char(_) if control || alt => {}
            code => {
                self.changes.message.key(code);
            }
        }
    }

    /// 最近のメッセージを欄に呼び出す（`older` なら 1 つ古いもの）。まだ読んでいなければ読み、読んだら呼び出す。
    fn recall(&mut self, older: bool) {
        let Some(Recent { messages, .. }) = &self.recent_messages else {
            self.recall_when_read = older;
            self.send(Request::RecentMessages);
            return;
        };
        let count = messages.len();
        let next = match (&self.recalled, older) {
            (None, true) if count > 0 => Some(0),
            (None, _) => return,
            (Some((i, _)), true) => Some((i + 1).min(count.saturating_sub(1))),
            (Some((0, _)), false) => None,
            (Some((i, _)), false) => Some(i - 1),
        };
        match next {
            Some(i) => {
                let before = match self.recalled.take() {
                    Some((_, before)) => before,
                    None => self.changes.message.text(),
                };
                let text = messages[i].clone();
                self.changes.message.set_text(&text);
                self.recalled = Some((i, before));
            }
            None => {
                if let Some((_, before)) = self.recalled.take() {
                    self.changes.message.set_text(&before);
                }
            }
        }
    }

    /// 欄に入れる（`I` の一覧で選んだもの）: `type:` は件名の頭を置き換え（もう種類があれば替える）、トレーラーは
    /// 本文の終わりに足す（もうあれば足さない。前の行がトレーラーでなければ空行で分ける）。アプリの Insert ▾ と同じ。
    fn insert_into_message(&mut self, item: &str) {
        let text = self.changes.message.text();
        let (summary, body) = text.split_once('\n').unwrap_or((text.as_str(), ""));
        let next = match item.strip_suffix(':') {
            Some(kind) if !kind.contains(' ') => {
                let rest = strip_commit_type(summary);
                match body {
                    "" => format!("{kind}: {rest}"),
                    body => format!("{kind}: {rest}\n{body}"),
                }
            }
            _ if text.lines().any(|l| l == item) => text.clone(),
            _ => {
                let trimmed = text.trim_end();
                let last = trimmed.lines().last().unwrap_or("");
                let after_trailer = trimmed.contains('\n') && is_trailer(last);
                match trimmed {
                    "" => format!("\n\n{item}"),
                    _ if after_trailer => format!("{trimmed}\n{item}"),
                    _ => format!("{trimmed}\n\n{item}"),
                }
            }
        };
        self.changes.message.set_text(&next);
        // 種類を入れたら件名の終わりへ、トレーラーなら終わりへ（set_text が終わりに置く）。
        if item.ends_with(':') && !item.contains(' ') {
            self.changes.message.move_to_summary_end();
        }
        self.recalled = None;
        self.focus = Focus::Message;
    }

    /// 欄をそのメッセージにして、欄に移る（最近のメッセージの一覧で選んだもの）。
    fn use_message(&mut self, message: &str) {
        self.changes.message.set_text(message);
        self.recalled = None;
        self.focus = Focus::Message;
    }

    /// 欄を `fixup! 件名`（件名だけ）か `squash! 件名`（書いていた本文は残す）にして、欄に移る。amend はやめる。
    fn use_fixup(&mut self, subject: &str, squash: bool) {
        let text = match squash {
            true => {
                let old = self.changes.message.text();
                let body = old.split_once('\n').map_or("", |(_, b)| b);
                format!("squash! {subject}\n{body}")
            }
            false => format!("fixup! {subject}"),
        };
        self.changes.message.set_text(text.trim_end_matches('\n'));
        self.changes.amend = false;
        self.focus = Focus::Message;
    }

    /// 欄のメッセージでコミットを作る。できないときは、理由を出す。
    fn commit(&mut self) {
        if self.committing {
            return;
        }
        if self.changes.message.is_empty() {
            self.error = Some(tr!("Write a commit message first"));
        } else if !self.changes.amend && self.changes.staged_count() == 0 {
            self.error = Some(tr!("Nothing is staged (Space stages a file)"));
        } else {
            self.committing = true;
            self.send(Request::MakeCommit {
                message: self.changes.message.text(),
                amend: self.changes.amend,
            });
        }
    }

    fn changes_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.changes.page();
        match code {
            // 取り出すフォルダを選ぶ（アプリの Check Out Only Some Folders…）。
            KeyCode::Char('W') => {
                self.type_sparse_folders();
                true
            }
            KeyCode::Char('d') if control => self.changes.move_selection(page / 2),
            KeyCode::Char('u') if control => self.changes.move_selection(-page / 2),
            KeyCode::Char('j') | KeyCode::Down => self.changes.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.changes.move_selection(-1),
            KeyCode::PageDown => self.changes.move_selection(page),
            KeyCode::PageUp => self.changes.move_selection(-page),
            KeyCode::Char('g') | KeyCode::Home => self.changes.select(0),
            KeyCode::Char('G') | KeyCode::End => self.changes.select(usize::MAX),
            KeyCode::Char(' ') => {
                if let Some(request) = self.changes.toggle() {
                    self.send(request);
                }
                false
            }
            // Stage All（コンフリクトのファイルは除く）。
            KeyCode::Char('+') => {
                match self.changes.stage_all() {
                    Some(request) => self.send(request),
                    None => self.notice = Some(tr!("Nothing to stage")),
                }
                true
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.focus = Focus::Sidebar;
                true
            }
            // 選んでいるファイルの Blame・File History を開く。
            // 作業ツリーのファイルを、git のエディタで開く（端末を渡す）。
            KeyCode::Char('e') => {
                if let Some(path) = self.changes.selected_path().map(str::to_owned) {
                    self.edit_working_file(&path);
                }
                true
            }
            KeyCode::Char('B' | 'L') => match self.changes.selected_path() {
                Some(path) => {
                    self.file.open(path, code == KeyCode::Char('B'), None);
                    self.sidebar.set_range_diff(false);
                    self.file_back = View::Changes;
                    self.view = View::File;
                    true
                }
                None => false,
            },
            // コンフリクトのあるファイルを、片側にして解決済みにする（先に確かめる）。
            // コンフリクトのファイルで `m`: Magic Merge（決まった規則で解けるものを解く）。
            KeyCode::Char('m') if self.changes.conflicted_path().is_some() => {
                self.magic_merge();
                true
            }
            KeyCode::Char('o' | 't') if self.changes.conflicted_path().is_some() => {
                let ours = code == KeyCode::Char('o');
                let path = self.changes.conflicted_path().unwrap_or("").to_owned();
                let ask = match ours {
                    true => tr!("Take the current side of {path}?", path = path),
                    false => tr!("Take the incoming side of {path}?", path = path),
                };
                self.confirm = Some((
                    ask,
                    tr!("take"),
                    Request::Run {
                        name: "resolve",
                        command: json!({"op": "takeSide", "path": path, "ours": ours}),
                    },
                ));
                true
            }
            KeyCode::Char('c') if self.changes.has_message_field() => {
                self.focus = Focus::Message;
                true
            }
            // untracked のファイルを .gitignore に足す: そのパスか、拡張子のパターン（アプリの右クリックと同じ）。
            KeyCode::Char('i') => {
                let Some(path) = self.changes.untracked_path().map(str::to_owned) else {
                    self.error = Some(tr!("Only an untracked file can be added to .gitignore"));
                    return true;
                };
                let add = |pattern: &str| Request::AddToGitignore {
                    pattern: pattern.to_owned(),
                };
                let mut choices = vec![('p', path.clone(), add(&path))];
                let name = path.rsplit('/').next().unwrap_or(&path);
                if let Some((stem, ext)) = name.rsplit_once('.')
                    && !stem.is_empty()
                    && !ext.is_empty()
                {
                    let pattern = format!("*.{ext}");
                    choices.push(('e', pattern.clone(), add(&pattern)));
                }
                self.choices = Some((tr!("Add to .gitignore:"), choices));
                true
            }
            // Fixup・Squash Commit For: 最近のコミットから選んで、欄を fixup! にする。
            KeyCode::Char('F' | 'R' | 'I') if self.changes.has_message_field() => {
                let kind = match code {
                    KeyCode::Char('F') => PickKind::Fixup,
                    KeyCode::Char('R') => PickKind::Recent,
                    _ => PickKind::Insert,
                };
                self.fixup_pick = Some(MessagePick::new(kind, self.recent_messages.as_ref()));
                if self.recent_messages.is_none() {
                    self.send(Request::RecentMessages);
                }
                true
            }
            // 直前のコミットを作り直す。欄が空なら、そのメッセージを入れる。
            KeyCode::Char('a') => {
                self.changes.amend = !self.changes.amend;
                if self.changes.amend
                    && self.changes.message.is_empty()
                    && let Some(id) = self.head.clone()
                {
                    self.send(Request::Message { id });
                }
                true
            }
            KeyCode::Char('s') if control => {
                self.commit();
                true
            }
            KeyCode::Char('p') if control => {
                self.push_after_commit = true;
                self.commit();
                // コミットできなかったら、push もしない。
                self.push_after_commit &= self.committing;
                true
            }
            KeyCode::Char('d') => {
                self.confirm = self.changes.discard_file().map(|(path, request)| {
                    let ask = tr!(
                        "Discard the changes in {path}? Undo brings them back.",
                        path = path
                    );
                    (ask, tr!("discard"), request)
                });
                self.confirm.is_some()
            }
            // 木のフォルダでは、Enter で畳む・開く。ファイルなら差分へ。
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                if !self.changes.toggle_folder() {
                    self.focus = Focus::Side;
                }
                true
            }
            // 一覧の形: パス → 木 → ファイル名（アプリの List・Tree・File Name）。この TUI の間だけ。
            KeyCode::Char('v') => {
                self.changes.next_mode();
                self.notice = Some(match self.changes.mode() {
                    crate::changes::ListMode::Path => tr!("Listing paths"),
                    crate::changes::ListMode::Tree => {
                        tr!("Listing as a tree (Enter folds a folder)")
                    }
                    crate::changes::ListMode::Name => tr!("Listing file names first"),
                });
                true
            }
            // パスで絞り込む（打つたびに効く）。
            KeyCode::Char('/') => {
                self.typing_filter = Some(self.changes.filter().to_owned());
                self.filtering_changes = true;
                true
            }
            KeyCode::Esc if !self.changes.filter().is_empty() => {
                self.changes.set_filter("");
                true
            }
            _ => false,
        }
    }

    fn changes_diff_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.changes.diff_page();
        // 選んでいる行を動かすだけなら、差分の中だけが変わる。
        let moved = match code {
            KeyCode::Char('d') if control => Some(self.changes.move_cursor(page / 2)),
            KeyCode::Char('u') if control => Some(self.changes.move_cursor(-page / 2)),
            KeyCode::Char('j') | KeyCode::Down => Some(self.changes.move_cursor(1)),
            KeyCode::Char('k') | KeyCode::Up => Some(self.changes.move_cursor(-1)),
            KeyCode::PageDown => Some(self.changes.move_cursor(page)),
            KeyCode::PageUp => Some(self.changes.move_cursor(-page)),
            KeyCode::Char('g') | KeyCode::Home => Some(self.changes.cursor_to(0)),
            KeyCode::Char('G') | KeyCode::End => Some(self.changes.cursor_to(u32::MAX)),
            KeyCode::Char(']') => Some(self.changes.jump_hunk(true)),
            KeyCode::Char('[') => Some(self.changes.jump_hunk(false)),
            _ => None,
        };
        if let Some(moved) = moved {
            if moved {
                self.damage |= Damage::SIDE;
            }
            return false;
        }
        match code {
            // 横に動かすのも、差分の中だけが変わる。
            KeyCode::Char('l') | KeyCode::Right => {
                if self.changes.pan(PAN) {
                    self.damage |= Damage::SIDE;
                }
                false
            }
            // 左の端にいるなら、動かさずに一覧へ戻る。
            KeyCode::Char('h') | KeyCode::Left => {
                if self.changes.pan(-PAN) {
                    self.damage |= Damage::SIDE;
                    false
                } else {
                    self.focus = Focus::Main;
                    true
                }
            }
            // 差分で無視する空白を替える（アプリの Ignore Whitespace）。その間は、片方の側だけを出し、行ごとの
            // stage・破棄はしない。
            KeyCode::Char('w') => {
                self.notice = Some(match self.changes.next_whitespace() {
                    Whitespace::Exact => tr!("Whitespace is compared"),
                    Whitespace::AtEol => {
                        tr!("Ignoring whitespace at line ends (lines aren’t staged meanwhile)")
                    }
                    Whitespace::Change => tr!(
                        "Ignoring changes in the amount of whitespace (lines aren’t staged meanwhile)"
                    ),
                    Whitespace::All => {
                        tr!("Ignoring all whitespace (lines aren’t staged meanwhile)")
                    }
                });
                true
            }
            // 重ねて出せないファイルで、作業ツリーの側と stage 済みの側を替える。
            KeyCode::Char('s') => {
                self.notice = Some(match self.changes.switch_side() {
                    Ok(true) => tr!("Showing the staged changes (s for the unstaged)"),
                    Ok(false) => tr!("Showing the unstaged changes (s for the staged)"),
                    Err(true) => {
                        tr!("This diff already shows both the staged and the unstaged changes")
                    }
                    Err(false) => tr!("This file has changes on one side only"),
                });
                true
            }
            // 行の範囲: `v` で始まりを決め、動かした所までを選ぶ。もう一度で、やめる。
            KeyCode::Char('v') => self.changes.toggle_mark(),
            KeyCode::Esc if self.changes.has_mark() => self.changes.toggle_mark(),
            // 共通の祖先の側は、コンフリクトに入っているとき（diff3 の形）だけ選べる。
            KeyCode::Char('a')
                if self.changes.conflict_at_cursor().is_some()
                    && !self.changes.conflict_has_base() =>
            {
                self.error = Some(tr!(
                    "This conflict has no base side (git writes it with merge.conflictStyle diff3 or zdiff3)"
                ));
                true
            }
            // コンフリクトの中では、そのコンフリクトを Current・Incoming・両方・共通の祖先のどれかにする（Undo
            // できる）。
            // コンフリクトのファイルの差分でも `m` で Magic Merge（ファイルのすべてのコンフリクトに）。
            KeyCode::Char('m') if self.changes.conflicted_path().is_some() => {
                self.magic_merge();
                true
            }
            KeyCode::Char('o' | 't' | 'b' | 'a') if self.changes.conflict_at_cursor().is_some() => {
                let choice = match code {
                    KeyCode::Char('o') => "ours",
                    KeyCode::Char('t') => "theirs",
                    KeyCode::Char('a') => "base",
                    _ => "both",
                };
                if let Some((path, index)) = self.changes.conflict_at_cursor() {
                    let command = json!({
                        "op": "resolveConflict", "path": path, "index": index, "choice": choice,
                    });
                    self.run("resolve", tr!("Resolving…"), command);
                }
                true
            }
            // 行（hunk の見出しなら、その hunk）の stage を切り替える。
            KeyCode::Char(' ') => match self.changes.toggle_rows() {
                Ok(Some(request)) => {
                    self.send(request);
                    false
                }
                Ok(None) => false,
                Err(error) => {
                    self.error = Some(error);
                    true
                }
            },
            KeyCode::Char('d') => match self.changes.discard_rows() {
                Ok(Some((target, request))) => {
                    let ask = match (target.hunk, target.rows.len()) {
                        (false, _) => tr!("Discard this line? Undo brings it back."),
                        (true, 1) => tr!("Discard this hunk (1 line)? Undo brings it back."),
                        (true, count) => tr!(
                            "Discard this hunk ({count} lines)? Undo brings it back.",
                            count = count
                        ),
                    };
                    self.confirm = Some((ask, tr!("discard"), request));
                    true
                }
                Ok(None) => false,
                Err(error) => {
                    self.error = Some(error);
                    true
                }
            },
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                self.focus = Focus::Main;
                true
            }
            _ => false,
        }
    }

    fn history_key(&mut self, code: KeyCode, control: bool, shift: bool) -> bool {
        // History を操作したら、開いていた stash ではなく選んでいるコミットを出す。
        self.pinned = None;
        let page = self.history.page();
        // 複数選択: `m` で印をつける・外す（アプリの ⌘-クリック）、Shift-↑↓ で続けてつける（⇧-クリック）。
        let marked = match code {
            KeyCode::Char('m') => Some(self.history.toggle_mark()),
            KeyCode::Down if shift => Some(self.history.mark_and_move(1)),
            KeyCode::Up if shift => Some(self.history.mark_and_move(-1)),
            // 2 つのときの差分の取り方: 向きを入れ替える、分かれたところから。
            KeyCode::Char('x') if self.history.mark_count() == 2 => {
                self.pair = match self.pair {
                    Pair::Forward => Pair::Reverse,
                    _ => Pair::Forward,
                };
                Some(true)
            }
            // 2 つを、それぞれを先端とする 2 つの版として比べる（rebase の前と後など。アプリの Compare as Two
            // Versions）。
            KeyCode::Char('=') if self.history.mark_count() == 2 => {
                let marks: Vec<String> = self
                    .history
                    .marks()
                    .map(|(_, m)| m.id.to_string())
                    .collect();
                let [newer, older] = &marks[..] else {
                    return false;
                };
                let title = tr!(
                    "{older} and {newer} compared as two versions",
                    older = &older[..8],
                    newer = &newer[..8]
                );
                self.compare_ranges(title, vec![format!("{older}...{newer}")]);
                return true;
            }
            // shallow clone の履歴を取り足す（アプリの最後の行の下の帯の Fetch More・Fetch All）。
            // shallow clone の履歴・partial clone の中身を取り足す（アプリの最後の行の下の帯、リポジトリの設定の
            // 取得とフォルダ）。
            KeyCode::Char('+') if self.history.is_shallow() || self.form["partial"].is_object() => {
                self.ask_fetch_more();
                return true;
            }
            KeyCode::Char('.') if self.history.mark_count() == 2 => {
                self.pair = match self.pair {
                    Pair::MergeBase => Pair::Forward,
                    _ => Pair::MergeBase,
                };
                Some(true)
            }
            KeyCode::Esc if self.history.clear_marks() => Some(true),
            _ => None,
        };
        if let Some(marked) = marked {
            if self.history.mark_count() < 2 {
                self.pair = Pair::Forward;
            }
            if marked {
                self.notice = match self.history.mark_count() {
                    0 => None,
                    n => Some(tr!(
                        "{count} selected: m adds or removes one, Esc clears",
                        count = n
                    )),
                };
            }
            return marked;
        }
        // 選択を動かすだけなら、History の中だけが変わる（詳細は、頼むときに印をつける）。
        let moved = match code {
            KeyCode::Char('d') if control => Some(self.history.move_selection(page / 2)),
            KeyCode::Char('u') if control => Some(self.history.move_selection(-page / 2)),
            KeyCode::Char('j') | KeyCode::Down => Some(self.history.move_selection(1)),
            KeyCode::Char('k') | KeyCode::Up => Some(self.history.move_selection(-1)),
            KeyCode::PageDown => Some(self.history.move_selection(page)),
            KeyCode::PageUp => Some(self.history.move_selection(-page)),
            KeyCode::Char('g') | KeyCode::Home => Some(self.history.select(0)),
            KeyCode::Char('G') | KeyCode::End => Some(self.history.select(u32::MAX)),
            // HEAD・親・子へ（アプリの Navigate）。`]` は下（古い方）、`[` は上へ動く。
            KeyCode::Char('@') => Some(self.history.go_to_head()),
            KeyCode::Char('o') => {
                self.toggle_history_order();
                None
            }
            // マージ済みのブランチを折りたたむ・戻す。
            KeyCode::Char('z') => {
                self.fold = !self.fold;
                self.expanded.clear();
                self.save_fold();
                self.notice = Some(match self.fold {
                    true => tr!(
                        "Merged branches folded: a merge shows how many commits it brought in; Z opens one"
                    ),
                    false => tr!("Merged branches shown"),
                });
                self.dirty = true;
                self.open();
                None
            }
            // 折りたたんだマージの、取り込んだ側を開く・閉じる。
            // たたんでいれば選んだ merge を開き、たたんでいなければ選んだ merge だけをたたむ（もう一度で戻す）。
            KeyCode::Char('Z') => {
                if let Some(id) = self.history.selected_id() {
                    let id = id.to_string();
                    match self.expanded.iter().position(|e| *e == id) {
                        Some(i) => {
                            self.expanded.remove(i);
                        }
                        None if self.fold && self.history.selected_folded() > 0 => {
                            self.expanded.push(id)
                        }
                        None if !self.fold && self.history.selected_is_merge() => {
                            self.expanded.push(id)
                        }
                        None if self.fold => {
                            self.notice = Some(tr!("This commit has nothing folded"))
                        }
                        None => self.notice = Some(tr!("Only a merge folds (z folds them all)")),
                    }
                    self.save_fold();
                    self.dirty = true;
                    self.open();
                }
                None
            }
            KeyCode::Char(']') => Some(self.history.go_to_parent()),
            KeyCode::Char('[') => Some(self.history.go_to_child()),
            _ => None,
        };
        if let Some(moved) = moved {
            if moved {
                self.damage |= Damage::MAIN;
            }
            return false;
        }
        match code {
            KeyCode::Char('/') => {
                // 前の語から直せるよう、入れておく。
                self.typing_query =
                    Some(self.search.as_ref().map_or(String::new(), |s| s.0.clone()));
                true
            }
            // 合うコミットだけにする・すべてに戻す（アプリの Show Only Matching Commits）。
            KeyCode::Char('&') => {
                match (self.matching.take(), &self.search) {
                    (Some(_), _) => {}
                    (None, Some((query, _))) => self.matching = Some(query.clone()),
                    (None, None) => {
                        self.error = Some(tr!("Search with / first"));
                        return true;
                    }
                }
                self.open();
                true
            }
            // コミットの ID を端末のクリップボードに（印があれば、印をつけたものすべて）。
            KeyCode::Char('Y') => self.copy_ids(),
            // 選んでいるコミットから、新しいブランチの worktree を作る（アプリの New Worktree…）。
            KeyCode::Char('w') => {
                let Some(id) = self.history.selected_id().map(|id| id.to_string()) else {
                    return false;
                };
                let short = id[..8].to_owned();
                self.naming = Naming::WorktreeBranch;
                self.typing_branch = Some((id, short, String::new()));
                true
            }
            // 印をつけたコミット: 古い順に cherry-pick、新しい順に revert、編集モードで 1 つにまとめる。
            KeyCode::Char('y' | 'v') if self.history.mark_count() >= 2 => {
                self.ask_pick_marked(code == KeyCode::Char('v'))
            }
            KeyCode::Char('E') if self.history.mark_count() >= 2 => self.combine_marked(),
            KeyCode::Char('c' | 'e' | 'F' | 'X' | '|') if self.history.mark_count() >= 2 => {
                self.error = Some(tr!(
                    "This works on one commit: Esc clears the selection first"
                ));
                true
            }
            // 選んでいるコミットを今のブランチに持ってくる・打ち消す・そこをチェックアウトする（先に確かめる）。
            KeyCode::Char('y' | 'v' | 'c' | 'e' | 'F' | 'X' | '|') => {
                self.ask_commit_operation(code)
            }
            // 選んでいるコミットから HEAD までを、編集モードで。
            KeyCode::Char('E') => {
                match self.history.selected_id() {
                    Some(id) => self.send(Request::EditRows {
                        from: id.to_string(),
                    }),
                    None => return false,
                }
                true
            }
            KeyCode::Char('n') => self.history.next_match(true),
            KeyCode::Char('N') => self.history.next_match(false),
            KeyCode::Esc => self.clear_search(),
            KeyCode::Char('h') | KeyCode::Left => {
                self.focus = Focus::Sidebar;
                true
            }
            KeyCode::Enter | KeyCode::Char(' ' | 'l') | KeyCode::Right => {
                self.focus = Focus::Side;
                true
            }
            _ => false,
        }
    }

    fn details_key(&mut self, code: KeyCode, control: bool) -> bool {
        let page = self.details.page();
        // 動かすだけなら、詳細の中だけが変わる。
        let scrolled = match code {
            KeyCode::Char('d') if control => Some(self.details.scroll(page / 2)),
            KeyCode::Char('u') if control => Some(self.details.scroll(-page / 2)),
            KeyCode::Char('j') | KeyCode::Down => Some(self.details.scroll(1)),
            KeyCode::Char('k') | KeyCode::Up => Some(self.details.scroll(-1)),
            KeyCode::PageDown | KeyCode::Char(' ') => Some(self.details.scroll(page)),
            KeyCode::PageUp => Some(self.details.scroll(-page)),
            KeyCode::Char('g') | KeyCode::Home => Some(self.details.scroll_to(0)),
            KeyCode::Char('G') | KeyCode::End => Some(self.details.scroll_to(u32::MAX)),
            KeyCode::Char('l') | KeyCode::Right => Some(self.details.pan(PAN)),
            // 左の端にいるなら、動かさずに一覧へ戻る（下で）。
            KeyCode::Char('h') | KeyCode::Left if self.details.panned() => {
                Some(self.details.pan(-PAN))
            }
            KeyCode::Char(']') => Some(self.details.jump_file(true)),
            KeyCode::Char('[') => Some(self.details.jump_file(false)),
            // 変わったファイルを木で出す・一覧に戻す（アプリの List / Tree）、木のフォルダをたたむ・開く。
            KeyCode::Char('t') => Some(self.details.toggle_tree()),
            KeyCode::Char('z') => Some(self.details.fold_all()),
            // 今見ているファイルの差分を畳む・開く（アプリのファイルの見出し）。
            KeyCode::Char('o') => Some(self.details.toggle_fold_in_view()),
            _ => None,
        };
        // 今見ているファイルの前後を広げる・ファイル全体を出す（アプリの hunk の見出しの印・Show Whole File）。
        // 読み直しは `request` が頼む。
        if code == KeyCode::Char('+') {
            return self.details.widen();
        }
        if code == KeyCode::Char('F') {
            return match self.details.toggle_whole_file() {
                Some(whole) => {
                    self.notice = Some(match whole {
                        true => tr!("Showing the whole file"),
                        false => tr!("Showing the changes only"),
                    });
                    true
                }
                None => false,
            };
        }
        // 差分で無視する空白を替える（アプリの Ignore Whitespace）。読み直しは `request` が頼む。
        if code == KeyCode::Char('w') {
            self.notice = Some(match self.details.next_whitespace() {
                Whitespace::Exact => tr!("Whitespace is compared"),
                Whitespace::AtEol => tr!("Ignoring whitespace at line ends"),
                Whitespace::Change => tr!("Ignoring changes in the amount of whitespace"),
                Whitespace::All => tr!("Ignoring all whitespace"),
            });
            return true;
        }
        if let Some(scrolled) = scrolled {
            if scrolled {
                self.damage |= Damage::SIDE;
            }
            return false;
        }
        match code {
            // 今見ているファイルの Blame（そのコミットの時点の）・File History を開く。
            KeyCode::Char('B' | 'L') if self.view != View::File => {
                match self.details.current_file() {
                    Some((path, id)) => {
                        let blame = code == KeyCode::Char('B');
                        let path = path.to_owned();
                        self.file.open(&path, blame, blame.then(|| id.to_string()));
                        self.sidebar.set_range_diff(false);
                        self.file_back = self.view;
                        self.view = View::File;
                        self.focus = Focus::Main;
                        true
                    }
                    None => false,
                }
            }
            // 今見ているファイルのパスをコピーする・作業ツリーのそのファイルをエディタで開く（アプリのファイルの
            // 見出しのメニューの Copy Path・Open）。
            // 詳細の中を探す。
            KeyCode::Char('/') => {
                self.typing_query = Some(self.details.finding().unwrap_or("").to_owned());
                self.typing_in_details = true;
                true
            }
            KeyCode::Char('n' | 'N') if self.details.finding().is_some() => {
                let query = self.details.finding().unwrap_or("").to_owned();
                self.find_in_details(&query, code == KeyCode::Char('n'));
                true
            }
            KeyCode::Char('Y') => match self.details.current_file() {
                Some((path, _)) => {
                    let path = path.to_owned();
                    self.notice = Some(tr!(
                        "Copied {path} (to the terminal’s clipboard)",
                        path = path
                    ));
                    self.copy = Some(path);
                    true
                }
                None => false,
            },
            KeyCode::Char('e') => match self.details.current_file() {
                Some((path, _)) => {
                    let path = path.to_owned();
                    self.edit_working_file(&path);
                    true
                }
                None => false,
            },
            KeyCode::Esc | KeyCode::Char('q' | 'h') | KeyCode::Backspace | KeyCode::Left => {
                self.focus = Focus::Main;
                true
            }
            _ => false,
        }
    }

    fn mouse(&mut self, mouse: MouseEvent) {
        // 確かめている間は、キーで答えるまで動かさない。
        if self.confirm.is_some() || self.choices.is_some() || !self.mouse {
            return;
        }
        let layout = self.layout();
        let inside = |area: Option<Rect>| {
            area.filter(|a| {
                a.x <= mouse.column
                    && mouse.column < a.right()
                    && a.y <= mouse.row
                    && mouse.row < a.bottom()
            })
        };
        let wheel = match mouse.kind {
            MouseEventKind::ScrollDown => Some(WHEEL_ROWS),
            MouseEventKind::ScrollUp => Some(-WHEEL_ROWS),
            _ => None,
        };
        let click = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left));
        let view = self.view;
        let pane = if inside(layout.sidebar).is_some() {
            Damage::SIDEBAR
        } else if inside(layout.main).is_some() {
            Damage::MAIN
        } else {
            Damage::SIDE
        };
        let changed = if let Some(area) = inside(layout.sidebar) {
            match wheel {
                Some(rows) => self.sidebar.scroll(rows),
                None if click => {
                    self.focus = Focus::Sidebar;
                    if self.sidebar.click(mouse.row - area.y) {
                        self.pick();
                    }
                    true
                }
                None => false,
            }
        } else if let Some(area) = inside(layout.main) {
            match wheel {
                Some(rows) => match view {
                    View::History => self.history.scroll(rows),
                    View::Reflog => self.reflog.scroll(rows),
                    View::File => self.file.scroll(rows),
                    View::Changes => self.changes.scroll_list(rows),
                },
                None if click => {
                    let focused = std::mem::replace(&mut self.focus, Focus::Main);
                    let line = mouse.row - area.y;
                    let selected = match view {
                        // Ctrl・Alt を押しながらなら、印をつける・外す（アプリの ⌘-クリック。Shift-クリックは
                        // 端末が文字の選択に使う）。
                        View::History
                            if mouse
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                        {
                            self.history.click(line);
                            self.history.toggle_mark()
                        }
                        View::History => self.history.click(line),
                        View::Reflog => self.reflog.click(line),
                        View::File => self.file.click(line),
                        // 一覧の下は、メッセージの欄。
                        View::Changes if line >= self.changes.list_height() => {
                            self.focus = Focus::Message;
                            false
                        }
                        View::Changes => self.changes.click(line),
                    };
                    selected || focused != self.focus
                }
                None => false,
            }
        } else if let Some(area) = inside(layout.side) {
            match wheel {
                Some(rows) if view == View::Changes => self.changes.scroll(rows),
                Some(rows) => self.details.scroll(rows),
                None if click => {
                    let focused = std::mem::replace(&mut self.focus, Focus::Side);
                    let selected = if view == View::Changes {
                        self.changes.click_diff(mouse.row - area.y)
                    } else {
                        self.details.click(mouse.row - area.y)
                    };
                    selected || focused != Focus::Side
                }
                None => false,
            }
        } else {
            false
        };
        // ホイールで動くのは、そのペインの中だけ。
        match wheel {
            Some(_) if changed => self.damage |= pane,
            _ => self.dirty |= changed,
        }
    }

    /// `damage` の所を描き（`areas` が返す範囲。呼ぶ側が消してある）、カーソルを置く位置を返す。
    pub fn draw(&mut self, buf: &mut Buffer, damage: Damage) -> Option<(u16, u16)> {
        let area = buf.area;
        if area.height < BARS + 1 {
            return None;
        }
        let all = damage == Damage::ALL;
        let mut cursor = None;
        let bar = Style::new().add_modifier(Modifier::REVERSED);

        // 一番上: リポジトリ、ブランチ、読んでいる間の印。
        let top = Rect::new(area.x, area.y, area.width, 1);
        buf.set_style(top, bar);
        let x = buf
            .set_stringn(
                area.x + 1,
                area.y,
                &self.name,
                usize::from(area.width.saturating_sub(2)),
                bar.add_modifier(Modifier::BOLD),
            )
            .0;
        let mut x = x;
        if let Some(branch) = &self.branch {
            x = buf
                .set_stringn(
                    x + 2,
                    area.y,
                    branch,
                    usize::from(area.right().saturating_sub(x + 3)),
                    bar,
                )
                .0;
            // 上流より進んでいる数と、遅れている数。
            let mut counts = String::new();
            if self.ahead > 0 {
                counts.push_str(&format!(" ↑{}", self.ahead));
            }
            if self.behind > 0 {
                counts.push_str(&format!(" ↓{}", self.behind));
            }
            if !counts.is_empty() && x + 1 < area.right() {
                x = buf
                    .set_stringn(x, area.y, counts, usize::from(area.right() - x - 1), bar)
                    .0;
            }
        }
        // shallow clone・partial clone の印。
        for mark in &self.form_marks {
            if x + 3 >= area.right() {
                break;
            }
            x = buf
                .set_stringn(
                    x + 2,
                    area.y,
                    format!("[{mark}]"),
                    usize::from(area.right() - x - 3),
                    bar.fg(Color::Yellow),
                )
                .0;
        }
        // サイドバーが出ていなければ、メインの画面の切り替え（数字のキー）を並べる。今の画面は太字。
        let tabs = self.layout().sidebar.is_none();
        let changes = match self.changes.count() {
            0 => tr!("1 Changes"),
            count => tr!("1 Changes ({count})", count = count),
        };
        for (view, label) in [
            (View::Changes, changes),
            (View::History, tr!("2 History")),
            (View::Reflog, tr!("3 Reflog")),
        ] {
            if !tabs || x + 3 >= area.right() {
                break;
            }
            let style = match self.view == view {
                true => bar.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                false => bar,
            };
            x = buf
                .set_stringn(
                    x + 3,
                    area.y,
                    label,
                    usize::from(area.right().saturating_sub(x + 4)),
                    style,
                )
                .0;
        }
        if self.busy() {
            // 動いている操作があれば、その名前。
            let loading = self.running.clone().unwrap_or_else(|| tr!("Loading…"));
            let width = loading.width() as u16 + 1;
            if width < area.width {
                buf.set_string(area.right() - width, area.y, loading, bar);
            }
        }

        let layout = self.layout();
        if let Some(at) = layout.sidebar.filter(|_| damage.has(Damage::SIDEBAR)) {
            self.sidebar
                .draw(at, buf, self.focus == Focus::Sidebar, self.changes.count());
            // メインの画面と並べたときは、間に区切りの線。
            if all && at.right() < area.right() {
                let line = Style::new().fg(Color::Indexed(238));
                for y in at.y..at.bottom() {
                    buf[(at.right(), y)].set_char('│').set_style(line);
                }
            }
        }
        let focused = self.focus == Focus::Main;
        if layout.main.is_none() {
            self.main_cursor = None;
        }
        if let Some(at) = layout.main.filter(|_| damage.has(Damage::MAIN)) {
            self.main_cursor = None;
            match self.view {
                View::History => self.history.draw(at, buf, self.clock.now, focused),
                View::Reflog => self.reflog.draw(at, buf, self.clock.now, focused),
                View::File => self.file.draw(at, buf, self.clock.now, focused),
                View::Changes => {
                    self.main_cursor =
                        self.changes
                            .draw_main(at, buf, focused, self.focus == Focus::Message);
                }
            }
        }
        cursor = cursor.or(self.main_cursor);
        if let Some(at) = layout.side.filter(|_| damage.has(Damage::SIDE)) {
            // 一覧と並べたときは、間に区切りの線。
            if all && layout.main.is_some() {
                let line = Style::new().fg(Color::Indexed(if focused { 238 } else { 244 }));
                for y in at.y..at.bottom() {
                    buf[(at.x - 1, y)].set_char('│').set_style(line);
                }
            }
            match self.view {
                View::History | View::Reflog | View::File => {
                    self.details.draw(at, buf, &self.clock, &self.theme);
                }
                View::Changes => self.changes.draw_diff(at, buf, &self.theme, !focused),
            }
        }

        // 一番下: エラーか、キーの案内。右に、今の位置（一覧は選んでいる行、詳細と差分は見えている最後の行）。
        let y = area.bottom() - 1;
        let (at, of) = match (self.view, self.focus) {
            (View::History, Focus::Main) => (
                (self.history.selected() + 1).min(self.history.total()),
                self.history.total(),
            ),
            (View::History | View::Reflog | View::File, Focus::Side) => self.details.position(),
            (View::File, Focus::Main | Focus::Sidebar) => self.file.position(),
            (View::Reflog, Focus::Main | Focus::Sidebar) => self.reflog.position(),
            (View::Changes, Focus::Main) => self.changes.list_position(),
            (View::Changes, Focus::Side) => self.changes.diff_position(),
            (_, Focus::Message) => self.changes.list_position(),
            (View::History, Focus::Sidebar) => (
                (self.history.selected() + 1).min(self.history.total()),
                self.history.total(),
            ),
            (View::Changes, Focus::Sidebar) => self.changes.list_position(),
        };
        let position = format!("{at}/{of}");
        let position_x = area
            .right()
            .saturating_sub(position.width() as u16 + 1)
            .max(area.x);
        let room = usize::from(position_x.saturating_sub(area.x + 2));
        #[cfg(unix)]
        let asked = self.questions.front();
        match (&self.confirm, &self.error) {
            // いくつかから選ぶ: 文と、選択肢のキーと名前。
            _ if self.choices.is_some() => {
                let (ask, options) = self.choices.as_ref().unwrap();
                let mut x =
                    buf.set_stringn(
                        area.x + 1,
                        y,
                        ask,
                        room,
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )
                    .0 + 3;
                let cancel = ('n', tr!("cancel"));
                for (key, label) in options
                    .iter()
                    .map(|o| (o.0, o.1.as_str()))
                    .chain([(cancel.0, cancel.1.as_str())])
                {
                    if x >= position_x {
                        break;
                    }
                    x =
                        buf.set_stringn(
                            x,
                            y,
                            format!("{key} {label}"),
                            usize::from(position_x - x),
                            Style::new(),
                        )
                        .0 + 3;
                }
            }
            // 尋ねられていること: 文と、打った答え（パスワードは隠す）。
            #[cfg(unix)]
            _ if asked.is_some() => {
                let question = asked.unwrap();
                let shown = match question.secret() {
                    true => "•".repeat(self.answer.chars().count()),
                    false => self.answer.clone(),
                };
                let x = buf
                    .set_stringn(
                        area.x + 1,
                        y,
                        question.prompt.trim(),
                        room,
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )
                    .0;
                let x = buf
                    .set_stringn(
                        x + 1,
                        y,
                        shown,
                        usize::from(position_x.saturating_sub(x + 2)),
                        Style::new(),
                    )
                    .0;
                cursor = Some((x.min(position_x.saturating_sub(1)), y));
            }
            (Some((ask, yes, _)), _) => {
                let x = buf
                    .set_stringn(
                        area.x + 1,
                        y,
                        ask,
                        room,
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )
                    .0;
                buf.set_stringn(
                    x + 3,
                    y,
                    match self.push_to_offered {
                        true => tr!("y {yes}   t another branch…   n cancel", yes = yes),
                        false => tr!("y {yes}   n cancel", yes = yes),
                    },
                    usize::from(position_x.saturating_sub(x + 3)),
                    Style::new(),
                );
            }
            (None, Some(error)) => {
                buf.set_stringn(area.x + 1, y, error, room, Style::new().fg(Color::Red));
            }
            (None, None) if self.typing_branch.is_some() => {
                let (_, at, name) = self.typing_branch.as_ref().unwrap();
                let text = match self.naming {
                    Naming::Branch => tr!("New branch at {at}: {name}", at = at, name = name),
                    Naming::Tag => tr!("New tag at {at}: {name}", at = at, name = name),
                    Naming::StashBranch => tr!(
                        "New branch where {at} was made, with it applied: {name}",
                        at = at,
                        name = name
                    ),
                    Naming::Rename => tr!("Rename {at} to: {name}", at = at, name = name),
                    Naming::Stash => tr!("Stash message (Enter for none): {name}", name = name),
                    Naming::Worktree => {
                        tr!("New worktree for {at} at: {name}", at = at, name = name)
                    }
                    Naming::WorktreeBranch => {
                        tr!(
                            "New worktree from {at} with a new branch: {name}",
                            at = at,
                            name = name
                        )
                    }
                    Naming::PushTo => {
                        tr!("Push {at} to (remote/branch): {name}", at = at, name = name)
                    }
                    Naming::Orphan => tr!("New orphan branch: {name}", name = name),
                    Naming::RemoteName => tr!("New remote's name: {name}", name = name),
                    Naming::RemoteUrl | Naming::RemoteEditUrl => {
                        tr!("URL of {at}: {name}", at = at, name = name)
                    }
                    Naming::RemoteRename => {
                        tr!("Rename the remote {at} to: {name}", at = at, name = name)
                    }
                    Naming::BundleFile => tr!("Fetch from the bundle at: {name}", name = name),
                    Naming::Patch => tr!("Apply the patch at: {name}", name = name),
                    Naming::OpenFile => {
                        let text = tr!("File History of (Tab completes): {name}", name = name);
                        match self.path_choices.is_empty() {
                            true => text,
                            false => format!("{text}    {}", self.path_choices),
                        }
                    }
                    Naming::SavePatch => {
                        tr!(
                            "Save {at} as patches in the folder (or one .mbox file): {name}",
                            at = at,
                            name = name
                        )
                    }
                    Naming::PatchVersion => tr!("Version of the patches (-v): {name}", name = name),
                    Naming::RangeDiffRanges => tr!(
                        "Compare the ranges (A...B, A..B C..D or base A B): {name}",
                        name = name
                    ),
                    Naming::SubmoduleUrl => {
                        tr!("URL of the submodule {at}: {name}", at = at, name = name)
                    }
                    Naming::SubmoduleBranch => tr!(
                        "Branch for {at} to follow (empty for the default): {name}",
                        at = at,
                        name = name
                    ),
                    Naming::SubmoduleForeach => tr!(
                        "Run in each submodule (a shell command, as git submodule foreach): {name}",
                        name = name
                    ),
                    Naming::SubmoduleAddUrl => {
                        tr!("URL of the submodule to add: {name}", name = name)
                    }
                    Naming::SubmoduleAddPath => tr!(
                        "Add {at} as a submodule at (a folder in this repository): {name}",
                        at = at,
                        name = name
                    ),
                    Naming::SubtreeDir => tr!(
                        "Add a subtree in the folder (from the top of the repository): {name}",
                        name = name
                    ),
                    Naming::SubtreeSource => tr!(
                        "Subtree {what} for {dir}: the URL and the branch or tag: {name}",
                        what = at,
                        dir = self.typing_branch.as_ref().map_or("", |t| t.0.as_str()),
                        name = name
                    ),
                    Naming::SubtreeSplit => tr!(
                        "New branch with only the history of {at}: {name}",
                        at = self.typing_branch.as_ref().map_or("", |t| t.0.as_str()),
                        name = name
                    ),
                    Naming::ActivityExport => tr!(
                        "Write the operations as JSON to (on this machine): {name}",
                        name = name
                    ),
                    Naming::CloneDest => tr!(
                        "Clone {at} into (on this machine): {name}",
                        at = at,
                        name = name
                    ),
                    Naming::SparseFolders => {
                        let text = tr!(
                            "Check out only the folders (spaces between, Tab completes, empty for all): {name}",
                            name = name
                        );
                        match self.path_choices.is_empty() {
                            true => text,
                            false => format!("{text}    {}", self.path_choices),
                        }
                    }
                    Naming::DeepenSince => tr!(
                        "Fetch the commits newer than (a date git reads, like 2024-01-31): {name}",
                        name = name
                    ),
                    Naming::ArchiveFolder => {
                        tr!("Export only the folder (from the top): {name}", name = name)
                    }
                    Naming::WorktreeLockReason => tr!("Reason for the lock: {name}", name = name),
                    Naming::WorktreeMove => tr!("Move the worktree to: {name}", name = name),
                    Naming::WorktreeLocate => {
                        tr!("Where its folder is now: {name}", name = name)
                    }
                    Naming::BundleBase => tr!(
                        "Leave out what the receiver has, up to (a branch or commit): {name}",
                        name = name
                    ),
                    Naming::BundleName => {
                        tr!("Fetch its branches as remote: {name}", name = name)
                    }
                    Naming::Bundle => {
                        tr!("Save the repository as a bundle at: {name}", name = name)
                    }
                    Naming::Archive => {
                        tr!(
                            "Export {at} to (.zip, .tar.gz or .tar): {name}",
                            at = at,
                            name = name
                        )
                    }
                };
                let x = buf.set_stringn(area.x + 1, y, text, room, Style::new()).0;
                cursor = Some((x.min(position_x.saturating_sub(1)), y));
            }
            (None, None) if self.typing_filter.is_some() => {
                let filter = self.typing_filter.as_deref().unwrap_or("");
                let text = tr!("Filter: {filter}", filter = filter);
                let x = buf.set_stringn(area.x + 1, y, text, room, Style::new()).0;
                cursor = Some((x.min(position_x.saturating_sub(1)), y));
            }
            (None, None) if self.typing_query.is_some() => {
                let query = self.typing_query.as_deref().unwrap_or("");
                let x = buf
                    .set_stringn(area.x + 1, y, format!("/{query}"), room, Style::new())
                    .0;
                cursor = Some((x.min(position_x.saturating_sub(1)), y));
            }
            (None, None) if self.notice.is_some() => {
                let notice = self.notice.as_deref().unwrap_or("");
                buf.set_stringn(area.x + 1, y, notice, room, Style::new().fg(Color::Green));
            }
            // 途中の操作: 「Rebase 3/7 · 2 conflicts」と、その操作のキー。
            (None, None) if self.sequence.is_some() => {
                let (kind, progress) = self.sequence.as_ref().unwrap();
                let mut state = kind.clone();
                if let Some((current, total)) = progress {
                    state.push_str(&format!(" {current}/{total}"));
                }
                match self.changes.conflicts() {
                    0 => {}
                    1 => state.push_str(&tr!(" · 1 conflict")),
                    count => state.push_str(&tr!(" · {count} conflicts", count = count)),
                }
                let x = buf
                    .set_stringn(
                        area.x + 1,
                        y,
                        state,
                        room,
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )
                    .0;
                buf.set_stringn(
                    x + 3,
                    y,
                    match self.retry_three_way {
                        true => tr!("C continue   K skip   A abort   R retry with 3-way"),
                        false => tr!("C continue   K skip   A abort"),
                    },
                    usize::from(position_x.saturating_sub(x + 3)),
                    Style::new(),
                );
            }
            (None, None) => {
                let hints = match (self.view, self.focus) {
                    (View::History, Focus::Main) => match &self.search {
                        // 検索している間は、語と、合った行のうち何番目か。
                        Some((query, running)) => {
                            let (at, of) = self.history.match_position();
                            let found = match (running, at) {
                                // 動いている間は、ここまでに合った数（と、内容の検索なら調べ終えた割合）。
                                (Some(_), _) if self.search_done > 0.0 => tr!(
                                    "searching… {percent}%, {count} so far",
                                    percent = (self.search_done * 100.0) as u32,
                                    count = of
                                ),
                                (Some(_), _) if of > 0 => {
                                    tr!("searching… {count} so far", count = of)
                                }
                                (Some(_), _) => tr!("searching…"),
                                (None, Some(at)) => format!("{at}/{of}"),
                                (None, None) => of.to_string(),
                            };
                            match self.matching {
                                Some(_) => tr!(
                                    "/{query}: {found}, only matching   n/N next/previous   & all   Esc clear",
                                    query = query,
                                    found = found
                                ),
                                None => tr!(
                                    "/{query}: {found}   n/N next/previous   & only matching   Esc clear",
                                    query = query,
                                    found = found
                                ),
                            }
                        }
                        None => tr!("j/k move   / search   Enter details   0 sidebar   q quit"),
                    },
                    (View::File, Focus::Main) => {
                        tr!(
                            "{title}   j/k move   Enter details   H in History   Esc back",
                            title = self.file.title()
                        )
                    }
                    (View::History | View::Reflog | View::File, Focus::Side) => {
                        tr!("j/k scroll   [/] file   t tree   h/l pan   Esc back")
                    }
                    (View::Reflog, Focus::Main) => match self.reflog.name() {
                        Some(name) => tr!(
                            "Reflog of {name}   j/k move   Enter details   3 HEAD’s reflog",
                            name = name
                        ),
                        None => {
                            tr!("j/k move   Enter details   b new branch   0 sidebar   q quit")
                        }
                    },
                    (View::Changes, Focus::Main) => {
                        tr!(
                            "j/k move   Space stage/unstage   c message   a amend   d discard   q quit"
                        )
                    }
                    (View::Changes, Focus::Side) => {
                        tr!("j/k line   [/] hunk   Space stage/unstage   d discard   Esc back")
                    }
                    (_, Focus::Message) => {
                        tr!("Ctrl-s commit   Ctrl-p commit and push   Ctrl-e editor   Esc back")
                    }
                    (_, Focus::Sidebar) => {
                        tr!("j/k move   Enter open   / filter   ? keys   Esc back   q quit")
                    }
                };
                buf.set_stringn(
                    area.x + 1,
                    y,
                    hints,
                    room,
                    Style::new().fg(Color::Indexed(244)),
                );
            }
        }
        buf.set_string(position_x, y, position, Style::new());
        if self.help {
            cursor = None;
            let lines = self.help_lines();
            let keys = lines.iter().map(|l| l.0.width()).max().unwrap_or(0) as u16;
            let text = lines.iter().map(|l| l.1.width()).max().unwrap_or(0) as u16;
            // 枠は画面の真ん中に。収まらない分は切る。
            let width = (keys + text + 7).min(area.width);
            let height = (lines.len() as u16 + 2).min(area.height);
            let at = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height - height) / 2,
                width,
                height,
            );
            frame_box(buf, area, at, &tr!(" Keys "));
            // 収まらない分は、`j` `k` で動かす。
            let inside = usize::from(height.saturating_sub(2));
            self.help_top = self.help_top.min(lines.len().saturating_sub(inside));
            if lines.len() > inside {
                let more = tr!(" j/k more ");
                let x = at.right().saturating_sub(more.width() as u16 + 2);
                if x > at.x + 8 {
                    buf.set_string(
                        x,
                        at.bottom() - 1,
                        more,
                        Style::new().fg(Color::Indexed(244)),
                    );
                }
            }
            for (y, (key, text)) in
                (at.y + 1..at.bottom().saturating_sub(1)).zip(lines.iter().skip(self.help_top))
            {
                buf.set_stringn(
                    at.x + 2,
                    y,
                    key,
                    usize::from(width.saturating_sub(4)),
                    Style::new().fg(Color::Cyan),
                );
                if width > keys + 6 {
                    buf.set_stringn(
                        at.x + 4 + keys,
                        y,
                        text,
                        usize::from(width - keys - 6),
                        Style::new(),
                    );
                }
            }
        }
        let shown = match self.showing_failure {
            true => self.failure_details.as_ref(),
            false => self.hook_output.as_ref(),
        };
        if let (Some(top), Some(output)) = (self.showing_hooks, shown) {
            cursor = None;
            let lines: Vec<&str> = output.lines().collect();
            let text = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
            let width = (text + 4).max(30).min(area.width);
            let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(1));
            let at = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height.saturating_sub(1) - height) / 2,
                width,
                height,
            );
            let title = match (self.showing_failure, self.details_are_output) {
                (true, true) => tr!(" Output of the command "),
                (true, false) => tr!(" Details of the failure "),
                (false, _) => tr!(" Output of the hooks "),
            };
            frame_box(buf, area, at, &title);
            let inside = usize::from(height.saturating_sub(2));
            let top = top.min(lines.len().saturating_sub(inside));
            self.showing_hooks = Some(top);
            if lines.len() > inside {
                let more = tr!(" j/k more ");
                let x = at.right().saturating_sub(more.width() as u16 + 2);
                if x > at.x + 8 {
                    buf.set_string(
                        x,
                        at.bottom() - 1,
                        more,
                        Style::new().fg(Color::Indexed(244)),
                    );
                }
            }
            for (y, line) in (at.y + 1..at.bottom().saturating_sub(1)).zip(lines.iter().skip(top)) {
                buf.set_stringn(
                    at.x + 2,
                    y,
                    line,
                    usize::from(width.saturating_sub(4)),
                    Style::new(),
                );
            }
        }
        if let Some(mode) = &self.edit_mode {
            // 一番下の行を残して、画面を覆う枠。
            let at = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
            frame_box(buf, area, at, &tr!(" Edit Commits "));
            if at.width > 4 && at.height > 2 {
                let inner = Rect::new(at.x + 2, at.y + 1, at.width - 4, at.height - 2);
                for y in inner.y..inner.bottom() {
                    for x in inner.x..inner.right() {
                        buf[(x, y)].reset();
                    }
                }
                mode.draw(inner, buf);
            }
        }
        // 確かめている間は、前と後の絵を重ねる。
        if let Some((title, drawing)) = self
            .question_drawing
            .as_ref()
            .filter(|_| self.confirm.is_some() || self.choices.is_some())
        {
            let width = (crate::drawing::width(drawing) + 4).max(48).min(area.width);
            let height = (crate::drawing::height(drawing) + 2).min(area.height.saturating_sub(1));
            let at = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height.saturating_sub(1) - height) / 2,
                width,
                height,
            );
            frame_box(buf, area, at, title);
            if at.width > 4 && at.height > 2 {
                let inner = Rect::new(at.x + 2, at.y + 1, at.width - 4, at.height - 2);
                for y in inner.y..inner.bottom() {
                    for x in inner.x..inner.right() {
                        buf[(x, y)].reset();
                    }
                }
                crate::drawing::draw(drawing, inner, buf);
            }
        }
        if let Some(ops) = &self.operations {
            self.draw_operations(ops, area, buf);
        }
        if let Some(cache) = &self.history_cache {
            self.draw_history_cache(cache, area, buf);
        }
        if let Some(daemon) = &self.daemon {
            self.draw_daemon(daemon, area, buf);
        }
        if let Some(top) = self.showing_events {
            self.draw_events(top, area, buf);
        }
        if self.showing_performance {
            self.draw_performance(area, buf);
        }
        if let Some(log) = &self.operation_log {
            let times: Vec<String> = log
                .entries
                .iter()
                .map(|(_, time)| crate::history::ago(self.clock.now, *time))
                .collect();
            let label_width = log
                .entries
                .iter()
                .map(|(l, _)| l.width())
                .max()
                .unwrap_or(0) as u16;
            let hint = match log.entries.is_empty() {
                true => tr!("Nothing to undo   Tab operations"),
                false => tr!("Enter undoes back to the selected one   Tab operations"),
            };
            let width = (label_width + 10)
                .max(hint.width() as u16 + 4)
                .max(36)
                .min(area.width);
            // 枠・記録の行・案内の行。
            let height = (log.entries.len() as u16 + 3).min(area.height);
            let at = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height - height) / 2,
                width,
                height,
            );
            frame_box(buf, area, at, &tr!(" Operation Log "));
            let inner = usize::from(width.saturating_sub(4));
            let rows = usize::from(height.saturating_sub(3));
            let top = (log.selected + 1).saturating_sub(rows);
            for (y, i) in (at.y + 1..at.bottom().saturating_sub(2)).zip(top..log.entries.len()) {
                // 選んでいる記録まで（戻るもの）を、すべて反転する。
                let undone = i <= log.selected;
                let style = match undone {
                    true => Style::new().add_modifier(Modifier::REVERSED),
                    false => Style::new(),
                };
                if undone {
                    for x in at.x + 1..at.right() - 1 {
                        buf[(x, y)].set_style(style);
                    }
                }
                let time_style = match undone {
                    true => style,
                    false => Style::new().fg(Color::Indexed(244)),
                };
                buf.set_stringn(at.x + 2, y, &times[i], 4, time_style);
                buf.set_stringn(
                    at.x + 7,
                    y,
                    &log.entries[i].0,
                    inner.saturating_sub(5),
                    style,
                );
            }
            if height >= 3 {
                buf.set_stringn(
                    at.x + 2,
                    at.bottom() - 2,
                    &hint,
                    inner,
                    Style::new().fg(Color::Indexed(244)),
                );
            }
        }
        if let Some(pick) = &self.branch_pick {
            let shown = pick.shown();
            let recent = tr!("recent");
            let remote = tr!("remote");
            let new = tr!("new branch");
            let rows: Vec<(&str, &str, &str)> = shown
                .iter()
                .map(|c| match c {
                    BranchChoice::Branch(name, true) => ("", *name, remote.as_str()),
                    BranchChoice::Branch(name, false) if pick.recent.iter().any(|r| r == name) => {
                        ("", *name, recent.as_str())
                    }
                    BranchChoice::Branch(name, false) => ("", *name, ""),
                    BranchChoice::New(name) => ("+ ", *name, new.as_str()),
                })
                .collect();
            cursor = choice_list(
                buf,
                area,
                &tr!(" Switch Branch "),
                &pick.query,
                &rows,
                pick.selected,
                &tr!("No branch matches"),
            );
        }
        if let Some(quick) = &self.quick_open {
            let shown = quick.shown();
            let rows: Vec<(&str, &str, &str)> = shown
                .iter()
                .map(|c| {
                    let mark = if c.favorite { "★ " } else { "  " };
                    (mark, c.name.as_str(), c.path.as_str())
                })
                .collect();
            let none = match (clonable(quick.query.trim()), quick.items.is_empty()) {
                (true, _) => tr!("Enter clones it"),
                (false, true) => tr!("No repositories yet: type a URL to clone one"),
                (false, false) => tr!("No repository matches"),
            };
            cursor = choice_list(
                buf,
                area,
                &tr!(" Quick Open "),
                &quick.query,
                &rows,
                quick.selected,
                &none,
            );
        }
        if let Some(pick) = &self.fixup_pick {
            let shown = pick.shown();
            let rows: Vec<(&str, &str, &str)> = shown.iter().map(|(_, s)| ("", *s, "")).collect();
            let title = match pick.kind {
                PickKind::Squash => tr!(" Squash Commit For (Tab: fixup!) "),
                PickKind::Fixup => tr!(" Fixup Commit For (Tab: squash!) "),
                PickKind::Recent => tr!(" Recent Messages "),
                PickKind::Insert => tr!(" Insert into the Message "),
            };
            let none = match &pick.items {
                None => tr!("Reading the recent commits…"),
                Some(_) => tr!("No commit matches"),
            };
            cursor = choice_list(buf, area, &title, &pick.query, &rows, pick.selected, &none);
        }
        if let Some(palette) = &self.palette {
            let entries = self.palette_entries(&palette.query);
            let keys: Vec<String> = entries.iter().map(|(e, _)| e.key_label()).collect();
            let key_width = keys.iter().map(|k| k.width()).max().unwrap_or(0) as u16;
            let title_width = entries.iter().map(|(_, t)| t.width()).max().unwrap_or(0) as u16;
            let none = tr!("No command matches");
            let wanted = (title_width + key_width + 7)
                .max(none.width() as u16 + 4)
                .max(palette.query.width() as u16 + 7)
                .max(36);
            let width = wanted.min(area.width);
            // 枠・打っている語の行・操作の行（なければ、ないと書く 1 行）。
            let height = (entries.len().max(1) as u16 + 3).min(area.height);
            let at = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height - height) / 2,
                width,
                height,
            );
            frame_box(buf, area, at, &tr!(" Commands "));
            let inner = usize::from(width.saturating_sub(4));
            if height >= 3 {
                let x = buf
                    .set_stringn(at.x + 2, at.y + 1, ":", inner, Style::new())
                    .0;
                let x = buf
                    .set_stringn(
                        x,
                        at.y + 1,
                        &palette.query,
                        inner.saturating_sub(1),
                        Style::new(),
                    )
                    .0;
                cursor = Some((x.min(at.right().saturating_sub(2)), at.y + 1));
            }
            // 収まらない分は、選んでいる行が見えるように送る。
            let rows = usize::from(height.saturating_sub(3));
            let top = (palette.selected + 1).saturating_sub(rows);
            if entries.is_empty() && rows > 0 {
                buf.set_stringn(
                    at.x + 2,
                    at.y + 2,
                    none,
                    inner,
                    Style::new().fg(Color::Indexed(244)),
                );
            }
            for (y, i) in (at.y + 2..at.bottom().saturating_sub(1)).zip(top..entries.len()) {
                let selected = i == palette.selected;
                let style = match selected {
                    true => Style::new().add_modifier(Modifier::REVERSED),
                    false => Style::new(),
                };
                if selected {
                    for x in at.x + 1..at.right() - 1 {
                        buf[(x, y)].set_style(style);
                    }
                }
                buf.set_stringn(at.x + 2, y, &entries[i].1, inner, style);
                let key = &keys[i];
                let key_x = at.right().saturating_sub(key.width() as u16 + 2);
                if key_x > at.x + 2 + entries[i].1.width() as u16 {
                    let style = match selected {
                        true => style,
                        false => Style::new().fg(Color::Cyan),
                    };
                    buf.set_string(key_x, y, key, style);
                }
            }
        }
        // 入力しているときだけ、カーソルを出す。
        cursor
    }
}

/// パッチの見通し（`readPatch` の `info`）を、確かめる文の後ろに付ける短い形に: メールの形なら件名、ただの diff なら
/// ファイルと足した・消した行の数（アプリの Apply Patch のシートの一覧の代わり）。
fn patch_preview(info: &Value) -> String {
    let patches: Vec<&Value> = info["patches"].as_array().into_iter().flatten().collect();
    let mut parts: Vec<String> = Vec::new();
    if info["kind"] == "mailbox" {
        parts.extend(
            patches
                .iter()
                .filter_map(|p| p["subject"].as_str())
                .map(|s| format!("“{s}”")),
        );
    } else {
        for file in patches
            .iter()
            .flat_map(|p| p["files"].as_array().into_iter().flatten())
        {
            let Some(path) = file["path"].as_str() else {
                continue;
            };
            parts.push(match file["binary"] == true {
                true => format!("{path} (binary)"),
                false => format!(
                    "{path} +{} −{}",
                    file["added"].as_u64().unwrap_or(0),
                    file["removed"].as_u64().unwrap_or(0)
                ),
            });
        }
    }
    const SHOWN: usize = 3;
    let more = parts.len().saturating_sub(SHOWN);
    parts.truncate(SHOWN);
    if more > 0 {
        parts.push(tr!("{count} more", count = more));
    }
    match parts.is_empty() {
        true => String::new(),
        false => format!(" ({})", parts.join(", ")),
    }
}

/// この機械のファイルのパス: 相対なら、TUI を始めたフォルダから。
fn local_path(file: &str) -> String {
    let path = std::path::Path::new(file);
    match path.is_absolute() {
        true => file.to_owned(),
        false => (std::env::current_dir().unwrap_or_default().join(path))
            .to_string_lossy()
            .into_owned(),
    }
}

/// 画面に重ねる枠を描く（中は空に。題は上の線に）。
/// 打った語で絞る一覧（Quick Open など）を、画面の中ほどに重ねる。行は（印、名前、薄く添える文）。打つ欄の
/// カーソルの位置を返す。
fn choice_list(
    buf: &mut Buffer,
    area: Rect,
    title: &str,
    query: &str,
    rows: &[(&str, &str, &str)],
    selected: usize,
    none: &str,
) -> Option<(u16, u16)> {
    let mark_width = rows.iter().map(|r| r.0.width()).max().unwrap_or(0);
    let name_width = rows.iter().map(|r| r.1.width()).max().unwrap_or(0);
    let dim_width = rows.iter().map(|r| r.2.width()).max().unwrap_or(0);
    let wanted = (mark_width + name_width + dim_width + 7)
        .max(title.width() + 4)
        .max(none.width() + 4)
        .max(44);
    let width = (wanted as u16).min(area.width);
    let height = (rows.len().max(1) as u16 + 3).min(area.height);
    let at = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame_box(buf, area, at, title);
    let inner = usize::from(width.saturating_sub(4));
    let mut cursor = None;
    if height >= 3 {
        let x = buf
            .set_stringn(at.x + 2, at.y + 1, "> ", inner, Style::new())
            .0;
        let x = buf
            .set_stringn(x, at.y + 1, query, inner.saturating_sub(2), Style::new())
            .0;
        cursor = Some((x.min(at.right().saturating_sub(2)), at.y + 1));
    }
    let shown = usize::from(height.saturating_sub(3));
    let top = (selected + 1).saturating_sub(shown);
    if rows.is_empty() && shown > 0 {
        let dim = Style::new().fg(Color::Indexed(244));
        buf.set_stringn(at.x + 2, at.y + 2, none, inner, dim);
    }
    for (y, i) in (at.y + 2..at.bottom().saturating_sub(1)).zip(top..rows.len()) {
        let is_selected = i == selected;
        let style = match is_selected {
            true => Style::new().add_modifier(Modifier::REVERSED),
            false => Style::new(),
        };
        if is_selected {
            for x in at.x + 1..at.right() - 1 {
                buf[(x, y)].set_style(style);
            }
        }
        let (mark, name, dim) = rows[i];
        let x = buf.set_stringn(at.x + 2, y, mark, inner, style).0;
        let room = usize::from(at.right().saturating_sub(x + 2));
        let x = buf.set_stringn(x, y, name, room, style).0;
        if !dim.is_empty() {
            let dim_style = match is_selected {
                true => style,
                false => Style::new().fg(Color::Indexed(244)),
            };
            let room = usize::from(at.right().saturating_sub(x + 4));
            buf.set_stringn(x + 2, y, dim, room, dim_style);
        }
    }
    cursor
}

/// バイト数を読みやすく（1.2 MB など）。
fn human_bytes(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / (1u64 << 10) as f64),
        b => format!("{b} B"),
    }
}

impl App {
    /// History Cache の枠: 永続キャッシュの大きさと、この回の書き込み（回数、合計、足した行・切った行、最後のもの）。
    fn draw_history_cache(&self, cache: &Value, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<String> = Vec::new();
        if cache.is_null() {
            lines.push(tr!("History isn't cached on disk for this repository"));
        } else {
            let n = |v: &Value| v.as_u64().unwrap_or(0);
            let writes = &cache["writes"];
            lines.push(tr!(
                "Cache file: {size}",
                size = human_bytes(n(&cache["size"]))
            ));
            lines.push(tr!(
                "Writes this run: {count} ({bytes} in all)",
                count = n(&writes["count"]),
                bytes = human_bytes(n(&writes["bytes"]))
            ));
            lines.push(tr!(
                "Rows added: {rows}, cut: {cut}",
                rows = n(&writes["rows"]),
                cut = n(&writes["cut"])
            ));
            lines.push(match writes["lastAt"].as_u64() {
                Some(ms) => tr!(
                    "Last write: {bytes}, {ago}",
                    bytes = human_bytes(n(&writes["lastBytes"])),
                    ago = crate::history::ago(self.clock.now, (ms / 1000) as i64)
                ),
                None => tr!("Nothing written yet this run"),
            });
        }
        let hint = tr!("r read again   Tab performance   other keys close");
        let text = lines
            .iter()
            .map(|l| l.width())
            .max()
            .unwrap_or(0)
            .max(hint.width()) as u16;
        let width = (text + 4).min(area.width);
        let height = (lines.len() as u16 + 4).min(area.height);
        let at = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame_box(buf, area, at, &tr!(" History Cache "));
        let inner = usize::from(width.saturating_sub(4));
        for (y, line) in (at.y + 1..at.bottom().saturating_sub(2)).zip(&lines) {
            buf.set_stringn(at.x + 2, y, line, inner, Style::new());
        }
        buf.set_stringn(
            at.x + 2,
            at.bottom() - 2,
            &hint,
            inner,
            Style::new().fg(Color::Indexed(244)),
        );
    }

    /// Performance の枠: 測るものごとに、回数・p50・p95・最大・最後（ミリ秒）。
    fn draw_performance(&self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<String> = Vec::new();
        let names = [
            (PERF_HISTORY, tr!("History reload")),
            (PERF_STATUS, tr!("Status reload")),
            (PERF_DIFF, tr!("Commit diff load")),
            (PERF_WORKING_DIFF, tr!("Working Copy diff load")),
        ];
        let width = names.iter().map(|(_, n)| n.width()).max().unwrap_or(0);
        lines.push(format!(
            "{:<width$}  {:>5}  {:>8}  {:>8}  {:>8}  {:>8}",
            "",
            tr!("count"),
            "p50",
            "p95",
            tr!("max"),
            tr!("last")
        ));
        for (metric, name) in &names {
            let Some((_, values)) = self.perf.iter().find(|(m, _)| m == metric) else {
                lines.push(format!("{name:<width$}  {:>5}", 0));
                continue;
            };
            let mut sorted: Vec<f64> = values.iter().copied().collect();
            sorted.sort_by(f64::total_cmp);
            let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
            let ms = |v: f64| format!("{v:.1}ms");
            lines.push(format!(
                "{name:<width$}  {:>5}  {:>8}  {:>8}  {:>8}  {:>8}",
                sorted.len(),
                ms(at(0.5)),
                ms(at(0.95)),
                ms(at(1.0)),
                ms(*values.back().unwrap_or(&0.0))
            ));
        }
        let hint =
            tr!("From asking to the answer, the last 200 of each   Tab events   other keys close");
        let text = lines
            .iter()
            .map(|l| l.width())
            .max()
            .unwrap_or(0)
            .max(hint.width()) as u16;
        let w = (text + 4).min(area.width);
        let height = (lines.len() as u16 + 4).min(area.height);
        let at = Rect::new(
            area.x + (area.width - w) / 2,
            area.y + (area.height - height) / 2,
            w,
            height,
        );
        frame_box(buf, area, at, &tr!(" Performance "));
        let inner = usize::from(w.saturating_sub(4));
        for (y, line) in (at.y + 1..at.bottom().saturating_sub(2)).zip(&lines) {
            buf.set_stringn(at.x + 2, y, line, inner, Style::new());
        }
        buf.set_stringn(
            at.x + 2,
            at.bottom() - 2,
            &hint,
            inner,
            Style::new().fg(Color::Indexed(244)),
        );
    }

    /// Events の枠: 変化の知らせと、それで読み直したもの（新しい順）。
    fn draw_events(&self, top: usize, area: Rect, buf: &mut Buffer) {
        let width = area.width.saturating_sub(4).max(20).min(area.width);
        let height = area.height.saturating_sub(2).max(5).min(area.height);
        let at = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame_box(buf, area, at, &tr!(" Events "));
        let inner = usize::from(width.saturating_sub(4));
        let dim = Style::new().fg(Color::Indexed(244));
        if self.event_log.is_empty() {
            buf.set_stringn(at.x + 2, at.y + 1, tr!("No change notices yet"), inner, dim);
        }
        let now = self.clock_now();
        for (y, (time, text)) in
            (at.y + 1..at.bottom().saturating_sub(2)).zip(self.event_log.iter().rev().skip(top))
        {
            let when = crate::history::ago(now, *time);
            buf.set_stringn(at.x + 2, y, &when, 4, dim);
            buf.set_stringn(at.x + 7, y, text, inner.saturating_sub(5), Style::new());
        }
        buf.set_stringn(
            at.x + 2,
            at.bottom() - 2,
            tr!("Change notices and the reloads they cause (newest first, up to 500)   j/k move   Tab daemon   other keys close"),
            inner,
            dim,
        );
    }

    /// Daemon の枠: 共有の daemon の pid・版・動いている時間・つながりの数と、開いているリポジトリ。
    fn draw_daemon(&self, s: &Value, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<String> = Vec::new();
        let duration = |ms: &Value| {
            let secs = ms.as_u64().unwrap_or(0) / 1000;
            match secs {
                s if s >= 3600 => format!("{}h {}m", s / 3600, s % 3600 / 60),
                s if s >= 60 => format!("{}m {}s", s / 60, s % 60),
                s => format!("{s}s"),
            }
        };
        if s.is_null() {
            lines.push(tr!("The shared daemon isn’t running"));
        } else {
            lines.push(tr!(
                "Running: pid {pid}, {version}, up {uptime}",
                pid = s["pid"],
                version = s["version"].as_str().unwrap_or(""),
                uptime = duration(&s["uptimeMs"])
            ));
            lines.push(tr!("Connections: {count}", count = s["connections"]));
            for r in s["repos"].as_array().into_iter().flatten() {
                let clients = r["clients"].as_u64().unwrap_or(0);
                let state = match clients {
                    0 => tr!(
                        "no connections (idle for {duration})",
                        duration = duration(&r["idleMs"])
                    ),
                    1 => tr!("1 connection"),
                    n => tr!("{count} connections", count = n),
                };
                lines.push(format!("  {}  {state}", r["path"].as_str().unwrap_or("")));
            }
            lines.push(tr!(
                "Exits after {duration} with no connections",
                duration = duration(&s["idleExitMs"])
            ));
        }
        let hint = tr!("S start   X stop   r read again   Tab operation log   other keys close");
        let text = lines
            .iter()
            .map(|l| l.width())
            .max()
            .unwrap_or(0)
            .max(hint.width()) as u16;
        let width = (text + 4).min(area.width);
        let height = (lines.len() as u16 + 4).min(area.height);
        let at = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame_box(buf, area, at, &tr!(" Daemon "));
        let inner = usize::from(width.saturating_sub(4));
        for (y, line) in (at.y + 1..at.bottom().saturating_sub(2)).zip(&lines) {
            buf.set_stringn(at.x + 2, y, line, inner, Style::new());
        }
        buf.set_stringn(
            at.x + 2,
            at.bottom() - 2,
            &hint,
            inner,
            Style::new().fg(Color::Indexed(244)),
        );
    }

    /// Operations の枠: 上に記録の一覧（✓ ✗ ●、所要時間、いつ）、下に選んでいるものの出力か失敗の理由。
    fn draw_operations(&self, ops: &Operations, area: Rect, buf: &mut Buffer) {
        let width = area.width.saturating_sub(4).max(20).min(area.width);
        let height = area.height.saturating_sub(2).max(6).min(area.height);
        let at = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame_box(buf, area, at, &tr!(" Operations "));
        let inner = usize::from(width.saturating_sub(4));
        let body = height.saturating_sub(3);
        let list_rows = (body / 2).max(1);
        let top = (ops.selected + 1).saturating_sub(usize::from(list_rows));
        let dim = Style::new().fg(Color::Indexed(244));
        if ops.entries.is_empty() {
            buf.set_stringn(at.x + 2, at.y + 1, tr!("Nothing has run yet"), inner, dim);
        }
        for (y, i) in (at.y + 1..at.y + 1 + list_rows).zip(top..ops.entries.len()) {
            let op = &ops.entries[i];
            let (mark, color) = match op.ok {
                Some(true) => ('✓', Color::Green),
                Some(false) => ('✗', Color::Red),
                None => ('●', Color::Yellow),
            };
            let selected = i == ops.selected;
            if selected {
                for x in at.x + 1..at.right() - 1 {
                    buf[(x, y)].set_style(Style::new().add_modifier(Modifier::REVERSED));
                }
            }
            buf[(at.x + 2, y)].set_char(mark).set_fg(color);
            let when = crate::history::ago(self.clock.now, op.started);
            let took = match (op.duration, &op.progress) {
                (_, Some(progress)) if op.ok.is_none() => progress.clone(),
                // 記録はミリ秒。
                (Some(ms), _) if ms >= 1000.0 => format!("{:.1}s", ms / 1000.0),
                (Some(ms), _) => format!("{ms:.0}ms"),
                _ => String::new(),
            };
            let right = format!("{took} {when}");
            let label = &op.label;
            let room = inner.saturating_sub(right.width() + 3);
            buf.set_stringn(
                at.x + 4,
                y,
                crate::history::fit(label, room),
                room,
                Style::new(),
            );
            let x = at.right().saturating_sub(right.width() as u16 + 2);
            buf.set_stringn(x, y, &right, right.width(), dim);
        }
        // 区切りと、選んでいるものの出力（失敗なら理由を先に）。
        let sep = at.y + 1 + list_rows;
        if sep < at.bottom().saturating_sub(2) {
            for x in at.x + 1..at.right() - 1 {
                buf[(x, sep)].set_char('─').set_fg(Color::Indexed(240));
            }
            if let Some(op) = ops.entries.get(ops.selected) {
                let mut lines: Vec<(String, Style)> = Vec::new();
                lines.push((format!("{} · {}", op.kind, op.label), dim));
                if let Some(error) = &op.error {
                    lines.extend(
                        error
                            .lines()
                            .map(|l| (l.to_owned(), Style::new().fg(Color::Red))),
                    );
                }
                if let Some(output) = &op.output {
                    lines.extend(output.lines().map(|l| (l.to_owned(), Style::new())));
                }
                for (y, (line, style)) in (sep + 1..at.bottom().saturating_sub(2)).zip(lines) {
                    buf.set_stringn(at.x + 2, y, line, inner, style);
                }
            }
        }
        buf.set_stringn(
            at.x + 2,
            at.bottom() - 2,
            tr!(
                "j/k move   x cancel a running one   r read again   s export JSON   Tab history cache   other keys close"
            ),
            inner,
            dim,
        );
    }
}

/// clone の途中で聞いていること。
struct ClonePlan {
    url: String,
    dest: String,
    /// 0: 取り方を聞いている、1: 一緒にすることを聞いている。
    stage: u8,
    /// 取り方（`y` すべて、`c` 中身は必要なときに、`r` 最近の履歴だけ）。
    download: char,
}

/// Quick Open に打ったものが clone できる場所か（URL、`user@host:path`、`.git`・`.bundle` で終わるもの）。
fn clonable(text: &str) -> bool {
    if text.is_empty() || text.contains(char::is_whitespace) {
        return false;
    }
    text.contains("://")
        || text.ends_with(".git")
        || text.ends_with(".bundle")
        || text
            .split_once(':')
            .is_some_and(|(host, _)| host.contains('@') && !host.contains('/'))
}

fn frame_box(buf: &mut Buffer, area: Rect, at: Rect, title: &str) {
    for y in at.y..at.bottom() {
        // 枠の左端が全角の文字の後ろ半分に重なるなら、その文字を消す。
        if at.x > area.x && buf[(at.x - 1, y)].symbol().width() > 1 {
            buf[(at.x - 1, y)].set_char(' ');
        }
        for x in at.x..at.right() {
            let edge_y = y == at.y || y == at.bottom() - 1;
            let edge_x = x == at.x || x == at.right() - 1;
            let ch = match (edge_y, edge_x) {
                (true, true) if y == at.y && x == at.x => '┌',
                (true, true) if y == at.y => '┐',
                (true, true) if x == at.x => '└',
                (true, true) => '┘',
                (true, false) => '─',
                (false, true) => '│',
                (false, false) => ' ',
            };
            buf[(x, y)].reset();
            buf[(x, y)].set_char(ch);
        }
    }
    buf.set_stringn(
        at.x + 2,
        at.y,
        title,
        usize::from(at.width.saturating_sub(4)),
        Style::new().add_modifier(Modifier::BOLD),
    );
}

/// `predictMerge` の `conflicts`（ファイルの配列。git が古くて調べられなければ null）。
fn conflict_files(conflicts: &Value) -> Option<Vec<String>> {
    conflicts.as_array().map(|files| {
        files
            .iter()
            .filter_map(|f| f.as_str().map(str::to_owned))
            .collect()
    })
}

/// 書き換えの見通し（`rewritePreview` の `info`）から、コンフリクトしそうなファイルをまとめて（分からなければ
/// `None`）。
fn rewrite_conflicts(info: &Value) -> Option<Vec<String>> {
    info["conflicts"].as_array().map(|commits| {
        let mut files: Vec<String> = commits
            .iter()
            .flat_map(|c| c["files"].as_array().cloned().unwrap_or_default())
            .filter_map(|f| f.as_str().map(str::to_owned))
            .collect();
        files.sort();
        files.dedup();
        files
    })
}

/// 書き換えの見通しから、一緒に動くブランチと、push に Force Push が要るかを、確かめる文に足す文にする。
fn rewrite_notes(info: &Value) -> String {
    let moving: Vec<&str> = (info["moving"].as_array().into_iter().flatten())
        .filter_map(|b| b["name"].as_str())
        .collect();
    let mut notes = String::new();
    if !moving.is_empty() {
        notes += &tr!(
            " {names} move along.",
            names = amonhen_model::i18n::list(&moving)
        );
    }
    if info["currentPushed"] == true {
        notes +=
            &tr!(" Its upstream has the commits it rewrites, so pushing will need Force Push.");
    }
    notes
}

/// コンフリクトの見込みを、確かめる文に足す一文にする（分からなければ空）。
fn risk_text(conflicts: Option<Vec<String>>) -> String {
    match conflicts {
        Some(files) if !files.is_empty() => {
            let shown: Vec<&str> = files.iter().take(3).map(String::as_str).collect();
            let mut names = amonhen_model::i18n::list(&shown);
            if files.len() > 3 {
                names = tr!(
                    "{names} and {count} more",
                    names = names,
                    count = files.len() - 3
                );
            }
            tr!(" Conflicts are likely in {files}.", files = names)
        }
        Some(_) => tr!(" No conflicts are expected."),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use amonhen_front::InProcess;

    use super::*;

    /// コミットが 6 つの History を開いたアプリ。
    fn app_with_history() -> (tempfile::TempDir, App, std::sync::mpsc::Receiver<Event>) {
        amonhen_model::testing::isolate_from_user_settings();
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = amonhen_model::process::command("git")
                .args(["-c", "user.name=Ann", "-c", "user.email=ann@example.com"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        for i in 0..6 {
            git(&[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("Commit {i}"),
            ]);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let clock = Clock {
            now: 0,
            local: |t| t.to_string(),
        };
        let mut app = App::new(
            Arc::new(InProcess::open(dir.path(), None).unwrap()),
            tx,
            clock,
        );
        app.resize(100, 12);
        app.start();
        crate::settle(&mut app, &rx).unwrap();
        (dir, app, rx)
    }

    fn bottom_line(app: &mut App) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 12));
        app.draw(&mut buf, Damage::ALL);
        (0..100).map(|x| buf[(x, 11)].symbol()).collect()
    }

    /// メッセージの欄の Alt-↑・Alt-↓: 最近のメッセージを新しい順に呼び出し、戻りきると書いていたものに戻す。
    /// 一覧の `R` は、選んだメッセージを欄に入れる
    #[test]
    fn recent_messages_are_recalled_in_the_message_field() {
        let (_dir, mut app, rx) = app_with_history();
        let press = |app: &mut App, code: KeyCode, modifiers: KeyModifiers| {
            app.handle(Event::Input(Input::Key(KeyEvent::new(code, modifiers))));
        };
        let typed = |app: &mut App, text: &str| {
            for ch in text.chars() {
                press(app, KeyCode::Char(ch), KeyModifiers::NONE);
            }
        };
        typed(&mut app, "1");
        crate::settle(&mut app, &rx).unwrap();
        typed(&mut app, "cDraft");
        press(&mut app, KeyCode::Up, KeyModifiers::ALT);
        crate::settle(&mut app, &rx).unwrap();
        assert_eq!(app.changes.message.text(), "Commit 5");
        press(&mut app, KeyCode::Up, KeyModifiers::ALT);
        assert_eq!(app.changes.message.text(), "Commit 4");
        press(&mut app, KeyCode::Down, KeyModifiers::ALT);
        assert_eq!(app.changes.message.text(), "Commit 5");
        press(&mut app, KeyCode::Down, KeyModifiers::ALT);
        assert_eq!(app.changes.message.text(), "Draft");
        // 戻りきったら、それ以上は何もしない。
        press(&mut app, KeyCode::Down, KeyModifiers::ALT);
        assert_eq!(app.changes.message.text(), "Draft");

        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        typed(&mut app, "R");
        let names: Vec<String> = (app.fixup_pick.as_ref().unwrap().shown().iter())
            .map(|(_, s)| (*s).to_owned())
            .collect();
        assert_eq!(names[..2], ["Commit 5", "Commit 4"]);
        typed(&mut app, "3");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.changes.message.text(), "Commit 3");
        assert!(app.fixup_pick.is_none());
    }

    /// 一覧の `I`: 種類は件名の頭を置き換え、トレーラーは本文の終わりに空行で分けて足す（同じものは足さない）
    #[test]
    fn insert_puts_a_type_and_trailers_into_the_message() {
        let (_dir, mut app, rx) = app_with_history();
        let press = |app: &mut App, code: KeyCode| {
            app.handle(Event::Input(Input::Key(KeyEvent::new(
                code,
                KeyModifiers::NONE,
            ))));
        };
        let typed = |app: &mut App, text: &str| {
            for ch in text.chars() {
                press(app, KeyCode::Char(ch));
            }
        };
        typed(&mut app, "1");
        crate::settle(&mut app, &rx).unwrap();
        typed(&mut app, "I");
        crate::settle(&mut app, &rx).unwrap();
        let shown: Vec<String> = (app.fixup_pick.as_ref().unwrap().shown().iter())
            .map(|(_, s)| (*s).to_owned())
            .collect();
        assert_eq!(shown[..2], ["feat:", "fix:"]);
        assert!(shown.contains(&"Co-authored-by: Ann <ann@example.com>".to_owned()));
        typed(&mut app, "fix:");
        press(&mut app, KeyCode::Enter);
        typed(&mut app, "Hello");
        assert_eq!(app.changes.message.text(), "fix: Hello");
        press(&mut app, KeyCode::Esc);
        for _ in 0..2 {
            typed(&mut app, "Ico-auth");
            press(&mut app, KeyCode::Enter);
            press(&mut app, KeyCode::Esc);
        }
        assert_eq!(
            app.changes.message.text(),
            "fix: Hello\n\nCo-authored-by: Ann <ann@example.com>"
        );
        typed(&mut app, "Ifeat:");
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.changes.message.text(),
            "feat: Hello\n\nCo-authored-by: Ann <ann@example.com>"
        );
    }

    #[test]
    fn commit_types_and_trailers_are_recognized() {
        assert_eq!(strip_commit_type("fix(ui)!: Crash"), "Crash");
        assert_eq!(strip_commit_type("feat: Add"), "Add");
        assert_eq!(strip_commit_type("Add a thing"), "Add a thing");
        assert_eq!(strip_commit_type("see: below"), "below");
        assert!(is_trailer("Signed-off-by: Ann <a@example.com>"));
        assert!(!is_trailer("Note that: this is prose"));
    }

    /// 編集モードの `r`: 行のメッセージを git のエディタで直す（`#` の行は捨てる）。当てるまで何も変わらない。
    #[test]
    fn edit_mode_rewords_through_the_editor() {
        let (_dir, mut app, rx) = app_with_history();
        let key = |app: &mut App, ch: char| {
            app.handle(Event::Input(Input::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            ))));
        };
        key(&mut app, '2');
        key(&mut app, 'j');
        key(&mut app, 'E');
        crate::settle(&mut app, &rx).unwrap();
        assert_eq!(app.edit_mode.as_ref().map(|m| m.rows.len()), Some(2));
        key(&mut app, 'r');
        let (text, _) = app.take_edit_message().expect("the message to edit");
        assert_eq!(text.trim(), "Commit 5");
        app.message_edited(Ok("Better\n# a comment\n".to_owned()));
        let mode = app.edit_mode.as_ref().unwrap();
        assert_eq!(mode.rows[0].action, crate::editmode::Action::Reword);
        assert_eq!(mode.rows[0].new_message.as_deref(), Some("Better"));
        assert!(app.take_edit_message().is_none());
    }

    /// コマンドの一覧の Quick Open: アプリと同じ候補（お気に入り、ブックマーク、最近開いたもの、手元の探すフォルダの
    /// 下で見つけたもの。重ならない）を打った語で絞り、Enter で選んだ場所を開き直す。
    #[test]
    fn quick_open_reopens_a_bookmark() {
        let (_dir, mut app, rx) = app_with_history();
        let config = tempfile::tempdir().unwrap();
        let vars = amonhen_model::settings::config_vars(config.path().to_str().unwrap());
        let env = amonhen_model::process::ClientEnv::from_vars(vars);
        let key = |app: &mut App, code: KeyCode| {
            app.handle(Event::Input(Input::Key(KeyEvent::new(
                code,
                KeyModifiers::NONE,
            ))));
        };
        let typed = |app: &mut App, text: &str| {
            for ch in text.chars() {
                key(app, KeyCode::Char(ch));
            }
        };
        let work = config.path().join("work");
        std::fs::create_dir_all(work.join("found/.git")).unwrap();
        let found = work.join("found").to_string_lossy().into_owned();
        amonhen_model::process::with_client_env(&env, || {
            // 何もなくても開く（URL を打てば clone できる）。
            typed(&mut app, ":quick");
            key(&mut app, KeyCode::Enter);
            assert!(app.quick_open.as_ref().is_some_and(|q| q.items.is_empty()));
            key(&mut app, KeyCode::Esc);
            let dir = config.path().join("amonhen");
            std::fs::create_dir_all(&dir).unwrap();
            let bookmarks = serde_json::json!({
                "items": [{"path": "/w/alpha"}, {"path": "ssh://host/src/tool", "favorite": true}],
                "scanRoots": [{"path": work.to_str().unwrap()}, {"path": "ssh://host/src"}]
            });
            std::fs::write(dir.join("bookmarks.json"), bookmarks.to_string()).unwrap();
            amonhen_model::recent::touch("/w/beta").unwrap();
            amonhen_model::recent::touch("daemon:/w/alpha").unwrap();
            typed(&mut app, ":quick");
            key(&mut app, KeyCode::Enter);
        });
        let names = |app: &App| -> Vec<String> {
            let quick = app.quick_open.as_ref().expect("Quick Open");
            quick.shown().iter().map(|c| c.name.clone()).collect()
        };
        assert_eq!(names(&app), ["tool", "alpha", "beta"], "favorites first");
        // 手元の探すフォルダの下のものは、見つかったら足す。
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(10)) {
                Ok(Event::Scanned(paths)) => {
                    assert_eq!(paths, std::slice::from_ref(&found));
                    app.handle(Event::Scanned(paths));
                    break;
                }
                Ok(other) => app.handle(other),
                Err(e) => panic!("no scan: {e}"),
            }
        }
        assert_eq!(names(&app), ["tool", "alpha", "beta", "found"]);
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 12));
        app.draw(&mut buf, Damage::ALL);
        let screen: String = (0..12)
            .map(|y| (0..100).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n")
            .collect();
        assert!(screen.contains(" Quick Open "), "{screen}");
        assert!(screen.contains("★ tool  ssh://host/src/tool"), "{screen}");
        assert!(screen.contains("  alpha  /w/alpha"), "{screen}");
        assert!(screen.contains("  beta  /w/beta"), "{screen}");
        typed(&mut app, "alp");
        assert_eq!(app.quick_open.as_ref().unwrap().shown().len(), 1);
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.take_next(),
            Some(crate::Next::Location("/w/alpha".to_owned()))
        );
        assert!(app.quit());
    }

    /// サイドバーの Submodules: submodule が並び、中に変更があれば ✎。Enter で TUI をそこで開き直す。
    /// History と status の読み直しにかかった時間は、Activity の Performance に残る
    #[test]
    fn reloads_are_timed_for_performance() {
        let (_dir, mut app, _rx) = app_with_history();
        let count = |app: &App, metric: &str| {
            app.perf
                .iter()
                .find(|(m, _)| *m == metric)
                .map_or(0, |(_, v)| v.len())
        };
        assert!(count(&app, PERF_HISTORY) >= 1);
        assert!(count(&app, PERF_STATUS) >= 1);
        app.showing_performance = true;
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 12));
        app.draw(&mut buf, Damage::ALL);
        let screen: String = (0..12)
            .flat_map(|y| (0..100).map(move |x| (x, y)))
            .map(|(x, y)| buf[(x, y)].symbol().to_owned())
            .collect();
        assert!(screen.contains(" Performance "), "{screen}");
        assert!(screen.contains("History reload"), "{screen}");
        assert!(screen.contains("p95"), "{screen}");
    }

    /// 変化の知らせは Activity の Events に残る（何が変わり、何を読み直すか）
    #[test]
    fn change_notices_are_kept_as_events() {
        let (_dir, mut app, rx) = app_with_history();
        app.handle(Event::Changed(CHANGED_REFS | CHANGED_WORKTREE));
        crate::settle(&mut app, &rx).unwrap();
        let (_, text) = app.event_log.back().unwrap();
        assert_eq!(
            text,
            "Changed: refs, the working tree → read the status again → read History and the refs again"
        );
        app.showing_events = Some(0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 12));
        app.draw(&mut buf, Damage::ALL);
        let screen: String = (0..12)
            .flat_map(|y| (0..100).map(move |x| (x, y)))
            .map(|(x, y)| buf[(x, y)].symbol().to_owned())
            .collect();
        assert!(screen.contains(" Events "), "{screen}");
        assert!(screen.contains("Changed: refs"), "{screen}");
    }

    /// History Cache のバイト数は、読みやすい単位で。
    #[test]
    fn bytes_are_shown_in_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(3 * 1024 * 1024 + 512 * 1024), "3.5 MB");
        assert_eq!(human_bytes(5 << 30), "5.0 GB");
    }

    #[test]
    fn submodules_are_listed_and_open_there() {
        let (dir, mut app, rx) = app_with_history();
        let source = tempfile::tempdir().unwrap();
        let git = |at: &std::path::Path, args: &[&str]| {
            let out = amonhen_model::process::command("git")
                .args(["-c", "user.name=Ann", "-c", "user.email=ann@example.com"])
                .args([
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "protocol.file.allow=always",
                ])
                .args(args)
                .current_dir(at)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {out:?}");
        };
        git(source.path(), &["init", "-q"]);
        std::fs::write(source.path().join("lib.txt"), "lib\n").unwrap();
        git(source.path(), &["add", "."]);
        git(source.path(), &["commit", "-q", "-m", "Lib"]);
        git(
            dir.path(),
            &[
                "submodule",
                "add",
                "-q",
                source.path().to_str().unwrap(),
                "lib",
            ],
        );
        git(dir.path(), &["commit", "-q", "-m", "Add lib"]);
        std::fs::write(dir.path().join("lib/lib.txt"), "changed\n").unwrap();
        app.open();
        crate::settle(&mut app, &rx).unwrap();
        let key = |app: &mut App, code: KeyCode| {
            app.handle(Event::Input(Input::Key(KeyEvent::new(
                code,
                KeyModifiers::NONE,
            ))));
        };
        // 一番下の行が lib。中に変更があるので ✎。
        for code in [KeyCode::Char('0'), KeyCode::Char('G')] {
            key(&mut app, code);
        }
        crate::settle(&mut app, &rx).unwrap();
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 12));
        app.draw(&mut buf, Damage::ALL);
        let text: String = (0..12)
            .map(|y| (0..100).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n")
            .collect();
        assert!(text.contains("Submodules"), "{text}");
        assert!(
            text.lines().any(|l| l.contains("lib") && l.contains('✎')),
            "{text}"
        );
        key(&mut app, KeyCode::Enter);
        let Some(crate::Next::Worktree(opened)) = app.take_next() else {
            panic!("the submodule to open");
        };
        assert_eq!(
            std::fs::canonicalize(opened).unwrap(),
            std::fs::canonicalize(dir.path().join("lib")).unwrap()
        );
    }

    /// サイドバーの worktree で Enter: ほかの worktree なら TUI を閉じてそのパスを返し、開いているものなら場所を
    /// 知らせるだけ。
    #[test]
    fn enter_on_another_worktree_reopens_there() {
        let (dir, mut app, rx) = app_with_history();
        let other = tempfile::tempdir().unwrap();
        let path = other.path().join("other");
        let out = amonhen_model::process::command("git")
            .args(["worktree", "add", "-q", "-b", "topic"])
            .arg(&path)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success());
        app.open();
        crate::settle(&mut app, &rx).unwrap();
        let key = |app: &mut App, code: KeyCode| {
            app.handle(Event::Input(Input::Key(KeyEvent::new(
                code,
                KeyModifiers::NONE,
            ))));
        };
        // 一番下の行が other。その上が、開いているもの。
        for code in [
            KeyCode::Char('0'),
            KeyCode::Char('G'),
            KeyCode::Char('k'),
            KeyCode::Enter,
        ] {
            key(&mut app, code);
        }
        assert!(app.take_next().is_none());
        assert!(!app.quit());
        assert!(bottom_line(&mut app).contains("This is the worktree that is open"));
        for code in [KeyCode::Char('j'), KeyCode::Enter] {
            key(&mut app, code);
        }
        let Some(crate::Next::Worktree(opened)) = app.take_next() else {
            panic!("a worktree to open");
        };
        assert_eq!(
            std::fs::canonicalize(opened).unwrap(),
            std::fs::canonicalize(&path).unwrap()
        );
        assert!(app.quit());
    }

    /// settings.json の `tui.mouse` が false なら、マウスを取り込まずに始める。読めなければ取り込んだまま理由を出す。
    #[test]
    fn the_mouse_setting_applies_at_start() {
        let (_dir, mut app, _rx) = app_with_history();
        app.apply_mouse_setting(Ok(None));
        assert_eq!(app.take_mouse_change(), None);
        assert!(app.mouse_captured());
        app.apply_mouse_setting(Ok(Some(false)));
        assert_eq!(app.take_mouse_change(), Some(false));
        assert!(!app.mouse_captured());
        let (_dir, mut app, _rx) = app_with_history();
        app.apply_mouse_setting(Err("Amon Hen’s settings can’t be read".into()));
        assert_eq!(app.take_mouse_change(), None);
        assert!(bottom_line(&mut app).contains("settings can’t be read"));
    }

    /// Working Copy の `e`: 選んでいるファイルを、作業ツリーの絶対パスでエディタに渡す。`ssh://` の先なら断る。
    #[test]
    fn e_opens_the_selected_file_only_when_the_files_are_here() {
        let (dir, mut app, rx) = app_with_history();
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        // 変化の知らせは来ないので、読み直させる。
        app.changes.refresh();
        let key = |app: &mut App, ch: char| {
            app.handle(Event::Input(Input::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            ))));
        };
        key(&mut app, '1');
        crate::settle(&mut app, &rx).unwrap();
        key(&mut app, 'e');
        let (file, workdir) = app.take_open_file().expect("a file to open");
        assert_eq!(
            std::fs::canonicalize(&file).unwrap(),
            std::fs::canonicalize(dir.path().join("a.txt")).unwrap()
        );
        assert!(workdir.is_some());
        assert!(app.take_open_file().is_none());
        // ほかの機械のファイルは開かない。
        app.set_files_here(false);
        key(&mut app, 'e');
        assert!(app.take_open_file().is_none());
        assert!(bottom_line(&mut app).contains("The files are on another machine"));
    }

    /// 動いている検索の途中の結果: 最初に届いた分で合った行へ移り、後から届く分では動かさない。一番下の行に、
    /// ここまでの数と割合を出す。
    #[test]
    fn partial_matches_show_while_the_search_runs() {
        let (_dir, mut app, _rx) = app_with_history();
        let partial = |query: &str, rows: &[u32], done: f32| Event::Partial {
            query: query.to_owned(),
            rows: rows.to_vec(),
            done,
        };
        app.search = Some(("x".to_owned(), Some(Arc::new(AtomicBool::new(false)))));
        app.history.begin_search();
        assert!(bottom_line(&mut app).contains("/x: searching…   n/N"));

        app.handle(partial("x", &[2, 4], 0.0));
        assert_eq!(app.history.selected(), 2);
        assert!(bottom_line(&mut app).contains("/x: searching… 2 so far"));

        // 見ている所を動かしたら、後から届く分はそのまま。
        app.handle(Event::Input(Input::Key(KeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
        ))));
        assert_eq!(app.history.selected(), 3);
        app.handle(partial("x", &[1, 2, 4], 0.5));
        assert_eq!(app.history.selected(), 3);
        assert_eq!(app.history.match_position(), (None, 3));
        assert!(bottom_line(&mut app).contains("/x: searching… 50%, 3 so far"));

        // ほかの検索のものは捨てる。
        app.handle(partial("y", &[0], 0.9));
        assert_eq!(app.history.match_position(), (None, 3));
    }
}
