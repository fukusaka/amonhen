//! 差分の帯の、行を比べるときに無視する空白を選ぶボタン（History の右の列と Working Copy。macOS 版の
//! `WhitespacePopUp`）。帯を狭くしないようにアイコンだけで、押すと 4 つの選び方を出す（今のものに印）。無視している
//! 間はアイコンをアクセントの色にする。選んだものは覚えない（開くたびに「空白を区別」から）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use amonhen_core::diff::Whitespace;
use amonhen_model::tr;
use gtk::prelude::*;

use crate::menu;

/// 並べる順（`Whitespace` の値と同じ）。
const MODES: [Whitespace; 4] = [
    Whitespace::Exact,
    Whitespace::AtEol,
    Whitespace::Change,
    Whitespace::All,
];

pub fn title(mode: Whitespace) -> String {
    match mode {
        Whitespace::Exact => tr!("Compare Whitespace"),
        Whitespace::AtEol => tr!("Ignore Whitespace at Line Ends"),
        Whitespace::Change => tr!("Ignore Changes in Whitespace"),
        Whitespace::All => tr!("Ignore All Whitespace"),
    }
}

pub fn tooltip(mode: Whitespace) -> String {
    match mode {
        Whitespace::Exact => tr!("Lines that differ only in whitespace are changes"),
        Whitespace::AtEol => {
            tr!("Ignore whitespace at the end of lines (git diff --ignore-space-at-eol)")
        }
        Whitespace::Change => tr!("Ignore changes in the amount of whitespace (git diff -b)"),
        Whitespace::All => tr!("Ignore all whitespace when comparing lines (git diff -w)"),
    }
}

/// 最初に選んでおくもの。ふだんは「空白を区別」で、確かめる用（`--snapshot`）に `AMH_WHITESPACE=0〜3` で変えられる
/// （macOS 版の `AMHWhitespace`）。
pub fn initial() -> Whitespace {
    std::env::var("AMH_WHITESPACE")
        .ok()
        .and_then(|v| v.trim().parse::<u8>().ok())
        .map_or(Whitespace::Exact, Whitespace::from_u8)
}

/// 選ぶボタン（最初は [`initial`]）。
#[derive(Clone)]
pub struct Picker {
    inner: Rc<Inner>,
}

struct Inner {
    button: gtk::Button,
    /// アイコン（macOS 版の space）。無視している間はアクセントの色。
    icon: crate::icon::Icon,
    mode: Cell<Whitespace>,
    on_change: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Picker {
    pub fn new() -> Self {
        let button = gtk::Button::new();
        button.add_css_class("flat");
        button.set_valign(gtk::Align::Center);
        // 押してもフォーカスを取らない（差分の J・K・Space がそのまま効くように。macOS 版と同じ）。
        button.set_focus_on_click(false);
        let icon = crate::icon::Icon::default();
        button.set_child(Some(&icon));
        button.update_property(&[gtk::accessible::Property::Label(&tr!("Whitespace"))]);
        let inner = Rc::new(Inner {
            button,
            icon,
            mode: Cell::new(initial()),
            on_change: RefCell::default(),
        });
        let weak = Rc::downgrade(&inner);
        inner.button.connect_clicked(move |b| {
            if let Some(inner) = weak.upgrade() {
                Inner::popup(&inner, b);
            }
        });
        inner.update();
        Self { inner }
    }

    /// 帯に置く部品。
    pub fn widget(&self) -> &gtk::Button {
        &self.inner.button
    }

    /// 選んでいるもの。
    pub fn whitespace(&self) -> Whitespace {
        self.inner.mode.get()
    }

    /// 選び直したら `f()` を呼ぶ。
    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        self.inner.on_change.replace(Some(Rc::new(f)));
    }
}

impl Inner {
    fn popup(this: &Rc<Self>, button: &gtk::Button) {
        let current = this.mode.get();
        let items = MODES
            .iter()
            .map(|&mode| {
                let weak = Rc::downgrade(this);
                menu::choice_with_tip(title(mode), mode == current, tooltip(mode), move || {
                    if let Some(this) = weak.upgrade() {
                        Inner::choose(&this, mode);
                    }
                })
            })
            .collect();
        menu::popup(
            button,
            button.width() as f64 / 2.0,
            button.height() as f64,
            items,
        );
    }

    fn choose(this: &Rc<Self>, mode: Whitespace) {
        if mode == this.mode.get() {
            return;
        }
        this.mode.set(mode);
        this.update();
        let f = this.on_change.borrow().clone();
        if let Some(f) = f {
            f();
        }
    }

    /// アイコンの色・ツールチップ（今の選び方を言う）。
    fn update(&self) {
        let mode = self.mode.get();
        self.icon
            .set(crate::icon::Kind::Space, mode != Whitespace::Exact);
        self.button.set_tooltip_text(Some(&format!(
            "{}\n{}",
            title(mode),
            tr!("Whitespace to ignore when comparing lines (as git diff does)")
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_follow_the_core_values() {
        for (i, mode) in MODES.iter().enumerate() {
            assert_eq!(*mode as usize, i);
            assert_eq!(Whitespace::from_u8(i as u8), *mode);
        }
    }
}
