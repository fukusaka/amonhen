//! shallow clone の History の最後の行の下に置く帯（macOS 版の `ShallowHistoryBand`）。ここより古い履歴を取得して
//! いないことを言い、取り足すボタン（さらに取る・すべて取る）を並べる。取っている間はボタンを止めて回る印を出す。
//! 取り足す数や日付を尋ねるシートは、リポジトリの設定の「取得とフォルダ」と Clone のシートも使う。

use std::cell::Cell;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, glib};
use serde_json::{Value, json};

use crate::sheets::{Response, Sheet};

/// 取り足す命令（`deepen`・`unshallow`）を実行する。終わったら（失敗しても）2 つ目の閉包を呼んでもらう。
pub type Fetch = Rc<dyn Fn(Value, Box<dyn FnOnce()>)>;

/// 「さらに取る」の数（項目の名前と、コミットの数）。リポジトリの設定の「取得とフォルダ」と Clone のシートも同じものを
/// 出す。
pub fn more_choices() -> Vec<(String, u32)> {
    vec![
        (tr!("100 Commits"), 100),
        (tr!("1,000 Commits"), 1_000),
        (tr!("10,000 Commits"), 10_000),
    ]
}

fn install_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    // 最後の行との境目に、途切れたことを表す点線。
    provider.load_from_string(
        ".shallow-band { border-top: 1px dashed alpha(currentColor, 0.25); padding: 0 14px; }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// 帯を作る。`fetch` で取り足す。`boundary_time` は「この日付まで…」の初期値の元（境界のコミットの日時。
/// 分からなければ None）。
pub fn band(fetch: Fetch, boundary_time: Rc<dyn Fn() -> Option<i64>>) -> gtk::Box {
    install_css();
    let label = gtk::Label::new(Some(&tr!("Older history isn’t fetched (shallow clone)")));
    label.add_css_class("dim-label");
    label.add_css_class("caption");
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_xalign(0.0);
    label.set_margin_end(6);
    let more = pull_down(&tr!("Fetch More"));
    more.set_tooltip_text(Some(&tr!(
        "Fetch older commits from the remote (git fetch --deepen)"
    )));
    let all = gtk::Button::with_label(&tr!("Fetch All"));
    all.set_tooltip_text(Some(&tr!(
        "Fetch the whole history from the remote (git fetch --unshallow)"
    )));
    let spinner = gtk::Spinner::new();
    spinner.set_visible(false);
    let widget = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    widget.add_css_class("shallow-band");
    for w in [&more, &all] {
        w.add_css_class("small");
        w.set_valign(gtk::Align::Center);
    }
    spinner.set_valign(gtk::Align::Center);
    widget.append(&label);
    widget.append(&more);
    widget.append(&all);
    widget.append(&spinner);
    widget.set_accessible_role(gtk::AccessibleRole::Group);
    widget.update_property(&[gtk::accessible::Property::Label(&label.text())]);
    // 表の空いた所と同じく、押しても行の選択を変えない（下の表に通さない）。
    let swallow = gtk::GestureClick::new();
    swallow.connect_pressed(|g, _, _, _| {
        g.set_state(gtk::EventSequenceState::Claimed);
    });
    widget.add_controller(swallow);
    // 取っている間はボタンを止めて回る印を出す。
    let (m, a, s) = (more.downgrade(), all.downgrade(), spinner.downgrade());
    let set_running = move |running: bool| {
        if let (Some(m), Some(a), Some(s)) = (m.upgrade(), a.upgrade(), s.upgrade()) {
            m.set_sensitive(!running);
            a.set_sensitive(!running);
            s.set_spinning(running);
            s.set_visible(running);
        }
    };
    let run: Rc<dyn Fn(Value)> = Rc::new(move |command| {
        set_running(true);
        let done = set_running.clone();
        fetch(command, Box::new(move || done(false)));
    });
    let r = run.clone();
    all.connect_clicked(move |_| r(json!({ "op": "unshallow" })));
    more.connect_clicked(move |b| {
        let window = b.root().and_downcast::<gtk::Window>();
        popup_more(b, window, run.clone(), boundary_time.clone());
    });
    widget
}

/// 押すとメニューを下に出すボタン（macOS 版の引き下げ式のボタン。名前の右に ▾）。
pub fn pull_down(title: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    content.append(&gtk::Label::new(Some(title)));
    content.append(&gtk::Image::from_icon_name("pan-down-symbolic"));
    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button
}

/// 「さらに取る」のメニュー: 決まった数・その他…（数を尋ねる）・この日付まで…（日付を尋ねる）。`run` に
/// `deepen` の命令を渡す。
pub fn popup_more(
    button: &gtk::Button,
    window: Option<gtk::Window>,
    run: Rc<dyn Fn(Value)>,
    boundary_time: Rc<dyn Fn() -> Option<i64>>,
) {
    let mut items = Vec::new();
    for (title, commits) in more_choices() {
        let r = run.clone();
        items.push(crate::menu::action(title, move || {
            r(json!({ "op": "deepen", "commits": commits }))
        }));
    }
    let (r, w) = (run.clone(), window.clone());
    items.push(crate::menu::action(tr!("Other…"), move || {
        let (r, w) = (r.clone(), w.clone());
        glib::spawn_future_local(async move {
            if let Some(commits) = ask_deepen(w.as_ref()).await {
                r(json!({ "op": "deepen", "commits": commits }));
            }
        });
    }));
    items.push(crate::menu::Item::Separator);
    items.push(crate::menu::action(tr!("Up to a Date…"), move || {
        let (r, w) = (run.clone(), window.clone());
        let boundary = boundary_time();
        glib::spawn_future_local(async move {
            if let Some(since) = ask_date(w.as_ref(), boundary).await {
                r(json!({ "op": "deepen", "since": since }));
            }
        });
    }));
    crate::menu::popup(button, 0.0, f64::from(button.height()), items);
}

/// 「さらに取る」の「その他…」。何コミット取り足すかを尋ねる。
pub async fn ask_deepen(parent: Option<&gtk::Window>) -> Option<u32> {
    ask_count(
        parent,
        &tr!("Fetch More History"),
        &tr!("How many older commits to fetch (git fetch --deepen)."),
        "Fetch",
        100,
    )
    .await
}

/// コミットの数（1 以上）を尋ねる。
pub async fn ask_count(
    parent: Option<&gtk::Window>,
    title: &str,
    message: &str,
    button: &str,
    initial: u32,
) -> Option<u32> {
    let sheet = Sheet::unnamed(parent, title, message);
    let field = gtk::SpinButton::with_range(1.0, f64::from(u32::MAX), 1.0);
    field.set_digits(0);
    field.set_value(f64::from(initial));
    field.set_activates_default(true);
    field.set_halign(gtk::Align::Start);
    field.set_width_chars(10);
    sheet.add(&field);
    sheet.buttons(button, false, None);
    field.grab_focus();
    let f = field.clone();
    if sheet.run().await != Response::Ok {
        return None;
    }
    // 打ち込んだまま押しても、その数で。
    f.update();
    let value = f.value();
    (value >= 1.0).then_some(value as u32)
}

/// 初めに出す日付: 境界のコミットの日時（分からなければ今）の 1 年前。今より後にはしない。
fn initial_date(boundary: Option<i64>, now: &glib::DateTime) -> glib::DateTime {
    let from = boundary
        .and_then(|t| glib::DateTime::from_unix_local(t).ok())
        .unwrap_or_else(|| now.clone());
    let year_before = from.add_years(-1).unwrap_or(from);
    if year_before > *now {
        now.clone()
    } else {
        year_before
    }
}

/// 取り足す日付を尋ね、git が読む形（`YYYY-MM-DD`）で返す。初めは `boundary`（境界のコミットの日時。分からなければ
/// 今）の 1 年前。今より後は選べない。
pub async fn ask_date(parent: Option<&gtk::Window>, boundary: Option<i64>) -> Option<String> {
    let sheet = Sheet::unnamed(
        parent,
        &tr!("Fetch History Up to a Date"),
        &tr!("Fetches the commits made after this date (git fetch --shallow-since)."),
    );
    let now = glib::DateTime::now_local().ok()?;
    let calendar = gtk::Calendar::new();
    calendar.select_day(&initial_date(boundary, &now));
    calendar.set_halign(gtk::Align::Start);
    sheet.add(&calendar);
    sheet.buttons("Fetch", false, None);
    let c = calendar.clone();
    if sheet.run().await != Response::Ok {
        return None;
    }
    let picked = c.date();
    let picked = if picked > now { now } else { picked };
    picked.format("%Y-%m-%d").ok().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_the_same_counts_as_macos() {
        assert_eq!(
            more_choices().iter().map(|c| c.1).collect::<Vec<_>>(),
            [100, 1_000, 10_000]
        );
    }

    #[test]
    fn starts_a_year_before_the_boundary() {
        let now = glib::DateTime::from_unix_local(1_800_000_000).unwrap();
        let boundary = 1_700_000_000;
        let date = initial_date(Some(boundary), &now);
        let expected = glib::DateTime::from_unix_local(boundary)
            .unwrap()
            .add_years(-1)
            .unwrap();
        assert_eq!(date.ymd(), expected.ymd());
        // 分からなければ今の 1 年前。
        assert_eq!(initial_date(None, &now).year(), now.year() - 1);
    }
}
