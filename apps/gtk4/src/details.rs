//! コミットの詳細。macOS 版の「詳細」の見せ方と同じ並び: 上に作者とコミッター、英語の見出しの表（Commit・
//! Parent・Child・Rev・Signature・Refs・Tag・Branches・Tags・Previous Tag・Pushed・Merged Tag・Merged・
//! Related）、件名・本文・トレーラー・notes。
//! すぐ読める分（`commit_info`）を先に出し、時間のかかる分（`commit_extras`: 署名の検証・子・含むブランチと
//! タグ・notes など）が届いたら表を作り直す。

use std::cell::{Cell as StdCell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::{PairMode, Repository};
use amonhen_model::{Oid, tr};

use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::Value;

/// 作者かコミッターの 1 人分（macOS 版の `PersonView`）: 左にアバター、右に名前（と役割）、メール、日時。名前と
/// メールは mailmap で置き換えたもので、記録と違えば記録されたものを下に小さく添える。
struct Person {
    root: gtk::Box,
    avatar: Rc<crate::avatars::Avatar>,
    text: gtk::Label,
    /// 記録された名前とメール（mailmap で置き換えて違うときだけ）。
    recorded: gtk::Label,
    role: String,
}

impl Person {
    fn new(role: String) -> Self {
        let avatar = crate::avatars::Avatar::new(32);
        let text = gtk::Label::new(None);
        text.set_xalign(0.0);
        text.set_yalign(0.0);
        text.set_wrap(true);
        text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        text.set_selectable(true);
        text.add_css_class("caption");
        let recorded = gtk::Label::new(None);
        recorded.set_xalign(0.0);
        recorded.set_wrap(true);
        recorded.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        recorded.set_selectable(true);
        recorded.add_css_class("caption");
        recorded.add_css_class("dim-label");
        recorded.set_visible(false);
        let lines = gtk::Box::new(gtk::Orientation::Vertical, 0);
        lines.set_hexpand(true);
        lines.append(&text);
        lines.append(&recorded);
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.append(&avatar.root);
        root.append(&lines);
        Self {
            root,
            avatar,
            text,
            recorded,
            role,
        }
    }

    fn show(&self, name: &str, email: &str, time: &str, recorded: Option<String>) {
        let e = |s: &str| glib::markup_escape_text(s).to_string();
        // 名前を押すと、History をその人のコミットで絞り込む（[`Details::connect_person`]）。
        let markup = format!(
            "<a href=\"person:{}\" title=\"{}\"><b>{}</b></a>  <span size=\"small\" alpha=\"55%\">{}</span>\n<span alpha=\"70%\">{}\n{}</span>",
            e(&glib::Uri::escape_string(name, None, false)),
            e(&tr!("Filter to this person’s commits")),
            e(name),
            e(&self.role),
            e(email),
            e(time)
        );
        self.text.set_markup(&markup);
        // 記録のものは、何なのかを吹き出しで説明する（macOS 版と同じ）。
        match recorded {
            Some(r) => {
                self.recorded
                    .set_markup(&format!("<span size=\"small\">{}</span>", e(&r)));
                self.recorded.set_tooltip_text(Some(&format!(
                    "{r}\n{}",
                    tr!("The name and email recorded in the commit. The mailmap shows them as above.")
                )));
                self.recorded.set_visible(true);
            }
            None => self.recorded.set_visible(false),
        }
        self.avatar.show(name, email);
        self.root.set_visible(true);
    }
}

pub struct Details {
    /// 上に検索の帯（Ctrl+F）、下にスクロールする詳細。
    pub root: gtk::Box,
    /// 件名・本文・notes を探す帯（macOS 版のメッセージの欄の検索の帯）。
    find: Rc<crate::messagefind::MessageFind>,
    column: gtk::Box,
    /// 上の帯: 作者とコミッターを左右に。
    people: gtk::Box,
    author: Person,
    committer: Person,
    table: gtk::Grid,
    subject: gtk::Label,
    body: gtk::Label,
    /// 本文の最後の段落のトレーラー（`Signed-off-by:` など。本文から除いて薄く並べる。macOS 版と同じ）。
    trailers: gtk::Label,
    notes: gtk::Label,
    info: RefCell<Option<Value>>,
    /// 詳細より先に届いた、時間のかかる分。
    extras: Rc<RefCell<Option<Value>>>,
    /// 複数のコミットを選んだときの見出し（ほかの部品の代わりに出す）。
    selection: gtk::Box,
    selection_title: gtk::Label,
    selection_note: gtk::Label,
    pairs: gtk::Box,
    selection_lines: gtk::Label,
    /// 右上のボタン（AI の翻訳・解説）を置く所。
    pub tools: gtk::Box,
    /// 署名を検証した場所（Signature の吹き出しに出す）。
    place: RefCell<crate::signature::Place>,
    /// コミット ID・ref の名前のリンクを押したとき（[`Details::connect_links`]。なければリンクにしない）。
    links: Rc<RefCell<Option<Links>>>,
    /// 作者の名前を押したとき（[`Details::connect_person`]）。
    on_person: Rc<RefCell<Option<OnRef>>>,
    /// 見せ方（簡潔・標準・詳細）。
    style: StdCell<Style>,
    /// 右上の見せ方の切り替え（[`Details::show_style_switch`] で出す）。
    styles: gtk::Box,
    style_buttons: Vec<gtk::ToggleButton>,
    /// 簡潔の 1 行（小さなアバター・作者・日時・ref・親）。
    brief: gtk::Box,
    brief_avatar: Rc<crate::avatars::Avatar>,
    brief_text: gtk::Label,
    /// 件名と本文の中で強調する語（History の検索の語。macOS 版の `highlight`）。
    highlight: RefCell<Vec<String>>,
    /// 右クリックのメニュー（ID・件名・メッセージ・git show のコピーと、Edit Message…。macOS 版の詳細の欄の
    /// メニュー）。文字の欄では、その欄のメニューの後ろに足す。
    menu: gio::Menu,
    /// メニューの Edit Message… の節（HEAD から辿れる merge でないコミットのときだけ入れる）。
    reword_section: gio::Menu,
    actions: gio::SimpleActionGroup,
    /// メニューの項目が使う、出しているコミット。
    menu_info: Rc<RefCell<Option<Value>>>,
    /// 書き直したメッセージを保存したとき（コミットの ID、メッセージ。[`Details::connect_edit_message`]）。
    on_reword: Rc<RefCell<Option<OnReword>>>,
    /// その場で書き直す欄（Edit Message…。macOS 版の `beginReword`）。
    reword: Rc<Reword>,
    /// 詳細のスクロール（書き直す間は隠して、書き直す欄を出す）。
    scrolled: gtk::ScrolledWindow,
}

/// 書き直したメッセージを保存したとき（コミットの ID、メッセージ）。
pub type OnReword = Rc<dyn Fn(&str, &str)>;

/// その場でメッセージを書き直す欄: 上に編集欄、下の帯に説明と「キャンセル」「保存」（macOS 版と同じ）。
struct Reword {
    root: gtk::Box,
    text: gtk::TextView,
    save: gtk::Button,
    cancel: gtk::Button,
    /// 書き直しているコミット（ID と、もとのメッセージ）。
    editing: RefCell<Option<(String, String)>>,
}

impl Reword {
    fn new() -> Self {
        let text = gtk::TextView::builder()
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(6)
            .bottom_margin(6)
            .left_margin(6)
            .right_margin(6)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .child(&text)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let hint = gtk::Label::new(Some(&tr!(
            "Ctrl+Return to save, Esc to cancel. You can Undo it."
        )));
        hint.add_css_class("caption");
        hint.add_css_class("dim-label");
        hint.set_xalign(0.0);
        hint.set_hexpand(true);
        hint.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let cancel = gtk::Button::with_label(&tr!("Cancel"));
        let save = gtk::Button::with_label(&tr!("Save"));
        save.add_css_class("suggested-action");
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_top(4);
        bar.set_margin_bottom(4);
        bar.set_margin_start(10);
        bar.set_margin_end(8);
        bar.append(&hint);
        bar.append(&cancel);
        bar.append(&save);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_vexpand(true);
        root.append(&scroll);
        root.append(&bar);
        root.set_visible(false);
        Self {
            root,
            text,
            save,
            cancel,
            editing: RefCell::default(),
        }
    }
}

/// 保存するメッセージ（前後の空白を除く）。空なら None、もとと同じなら `Some(None)`（何もせず閉じる）。
fn reworded(original: &str, edited: &str) -> Option<Option<String>> {
    let message = edited.trim();
    if message.is_empty() {
        return None;
    }
    Some((message != original.trim()).then(|| message.to_owned()))
}

/// 詳細の上の欄の見せ方（macOS 版の `CommitHeaderStyle`。同じ順の番号で覚える）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    /// 件名と 1 行のメタ情報。差分を広く見たいとき。
    Brief,
    /// 件名・ref・作者・ID・署名・含むタグと本文。
    Standard,
    /// 項目ごとの表と本文。
    Full,
}

/// 見せ方を覚える鍵（macOS 版と同じ名前）。
const STYLE_KEY: &str = "commitHeaderStyle";

impl Style {
    const ALL: [Style; 3] = [Style::Brief, Style::Standard, Style::Full];

