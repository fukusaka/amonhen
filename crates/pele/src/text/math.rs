//! 描いている間の、ホストの数式の組版（pele の `src/text/math.ts`）。

use std::cell::RefCell;
use std::collections::HashMap;

use crate::js;
use crate::types::{MathMarkup, MathResolver};

thread_local! {
    static RESOLVER: RefCell<Option<MathResolver>> = const { RefCell::new(None) };
    static FOUND: RefCell<HashMap<String, Option<MathMarkup>>> = RefCell::new(HashMap::new());
}

pub fn with_math<T>(math: Option<MathResolver>, run: impl FnOnce() -> T) -> T {
    let outer = RESOLVER.with(|r| r.replace(math));
    let outer_found = FOUND.with(|f| f.replace(HashMap::new()));
    let out = run();
    RESOLVER.with(|r| *r.borrow_mut() = outer);
    FOUND.with(|f| *f.borrow_mut() = outer_found);
    out
}

pub fn math_active() -> bool {
    RESOLVER.with(|r| r.borrow().is_some())
}

fn valid(size: f64) -> bool {
    size.is_finite() && size > 0.0
}

pub fn typeset(tex: &str, size: f64) -> Option<MathMarkup> {
    let resolver = RESOLVER.with(|r| r.borrow().clone())?;
    let key = format!("{}\n{}", js::fmt(size), tex);
    if let Some(found) = FOUND.with(|f| f.borrow().get(&key).cloned()) {
        return found;
    }
    let markup = resolver(tex, size).filter(|m| valid(m.width()) && valid(m.height()));
    FOUND.with(|f| f.borrow_mut().insert(key, markup.clone()));
    markup
}

/// Mermaid と同じく、図の題は文字のまま。
pub fn without_math<T>(run: impl FnOnce() -> T) -> T {
    let outer = RESOLVER.with(|r| r.replace(None));
    let out = run();
    RESOLVER.with(|r| *r.borrow_mut() = outer);
    out
}
