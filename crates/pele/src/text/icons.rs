//! 描いている間の、ホストのアイコン（pele の `src/text/icons.ts`）。ラベルはホストがその名のアイコンを持つときだけ
//! 場所を空け、配置はそれを描く前に知る必要がある。

use std::cell::RefCell;
use std::collections::HashMap;

use crate::types::IconResolver;

thread_local! {
    static RESOLVER: RefCell<Option<IconResolver>> = const { RefCell::new(None) };
    static FOUND: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

pub fn with_icons<T>(icons: Option<IconResolver>, run: impl FnOnce() -> T) -> T {
    let outer = RESOLVER.with(|r| r.replace(icons));
    let outer_found = FOUND.with(|f| f.replace(HashMap::new()));
    let out = run();
    RESOLVER.with(|r| *r.borrow_mut() = outer);
    FOUND.with(|f| *f.borrow_mut() = outer_found);
    out
}

pub fn icon_markup(name: &str) -> String {
    let Some(resolver) = RESOLVER.with(|r| r.borrow().clone()) else {
        return String::new();
    };
    if let Some(m) = FOUND.with(|f| f.borrow().get(name).cloned()) {
        return m;
    }
    let markup = resolver(name).unwrap_or_default();
    FOUND.with(|f| f.borrow_mut().insert(name.to_string(), markup.clone()));
    markup
}