    fn title(self) -> String {
        match self {
            Style::Brief => tr!("Brief"),
            Style::Standard => tr!("Standard"),
            Style::Full => tr!("Full"),
        }
    }

    /// 覚えている見せ方（なければ標準）。
    fn saved() -> Self {
        match crate::settings::string(STYLE_KEY).as_deref() {
            Some("0") => Style::Brief,
            Some("2") => Style::Full,
            _ => Style::Standard,
        }
    }

    fn save(self) {
        let n = Self::ALL.iter().position(|s| *s == self).unwrap_or(1);
        crate::settings::set_string(STYLE_KEY, &n.to_string());
    }
}

/// ref の名前のリンクを押したとき。
pub type OnRef = Rc<dyn Fn(&str)>;

/// コミット ID・ref の名前のリンクの行き先（macOS 版の `onSelectCommit`・`onSelectRef`）。
#[derive(Clone)]
struct Links {
    /// メッセージの中の短い ID を引くリポジトリ。
    repo: Rc<dyn Fn() -> Option<Arc<Repository>>>,
    commit: Rc<dyn Fn(Oid)>,
    /// ref の名前（Refs・Branches・Pushed）。なければ名前はリンクにしない。
    reference: Option<OnRef>,
}

/// リンクの URI の形（macOS 版の `CommitHeaderRenderer` と同じ）。`commit:<40 桁>` はそのコミットを選び、
/// `rev:<短い ID>` は引いてから選ぶ。`ref:<名前>` はその ref の履歴に絞り込む。
const COMMIT_SCHEME: &str = "commit:";
const REV_SCHEME: &str = "rev:";
const REF_SCHEME: &str = "ref:";

/// 押したリンクが詳細の中のもの（コミット・短い ID・ref）なら、その行き先へ。URL ならそのまま（ブラウザで開く）。
fn follow(links: &RefCell<Option<Links>>, uri: &str) -> glib::Propagation {
    let ours = [COMMIT_SCHEME, REV_SCHEME, REF_SCHEME]
        .iter()
        .any(|s| uri.starts_with(s));
    if !ours {
        return glib::Propagation::Proceed;
    }
    let Some(links) = links.borrow().clone() else {
        return glib::Propagation::Stop;
    };
    if let Some(id) = uri.strip_prefix(COMMIT_SCHEME).and_then(Oid::from_hex) {
        (links.commit)(id);
    } else if let Some(rev) = uri.strip_prefix(REV_SCHEME) {
        // メッセージの中の短い ID。コミットを引けたら選ぶ（引けなければ何もしない）。
        if let Some(repo) = (links.repo)() {
            let rev = rev.to_owned();
            glib::spawn_future_local(async move {
                let answer = crate::sheets::run(
                    &repo,
                    serde_json::json!({ "op": "resolveCommit", "rev": rev }),
                )
                .await;
                match answer["text"].as_str().and_then(Oid::from_hex) {
                    Some(id) => (links.commit)(id),
                    None => {
                        if let Some(display) = gtk::gdk::Display::default() {
                            display.beep();
                        }
                    }
                }
            });
        }
    } else if let Some(name) = uri
        .strip_prefix(REF_SCHEME)
        .and_then(|n| glib::Uri::unescape_string(n, None::<&str>))
        && let Some(f) = &links.reference
    {
        f(&name);
    }
    glib::Propagation::Stop
}

fn esc(s: &str) -> String {
    glib::markup_escape_text(s).to_string()
}

/// コミット ID のリンク（`length` 桁を見せる）。
/// 親の ID を並べたもの（`ids` は markup。等幅）。`not_fetched`（shallow clone の境界のコミット）なら、薄くして
/// 「（取得していません）」を添える。
fn parent_ids(ids: &[String], separator: &str, not_fetched: bool) -> String {
    if !not_fetched {
        return format!("<tt>{}</tt>", ids.join(separator));
    }
    format!(
        "<span alpha=\"40%\"><tt>{}</tt></span> <span alpha=\"60%\">{}</span>",
        ids.join(separator),
        esc(&tr!("(not fetched)"))
    )
}

fn commit_link(hex: &str, length: usize) -> String {
    format!(
        "<a href=\"{COMMIT_SCHEME}{}\">{}</a>",
        esc(hex),
        esc(&hex[..length.min(hex.len())])
    )
}

