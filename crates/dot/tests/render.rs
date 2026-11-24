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
    (
        "neato",
        "graph { layout=neato; a [pos=\"0,0!\"]; d [pos=\"3,0!\"]; a -- b -- c -- d; b -- e; c -- e [len=2]; a -- f; f -- f; e -- d; e -- d [label=\"x\"] }",
    ),
    (
        "fdp",
        "graph { layout=fdp; subgraph cluster_a { label=A; a1 -- a2 -- a3 } subgraph cluster_b { label=B; b1 -- b2 } a1 -- b1; c -- a3; c -- b2; c -- d }",
    ),
    (
        "sfdp",
        "graph { layout=sfdp; overlap=false; a -- {b c d e}; b -- {f g}; c -- {h i}; d -- j; e -- {k l m}; m -- a }",
    ),
    (
        "twopi",
        "digraph { layout=twopi; root=r; r -> {a b c}; a -> {a1 a2 a3}; b -> b1; c -> {c1 c2}; a1 -> x }",
    ),
    (
        "circo",
        "graph { layout=circo; a -- b -- c -- d -- e -- a; c -- f -- g -- c; e -- h; h -- i; h -- j }",
    ),
    (
        "osage",
        "graph { layout=osage; subgraph cluster_a { label=A; a1; a2; a3 } subgraph cluster_b { label=B; b1; b2 } c; d; a1 -- b1 }",
    ),
    (
        "patchwork",
        "graph { layout=patchwork; subgraph cluster_a { a1 [area=4]; a2 [area=2]; subgraph cluster_c { c1; c2 } } b1 [area=3]; b2 }",
    ),
    (
        "gradients",
        "digraph { bgcolor=\"white:lightgrey\"; gradientangle=90; node [shape=plain]; t [label=<<table bgcolor=\"yellow:blue\" gradientangle=\"90\" style=\"rounded\"><tr><td bgcolor=\"red:white\" style=\"radial\">radial</td><td bgcolor=\"green;0.3:white\">fraction</td></tr></table>>]; subgraph cluster_a { label=A; bgcolor=\"pink:white\"; style=radial; a } t -> a }",
    ),
    (
        "lr_titles",
        "digraph { rankdir=LR; subgraph cluster_a { label=\"A rather long cluster title\"; x } subgraph cluster_b { label=\"Another long title\"; subgraph cluster_c { label=\"The innermost title of all\"; y } y -> z } w -> x -> y }",
    ),
    (
        "ortho_free",
        "graph { layout=neato; splines=ortho; a -- b -- c -- a; c -- d }",
    ),
];

fn data(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(format!("{name}.svg"))
}

/// 画像の見本の読み手: 96×48 画素の PNG（頭だけ）と 40×20 の SVG。
fn sample_images(src: &str) -> Option<Vec<u8>> {
    match src {
        "img/a.png" => {
            let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
            png.extend_from_slice(&96u32.to_be_bytes());
            png.extend_from_slice(&48u32.to_be_bytes());
            png.extend_from_slice(&[8, 2, 0, 0, 0]);
            Some(png)
        }
        "b.svg" => Some(
            b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 40 20\"><rect width=\"40\" height=\"20\" fill=\"teal\"/></svg>"
                .to_vec(),
        ),
        _ => None,
    }
}

const IMAGE_SAMPLE: &str = "digraph { a [image=\"img/a.png\", label=\"\"]; b [shape=box, image=\"b.svg\", imagescale=true, width=2, height=1, fixedsize=true, label=\"fit\"]; c [shape=box, image=\"img/a.png\", imagepos=tl, width=2.5, height=1, label=\"top left\"]; d [shape=plain, label=<<table><tr><td><img src=\"img/a.png\"/></td><td width=\"100\"><img src=\"b.svg\" scale=\"width\"/></td></tr></table>>]; a -> b -> c -> d }";

