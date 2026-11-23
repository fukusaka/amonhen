//! 大きな図・深い図の歯止め。上限いっぱいの文字列でも、止まらず、メモリを食い尽くさず、pele と同じ知らせを返すか
//! （pele 1.13.0 を node で動かして確かめた答え）。

use amonhen_pele::{ErrorCode, render};

fn limit_error(source: &str) -> String {
    match render(source) {
        Ok(_) => panic!("rendered: {}", &source[..source.len().min(60)]),
        Err(e) => {
            assert_eq!(e.code, ErrorCode::Limit, "{}", e.message);
            e.message
        }
    }
}

fn dotted(n: usize) -> String {
    vec!["a"; n].join(".")
}

const FLAT: &str = "---\nconfig:\n  class:\n    hierarchicalNamespaces: false\n---\n";

#[test]
fn class_namespace_of_many_dotted_parts() {
    // `a.a.a…` の祖先をすべて名前空間の枠に描くので、2 万 4 千の部分なら枠の文字列が JS の文字列の上限を超える。
    let ns = |n| format!("classDiagram\nnamespace {} {{\nclass X\n}}\n", dotted(n));
    assert_eq!(limit_error(&ns(24000)), "Diagram is nested too deeply.");
    assert_eq!(
        limit_error(&ns(2000)),
        "Diagram output is longer than the limit of 4000000 characters."
    );
    assert!(render(&ns(1800)).is_ok());
    // 宣言した名前空間だけを描くなら pele は描ける。ここでは祖先の名前を作らずに止める（わざとの違い）。
    assert!(render(&format!("{FLAT}{}", ns(1999))).is_ok());
    assert_eq!(
        limit_error(&format!("{FLAT}{}", ns(2001))),
        "Class diagram namespace names are too long to draw."
    );
}

#[test]
fn class_namespaces_nested_deeply() {
    let nested = (0..2400)
        .map(|i| format!("namespace n{i} {{\n"))
        .collect::<String>()
        + "class X\n"
        + &"}\n".repeat(2400);
    assert_eq!(
        limit_error(&format!("classDiagram\n{nested}")),
        "Diagram output is longer than the limit of 4000000 characters."
    );
    assert_eq!(
        limit_error(&format!("{FLAT}classDiagram\n{nested}")),
        "Diagram output is longer than the limit of 4000000 characters."
    );
}