/// ref の名前を並べたリンク（押すと、その ref の履歴に絞り込む）。
fn ref_links(names: &[String]) -> String {
    names
        .iter()
        .map(|name| {
            format!(
                "<a href=\"{REF_SCHEME}{}\" title=\"{}\">{}</a>",
                esc(&glib::Uri::escape_string(name, None, false)),
                esc(&tr!("Show only the history of {name}", name = name)),
                esc(name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// 表の値（そのままの文字か、リンクを含むマークアップ）。
enum Cell {
    Text(String),
    Markup(String),
    /// 色を付けた文字（CSS のクラス）。
    Styled(String, &'static str),
    /// 部品そのもの（署名の印など）。
    Widget(gtk::Widget),
}

/// 2 つ選んだときの差分の取り方の切り替え（macOS 版と同じ言葉。訳の鍵）。
const PAIRS: [(PairMode, &str); 3] = [
    (PairMode::Forward, "A..B"),
    (PairMode::Reverse, "Reverse"),
    (PairMode::MergeBase, "From the merge-base (A...B)"),
];

impl Details {
    /// 作者・コミッターの名前を押したら `f(名前)` を呼ぶ（macOS 版と同じく History をその人で絞り込む）。
    pub fn connect_person(&self, f: impl Fn(&str) + 'static) {
        let f: OnRef = Rc::new(f);
        self.on_person.replace(Some(f.clone()));
        for person in [&self.author, &self.committer] {
            let f = f.clone();
            person.text.connect_activate_link(move |_, uri| {
                match uri
                    .strip_prefix("person:")
                    .map(|n| glib::Uri::unescape_string(n, None::<&str>))
                {
                    Some(Some(name)) => {
                        f(&name);
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            });
        }
    }

    pub fn new() -> Self {
        let label = |css: &[&str]| {
            let l = gtk::Label::new(None);
            l.set_xalign(0.0);
            l.set_wrap(true);
            l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            l.set_selectable(true);
            for c in css {
                l.add_css_class(c);
            }
            l
        };
        let author = Person::new(tr!("Author"));
        let committer = Person::new(tr!("Committer"));
        let people = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        people.set_homogeneous(true);
        people.append(&author.root);
        people.append(&committer.root);
        let table = gtk::Grid::builder()
            .column_spacing(10)
            .row_spacing(2)
            .build();
        let subject = label(&["title-4"]);
        let body = label(&[]);
        // 本文を等幅にするか（Settings > Appearance）。
        let monospace = |body: &gtk::Label| {
            if crate::settings::bool(crate::settings::MONOSPACED_COMMIT_MESSAGE) {
                body.add_css_class("monospace");
            } else {
                body.remove_css_class("monospace");
            }
        };
        monospace(&body);
        let weak = body.downgrade();
        crate::settings::connect_changed_while(&body, move |key| {
            if key == crate::settings::MONOSPACED_COMMIT_MESSAGE
                && let Some(body) = weak.upgrade()
            {
                monospace(&body);
            }
        });
        let trailers = label(&["dim-label"]);
        let notes = label(&[]);
        let selection_title = label(&["title-4"]);
        let selection_note = label(&["dim-label", "caption"]);
        let pairs = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let selection_lines = label(&["caption"]);
        let selection = gtk::Box::new(gtk::Orientation::Vertical, 6);
        selection.append(&selection_title);
        selection.append(&selection_note);
        selection.append(&pairs);
        selection.append(&selection_lines);
        // 上の帯: 作者（複数選んだときは選び方の見出し）と、右上のボタン。
        let tools = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        tools.set_valign(gtk::Align::Start);
        let lead = gtk::Box::new(gtk::Orientation::Vertical, 8);
        lead.set_hexpand(true);
        lead.append(&selection);
        lead.append(&people);
        // 簡潔の 1 行（作者の帯と表の代わり）。
        let brief_avatar = crate::avatars::Avatar::new(16);
        brief_avatar.root.set_valign(gtk::Align::Center);
        let brief_text = label(&["caption"]);
        brief_text.set_hexpand(true);
        let brief = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        brief.append(&brief_avatar.root);
        brief.append(&brief_text);
        brief.set_visible(false);
        lead.append(&brief);
        // 右上の見せ方の切り替え（簡潔・標準・詳細）。Push の確認のように埋め込むときは出さず、標準にする。
        let styles = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        styles.add_css_class("linked");
        styles.set_valign(gtk::Align::Start);
        let mut style_buttons: Vec<gtk::ToggleButton> = Vec::new();
        for style in Style::ALL {
            let button = gtk::ToggleButton::with_label(&style.title());
            button.add_css_class("caption");
            if let Some(first) = style_buttons.first() {
                button.set_group(Some(first));
            }
            styles.append(&button);
            style_buttons.push(button);
        }
        styles.set_visible(false);
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        head.append(&lead);
        head.append(&tools);
        head.append(&styles);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
        column.set_margin_top(10);
        column.set_margin_bottom(10);
        column.set_margin_start(12);
        column.set_margin_end(12);
        column.append(&head);
        column.append(&table);
        column.append(&subject);
        column.append(&body);
        column.append(&trailers);
        column.append(&notes);
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&column)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        // 探すのは、macOS 版のメッセージの欄と同じく見出し・表・件名・本文・notes（作者の帯は除く）。
        let searched = {
            let fixed = [selection_title.clone(), selection_note.clone()];
            let (lines, table) = (selection_lines.clone(), table.clone());
            let message = [
                subject.clone(),
                body.clone(),
                trailers.clone(),
                notes.clone(),
            ];
            move || {
                let mut labels: Vec<gtk::Label> = fixed.to_vec();
                labels.push(lines.clone());
                let mut child = table.first_child();
                while let Some(c) = child {
                    child = c.next_sibling();
                    if let Ok(label) = c.downcast::<gtk::Label>() {
                        labels.push(label);
                    }
                }
                labels.extend(message.iter().cloned());
                labels
            }
        };
        let find = crate::messagefind::MessageFind::new(&root, &scrolled, searched);
        root.append(&find.bar);
        root.append(&scrolled);
        let reword = Rc::new(Reword::new());
        root.append(&reword.root);
        let details = Self {
            root,
            find,
            column,
            people,
            author,
            committer,
            table,
            subject,
            body,
            trailers,
            notes,
            info: RefCell::default(),
            extras: Rc::default(),
            selection,
            selection_title,
            selection_note,
            pairs,
            selection_lines,
            tools,
            place: RefCell::default(),
            links: Rc::default(),
            on_person: Rc::default(),
            style: StdCell::new(Style::Standard),
            styles,
            style_buttons,
            brief,
            brief_avatar,
            brief_text,
            highlight: RefCell::default(),
            menu: gio::Menu::new(),
            reword_section: gio::Menu::new(),
            actions: gio::SimpleActionGroup::new(),
            menu_info: Rc::default(),
            on_reword: Rc::default(),
            reword,
            scrolled,
        };
        details.install_menu();
        details.install_reword();
        for label in [&details.body, &details.notes] {
            let links = details.links.clone();
            label.connect_activate_link(move |_, uri| follow(&links, uri));
        }
        // 簡潔の 1 行: 作者の名前・ref・親のリンク。
        let (links, person) = (details.links.clone(), details.on_person.clone());
        details.brief_text.connect_activate_link(move |_, uri| {
            if let Some(name) = uri
                .strip_prefix("person:")
                .and_then(|n| glib::Uri::unescape_string(n, None::<&str>))
            {
                if let Some(f) = person.borrow().clone() {
                    f(&name);
                }
                return glib::Propagation::Stop;
            }
            follow(&links, uri)
        });
        details.clear();
        details
    }

    /// 親・子・取り込んだコミットの ID とメッセージの中の短い ID を押したら `commit(ID)`（短い ID は `repo` で
    /// 引く）、ref の名前を押したら `reference(名前)` を呼ぶ（macOS 版の `onSelectCommit`・`onSelectRef`）。
    /// 呼ぶまでは、ID や名前をリンクにしない。
    pub fn connect_links(
        &self,
        repo: Rc<dyn Fn() -> Option<Arc<Repository>>>,
        commit: Rc<dyn Fn(Oid)>,
        reference: Option<OnRef>,
    ) {
        self.links.replace(Some(Links {
            repo,
            commit,
            reference,
        }));
    }

    /// 右上に見せ方の切り替え（簡潔・標準・詳細）を出し、覚えている見せ方にする（macOS 版の詳細の欄。出さなければ
    /// 標準のまま）。切り替えたら覚え、欄の高さを中身に合わせ直す。
    pub fn show_style_switch(self: &Rc<Self>) {
        self.styles.set_visible(true);
        let saved = Style::saved();
        for (style, button) in Style::ALL.into_iter().zip(&self.style_buttons) {
            button.set_active(style == saved);
            let weak = Rc::downgrade(self);
            button.connect_toggled(move |button| {
                let Some(d) = weak.upgrade() else { return };
                if !button.is_active() || d.style.get() == style {
                    return;
                }
                style.save();
                d.set_style(style);
                // 右の列の境目を、中身の高さに合わせ直す。
                if let Some(paned) = d.root.parent().and_downcast::<gtk::Paned>() {
                    crate::fit_details(&paned, d.clone());
                }
            });
        }
        self.set_style(saved);
    }

    /// 見せ方を変えて出し直す。
    fn set_style(&self, style: Style) {
        self.style.set(style);
        let info = self.info.borrow().clone();
        if let Some(info) = info {
            self.show(&info);
        }
    }

    /// 右クリックのメニューを作る（項目はいま出しているコミットのもの）。
    fn install_menu(&self) {
        let copies = gio::Menu::new();
        let info = self.menu_info.clone();
        type Pick = fn(&Value) -> String;
        let items: [(&str, String, Pick); 5] = [
            ("copy-id", tr!("Copy Commit ID"), |i| {
                str_of(i, "oid").to_owned()
            }),
            ("copy-short-id", tr!("Copy Short ID"), |i| {
                crate::titles::short(str_of(i, "oid")).to_owned()
            }),
            ("copy-subject", tr!("Copy Subject"), |i| {
                let m = str_of(i, "message").trim();
                m.split('\n').next().unwrap_or(m).to_owned()
            }),
            ("copy-message", tr!("Copy Message"), |i| {
                str_of(i, "message").trim().to_owned()
            }),
            ("copy-show", tr!("Copy git show Command"), |i| {
                format!("git show {}", str_of(i, "oid"))
            }),
        ];
        for (name, title, pick) in items {
            let action = gio::SimpleAction::new(name, None);
            let (root, info) = (self.root.downgrade(), info.clone());
            action.connect_activate(move |_, _| {
                if let (Some(root), Some(i)) = (root.upgrade(), info.borrow().as_ref()) {
                    root.clipboard().set_text(&pick(i));
                }
            });
            self.actions.add_action(&action);
            copies.append(Some(&title), Some(&format!("details.{name}")));
        }
        let reword = gio::SimpleAction::new("edit-message", None);
        let (editor, scrolled, root, i) = (
            self.reword.clone(),
            self.scrolled.downgrade(),
            self.root.downgrade(),
            info.clone(),
        );
        reword.connect_activate(move |_, _| {
            let (Some(scrolled), Some(root)) = (scrolled.upgrade(), root.upgrade()) else {
                return;
            };
            if let Some(info) = i.borrow().as_ref() {
                begin_reword(&editor, &scrolled, &root, info);
            }
        });
        self.actions.add_action(&reword);
        self.reword_section
            .append(Some(&tr!("Edit Message…")), Some("details.edit-message"));
        self.menu.append_section(None, &copies);
        self.root
            .insert_action_group("details", Some(&self.actions));
        // 文字の欄の外を右クリックしても、同じメニュー。
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let (column, menu, info) = (self.column.downgrade(), self.menu.clone(), info);
        click.connect_pressed(move |_, _, x, y| {
            let Some(column) = column.upgrade() else {
                return;
            };
            if info.borrow().is_none() {
                return;
            }
            let popover = gtk::PopoverMenu::from_model(Some(&menu));
            popover.set_has_arrow(false);
            popover.set_halign(gtk::Align::Start);
            popover.set_parent(&column);
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.connect_closed(|p| {
                let p = p.clone();
                glib::idle_add_local_once(move || p.unparent());
            });
            popover.popup();
        });
        self.column.add_controller(click);
        for label in [
            &self.subject,
            &self.body,
            &self.trailers,
            &self.notes,
            &self.brief_text,
            &self.author.text,
            &self.committer.text,
        ] {
            // キー（Menu・Shift+F10）で開く欄のメニューにも足しておく。
            label.set_extra_menu(Some(&self.menu));
            // 文字の欄の右クリック: コピーの項目と Edit Message… を先に、その後に文字のコピー（macOS 版の本文の
            // メニューの順）。GTK の欄のメニューは足した項目を後ろに置くので、自分で出す。
            let text = gio::Menu::new();
            text.append(Some(&tr!("Copy")), Some("clipboard.copy"));
            let label_menu = gio::Menu::new();
            label_menu.append_section(None, &self.menu);
            label_menu.append_section(None, &text);
            let click = gtk::GestureClick::new();
            click.set_button(gtk::gdk::BUTTON_SECONDARY);
            click.set_propagation_phase(gtk::PropagationPhase::Capture);
            let (weak, info) = (label.downgrade(), self.menu_info.clone());
            click.connect_pressed(move |gesture, _, x, y| {
                let Some(label) = weak.upgrade() else { return };
                if info.borrow().is_none() {
                    return;
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);
                let popover = gtk::PopoverMenu::from_model(Some(&label_menu));
                popover.set_has_arrow(false);
                popover.set_halign(gtk::Align::Start);
                popover.set_parent(&label);
                popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
                popover.connect_closed(|p| {
                    let p = p.clone();
                    glib::idle_add_local_once(move || p.unparent());
                });
                popover.popup();
            });
            label.add_controller(click);
        }
    }

    /// 右クリックのメニューの項目を、出しているコミットに合わせる。Edit Message… は、HEAD から辿れる merge でない
    /// コミット（時間のかかる分が届いてから分かる）のときだけ（macOS 版と同じ）。
    fn update_menu(&self) {
        let info = self.info.borrow().clone();
        self.menu_info.replace(info.clone());
        let rewordable = self.on_reword.borrow().is_some()
            && info
                .as_ref()
                .is_some_and(|i| i["parents"].as_array().is_none_or(|p| p.len() <= 1))
            && self
                .extras
                .borrow()
                .as_ref()
                .is_some_and(|e| !e["headDistance"].is_null());
        let shown = self.menu.n_items() > 1;
        if rewordable && !shown {
            self.menu.append_section(None, &self.reword_section);
        } else if !rewordable && shown {
            self.menu.remove(1);
        }
    }

    /// 書き直す欄の保存・キャンセル（Ctrl+Return・Esc）。
    fn install_reword(&self) {
        let editor = self.reword.clone();
        let save: Rc<dyn Fn()> = Rc::new({
            let (editor, scrolled, extras, on_reword) = (
                Rc::downgrade(&editor),
                self.scrolled.downgrade(),
                self.extras.clone(),
                self.on_reword.clone(),
            );
            move || {
                let (Some(editor), Some(scrolled)) = (editor.upgrade(), scrolled.upgrade()) else {
                    return;
                };
                save_reword(&editor, &scrolled, &extras, &on_reword);
            }
        });
        let s = save.clone();
        editor.save.connect_clicked(move |_| s());
        let (e, sc) = (Rc::downgrade(&editor), self.scrolled.downgrade());
        editor.cancel.connect_clicked(move |_| {
            if let (Some(e), Some(sc)) = (e.upgrade(), sc.upgrade()) {
                end_reword(&e, &sc);
            }
        });
        // 編集欄より先に受ける（Ctrl+Return で改行を入れさせない）。
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (e, sc) = (Rc::downgrade(&editor), self.scrolled.downgrade());
        keys.connect_key_pressed(move |_, key, _, state| {
            let (Some(e), Some(sc)) = (e.upgrade(), sc.upgrade()) else {
                return glib::Propagation::Proceed;
            };
            match key {
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter
                    if state.contains(gtk::gdk::ModifierType::CONTROL_MASK) =>
                {
                    save();
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Escape => {
                    end_reword(&e, &sc);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        editor.root.add_controller(keys);
    }

    /// 書き直しをやめる（ほかのコミットを選んだときも。macOS 版と同じ）。
    fn end_reword(&self) {
        end_reword(&self.reword, &self.scrolled);
    }

    /// 右クリックのメニューの Edit Message… で、メッセージをその場で書き直す。保存したら
    /// `f(コミット ID, メッセージ)` を呼ぶ（呼ぶまでは Edit Message… を出さない）。
    pub fn connect_edit_message(&self, f: impl Fn(&str, &str) + 'static) {
        self.on_reword.replace(Some(Rc::new(f)));
        self.update_menu();
    }

    /// History の検索の問い合わせ（消えたら None）。件名・本文・トレーラー・notes の中の語に色を付ける。
    pub fn set_search(&self, query: Option<&str>) {
        let terms = highlight_terms(query);
        if *self.highlight.borrow() == terms {
            return;
        }
        self.highlight.replace(terms);
        self.apply_highlight();
    }

    /// 件名・本文・トレーラー・notes で、検索の語に背景の色を付ける（大文字・小文字は区別しない。macOS 版の
    /// `highlightTerms`）。
    fn apply_highlight(&self) {
        let terms = self.highlight.borrow();
        for label in [&self.subject, &self.body, &self.trailers, &self.notes] {
            let attrs = gtk::pango::AttrList::new();
            let text = label.text();
            for term in terms.iter() {
                for (start, end) in find_ignoring_case(&text, term) {
                    let mut color = gtk::pango::AttrColor::new_background(65535, 52428, 0);
                    color.set_start_index(start as u32);
                    color.set_end_index(end as u32);
                    attrs.insert(color);
                    let mut alpha = gtk::pango::AttrInt::new_background_alpha(23000);
                    alpha.set_start_index(start as u32);
                    alpha.set_end_index(end as u32);
                    attrs.insert(alpha);
                }
            }
            label.set_attributes(Some(&attrs));
        }
    }

    /// 出しているコミットの ID（1 つのコミットを出していなければ None）。
    pub fn oid(&self) -> Option<String> {
        self.info
            .borrow()
            .as_ref()
            .map(|i| str_of(i, "oid").to_owned())
    }

    /// 出しているコミットの最初の親（`parent`）か最初の子（Navigate > Go to Parent・Go to Child。macOS 版の
    /// `currentParents`・`currentChildren`）。子は時間のかかる分が届いてから。
    pub fn relative(&self, parent: bool) -> Option<Oid> {
        let list = if parent {
            // shallow clone の境界のコミットの親は取得していないので、移れない。
            let info = self.info.borrow();
            let info = info.as_ref()?;
            if info["parentsNotFetched"].as_bool() == Some(true) {
                return None;
            }
            strings(&info["parents"])
        } else {
            self.info.borrow().as_ref()?;
            strings(&self.extras.borrow().as_ref()?["children"])
        };
        list.first().and_then(|h| Oid::from_hex(h))
    }

    /// 開いたリポジトリの場所（署名を検証した場所を、Signature の吹き出しに出すため）。
    pub fn set_location(&self, location: &str) {
        self.place.replace(crate::signature::Place::of(location));
    }

    pub fn clear(&self) {
        self.end_reword();
        self.set_selection_mode(false);
        self.info.replace(None);
        self.extras.replace(None);
        self.author.root.set_visible(false);
        self.committer.root.set_visible(false);
        self.brief.set_visible(false);
        self.clear_table();
        self.subject.set_text("");
        for l in [&self.body, &self.trailers, &self.notes] {
            l.set_text("");
            l.set_visible(false);
        }
        self.apply_highlight();
        self.update_menu();
        self.find.refresh();
    }

    /// コミットではないもの（Push のシートの「送る変更すべて」など）: 件名と本文だけを出す。
    pub fn show_text(&self, subject: &str, body: &str) {
        self.clear();
        self.subject.set_text(subject);
        self.body.set_text(body);
        self.body.set_visible(!body.is_empty());
        self.find.refresh();
    }

    fn clear_table(&self) {
        while let Some(child) = self.table.first_child() {
            self.table.remove(&child);
        }
    }

    /// 複数選んだときの見出しか、1 つのコミットの詳細か。
    fn set_selection_mode(&self, on: bool) {
        self.selection.set_visible(on);
        // 簡潔では、作者の帯の代わりに 1 行（[`Details::show_brief`]）。
        self.people
            .set_visible(!on && self.style.get() != Style::Brief);
        self.table.set_visible(!on);
        self.subject.set_visible(!on);
    }

    /// 複数のコミットを選んだときの見出し（macOS 版と同じ）: 数、差分の取り方の説明、2 つなら取り方の
    /// 切り替え、各コミットの 1 行（短い ID・件名・作者）。`kind` は `range`・`pair`・`each`（読む前は None）。
    pub fn show_selection(
        &self,
        count: usize,
        kind: Option<&str>,
        pair: PairMode,
        lines: &[(String, String, String)],
        on_pair: Rc<dyn Fn(PairMode)>,
    ) {
        self.clear();
        self.set_selection_mode(true);
        self.selection_title
            .set_text(&tr!("{count} commits selected", count = count));
        let note = match (kind, pair) {
            (Some("range"), _) => tr!(
                "They are consecutive, so this is the diff from the parent of the oldest to the newest (the same as squashing them into one)."
            ),
            (Some("pair"), PairMode::Forward) => tr!(
                "Two non-consecutive commits, so this is the diff from the older to the newer (A..B)."
            ),
            (Some("pair"), PairMode::Reverse) => tr!(
                "The diff of the two non-consecutive commits from the newer to the older (reversed)."
            ),
            (Some("pair"), PairMode::MergeBase) => tr!(
                "The diff of the two non-consecutive commits from their common ancestor to the newer (A...B: only the changes on the newer side)."
            ),
            (Some(_), _) => tr!(
                "They are not contiguous, so each commit’s diff is listed file by file (headed by the path and short ID)."
            ),
            (None, _) => tr!("Loading the diff…"),
        };
        self.selection_note.set_text(&note);
        while let Some(child) = self.pairs.first_child() {
            self.pairs.remove(&child);
        }
        self.pairs.set_visible(kind == Some("pair"));
        if kind == Some("pair") {
            // 取り方の切り替えは、説明の下の文中のリンク（macOS 版と同じ。押すと読み直す）。
            let modes: Vec<(usize, PairMode, &str)> = PAIRS
                .into_iter()
                .enumerate()
                .filter(|(_, (mode, _))| *mode != pair)
                .map(|(i, (mode, title))| (i, mode, title))
                .collect();
            let markup: Vec<String> = modes
                .iter()
                .map(|(i, _, title)| {
                    format!(
                        "<a href=\"pair:{i}\">{}</a>",
                        glib::markup_escape_text(&amonhen_model::i18n::translate(title))
                    )
                })
                .collect();
            let links = gtk::Label::new(None);
            links.set_markup(&markup.join("　"));
            links.set_xalign(0.0);
            links.add_css_class("caption");
            let f = on_pair.clone();
            links.connect_activate_link(move |_, uri| {
                if let Some(&(_, mode, _)) = uri
                    .strip_prefix("pair:")
                    .and_then(|i| i.parse::<usize>().ok())
                    .and_then(|i| modes.iter().find(|(n, _, _)| *n == i))
                {
                    f(mode);
                }
                glib::Propagation::Stop
            });
            self.pairs.append(&links);
        }
        let text: Vec<String> = lines
            .iter()
            .map(|(id, subject, author)| format!("{id}  {subject}  — {author}"))
            .collect();
        self.selection_lines.set_text(&text.join("\n"));
        self.find.refresh();
    }

    /// すぐ読める分を出す（時間のかかる分は、届いたら `show_extras`）。
    pub fn show(&self, info: &Value) {
        self.info.replace(Some(info.clone()));
        let message = info["message"].as_str().unwrap_or_default();
        let subject = message.trim().split('\n').next().unwrap_or_default();
        self.subject.set_text(subject.trim_end());
        let brief = self.style.get() == Style::Brief;
        // URL はリンクにする（`#123` は、リモートの場所を読んでから）。簡潔では件名だけ（macOS 版と同じ）。
        self.fill_message(info, None);
        self.people.set_visible(!brief);

        // 上の帯: 作者と（違う人なら）コミッター。
        // mailmap で置き換えたなら、記録の名前とメールを下に添える。
        let recorded = |key: &str| {
            info[key].is_object().then(|| {
                tr!(
                    "Recorded: {identity}",
                    identity = format!(
                        "{} <{}>",
                        str_of(&info[key], "name"),
                        str_of(&info[key], "email")
                    )
                )
            })
        };
        let when = |time: &str, tz: &str| {
            let d = date(info[time].as_i64().unwrap_or_default());
            match str_of(info, tz) {
                "" => d,
                tz => format!("{d} ({tz})"),
            }
        };
        self.author.show(
            str_of(info, "author"),
            str_of(info, "authorEmail"),
            &when("authorTime", "authorTz"),
            recorded("recordedAuthor"),
        );
        // 作者とコミッターが同じ人（名前とメールが同じ、記録も同じ）なら、作者だけを出す。
        let same = info["committer"] == info["author"]
            && str_of(info, "committerEmail").to_lowercase()
                == str_of(info, "authorEmail").to_lowercase()
            && info["recordedCommitter"] == info["recordedAuthor"];
        if same {
            self.committer.root.set_visible(false);
        } else {
            self.committer.show(
                str_of(info, "committer"),
                str_of(info, "committerEmail"),
                &when("committerTime", "committerTz"),
                recorded("recordedCommitter"),
            );
        }
        self.people.set_homogeneous(!same);
        let extras = self.extras.borrow().clone();
        match extras {
            Some(extras) => self.show_extras(&extras),
            None => self.build_table(None),
        }
        self.apply_highlight();
        self.update_menu();
        self.find.refresh();
    }

    /// 時間のかかる分（`commit_extras` の答え）を足す。`info` と同じコミットのものであること。
    pub fn show_extras(&self, extras: &Value) {
        self.extras.replace(Some(extras.clone()));
        if self.info.borrow().is_none() {
            return;
        }
        self.build_table(Some(extras));
        // 本文の `#123` を、リモートのリポジトリの issue・pull request へのリンクにする（macOS 版と同じ）。
        let info = self.info.borrow().clone();
        if let Some(info) = info {
            self.fill_message(&info, Some(extras));
        }
        self.apply_highlight();
        self.update_menu();
        self.find.refresh();
    }

    /// 本文（トレーラーは除く）・トレーラー・notes を出す（macOS 版の `renderMessage`）。簡潔では出さない。
    /// `extras` が届いていれば、`#123` をリモートの issue へのリンクにし、notes を「Notes:」の見出しの下に出す。
    fn fill_message(&self, info: &Value, extras: Option<&Value>) {
        let brief = self.style.get() == Style::Brief;
        let revs = self.links.borrow().is_some();
        let web = extras.and_then(|e| web_url(str_of(e, "originUrl")));
        let trailers: Vec<(&str, &str)> = info["trailers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| (str_of(t, "key"), str_of(t, "value")))
            .collect();
        let body = body_without_trailers(str_of(info, "message"), !trailers.is_empty());
        self.body.set_markup(&linked(&body, web.as_deref(), revs));
        self.body.set_visible(!body.is_empty() && !brief);
        let lines: Vec<String> = trailers
            .iter()
            .map(|(key, value)| {
                format!(
                    "<span weight=\"medium\">{}: </span>{}",
                    esc(key),
                    linked(value, None, false)
                )
            })
            .collect();
        self.trailers.set_markup(&lines.join("\n"));
        self.trailers.set_visible(!lines.is_empty() && !brief);
        let notes = extras.map_or("", |e| str_of(e, "notes")).trim();
        self.notes.set_markup(&format!(
            "<span size=\"small\" weight=\"bold\" alpha=\"60%\">Notes:</span>\n{}",
            linked(notes, web.as_deref(), revs)
        ));
        self.notes.set_visible(!notes.is_empty() && !brief);
    }

    /// 簡潔の 1 行: 小さなアバター・作者（押すとその人のコミットで絞り込む）・作者の日時・ref・親（macOS 版の
    /// `renderCompact`）。
    fn show_brief(&self, info: &Value) {
        let (commits, names) = {
            let links = self.links.borrow();
            (
                links.is_some(),
                links.as_ref().is_some_and(|l| l.reference.is_some()),
            )
        };
        let author = str_of(info, "author");
        self.brief_avatar.show(author, str_of(info, "authorEmail"));
        let mut markup = format!(
            "<a href=\"person:{}\" title=\"{}\"><b>{}</b></a>  <span alpha=\"60%\">{}</span>",
            esc(&glib::Uri::escape_string(author, None, false)),
            esc(&tr!("Show only commits by {name}", name = author)),
            esc(author),
            esc(&date(info["authorTime"].as_i64().unwrap_or_default()))
        );
        let refs = strings(&info["refs"]);
        if !refs.is_empty() {
            markup.push_str("  ");
            markup.push_str(&if names {
                format!("<b>{}</b>", ref_links(&refs))
            } else {
                format!("<b>{}</b>", esc(&refs.join(", ")))
            });
        }
        let parents = strings(&info["parents"]);
        if !parents.is_empty() {
            let title = if parents.len() > 1 {
                "Parents"
            } else {
                "Parent"
            };
            let not_fetched = info["parentsNotFetched"].as_bool() == Some(true);
            let ids: Vec<String> = parents
                .iter()
                .map(|id| {
                    if commits && !not_fetched {
                        commit_link(id, 12)
                    } else {
                        esc(&id[..12.min(id.len())])
                    }
                })
                .collect();
            markup.push_str(&format!(
                "  <span alpha=\"60%\">{title}</span> {}",
                parent_ids(&ids, ", ", not_fetched)
            ));
        }
        self.brief_text.set_markup(&markup);
        self.brief.set_visible(true);
    }

    fn build_table(&self, extras: Option<&Value>) {
        self.clear_table();
        let info = self.info.borrow();
        let Some(info) = info.as_ref() else { return };
        let style = self.style.get();
        self.brief.set_visible(false);
        if style == Style::Brief {
            self.show_brief(info);
            return;
        }
        // 詳細だけの行（標準は、ID・親・署名・ref・タグ・取り込んだ数に絞る。macOS 版の `renderStandard`）。
        let full = style == Style::Full;
        let (commits, names) = {
            let links = self.links.borrow();
            (
                links.is_some(),
                links.as_ref().is_some_and(|l| l.reference.is_some()),
            )
        };
        // 40 桁の ID を 1 行ずつ（押すとそのコミットへ移る）。
        let ids = |ids: &[String]| {
            if commits {
                Cell::Markup(
                    ids.iter()
                        .map(|id| commit_link(id, 40))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            } else {
                Cell::Text(ids.join("\n"))
            }
        };
        // ref の名前を並べる（押すと、その ref の履歴に絞り込む）。
        let refs_cell = |list: &[String]| {
            if names {
                Cell::Markup(ref_links(list))
            } else {
                Cell::Text(list.join(", "))
            }
        };
        let mut rows: Vec<(String, Cell)> =
            vec![("Commit".into(), Cell::Text(str_of(info, "oid").into()))];
        let parents = strings(&info["parents"]);
        if !parents.is_empty() {
            let title = if parents.len() > 1 {
                "Parents"
            } else {
                "Parent"
            };
            // shallow clone の境界のコミットの親は取得していない（History で選べない）ので、リンクにせずに薄く出し、
            // そう添える（macOS 版の `parentLinks`）。
            if info["parentsNotFetched"].as_bool() == Some(true) {
                let plain: Vec<String> = parents.iter().map(|id| esc(id)).collect();
                rows.push((title.into(), Cell::Markup(parent_ids(&plain, "\n", true))));
            } else {
                rows.push((title.into(), ids(&parents)));
            }
        }
        if let Some(extras) = extras.filter(|_| full) {
            let children = strings(&extras["children"]);
            if !children.is_empty() {
                let title = if children.len() > 1 {
                    "Children"
                } else {
                    "Child"
                };
                rows.push((title.into(), ids(&children)));
            }
            if let Some(rev) = extras["headRev"].as_str().filter(|r| !r.is_empty()) {
                rows.push(("Rev".into(), Cell::Text(rev.into())));
            }
        }
        // 署名（押すと吹き出しで詳細）。
        if let Some(status) = commit_signature_status(info, extras) {
            let check = extras
                .map(|e| e["signature"].clone())
                .filter(|s| !s.is_null());
            rows.push((
                "Signature".into(),
                Cell::Widget(crate::signature::row(
                    status,
                    check,
                    info["signed"].as_str().unwrap_or("gpg").to_owned(),
                    recorded_committer_email(info).to_owned(),
                    self.place.borrow().clone(),
                    None,
                )),
            ));
        }
        let refs = strings(&info["refs"]);
        if !refs.is_empty() {
            rows.push(("Refs".into(), refs_cell(&refs)));
        }
        // このコミットを指すタグ: 名前・タガーと日時・件名と署名の印（軽量タグは名前だけ）。時間のかかる分で届く。
        if let Some(extras) = extras {
            for tag in extras["tagObjects"].as_array().into_iter().flatten() {
                let mut text = format!("<b>{}</b>", esc(str_of(tag, "name")));
                if tag["id"].is_null() {
                    text.push_str(&dim(&tr!(" (lightweight tag)")));
                    rows.push(("Tag".into(), Cell::Markup(text)));
                    continue;
                }
                let mut who = vec![str_of(tag, "tagger").to_owned()];
                if let Some(t) = tag["time"].as_i64() {
                    who.push(date(t));
                }
                let who: Vec<String> = who.into_iter().filter(|w| !w.is_empty()).collect();
                if !who.is_empty() {
                    text.push_str(&dim(&format!(" ({})", who.join(" · "))));
                }
                if !str_of(tag, "subject").is_empty() {
                    text.push_str(&dim(&format!(" — {}", str_of(tag, "subject"))));
                }
                let check = (!tag["signature"].is_null()).then(|| tag["signature"].clone());
                let mark = tag_signature_status(tag["signed"].as_str(), check.as_ref(), true, tag)
                    .map(|status| self.tag_mark(status, check, tag));
                rows.push(("Tag".into(), tag_cell(&text, mark)));
            }
        }
        if let Some(extras) = extras.filter(|_| full) {
            let branches = strings(&extras["branches"]);
            if !branches.is_empty() {
                rows.push(("Branches".into(), refs_cell(&branches)));
            }
            let count = extras["tagCount"].as_u64().unwrap_or(0) as usize;
            if count > 0 {
                let tags = strings(&extras["tags"]);
                let shown = tags.iter().take(12).cloned().collect::<Vec<_>>().join(", ");
                let more = if count > 12 {
                    format!(" {}", tr!("and {count} more tags", count = count - 12))
                } else {
                    String::new()
                };
                rows.push(("Tags".into(), Cell::Text(shown + &more)));
            }
            if let Some(previous) = extras["previousTag"].as_str() {
                rows.push(("Previous Tag".into(), Cell::Text(previous.into())));
            }
            if !extras["originUrl"].is_null() {
                let remote = strings(&extras["remoteBranches"]);
                // どのリモートブランチにも含まれなければ、橙で「未 push」（macOS 版と同じ）。
                let pushed = if remote.is_empty() {
                    Cell::Styled(tr!("Not pushed (not on any remote branch)"), "warning")
                } else {
                    refs_cell(&remote)
                };
                rows.push(("Pushed".into(), pushed));
            }
        }
        // merge したタグ（merge コミットに埋め込まれたタグ）と、その署名の印。
        for (i, tag) in info["mergedTags"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let mut text = esc(str_of(tag, "name"));
            if !str_of(tag, "tagger").is_empty() {
                text.push_str(&dim(&format!(" ({})", str_of(tag, "tagger"))));
            }
            if !str_of(tag, "subject").is_empty() {
                text.push_str(&dim(&format!(" — {}", str_of(tag, "subject"))));
            }
            let check = extras
                .map(|e| e["mergedTagSignatures"][i].clone())
                .filter(|s| !s.is_null());
            let mark = tag_signature_status(
                tag["signed"].as_str(),
                check.as_ref(),
                extras.is_some(),
                tag,
            )
            .map(|status| self.tag_mark(status, check, tag));
            rows.push(("Merged Tag".into(), tag_cell(&text, mark)));
        }
        if let Some(extras) = extras {
            let merged = extras["mergedCount"].as_u64().unwrap_or(0);
            if merged > 0 {
                // 10 桁の ID（押すとそのコミットへ）・件名・作者（macOS 版の `summaryLine`）。
                let mut text = esc(&tr!("{count} commits", count = merged));
                // 標準は数だけ。
                let listed = if full { 20 } else { 0 };
                for commit in extras["merged"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(listed)
                {
                    text.push('\n');
                    text.push_str(&summary_line(commit, commits));
                }
                if full && merged > 20 {
                    text.push('\n');
                    text.push_str(&dim(&tr!("and {count} more", count = merged - 20)));
                }
                rows.push(("Merged".into(), Cell::Markup(text)));
            }
            // メッセージに書かれたコミット（cherry-pick 元・revert 元・Fixes・そのほか。書かれた順）。
            if full {
                for (kind, commit) in
                    related_commits(str_of(info, "message"), &extras["references"])
                {
                    let text = format!("{}  {}", dim(&kind), summary_line(&commit, commits));
                    rows.push(("Related".into(), Cell::Markup(text)));
                }
            }
        }
        for (i, (title, value)) in rows.into_iter().enumerate() {
            let head = gtk::Label::new(Some(&title));
            head.set_xalign(1.0);
            head.set_yalign(0.0);
            head.add_css_class("dim-label");
            head.add_css_class("caption");
            self.table.attach(&head, 0, i as i32, 1, 1);
            let cell = match value {
                Cell::Widget(widget) => {
                    self.table.attach(&widget, 1, i as i32, 1, 1);
                    continue;
                }
                Cell::Text(t) => self.cell_label(&t, false),
                Cell::Styled(t, css) => {
                    let cell = self.cell_label(&t, false);
                    cell.add_css_class(css);
                    cell
                }
                Cell::Markup(m) => self.cell_label(&m, true),
            };
            if title == "Commit" || title.starts_with("Parent") || title.starts_with("Child") {
                cell.add_css_class("monospace");
            }
            self.table.attach(&cell, 1, i as i32, 1, 1);
        }
    }

    /// 表の値の欄（`markup` なら、リンクを詳細の中の行き先へ）。
    fn cell_label(&self, text: &str, markup: bool) -> gtk::Label {
        let cell = gtk::Label::new(None);
        if markup {
            cell.set_markup(text);
            let links = self.links.clone();
            cell.connect_activate_link(move |_, uri| follow(&links, uri));
        } else {
            cell.set_text(text);
        }
        cell.set_xalign(0.0);
        cell.set_wrap(true);
        cell.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        cell.set_selectable(true);
        cell.set_hexpand(true);
        cell.add_css_class("caption");
        cell.set_extra_menu(Some(&self.menu));
        cell
    }

    /// タグの署名の印（押すと、タグの名前とメッセージの入った吹き出し。タガーのメールと比べる）。
    fn tag_mark(
        &self,
        status: crate::signature::Status,
        check: Option<Value>,
        tag: &Value,
    ) -> gtk::Widget {
        crate::signature::row(
            status,
            check,
            tag["signed"].as_str().unwrap_or("gpg").to_owned(),
            str_of(tag, "taggerEmail").to_owned(),
            self.place.borrow().clone(),
            Some(crate::signature::Tag {
                name: str_of(tag, "name").to_owned(),
                message: str_of(tag, "message").to_owned(),
            }),
        )
    }

    /// 出しているメッセージ（複数のコミットを選んでいれば、各コミットの 1 行の一覧）。
    pub fn message(&self) -> String {
        match self.info.borrow().as_ref() {
            Some(info) => str_of(info, "message").to_owned(),
            None if self.selection.is_visible() => self.selection_lines.text().into(),
            None => {
                let (subject, body) = (self.subject.text(), self.body.text());
                if body.is_empty() {
                    subject.into()
                } else {
                    format!("{subject}\n\n{body}")
                }
            }
        }
    }

    /// 幅 `width` で、中身をすべて見せるのに要る高さ。
    pub fn natural_height(&self, width: i32) -> i32 {
        let (_, natural, _, _) = self.column.measure(gtk::Orientation::Vertical, width);
        natural
    }
}

/// メッセージの欄を書き直す欄に替える。詳細が低い（簡潔の）ときは、書き直す間だけ広げる（macOS 版と同じ）。
fn begin_reword(editor: &Reword, scrolled: &gtk::ScrolledWindow, root: &gtk::Box, info: &Value) {
    if editor.editing.borrow().is_some() {
        return;
    }
    let (oid, message) = (str_of(info, "oid"), str_of(info, "message"));
    editor
        .editing
        .replace(Some((oid.to_owned(), message.to_owned())));
    editor.text.buffer().set_text(message.trim());
    if let Some(paned) = root
        .ancestor(gtk::Paned::static_type())
        .and_downcast::<gtk::Paned>()
        && paned
            .start_child()
            .is_some_and(|c| c == *root.upcast_ref::<gtk::Widget>() || root.is_ancestor(&c))
        && paned.position() < 160
    {
        paned.set_position(220);
    }
    scrolled.set_visible(false);
    editor.root.set_visible(true);
    editor.text.grab_focus();
}

fn end_reword(editor: &Reword, scrolled: &gtk::ScrolledWindow) {
    if editor.editing.take().is_none() {
        return;
    }
    editor.root.set_visible(false);
    scrolled.set_visible(true);
}

/// 保存: 変えていなければ閉じるだけ。リモートブランチに含まれていれば、後の push に Force Push が要ることを
/// 確かめてから書き直す（macOS 版と同じ）。
fn save_reword(
    editor: &Rc<Reword>,
    scrolled: &gtk::ScrolledWindow,
    extras: &Rc<RefCell<Option<Value>>>,
    on_reword: &Rc<RefCell<Option<OnReword>>>,
) {
    let Some((oid, original)) = editor.editing.borrow().clone() else {
        return;
    };
    let buffer = editor.text.buffer();
    let edited = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
    let message = match reworded(&original, &edited) {
        None => {
            editor.root.display().beep();
            return;
        }
        Some(None) => return end_reword(editor, scrolled),
        Some(Some(message)) => message,
    };
    let Some(f) = on_reword.borrow().clone() else {
        return;
    };
    let remotes = extras
        .borrow()
        .as_ref()
        .map(|e| strings(&e["remoteBranches"]))
        .unwrap_or_default();
    if remotes.is_empty() {
        end_reword(editor, scrolled);
        f(&oid, &message);
        return;
    }
    let parent = editor.root.root().and_downcast::<gtk::Window>();
    let (editor, scrolled) = (editor.clone(), scrolled.clone());
    glib::spawn_future_local(async move {
        let sheet = crate::sheets::Sheet::unnamed(
            parent.as_ref(),
            &tr!("Edit a pushed commit?"),
            &tr!(
                "It is in {branches}, so pushing afterwards needs Force Push.",
                branches = remotes.join(", ")
            ),
        );
        sheet.buttons(&tr!("Edit Message"), false, None);
        if sheet.run().await != crate::sheets::Response::Ok {
            return;
        }
        // 確かめている間に、ほかのコミットを選んでいたら書き直さない。
        if editor.editing.borrow().as_ref().map(|(o, _)| o.as_str()) != Some(oid.as_str()) {
            return;
        }
        end_reword(&editor, &scrolled);
        f(&oid, &message);
    });
}

/// コミットの署名の状態（macOS 版の `signatureStatus`）。署名がなければ None。時間のかかる分が届くまでは
/// Checking…、届いて検証の結果がなければ No key。
fn commit_signature_status(
    info: &Value,
    extras: Option<&Value>,
) -> Option<crate::signature::Status> {
    use crate::signature::Status;
    let check = extras.map(|e| &e["signature"]).filter(|s| !s.is_null());
    let kind = info["signed"].as_str();
    if kind.is_none() && check.is_none() {
        return None;
    }
    Some(match (extras, check) {
        (None, _) => Status::Pending,
        (Some(_), None) => Status::NoKey,
        (Some(_), Some(_)) => Status::new(check, recorded_committer_email(info), kind),
    })
}

/// タグの署名の状態（macOS 版の `tagSignatureStatus`。タガーのメールと比べる）。署名がなければ None。
fn tag_signature_status(
    signed: Option<&str>,
    check: Option<&Value>,
    loaded: bool,
    tag: &Value,
) -> Option<crate::signature::Status> {
    use crate::signature::Status;
    signed?;
    Some(match (loaded, check) {
        (false, _) => Status::Pending,
        (true, None) => Status::NoKey,
        (true, Some(_)) => Status::new(check, str_of(tag, "taggerEmail"), signed),
    })
}

/// コミットに記録されたコミッターのメール（署名と比べるのはこれ。mailmap で置き換えたものではない。macOS 版の
/// `recordedCommitterEmail`）。
fn recorded_committer_email(info: &Value) -> &str {
    info["recordedCommitter"]["email"]
        .as_str()
        .unwrap_or_else(|| str_of(info, "committerEmail"))
}

/// 薄くした文字のマークアップ。
fn dim(text: &str) -> String {
    format!("<span alpha=\"60%\">{}</span>", esc(text))
}

/// タグの行: 名前などのマークアップと、署名の印（あれば右に並べる）。
fn tag_cell(markup: &str, mark: Option<gtk::Widget>) -> Cell {
    let Some(mark) = mark else {
        return Cell::Markup(markup.to_owned());
    };
    let label = gtk::Label::new(None);
    label.set_markup(markup);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.add_css_class("caption");
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.set_hexpand(true);
    line.append(&label);
    line.append(&mark);
    Cell::Widget(line.upcast())
}

/// 取り込んだ・メッセージに書かれたコミットの 1 行: 10 桁の ID（`link` なら押すとそのコミットへ）・件名・作者
/// （macOS 版の `summaryLine`）。
fn summary_line(commit: &Value, link: bool) -> String {
    let oid = str_of(commit, "oid");
    let id = if link {
        commit_link(oid, 10)
    } else {
        esc(&oid[..10.min(oid.len())])
    };
    let mut text = format!("<tt>{id}</tt>  {}", esc(str_of(commit, "subject")));
    if let Some(author) = commit["author"].as_str() {
        text.push_str(&dim(&format!(" — {author}")));
    }
    text
}

/// メッセージに書かれたコミット（`references`: 書かれた ID → コミットの 1 行）を、書かれた順に、書かれ方と一緒に
/// （macOS 版の `relatedCommits`）。
fn related_commits(message: &str, references: &Value) -> Vec<(String, Value)> {
    let Some(references) = references.as_object() else {
        return Vec::new();
    };
    let mut found: Vec<(usize, String, Value)> = references
        .iter()
        .filter_map(|(token, commit)| {
            let at = message.find(token.as_str())?;
            let start = message[..at].rfind('\n').map_or(0, |i| i + 1);
            let end = message[at..].find('\n').map_or(message.len(), |i| at + i);
            let line = message[start..end].to_lowercase();
            let kind = if line.contains("cherry picked from") {
                tr!("Cherry-picked from")
            } else if line.contains("reverts commit") {
                tr!("Reverts")
            } else if line.starts_with("fixes:") {
                tr!("Fixes")
            } else {
                tr!("Mentions")
            };
            Some((at, kind, commit.clone()))
        })
        .collect();
    found.sort_by_key(|(at, _, _)| *at);
    found.into_iter().map(|(_, k, c)| (k, c)).collect()
}

/// `git@github.com:a/b.git`・`ssh://git@host/a/b`・`https://host/a/b.git` を、ブラウザで開ける URL にする（macOS 版の
/// `CommitHeaderRenderer.webURL`）。
fn web_url(remote: &str) -> Option<String> {
    let mut s = remote.trim();
    if s.is_empty() {
        return None;
    }
    s = s.strip_suffix(".git").unwrap_or(s);
    if s.starts_with("http://") || s.starts_with("https://") {
        return Some(s.to_owned());
    }
    let host_of = |h: &str| {
        let h = h.rsplit('@').next().unwrap_or(h);
        h.split(':').next().unwrap_or(h).to_owned()
    };
    if let Some(rest) = s.strip_prefix("ssh://") {
        let (host, path) = rest.split_at(rest.find('/')?);
        return Some(format!("https://{}{path}", host_of(host)));
    }
    // scp の形（user@host:path）。
    if s.contains("://") {
        return None;
    }
    let (host, path) = s.split_once(':')?;
    Some(format!(
        "https://{}/{}",
        host_of(host),
        path.trim_matches('/')
    ))
}

/// 本文を、URL と（`web` があれば）`#123` をリンクにした Pango のマークアップにする。`#123` は `web` の issue
/// （GitLab なら `-/issues`）へ。GitHub は pull request の番号でも開ける。`revs` なら 7〜40 桁の 16 進（前後が
/// 英数字でないもの）もコミット ID とみなしてリンクにする（押すと引いてから選ぶ。macOS 版と同じ）。
fn linked(text: &str, web: Option<&str>, revs: bool) -> String {
    let e = |s: &str| glib::markup_escape_text(s).to_string();
    let link = |href: &str, label: &str| format!("<a href=\"{}\">{}</a>", e(href), e(label));
    let issues = web.map(|w| {
        let gitlab = w.to_lowercase().contains("gitlab");
        format!("{w}/{}", if gitlab { "-/issues" } else { "issues" })
    });
    let bytes = text.as_bytes();
    let mut out = String::new();
    let (mut i, mut plain) = (0, 0);
    while i < text.len() {
        let rest = &text[i..];
        // URL（空白まで。末尾の句読点と閉じ括弧は含めない）。
        if rest.starts_with("https://") || rest.starts_with("http://") {
            let mut end = rest
                .find(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"')
                .unwrap_or(rest.len());
            while end > 0 && rest[..end].ends_with(['.', ',', ';', ':', '!', '?', ')', '\'']) {
                end -= 1;
            }
            out.push_str(&e(&text[plain..i]));
            out.push_str(&link(&rest[..end], &rest[..end]));
            i += end;
            plain = i;
            continue;
        }
        // `#123`（前が英数字や `&` でなく、後ろが単語の終わり）。
        if let Some(issues) = &issues
            && bytes[i] == b'#'
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'&'))
        {
            let digits = rest[1..]
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len() - 1);
            let after = rest[1 + digits..].chars().next();
            if digits > 0 && !after.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                let number = &rest[1..1 + digits];
                out.push_str(&e(&text[plain..i]));
                out.push_str(&link(&format!("{issues}/{number}"), &rest[..1 + digits]));
                i += 1 + digits;
                plain = i;
                continue;
            }
        }
        // 7〜40 桁の 16 進（前が英数字や `/` でなく、後ろが英数字でない）。
        if revs
            && matches!(bytes[i], b'0'..=b'9' | b'a'..=b'f')
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'/'))
        {
            let digits = rest
                .find(|c: char| !matches!(c, '0'..='9' | 'a'..='f'))
                .unwrap_or(rest.len());
            let after = rest[digits..].chars().next();
            if (7..=40).contains(&digits) && !after.is_some_and(|c| c.is_ascii_alphanumeric()) {
                out.push_str(&e(&text[plain..i]));
                out.push_str(&link(
                    &format!("{REV_SCHEME}{}", &rest[..digits]),
                    &rest[..digits],
                ));
                i += digits;
                plain = i;
                continue;
            }
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    out.push_str(&e(&text[plain..]));
    out
}

/// History の検索の問い合わせから、件名と本文で強調する語を取り出す（`author:`・`path:` などは除く。macOS 版の
/// `highlightTerms(of:)`）。
fn highlight_terms(query: Option<&str>) -> Vec<String> {
    let Some(query) = query else {
        return Vec::new();
    };
    let mut terms = Vec::new();
    let mut flush = |word: &mut String| {
        let w = std::mem::take(word);
        let lower = w.to_lowercase();
        if w.is_empty() || lower == "or" || lower == "and" {
            return;
        }
        match w.split_once(':') {
            Some((key, value)) => {
                let key = key.to_lowercase();
                if matches!(key.as_str(), "subject" | "message") && !value.is_empty() {
                    terms.push(value.to_owned());
                }
                if !matches!(
                    key.as_str(),
                    "author"
                        | "commit"
                        | "id"
                        | "path"
                        | "file"
                        | "contents"
                        | "content"
                        | "subject"
                        | "message"
                ) {
                    terms.push(w);
                }
            }
            None => terms.push(w),
        }
    };
    let (mut word, mut quoted) = (String::new(), false);
    for c in query.chars() {
        if c == '"' {
            quoted = !quoted;
        } else if !quoted && (c.is_whitespace() || "()|".contains(c)) {
            flush(&mut word);
        } else {
            word.push(c);
        }
    }
    flush(&mut word);
    terms
}

/// `text` の中の `term`（大文字・小文字は区別しない）の位置（バイトの範囲）。
fn find_ignoring_case(text: &str, term: &str) -> Vec<(usize, usize)> {
    let needle: Vec<char> = term.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        // `i` の文字から、小文字にして `needle` と同じになるところまで。
        let (mut j, mut k) = (i, 0);
        while j < chars.len() && k < needle.len() {
            let lower: Vec<char> = chars[j].1.to_lowercase().collect();
            if needle[k..].starts_with(&lower) {
                k += lower.len();
                j += 1;
            } else {
                break;
            }
        }
        if k == needle.len() {
            let end = chars.get(j).map_or(text.len(), |(b, _)| *b);
            found.push((chars[i].0, end));
            i = j;
        } else {
            i += 1;
        }
    }
    found
}

/// 本文（件名の次の行から）。`trailers` なら、最後の段落（トレーラー）を除く（macOS 版の `bodyWithoutTrailers`）。
fn body_without_trailers(message: &str, trailers: bool) -> String {
    let message = message.trim();
    let body = message
        .split_once('\n')
        .map_or("", |(_, b)| b)
        .trim_matches('\n');
    if !trailers {
        return body.trim().to_owned();
    }
    match body.rfind("\n\n") {
        Some(split) => body[..split].trim_matches('\n').trim().to_owned(),
        // 本文がトレーラーだけ。
        None => String::new(),
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_default()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect()
}

fn date(time: i64) -> String {
    crate::titles::date_time(time, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_become_web_urls() {
        assert_eq!(
            web_url("git@github.com:a/b.git").as_deref(),
            Some("https://github.com/a/b")
        );
        assert_eq!(
            web_url("ssh://git@gitlab.example.com:2222/a/b").as_deref(),
            Some("https://gitlab.example.com/a/b")
        );
        assert_eq!(
            web_url("https://github.com/a/b.git").as_deref(),
            Some("https://github.com/a/b")
        );
        assert_eq!(web_url(""), None);
    }

    #[test]
    fn issue_numbers_and_urls_become_links() {
        let web = Some("https://github.com/a/b");
        assert_eq!(
            linked("Fixes #12, see https://x.org/p.", web, false),
            "Fixes <a href=\"https://github.com/a/b/issues/12\">#12</a>, see \
             <a href=\"https://x.org/p\">https://x.org/p</a>."
        );
        // 単語の途中・`&#` の後ろ・数字でないものはリンクにしない。リモートがなければ `#12` もそのまま。
        assert_eq!(
            linked("a#1 &#38; #x <b>", web, false),
            "a#1 &amp;#38; #x &lt;b&gt;"
        );
        assert_eq!(linked("#7", None, false), "#7");
        assert_eq!(
            linked("#7", Some("https://gitlab.com/a/b"), false),
            "<a href=\"https://gitlab.com/a/b/-/issues/7\">#7</a>"
        );
    }

    #[test]
    fn short_ids_in_messages_become_links() {
        assert_eq!(
            linked("Reverts 1a2b3c4d (see deadbeefcafe).", None, true),
            "Reverts <a href=\"rev:1a2b3c4d\">1a2b3c4d</a> (see \
             <a href=\"rev:deadbeefcafe\">deadbeefcafe</a>)."
        );
        // 6 桁・単語の途中・パスの中・大文字・URL の中はリンクにしない。リンクにしないなら、そのまま。
        assert_eq!(
            linked("abc123 x1a2b3c4d a/1a2b3c4d 1A2B3C4D", None, true),
            "abc123 x1a2b3c4d a/1a2b3c4d 1A2B3C4D"
        );
        assert_eq!(
            linked("https://x.org/1a2b3c4d", None, true),
            "<a href=\"https://x.org/1a2b3c4d\">https://x.org/1a2b3c4d</a>"
        );
        assert_eq!(linked("1a2b3c4d", None, false), "1a2b3c4d");
    }

    #[test]
    fn search_terms_are_highlighted() {
        assert_eq!(
            highlight_terms(Some(
                "fix author:linus (\"bad thing\" or subject:oops) path:fs/"
            )),
            ["fix", "bad thing", "oops"]
        );
        assert!(highlight_terms(None).is_empty());
        assert_eq!(find_ignoring_case("Fix fIX ふix", "fix"), [(0, 3), (4, 7)]);
        assert_eq!(find_ignoring_case("aÄb", "ä"), [(1, 3)]);
    }

    #[test]
    fn related_commits_follow_the_message() {
        let message = "Fix\n\nReverts commit 2222222.\n(cherry picked from commit 1111111)\nSee 3333333\nFixes: 4444444 (\"x\")";
        let commit = |oid: &str| serde_json::json!({ "oid": oid, "subject": "s" });
        let references = serde_json::json!({
            "1111111": commit("1"), "2222222": commit("2"), "3333333": commit("3"), "4444444": commit("4"),
        });
        let kinds: Vec<(String, String)> = related_commits(message, &references)
            .into_iter()
            .map(|(k, c)| (k, str_of(&c, "oid").to_owned()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("Reverts".to_owned(), "2".to_owned()),
                ("Cherry-picked from".to_owned(), "1".to_owned()),
                ("Mentions".to_owned(), "3".to_owned()),
                ("Fixes".to_owned(), "4".to_owned()),
            ]
        );
    }

    #[test]
    fn reword_saves_only_a_changed_message() {
        assert_eq!(reworded("a\n\nb\n", "  \n"), None);
        assert_eq!(reworded("a\n\nb\n", "a\n\nb"), Some(None));
        assert_eq!(reworded("a\n", " c\n\nd \n"), Some(Some("c\n\nd".into())));
    }

    #[test]
    fn trailers_leave_the_body() {
        let message = "Fix it\n\nBody.\n\nMore.\n\nSigned-off-by: A <a@x>\n";
        assert_eq!(body_without_trailers(message, true), "Body.\n\nMore.");
        assert_eq!(
            body_without_trailers(message, false),
            "Body.\n\nMore.\n\nSigned-off-by: A <a@x>"
        );
        assert_eq!(body_without_trailers("Fix\n\nSigned-off-by: A", true), "");
    }

    #[test]
    fn ref_names_link_with_escaped_names() {
        let markup = ref_links(&["main".into(), "a&b".into()]);
        assert!(markup.starts_with("<a href=\"ref:main\""));
        assert!(markup.contains(">a&amp;b</a>"));
        assert!(markup.contains("href=\"ref:a%26b\""));
    }
}