#[test]
fn matches_the_image_sample() {
    let svg = amonhen_dot::render_with_images(
        IMAGE_SAMPLE,
        amonhen_dot::engines::Engine::Dot,
        Some(&sample_images),
    )
    .unwrap();
    assert!(svg.notes.is_empty(), "{:?}", svg.notes);
    assert_eq!(svg.svg.matches("<image ").count(), 5);
    let path = data("images");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&path, &svg.svg).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(svg.svg, want, "images changed (BLESS=1 to update)");
    // 読み手がなければ注記して、画像は描かない
    let plain = render(IMAGE_SAMPLE).unwrap();
    assert_eq!(plain.notes, [Note::Images]);
    assert!(!plain.svg.contains("<image "));
}

/// LR・RL のクラスタの箱は題より広い（題が箱からはみ出さない）。
#[test]
fn sideways_clusters_fit_their_titles() {
    for dir in ["LR", "RL"] {
        let src = format!(
            "digraph {{ rankdir={dir}; subgraph cluster_a {{ label=\"A very long title for one node\"; x }} subgraph cluster_b {{ label=\"Outer title that is longer still\"; subgraph cluster_c {{ label=\"Inner\"; y }} }} w -> x -> y }}"
        );
        let svg = render(&src).unwrap().svg;
        for group in svg.split("<g class=\"dot-cluster\"").skip(1) {
            let width: f64 = group
                .split("width=\"")
                .nth(1)
                .and_then(|r| r.split('"').next())
                .and_then(|w| w.parse().ok())
                .unwrap();
            let title = group.split("class=\"dot-cluster-label\"").nth(1).unwrap();
            let text = title.split('>').nth(1).unwrap().split('<').next().unwrap();
            let need = amonhen_dot::text::plain_label(
                text,
                &amonhen_dot::text::Names::default(),
                &amonhen_dot::text::Font::new(14.0),
            )
            .width;
            assert!(width >= need, "{dir}: {text}: {width} < {need}\n{svg}");
        }
    }
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

/// `>word<` を中に持つ `<text …>` の開きの札。
fn text_tag<'a>(svg: &'a str, word: &str) -> &'a str {
    let end = svg
        .find(&format!(">{word}<"))
        .unwrap_or_else(|| panic!("no {word}: {svg}"));
    let start = svg[..end].rfind("<text").expect("a text element");
    &svg[start..end]
}

