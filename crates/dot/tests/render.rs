//! 描いた図の確かめ: 見本の SVG（`data/*.svg`。`BLESS=1` で書き直す）、注記、上限、壊れた文で止まらないこと。

use std::path::Path;

use amonhen_dot::{Note, render};

const SAMPLES: &[(&str, &str)] = &[
    ("chain", "digraph { a -> b -> c; a -> c }"),
    (
        "clusters",
        "digraph { subgraph cluster_a { label=\"A\"; a1 -> a2 } subgraph cluster_b { b1 -> b2 } start -> a1; start -> b1; a2 -> end; b2 -> end; start [shape=Mdiamond]; end [shape=Msquare] }",
    ),
    (
        "records",
        "digraph { node [shape=record]; s1 [label=\"<f0> left|<f1> mid|<f2> right\"]; s2 [label=\"{a|{b|<p> c}}\"]; s1:f2 -> s2:p; s1:f0 -> s2 }",
    ),
    (
        "lr",
        "digraph { rankdir=LR; a -> b [label=\"x\"]; b -> b [label=\"loop\"]; b -> c [dir=both, arrowtail=odiamond, arrowhead=crow]; {rank=same; c; d} c -> d }",
    ),
    (
        "styles",
        "graph { node [style=filled, fillcolor=\"#ffcc00\"]; a -- b [color=\"red:blue\", penwidth=2]; b [shape=box, style=\"rounded,dashed,filled\", fillcolor=\"red:yellow\"]; c [label=<<b>bold</b> <i>it</i>>]; b -- c [style=dotted, label=\"e\"] }",
    ),
];

fn data(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(format!("{name}.svg"))
}

#[test]
fn matches_the_samples() {
    let bless = std::env::var_os("BLESS").is_some();
    for (name, src) in SAMPLES {
        let svg = render(src).unwrap_or_else(|e| panic!("{name}: {e}")).svg;
        let path = data(name);
        if bless {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &svg).unwrap();
            continue;
        }
        let want = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(svg, want, "{name} changed (BLESS=1 to update)");
    }
}

#[test]
fn uses_theme_variables_and_given_colors() {
    let svg = render("digraph { a [color=red, fontcolor=\"#00ff00\"]; a -> b }")
        .unwrap()
        .svg;
    assert!(svg.contains("stroke=\"#ff0000\""), "{svg}");
    assert!(svg.contains("fill=\"#00ff00\""));
    assert!(svg.contains("var(--_l)") && svg.contains("var(--_b)") && svg.contains("var(--_s)"));
    assert!(svg.contains(">a</text>") && svg.contains(">b</text>"));
}

#[test]
fn notes_approximations() {
    let notes = |src: &str| render(src).unwrap().notes;
    assert!(notes("digraph { a -> b }").is_empty());
    assert!(
        notes("digraph { a [shape=plaintext, label=<<table><tr><td>x</td></tr></table>>] }")
            .is_empty()
    );
    assert_eq!(
        notes(
            "digraph { a [shape=plain, label=<<table><tr><td><img src=\"x.png\"/></td></tr></table>>] }"
        ),
        [Note::Images]
    );
    assert_eq!(notes("digraph { a [image=\"x.png\"] }"), [Note::Images]);
    assert_eq!(notes("digraph { a [shape=blob] }"), [Note::UnknownShapes]);
    assert!(notes("digraph { splines=ortho; a -> b -> c; a -> c; b -> b }").is_empty());
    assert_eq!(
        notes("digraph { a [color=nocolor] }"),
        [Note::UnknownColors]
    );
    assert_eq!(
        notes("digraph { node [colorscheme=nosuch9, color=3]; a }"),
        [Note::ColorSchemes]
    );
    // ColorBrewer の配色は描ける
    assert!(
        notes("digraph { node [colorscheme=blues9, style=filled, fillcolor=3]; a [color=\"/set312/12\"] }")
            .is_empty()
    );
    assert_eq!(
        notes("graph { layout=neato; a -- b }"),
        [Note::OtherLayouts]
    );
    // 大きさの属性は黙って無視する
    assert!(notes("digraph { size=\"4,4\"; ratio=fill; dpi=300; rotate=90; a }").is_empty());
}

