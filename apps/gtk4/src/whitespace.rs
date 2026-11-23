//! 差分の帯の、行を比べるときに無視する空白を選ぶ欄（History の右の列と Working Copy。macOS 版の
//! `WhitespacePopUp`）。選んだものは覚えない（開くたびに「空白を区別」から）。

use amonhen_core::diff::Whitespace;
use amonhen_model::tr;
use gtk::prelude::*;

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

/// 選ぶ欄（最初は [`initial`]）。選んだものは [`selected`] で読む。
pub fn dropdown() -> gtk::DropDown {
    let titles: Vec<String> = MODES.iter().map(|&m| title(m)).collect();
    let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
    let dropdown = gtk::DropDown::from_strings(&refs);
    // 一覧の行には、それぞれの説明をツールチップに。
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        item.set_child(Some(&label));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(label), Some(text)) = (
            item.child().and_downcast::<gtk::Label>(),
            item.item().and_downcast::<gtk::StringObject>(),
        ) else {
            return;
        };
        label.set_text(&text.string());
        if let Some(&mode) = MODES.get(item.position() as usize) {
            label.set_tooltip_text(Some(&tooltip(mode)));
        }
    });
    dropdown.set_list_factory(Some(&factory));
    dropdown.set_selected(initial() as u32);
    dropdown.set_valign(gtk::Align::Center);
    let tip = tr!("Whitespace to ignore when comparing lines (as git diff does)");
    dropdown.set_tooltip_text(Some(&tip));
    dropdown.update_property(&[gtk::accessible::Property::Label(&tip)]);
    dropdown
}

/// `dropdown` で選んでいるもの。
pub fn selected(dropdown: &gtk::DropDown) -> Whitespace {
    MODES
        .get(dropdown.selected() as usize)
        .copied()
        .unwrap_or_default()
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