#[test]
fn text_on_explicit_fills_stays_readable() {
    // 表・升の bgcolor の上は、いちばん内側の塗りで読める色に（文の色があれば文の色）
    let svg = render(
        "digraph { node [shape=plain]; o [label=<<TABLE BGCOLOR=\"lightyellow\"><TR><TD>user_id</TD><TD BGCOLOR=\"navy\">dark</TD><TD><FONT COLOR=\"red\">own</FONT></TD></TR></TABLE>>]; p [label=<<TABLE><TR><TD>bare</TD></TR></TABLE>>]; q [shape=box, style=filled, fillcolor=navy, label=<<TABLE BORDER=\"0\"><TR><TD>inside</TD></TR></TABLE>>]; r [shape=box, style=filled, fillcolor=navy, fontcolor=yellow, label=<<TABLE BGCOLOR=\"white\"><TR><TD>given</TD></TR></TABLE>>] }",
    )
    .unwrap()
    .svg;
    assert!(
        text_tag(&svg, "user_id").contains("fill=\"#1f1f1f\""),
        "{svg}"
    );
    assert!(text_tag(&svg, "dark").contains("fill=\"#ffffff\""), "{svg}");
    let own = text_tag(&svg, "own");
    assert!(
        own.contains("fill=\"#ff0000\"") && own.matches("fill=").count() == 1,
        "{svg}"
    );
    assert!(!text_tag(&svg, "bare").contains("fill="), "{svg}");
    assert!(
        text_tag(&svg, "inside").contains("fill=\"#ffffff\""),
        "{svg}"
    );
    let given = text_tag(&svg, "given");
    assert!(
        given.contains("fill=\"#ffff00\"") && given.matches("fill=").count() == 1,
        "{svg}"
    );
    // クラスタの題: 塗りか bgcolor の上なら読める色、なければ控えめな色
    let svg = render(
        "digraph { subgraph cluster_a { label=\"Filled\"; style=filled; fillcolor=\"#202060\"; a } subgraph cluster_b { label=\"Back\"; bgcolor=\"#ffeecc\"; b } subgraph cluster_c { label=\"Plain\"; c } subgraph cluster_d { label=\"Own\"; bgcolor=black; fontcolor=red; d } }",
    )
    .unwrap()
    .svg;
    assert!(
        text_tag(&svg, "Filled").contains("fill=\"#ffffff\""),
        "{svg}"
    );
    assert!(text_tag(&svg, "Back").contains("fill=\"#1f1f1f\""), "{svg}");
    assert!(
        text_tag(&svg, "Plain").contains("fill=\"var(--_m)\""),
        "{svg}"
    );
    let own = text_tag(&svg, "Own");
    assert!(
        own.contains("fill=\"#ff0000\"") && own.matches("fill=").count() == 1,
        "{svg}"
    );
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
        notes("digraph { node [colorscheme=blues9, style=filled, fillcolor=3]; a [color=\"/set312/12\"]; b [colorscheme=ylorrd9, fillcolor=9] }")
            .is_empty()
    );
    // ほかの配置は描ける。知らない名前と、近いもので済ませる属性は注記
    for engine in [
        "neato",
        "fdp",
        "sfdp",
        "twopi",
        "circo",
        "osage",
        "patchwork",
        "nop",
    ] {
        assert!(
            notes(&format!("graph {{ layout={engine}; a -- b -- c; a -- c }}")).is_empty(),
            "{engine}"
        );
    }
    assert_eq!(
        notes("graph { layout=spring; a -- b }"),
        [Note::OtherLayouts]
    );
    assert_eq!(
        notes("graph { layout=neato; overlap=compress; a -- b }"),
        [Note::LayoutOptions]
    );
    assert_eq!(
        notes("graph { layout=neato; mode=hier; a -- b }"),
        [Note::LayoutOptions]
    );
    assert_eq!(
        notes("graph { layout=osage; packmode=clust; a -- b }"),
        [Note::LayoutOptions]
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
    for engine in [
        "dot",
        "neato",
        "fdp",
        "sfdp",
        "twopi",
        "circo",
        "osage",
        "patchwork",
    ] {
        let started = std::time::Instant::now();
        let d = render(&format!("digraph {{ layout={engine}; {body} }}")).unwrap();
        assert!(d.svg.len() < amonhen_dot::MAX_OUTPUT);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "{engine}"
        );
    }
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
        "samehead",
        "sametail",
        "newrank",
        "labelloc",
        "b",
        "<<table border=\"3\" cellspacing=\"-9\"><tr><td rowspan=\"9\" port=\"p\">a</td><td colspan=\"0\"><table><tr><td><img/></td></tr></table></td></tr><hr/><tr><td>",
        "\"</td><vr/><td fixedsize=\\\"true\\\" width=\\\"-1\\\">\"",
        "\"a:p\"",
        "\"/set312/12\"",
        "layout",
        "neato",
        "fdp",
        "sfdp",
        "twopi",
        "circo",
        "osage",
        "patchwork",
        "nop",
        "pos",
        "\"1,2!\"",
        "\"-1e9,3\"",
        "pin",
        "overlap",
        "scale",
        "prism",
        "sep",
        "\"+-4,1e9\"",
        "len",
        "root",
        "start",
        "random7",
        "regular",
        "mindist",
        "area",
        "packmode",
        "\"array_c3u\"",
        "sortv",
        "K",
        "maxiter",
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

#[test]
fn samehead_edges_meet_at_one_point() {
    let svg =
        render("digraph { a -> d [samehead=h]; b -> d [samehead=h]; c -> d [samehead=h]; x -> d }")
            .unwrap()
            .svg;
    let arrows: Vec<&str> = svg
        .split("<polygon points=\"")
        .skip(1)
        .map(|s| s.split(' ').next().unwrap())
        .collect();
    assert_eq!(arrows.len(), 4);
    // 同じ値の 3 本の矢じりの先は同じ点、ほかの 1 本は別
    let first = arrows[0];
    assert_eq!(
        arrows.iter().filter(|a| **a == first).count(),
        3,
        "{arrows:?}"
    );
}
