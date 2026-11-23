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

fn venn_union(n: usize, sized: bool) -> String {
    let mut s = String::from("venn-beta\n");
    for i in 0..n {
        if sized {
            s += &format!("set S{i}:{}\n", i + 1);
        } else {
            s += &format!("set S{i}\n");
        }
    }
    let ids: Vec<String> = (0..n).map(|i| format!("S{i}")).collect();
    s + "union " + &ids.join(",") + "\n"
}

#[test]
fn venn_union_of_many_sets() {
    // 2 つずつの組（160 万）を名前で作らず、緩める回のない数の円では離れ方を求めない。pele と同じ絵になる。
    assert!(render(&venn_union(1800, false)).is_ok());
    // 大きさの違う 400 の集合: 8 万の組の離れ方を二分法で求めると数秒かかる。pele は描く（わざとの違い）。
    assert_eq!(
        limit_error(&venn_union(400, true)),
        "The venn diagram has too many overlapping sets of different sizes to lay out."
    );
    assert!(render(&venn_union(100, true)).is_ok());
}

#[test]
#[ignore = "pele の仕事の上限まで日付を書き出すので、debug では 1.5 秒ほど"]
fn gantt_dated_excludes_with_long_formats() {
    // 書式で書いた日付で除く日を探すと、日ごとに書式を書き出す。閉じない `[` の続く書式は、位置ごとに `]` を
    // 終わりまで探して止まらなかった（pele も 1 分で終わらない）。どれも pele の仕事の上限の知らせで終わる。
    let work = "The gantt chart takes too much work to schedule.";
    let chart = |format: &str, start: &str| {
        format!("gantt\ndateFormat {format}\nexcludes 2000-01-01\nsection s\nt :{start}, 100000d\n")
    };
    let unclosed = format!("YYYY-MM-DD {}", "[".repeat(20000));
    assert_eq!(limit_error(&chart(&unclosed, "2000-01-01")), work);
    let literal = format!("YYYY-MM-DD{}", " [x]".repeat(2000));
    let start = format!("2000-01-01{}", " x".repeat(2000));
    assert_eq!(limit_error(&chart(&literal, &start)), work);
}

#[test]
fn many_entity_codes_in_a_syntax_error() {
    // 知らせの位置を元の文字列に写すとき、`#1;` の符号ごとに頭から UTF-16 の位置を数えていた（2 乗）。
    let source = format!("info\n{}\n", "#1;".repeat(16000));
    let e = render(&source).unwrap_err();
    assert!(
        e.message.starts_with("Lexical error on line 2."),
        "{}",
        e.message
    );
    assert_eq!(e.line, 2);
}

/// 上限いっぱい（5 万字）の入れ子。pele はどれも JS のスタック（約 1 MB）で描けるので、ここも 1 MB のスタックで
/// 溢れずに描けることを確かめる（入れ子の深さに比例する再帰を足すと、ここで溢れる）。
#[test]
fn deepest_nesting_within_the_source_limit() {
    const N: usize = 50_000;
    let fill = |head: &str, open: &str, inner: &str, close: &str| {
        let n = (N - head.len() - inner.len()) / (open.len() + close.len());
        format!("{head}{}{inner}{}", open.repeat(n), close.repeat(n))
    };
    let cases = [
        fill("flowchart TD\n", "subgraph s\n", "A-->B\n", "end\n"),
        fill("stateDiagram-v2\n", "state a {\n", "[*] --> A\n", "}\n"),
        fill("block\n", "block\n", "a\n", "end\n"),
        fill(
            "C4Context\n",
            "Boundary(b, \"b\") {\n",
            "Person(p, \"P\")\n",
            "}\n",
        ),
        fill("agentflow-beta TB\n", "flow f[\"F\"]\n", "a-->b\n", "end\n"),
        fill("sequenceDiagram\n", "loop x\n", "A->>B: m\n", "end\n"),
        fill("classDiagram\n", "namespace n {\n", "class X\n", "}\n"),
        fill("railroad-ebnf-beta\na = ", "(", "\"x\"", ")") + " ;\n",
        fill("railroad-ebnf-beta\na = ", "[", "\"x\"", "]") + " ;\n",
        fill("railroad-peg-beta\na <- ", "(", "\"x\"", ")") + " ;\n",
        fill("%%{init: {\"a\":", "[", "", "]") + "}}%%\ngraph TD\nA\n",
        fill("%%{init: ", "{\"\":", "1", "}") + "}%%\ngraph TD\nA\n",
        fill("eventmodeling\ntf 01 evt E { a: ", "{ a: ", "string", " }") + " }\n",
        fill("flowchart TD\nA[\"`", "**_", "x", "_**") + "`\"]\n",
        fill("flowchart TD\nA[\"", "<b>", "x", "</b>") + "\"]\n",
        fill("---\nconfig: ", "{a: ", "1", "}") + "\n---\ngraph TD\nA\n",
        format!(
            "mindmap\n{}",
            (0..300)
                .map(|i| format!("{}a\n", " ".repeat(i)))
                .collect::<String>()
        ),
    ];
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            for source in &cases {
                // 描けても、上限の知らせでもよい。溢れないこと。
                let _ = render(source);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