#[test]
fn refuses_what_is_too_big() {
    let long = format!("digraph {{ a }}{}", " ".repeat(50_000));
    assert!(render(&long).unwrap_err().message.contains("limit"));
    let nodes: String = (0..2100).map(|i| format!("n{i};")).collect();
    assert!(render(&format!("digraph {{ {nodes} }}")).is_err());
    let product = format!(
        "digraph {{ {{ {} }} -> {{ {} }} }}",
        (0..80)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join(" "),
        (0..80)
            .map(|i| format!("b{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    assert!(render(&product).unwrap_err().message.contains("edges"));
    // 深い入れ子もスタックをあふれさせない（上限より深ければ誤り）
    let nested = |depth: usize| {
        let open: String = (0..depth)
            .map(|i| format!("subgraph cluster_{i} {{ n{i}; "))
            .collect();
        format!("digraph {{ {open}{} }}", "}".repeat(depth))
    };
    assert!(render(&nested(190)).is_ok());
    assert!(render(&nested(400)).is_err());
}

#[test]
fn draws_big_graphs_in_time() {
    // 5 万文字近くの込み入ったグラフも、遅いマシンでも数秒より短く
    let mut seed: u64 = 7;
    let mut next = |n: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    let body: String = (0..4000)
        .map(|_| format!("{}->{};", next(900), next(900)))
        .collect();
    let started = std::time::Instant::now();
    let d = render(&format!("digraph {{ {body} }}")).unwrap();
    assert!(d.svg.len() < amonhen_dot::MAX_OUTPUT);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

/// 決まった種の乱数で作った壊れた文・ありえない属性で、panic せず終わる。
#[test]
fn survives_random_sources() {
    let words = [
        "digraph",
        "graph",
        "strict",
        "subgraph",
        "cluster_x",
        "node",
        "edge",
        "{",
        "}",
        "[",
        "]",
        ";",
        ",",
        "=",
        ":",
        "->",
        "--",
        "a",
        "b",
        "c",
        "\"q\\l\\N\"",
        "<<b>x</b><table><tr><td>",
        "shape",
        "record",
        "\"{a|<p>b|{c}}\"",
        "label",
        "rank",
        "same",
        "min",
        "sink",
        "rankdir",
        "LR",
        "BT",
        "color",
        "\"red:blue;0.5\"",
        "style",
        "\"filled,rounded,wedged\"",
        "port",
        "p:n",
        "lhead",
        "compound",
        "true",
        "minlen",
        "-3",
        "weight",
        "1e9",
        "constraint",
        "false",
        "arrowhead",
        "lteeoldiamondcrow",
        "sides",
        "1000",
        "skew",
        "nan",
        "width",
        "-5",
        "fixedsize",
        "peripheries",
        "99",
        "splines",
        "curved",
        "ortho",
        "xlabel",
        "headlabel",
        "labelangle",
        "inf",
        "fontsize",
        "0",
        "penwidth",
        "1e300",
        "fillcolor",
        "\"/blues9/3\"",
        "polygon",
        "orientation",
        "720",
        "distortion",
        "Mrecord",
        "point",
    ];
    let mut seed: u64 = 1;
    let mut next = |n: usize| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as usize) % n
    };
    let rounds: usize = std::env::var("DOT_FUZZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400);
    for _ in 0..rounds {
        let len = 3 + next(80);
        let mut src = String::from(if next(2) == 0 { "digraph {" } else { "graph {" });
        for _ in 0..len {
            src.push(' ');
            src.push_str(words[next(words.len())]);
        }
        if next(3) > 0 {
            src.push_str(" }");
        }
        let _ = render(&src);
    }
    // 1 文字ずつ壊す
    for (_, base) in SAMPLES {
        let bytes = base.as_bytes();
        for i in 0..bytes.len() {
            let mut b = bytes.to_vec();
            let junk = b"{}[];=\"<>|:-";
            b[i] = junk[next(junk.len())];
            let _ = render(&String::from_utf8_lossy(&b));
            let _ = render(&String::from_utf8_lossy(&bytes[..i]));
        }
    }
}
