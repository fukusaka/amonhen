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
        "flat_labels",
        "digraph { a -> {b c d}; {rank=same; b -> d [label=\"skips c\"]; b -> c [label=\"next\\nto\"]} }",
    ),
    (
        "loops",
        "digraph { x -> a; a -> a [tailport=n, headport=n, label=n]; a:w -> a:w; a -> a [tailport=s]; a -> a; a -> b; b -> b }",
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
        "fdp_pin",
        "graph { layout=fdp; subgraph cluster_a { label=A; a1 [pos=\"0,0!\"]; a2 [pos=\"2,-1!\"]; a1 -- a2 -- a3 } b -- a3; b -- c; c -- a1 }",
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
        "ortho_dot",
        "digraph { splines=ortho; node [shape=box]; a -> {b c d}; a:w -> e; b:s -> e:w; {c d} -> e [samehead=h]; c:se -> f:ne; x -> f }",
    ),
    (
        "ortho_tracks",
        "digraph { splines=ortho; a -> {b c d e f}; {b c d e f} -> g; b -> d; c -> f; a -> g; a -> h -> g }",
    ),
    (
        "loops_labels",
        "digraph { x -> a; a -> a [tailport=n, headport=n, label=n]; a:w -> a:w; a -> a; a -> {b c}; {rank=same; b -> c [label=\"next\\nto\"]} }",
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
    (
        "ortho_neato",
        "graph { layout=neato; overlap=false; splines=ortho; node [shape=box]; c -- {a b d e f g}; a -- b; d -- e; f -- g; a -- g; c -- d }",
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

/// ノードの中のラベルの文字の y（ノードの `data-id` で）。
fn label_y(svg: &str, id: &str) -> f64 {
    let node = svg
        .split(&format!("data-id=\"{id}\""))
        .nth(1)
        .unwrap()
        .split("class=\"dot-label\"")
        .nth(1)
        .unwrap();
    node.split("y=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .and_then(|v| v.parse().ok())
        .unwrap()
}

#[test]
fn places_node_labels_by_labelloc() {
    let svg = render("digraph { node [shape=box, height=1.5]; t [labelloc=t]; c; b [labelloc=b]; e [shape=ellipse, labelloc=top] }")
        .unwrap()
        .svg;
    let (t, c, b, e) = (
        label_y(&svg, "t"),
        label_y(&svg, "c"),
        label_y(&svg, "b"),
        label_y(&svg, "e"),
    );
    // 高さ 108 の箱: 上下にそれぞれ (108 − 文字 16.8 − 余白 7.92) / 2 ずつ
    assert!((c - t - 41.64).abs() < 0.1, "{t} {c}");
    assert!((b - c - 41.64).abs() < 0.1, "{b} {c}");
    // 楕円は内に収まる高さ（108 / √2）の中で上に
    assert!(e < c && c - e < 41.64 && c - e > 20.0, "{e} {c}");
}

#[test]
fn decorates_edge_labels() {
    let svg = render(
        "digraph { a -> b [label=\"dec\", decorate=true, color=red]; c -> d [label=\"plain\"] }",
    )
    .unwrap()
    .svg;
    let labels = svg.split("class=\"dot-labels\"").nth(1).unwrap();
    assert_eq!(labels.matches("<path ").count(), 1, "{svg}");
    assert!(labels.contains("fill=\"none\" stroke=\"#ff0000\""), "{svg}");
}

#[test]
fn tapers_edges() {
    let svg = render("digraph { a -> b [style=tapered, penwidth=7, color=blue]; c -> d [style=tapered, dir=none] }")
        .unwrap()
        .svg;
    let edge = svg.split("data-id=\"a-&gt;b\"").nth(1).unwrap();
    // 線ではなく塗った外形
    assert!(edge.starts_with("><path d=\"M"), "{svg}");
    assert!(
        edge.contains("Z\" fill=\"#0000ff\" stroke=\"none\""),
        "{svg}"
    );
    assert!(!svg.contains("fill=\"none\" stroke=\"#0000ff\""), "{svg}");
}

/// `data-id` の要素の中身（次の `data-id` まで）。
fn part<'a>(svg: &'a str, id: &str) -> &'a str {
    let rest = svg.split(&format!("data-id=\"{id}\"")).nth(1).unwrap();
    rest.split("data-id=").next().unwrap()
}

#[test]
fn defaults_stay_readable_on_given_grounds() {
    let svg = render(
        "digraph { subgraph cluster_a { bgcolor=\"#ffeeee:#eeeeff\"; node [shape=plain]; a -> b [label=\"e\"]; c [fontcolor=red]; d [shape=box]; b -> c [color=blue] } subgraph cluster_n { style=filled; fillcolor=navy; n [shape=none] } x [shape=plaintext]; x -> a; b -> n }",
    )
    .unwrap()
    .svg;
    // 明るい塗りの上の plain のノードの文字と、中の辺・矢じり・ラベルは黒に近い色
    assert!(part(&svg, "a").contains("fill=\"#1f1f1f\""), "{svg}");
    let edge = part(&svg, "a-&gt;b");
    assert!(
        edge.contains("stroke=\"#1f1f1f\"") && edge.contains("fill=\"#1f1f1f\""),
        "{svg}"
    );
    assert!(!edge.contains("var(--_l)"), "{svg}");
    let label = svg.split("class=\"dot-edge-label\"").nth(1).unwrap();
    assert!(
        label
            .split('>')
            .next()
            .unwrap()
            .contains("fill=\"#1f1f1f\""),
        "{svg}"
    );
    // 文の色が勝つ。塗りの指定のない枠のノードも塗らないので、枠と文字は地の上で読める色
    assert!(part(&svg, "c").contains("fill=\"#ff0000\""), "{svg}");
    assert!(!part(&svg, "c").contains("#1f1f1f"), "{svg}");
    let d = part(&svg, "d");
    assert!(
        d.contains("fill=\"none\" stroke=\"#1f1f1f\"")
            && d.contains("class=\"dot-label\" fill=\"#1f1f1f\""),
        "{svg}"
    );
    assert!(
        part(&svg, "b-&gt;c").contains("stroke=\"#0000ff\""),
        "{svg}"
    );
    // 濃い塗りの上は白、地をまたぐ辺・地のないノードはテーマの色
    assert!(part(&svg, "n").contains("fill=\"#ffffff\""), "{svg}");
    assert!(part(&svg, "b-&gt;n").contains("var(--_l)"), "{svg}");
    assert!(part(&svg, "x-&gt;a").contains("var(--_l)"), "{svg}");
    assert!(!part(&svg, "x").contains("#1f1f1f"), "{svg}");
    // グラフの bgcolor の上も同じ（題も）
    let dark = render("digraph { bgcolor=black; label=\"T\"; node [shape=plaintext]; p -> q }")
        .unwrap()
        .svg;
    assert!(part(&dark, "p").contains("fill=\"#ffffff\""), "{dark}");
    assert!(
        part(&dark, "p-&gt;q").contains("stroke=\"#ffffff\""),
        "{dark}"
    );
    let title = dark.split("class=\"dot-title\"").nth(1).unwrap();
    assert!(
        title.split('>').next().unwrap().contains("#ffffff"),
        "{dark}"
    );
}

/// 最初の `<rect`・`<ellipse` などの属性の数。
fn attr_num(s: &str, name: &str) -> f64 {
    s.split(&format!(" {name}=\""))
        .nth(1)
        .and_then(|r| r.split('"').next())
        .and_then(|v| v.parse().ok())
        .unwrap()
}

fn measured(text: &str) -> f64 {
    amonhen_dot::text::plain_label(
        text,
        &amonhen_dot::text::Names::default(),
        &amonhen_dot::text::Font::new(14.0),
    )
    .width
}

/// アプリの書体が測り（Arial）より 15% 広くても、クラスタの題・枠のあるノードの文字は線に触れない。
#[test]
fn leaves_room_for_wider_app_fonts() {
    let title = "A rather long cluster title that is wider than its nodes";
    let need = measured(title) * 1.15 + 16.0;
    for (dir, layout) in [
        ("TB", "dot"),
        ("LR", "dot"),
        ("RL", "dot"),
        ("BT", "dot"),
        ("TB", "fdp"),
        ("TB", "osage"),
        ("TB", "neato"),
    ] {
        let svg = render(&format!(
            "digraph {{ rankdir={dir}; layout={layout}; subgraph cluster_a {{ label=\"{title}\"; x }} w -> x }}"
        ))
        .unwrap()
        .svg;
        let cluster = svg.split("class=\"dot-cluster\"").nth(1).unwrap();
        let width = attr_num(cluster, "width");
        assert!(width >= need - 0.01, "{dir} {layout}: {width} < {need}");
    }
    // 枠のあるノードと record の欄
    let long = "a label long enough to set the width";
    let svg = render(&format!(
        "digraph {{ b [shape=box, label=\"{long}\"]; r [shape=record, label=\"{long}|x\"] }}"
    ))
    .unwrap()
    .svg;
    let w = measured(long) * 1.15;
    let b = svg.split("data-id=\"b\"").nth(1).unwrap();
    let bw = attr_num(b, "width");
    assert!(bw >= w + 2.0 * 7.9, "box {bw} < {w}");
    // record の欄: 長い方の欄の左右に 8 ずつ（欄の線は 2 本目の x）
    let r = svg.split("data-id=\"r\"").nth(1).unwrap();
    let left = attr_num(r, "x");
    let sep: f64 = r
        .split("<path d=\"M")
        .nth(1)
        .and_then(|p| p.split(',').next())
        .and_then(|v| v.parse().ok())
        .unwrap();
    assert!(sep - left >= w + 16.0 - 0.01, "record {} < {w}", sep - left);
}

#[test]
fn stripes_clusters() {
    let svg = render(
        "digraph { subgraph cluster_a { style=striped; fillcolor=\"red;0.5:green:blue\"; a; b } }",
    )
    .unwrap()
    .svg;
    let cluster = svg.split("class=\"dot-cluster\"").nth(1).unwrap();
    for color in ["#ff0000", "#00ff00", "#0000ff"] {
        assert!(
            cluster.contains(&format!("fill=\"{color}\" stroke=\"none\"")),
            "{svg}"
        );
    }
    // 帯の上に枠。グラデーションは作らない
    assert!(cluster.contains("fill=\"none\" stroke="), "{svg}");
    assert!(!svg.contains("Gradient"), "{svg}");
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
    assert!(svg.contains("var(--_l)") && svg.contains("var(--_b)"));
    assert!(svg.contains(">a</text>") && svg.contains(">b</text>"));
    // 塗りの指定のないノードは塗らない（Graphviz と同じく透明）
    assert!(
        !svg.contains("var(--_s)") && !svg.contains("var(--_a)"),
        "{svg}"
    );
    let filled = render("digraph { a [style=filled] }").unwrap().svg;
    assert!(filled.contains("fill=\"var(--_a)\""), "{filled}");
    // クラスタも塗りの指定がなければ塗らない
    let svg = render("digraph { subgraph cluster_a { a } subgraph cluster_b { style=filled; b } }")
        .unwrap()
        .svg;
    assert!(
        part(&svg, "cluster_a").contains("fill=\"none\" stroke=\"var(--_b)\""),
        "{svg}"
    );
    assert!(
        part(&svg, "cluster_b").contains("fill=\"var(--_a)\" stroke"),
        "{svg}"
    );
}

#[test]
fn edges_cross_the_title_above_their_node() {
    // 題の真下のノードへの線は、題の脇に回らずまっすぐ（Graphviz と同じ）。ortho も
    for splines in ["", "splines=ortho;"] {
        let svg = render(&format!(
            "digraph {{ {splines} x; subgraph cluster_a {{ label=\"A long cluster title\"; a }} x -> a }}"
        ))
        .unwrap()
        .svg;
        let d = part(&svg, "x-&gt;a").split("d=\"M").nth(1).unwrap();
        let d = d.split('"').next().unwrap();
        let xs: Vec<f64> = d
            .split(['C', 'L', ' '])
            .filter_map(|p| p.split(',').next()?.parse().ok())
            .collect();
        assert!(
            xs.iter().all(|x| (x - xs[0]).abs() < 0.01),
            "{splines}: {svg}"
        );
    }
}

#[test]
fn ortho_edges_ignore_ports() {
    // Graphviz と同じく、splines=ortho の線はポートを見ない: `a:y:e` からの線も下の辺から出て、`b:n` への輪も
    // 右に回る（ポートの側から出るなら、a の右の辺から出て、輪は上に回る）
    let svg = render(
        "digraph { splines=ortho; node [shape=record]; a [label=\"<x> x|<y> y\"]; b; c; a:y:e -> b; a -> c; b -> b:n }",
    )
    .unwrap()
    .svg;
    let start = |id: &str| {
        let d = part(&svg, id).split("d=\"M").nth(1).unwrap();
        let mut it = d.split(['C', ',']);
        let x: f64 = it.next().unwrap().parse().unwrap();
        let y: f64 = it.next().unwrap().parse().unwrap();
        (x, y)
    };
    let (x, y) = start("a-&gt;b");
    let a = part(&svg, "a");
    let (left, top) = (attr_num(a, "x"), attr_num(a, "y"));
    let bottom = top + attr_num(a, "height");
    assert!(
        x > left + 0.5 && x < left + attr_num(a, "width") - 0.5 && (y - bottom).abs() < 0.01,
        "{svg}"
    );
    let (x, _) = start("b-&gt;b");
    let b = part(&svg, "b");
    let right = attr_num(b, "x") + attr_num(b, "width");
    assert!((x - right).abs() < 0.01, "{svg}");
}

#[test]
fn fontname_picks_the_typeface() {
    let svg = render(
        "digraph { node [fontname=\"Helvetica,Arial,sans-serif\"]; a; b [fontname=\"Courier-Bold\"]; c [label=<x <font face=\"Times\">y</font>>] }",
    )
    .unwrap()
    .svg;
    assert!(
        text_tag(&svg, "a").contains("font-family=\"Helvetica, Arial, sans-serif\""),
        "{svg}"
    );
    let b = text_tag(&svg, "b");
    assert!(
        b.contains("font-weight=\"bold\"")
            && b.contains("font-family=\"Courier, 'Courier New', var(--_fm)\""),
        "{svg}"
    );
    assert!(
        svg.contains("font-family=\"Times, 'Times New Roman', serif\">y</tspan>"),
        "{svg}"
    );
}

#[test]
fn pads_the_drawing_as_graphviz_does() {
    // 既定は 4 点、`pad` はインチ（x,y か 1 つ）
    let size = |src: &str| {
        let svg = render(src).unwrap().svg;
        let view = svg.split("viewBox=\"0 0 ").nth(1).unwrap();
        let mut it = view.split('"').next().unwrap().split(' ');
        let w: f64 = it.next().unwrap().parse().unwrap();
        let h: f64 = it.next().unwrap().parse().unwrap();
        (w, h)
    };
    assert_eq!(size("digraph { a }"), (62.0, 44.0));
    assert_eq!(size("digraph { pad=\"1,0.5\"; a }"), (198.0, 108.0));
    assert_eq!(size("digraph { pad=0; a }"), (54.0, 36.0));
    assert_eq!(size("digraph { pad=nope; a }"), (62.0, 44.0));
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
    // クラスタの題: 塗りか bgcolor の上なら読める色、なければ文字の色（fill を書かず、図の文字の色を継ぐ）
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
    assert!(!text_tag(&svg, "Plain").contains("fill="), "{svg}");
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
    // 見本の大きさの図は、手間の上限で打ち切らない
    for (name, src) in SAMPLES {
        assert!(!notes(src).contains(&Note::WorkLimit), "{name}");
    }
}

#[test]
fn refuses_what_is_too_big() {
    let long = format!("digraph {{ a }}{}", " ".repeat(50_000));
    assert_eq!(
        render(&long).unwrap_err(),
        amonhen_dot::Error::TooLong { limit: 50_000 }
    );
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
    assert_eq!(
        render(&product).unwrap_err(),
        amonhen_dot::Error::TooManyEdges { limit: 5_000 }
    );
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
        // 手間の上限で打ち切った配置は注記する（dot は単体法の入れ替え、neato は stress の回数、fdp は力の回数）
        if matches!(engine, "dot" | "neato" | "fdp") {
            assert!(
                d.notes.contains(&Note::WorkLimit),
                "{engine}: {:?}",
                d.notes
            );
        }
        // 仮のノードの上限を超えた長い辺は直線にして注記する（dot だけ）
        assert_eq!(
            d.notes.contains(&Note::LongEdges),
            engine == "dot",
            "{engine}: {:?}",
            d.notes
        );
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
        "tailport",
        "headport",
        "se",
        "w",
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

/// `splines=ortho` の線はどの配置でも縦か横の線だけ（ポート・samehead・ラベル・クラスタ・向き入り）。
#[test]
fn ortho_edges_are_axis_aligned() {
    let body = "node [shape=record]; a [label=\"<p> x|<q> y|{z|<r> w}\"]; b; c [shape=ellipse]; \
        d [shape=box]; subgraph cluster_k { label=K; e; f } \
        a:p -> b:n; a:r:s -> c; a -> d [samehead=h]; b -> d [samehead=h]; c:ne -> e:w; \
        d:se -> f; b -> f [label=l]; e -> a:q; f -> a; c -> c; {rank=same; b -> c [label=m]}";
    for engine in ["dot", "neato", "fdp", "sfdp", "twopi", "circo", "osage"] {
        for rankdir in ["TB", "LR", "BT", "RL"] {
            let src =
                format!("digraph {{ layout={engine}; rankdir={rankdir}; splines=ortho; {body} }}");
            let svg = render(&src).unwrap().svg;
            let edges = svg
                .split("class=\"dot-edge\"")
                .skip(1)
                .map(|g| g.split("</g>").next().unwrap());
            let paths = edges.flat_map(|g| {
                g.split("<path d=\"")
                    .skip(1)
                    .map(|s| s.split('"').next().unwrap())
            });
            let mut seen = 0;
            for d in paths {
                seen += 1;
                let nums: Vec<f64> = d
                    .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                    .filter(|t| !t.is_empty())
                    .filter_map(|t| t.parse().ok())
                    .collect();
                // M x y、それから C の 6 つ組（終わりの点は最後の 2 つ）
                let mut prev = (nums[0], nums[1]);
                for c in nums[2..].as_chunks::<6>().0 {
                    let next = (c[4], c[5]);
                    assert!(
                        (prev.0 - next.0).abs() < 0.02 || (prev.1 - next.1).abs() < 0.02,
                        "{engine} {rankdir}: {d}"
                    );
                    prev = next;
                }
            }
            assert!(seen >= 8, "{engine} {rankdir}: {seen}");
        }
    }
}

/// 辺ごとの線の折れ線（M と C の終わりの点）。
fn edge_polylines(svg: &str) -> Vec<Vec<(f64, f64)>> {
    svg.split("class=\"dot-edge\"")
        .skip(1)
        .filter_map(|g| {
            let g = g.split("</g>").next()?;
            let d = g.split("<path d=\"").nth(1)?.split('"').next()?;
            let nums: Vec<f64> = d
                .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                .filter(|t| !t.is_empty())
                .filter_map(|t| t.parse().ok())
                .collect();
            let mut pts = vec![(nums[0], nums[1])];
            for c in nums[2..].as_chunks::<6>().0 {
                pts.push((c[4], c[5]));
            }
            Some(pts)
        })
        .collect()
}

/// dot の ortho では、違う辺の線が同じ所に重ならない（samehead・sametail で 1 点に会う組を除く）。
#[test]
fn dot_ortho_edges_do_not_share_segments() {
    let graphs = [
        "digraph { splines=ortho; node [shape=box]; a -> {b c d}; a:w -> e; b:s -> e:w; {c d} -> e [samehead=h]; c:se -> f:ne; x -> f }",
        "digraph { splines=ortho; node [shape=box]; {a b c} -> d [samehead=h]; x -> d; d -> {e f g}; a -> g; b:s -> e:s; c:n -> f; {rank=same; e -> g [label=flat]} }",
        "digraph { splines=ortho; rankdir=LR; node [shape=record]; s1 [label=\"<f0> left|<f1> mid|<f2> right\"]; s2 [label=\"{a|{b|<p> c}}\"]; s3 [label=\"<x> x|<y> y\"]; s1:f2 -> s2:p; s1:f0 -> s2; s1:f1 -> s3:y; s3:x -> s2:p:n }",
        "digraph { splines=ortho; a -> {b c d e f}; {b c d e f} -> g; b -> d; c -> f; a -> g; a -> h -> g }",
        "digraph { splines=ortho; rankdir=BT; a -> {b c}; b -> {d e}; c -> {d e}; a -> d; a -> e }",
        // 真下に並ぶノードの尾と頭の端が同じ x にならない・込んだ側は横からも出る
        "digraph { splines=ortho; node [shape=box]; {a b c} -> {d e f} }",
        "digraph { splines=ortho; a -> {b c d e f g}; {b c d e f g} -> h }",
    ];
    for src in graphs {
        let svg = render(src).unwrap().svg;
        let lines = edge_polylines(&svg);
        // (辺, 縦か, 位置, 始め, 終わり)
        let mut segs: Vec<(usize, bool, f64, f64, f64)> = Vec::new();
        for (i, p) in lines.iter().enumerate() {
            for w in p.windows(2) {
                let (a, b) = (w[0], w[1]);
                if (a.0 - b.0).abs() < 0.02 && (a.1 - b.1).abs() > 0.02 {
                    segs.push((i, true, a.0, a.1.min(b.1), a.1.max(b.1)));
                } else if (a.1 - b.1).abs() < 0.02 && (a.0 - b.0).abs() > 0.02 {
                    segs.push((i, false, a.1, a.0.min(b.0), a.0.max(b.0)));
                }
            }
        }
        let meet = |a: usize, b: usize| {
            let (p, q) = (&lines[a], &lines[b]);
            let same = |x: (f64, f64), y: (f64, f64)| (x.0 - y.0).abs() + (x.1 - y.1).abs() < 0.5;
            same(p[0], q[0]) || same(p[p.len() - 1], q[q.len() - 1])
        };
        for (k, a) in segs.iter().enumerate() {
            for b in &segs[k + 1..] {
                if a.0 == b.0 || a.1 != b.1 || (a.2 - b.2).abs() > 0.5 || meet(a.0, b.0) {
                    continue;
                }
                let overlap = a.4.min(b.4) - a.3.max(b.3);
                assert!(overlap <= 3.0, "{src}: {a:?} {b:?}");
            }
        }
    }
}

/// dot の ortho で、1 つのランクの間に段が多く要るときは間を広げ、違う辺の横の線を段の間（6pt）より近く重ねない。
#[test]
fn dot_ortho_tracks_get_room_between_ranks() {
    let src = "digraph { splines=ortho; {a b c d} -> {e f g h} }";
    let svg = render(src).unwrap().svg;
    let lines = edge_polylines(&svg);
    // (辺, y, 始め, 終わり)
    let mut runs: Vec<(usize, f64, f64, f64)> = Vec::new();
    for (i, p) in lines.iter().enumerate() {
        for w in p.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a.1 - b.1).abs() < 0.02 && (a.0 - b.0).abs() > 3.0 {
                runs.push((i, a.1, a.0.min(b.0), a.0.max(b.0)));
            }
        }
    }
    assert!(runs.len() > 10, "{runs:?}");
    for (k, a) in runs.iter().enumerate() {
        for b in &runs[k + 1..] {
            if a.0 == b.0 || a.3.min(b.3) - a.2.max(b.2) <= 3.0 {
                continue;
            }
            assert!((a.1 - b.1).abs() >= 5.5, "{a:?} {b:?}");
        }
    }
}

/// dot の同じランクの弧は、ランクのすぐ上の空きを通る（上のランクのノードは越えない）。重なる弧は別の高さ。
#[test]
fn dot_flat_arcs_stay_below_the_rank_above() {
    let src = "digraph { p -> {b c d e}; q -> c; r -> d; {rank=same; b -> d; b -> e; c -> e} }";
    let svg = render(src).unwrap().svg;
    // 上のランク（p・q・r）の下の端
    let bottom = svg
        .split("<ellipse ")
        .skip(1)
        .filter_map(|e| {
            let num = |k: &str| -> Option<f64> {
                e.split(&format!("{k}=\""))
                    .nth(1)?
                    .split('"')
                    .next()?
                    .parse()
                    .ok()
            };
            Some((num("cy")?, num("ry")?))
        })
        .map(|(cy, ry)| cy + ry)
        .fold(f64::MAX, f64::min);
    let lines = edge_polylines(&svg);
    // 同じランクの弧は最後の 3 本
    let arcs = &lines[lines.len() - 3..];
    let mut tops: Vec<f64> = Vec::new();
    for arc in arcs {
        let top = arc.iter().map(|p| p.1).fold(f64::MAX, f64::min);
        assert!(top > bottom + 4.0, "{arc:?} above {bottom}");
        tops.push(top);
    }
    tops.sort_by(f64::total_cmp);
    for w in tops.windows(2) {
        assert!(w[1] - w[0] >= 4.0, "{tops:?}");
    }
}

/// dot の曲線は、ポートのある端も通り道の中を進む: 下へ向くポートから出て上から入る線は、上から下へ一方向に進む。
#[test]
fn dot_port_edges_run_one_way() {
    let src = "digraph { a -> {x y z}; a:s -> c:n; b:s -> c:n; {x y z} -> c }";
    let svg = render(src).unwrap().svg;
    let lines = edge_polylines(&svg);
    for line in &lines[3..5] {
        for w in line.windows(2) {
            assert!(w[1].1 >= w[0].1 - 0.5, "{line:?}");
        }
    }
}

/// dot の ortho で、ランクの間を広げる上限（ranksep）を超えるほど段が要っても、違う辺の横の線は 3pt より近く重ねない。
#[test]
fn dot_ortho_tracks_keep_a_least_gap() {
    let src = "digraph { splines=ortho; {a b c d e f g h} -> {i j k l m n o p} }";
    let svg = render(src).unwrap().svg;
    let lines = edge_polylines(&svg);
    let mut runs: Vec<(usize, f64, f64, f64)> = Vec::new();
    for (i, p) in lines.iter().enumerate() {
        for w in p.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a.1 - b.1).abs() < 0.02 && (a.0 - b.0).abs() > 3.0 {
                runs.push((i, a.1, a.0.min(b.0), a.0.max(b.0)));
            }
        }
    }
    for (k, a) in runs.iter().enumerate() {
        for b in &runs[k + 1..] {
            if a.0 == b.0 || a.3.min(b.3) - a.2.max(b.2) <= 3.0 {
                continue;
            }
            assert!((a.1 - b.1).abs() >= 2.9, "{a:?} {b:?}");
        }
    }
}

/// dot の横の位置は、同じ費用で動ける幅の真ん中: 2 つの子を持つノードは子の間に、その上の 1 本道も一緒に。
#[test]
fn dot_centers_nodes_between_their_children() {
    let cx = |svg: &str, id: &str| attr_num(part(svg, id), "cx");
    // ラベルがあるとランクが倍になり、辺は仮のノードを通る
    for src in [
        "digraph { x -> a; a -> {b c}; {rank=same; b -> c} }",
        "digraph { x -> a; a -> {b c}; {rank=same; b -> c [label=\"next\"]} }",
    ] {
        let svg = render(src).unwrap().svg;
        let mid = (cx(&svg, "b") + cx(&svg, "c")) / 2.0;
        assert!((cx(&svg, "a") - mid).abs() <= 1.0, "{svg}");
        assert!((cx(&svg, "x") - cx(&svg, "a")).abs() <= 1.0, "{svg}");
    }
    let svg = render(
        "digraph { s -> {a b}; subgraph cluster_a { a -> a2 } subgraph cluster_b { b -> b2 } }",
    )
    .unwrap()
    .svg;
    let mid = (cx(&svg, "a") + cx(&svg, "b")) / 2.0;
    assert!((cx(&svg, "s") - mid).abs() <= 1.0, "{svg}");
}

/// dot の同じランクの辺は、並べ替えの中で尾を頭の左に保つ。後から直すと、上のランクの辺が交差していた。
#[test]
fn dot_keeps_flat_edge_order_while_ordering() {
    let svg = render(
        "digraph { node [shape=box]; impl -> exe; cpp -> link; {rank=same exe link} link -> exe }",
    )
    .unwrap()
    .svg;
    let x = |id: &str| {
        let p = part(&svg, id);
        attr_num(p, "x") + attr_num(p, "width") / 2.0
    };
    assert!(x("link") < x("exe"), "{svg}");
    assert!(x("cpp") < x("impl"), "{svg}");
}

/// 見えない辺は同じランクの弧の段を取らない: 隣どうしの見える辺は、見えない辺があっても真っすぐ通る。
#[test]
fn dot_invisible_flat_edges_leave_the_straight_lane() {
    let with = render("digraph { {rank=same a b} a -> b [style=invis]; a -> b }")
        .unwrap()
        .svg;
    let without = render("digraph { {rank=same a b} a -> b }").unwrap().svg;
    let path = |svg: &str| {
        svg.split("<path")
            .nth(1)
            .unwrap()
            .split("/>")
            .next()
            .unwrap()
            .to_owned()
    };
    assert_eq!(path(&with), path(&without), "{with}");
}

/// dot の並べ替えはポートの左右を見る: 右のポートへ着く辺の尾は、左のポートへ着く辺の尾の右に（Graphviz と同じ）。
#[test]
fn dot_orders_nodes_by_their_ports() {
    let svg = render(
        "digraph { node [shape=record]; r [label=\"<l>left|<r>right\"]; a; b; a -> r:r; b -> r:l }",
    )
    .unwrap()
    .svg;
    let x = |id: &str| attr_num(part(&svg, id), "x");
    assert!(x("b") < x("a"), "{svg}");
}

/// ratio はノードの大きさを変えずに位置を伸ばして縦横の比を合わせる。数ならその比、fill は size の比。
#[test]
fn dot_ratio_stretches_positions() {
    let view = |src: &str| {
        let svg = render(src).unwrap().svg;
        let v: Vec<f64> = svg
            .split("viewBox=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .split(' ')
            .map(|n| n.parse().unwrap())
            .collect();
        (v[2], v[3], svg)
    };
    let (w, h, svg) = view("digraph { ratio=2; a -> {b c d} }");
    assert!((h / w - 2.0).abs() < 0.1, "{w} {h}");
    assert!(
        (attr_num(part(&svg, "a"), "rx") - 27.0).abs() < 0.01,
        "{svg}"
    );
    let (w, h, _) = view("digraph { rankdir=LR; ratio=fill; size=\"7,1\"; a -> {b c d} }");
    assert!((h / w - 1.0 / 7.0).abs() < 0.03, "{w} {h}");
    // ratio がなければ size は配置を変えない
    assert_eq!(
        view("digraph { a -> {b c d} }").0,
        view("digraph { size=\"1,9\"; a -> {b c d} }").0
    );
}

/// クラスタをまたぐ閉路で、どちらを上にしても辺の長さが同じなら、背の高いクラスタが上（同じ高さなら先に書いたほう）。
/// Graphviz の絵と同じ。
#[test]
fn dot_puts_the_taller_cluster_first_in_a_tied_cycle() {
    let cy = |svg: &str, id: &str| attr_num(part(svg, id), "cy");
    for src in [
        "digraph { subgraph cluster_small { a -> b } subgraph cluster_big { p -> q -> r -> s -> t; t -> p } t -> a; b -> q }",
        "digraph { subgraph cluster_b { b1 } subgraph cluster_a { a1 -> a2 -> a3 } a3 -> b1; b1 -> a1 }",
    ] {
        let svg = render(src).unwrap().svg;
        let (top, bottom) = if src.contains("cluster_small") {
            ("p", "a")
        } else {
            ("a1", "b1")
        };
        assert!(cy(&svg, top) < cy(&svg, bottom), "{svg}");
    }
    let svg = render("digraph { subgraph cluster_x { x1 -> x2 } subgraph cluster_y { y1 -> y2 } y2 -> x1; x2 -> y1 }")
        .unwrap()
        .svg;
    assert!(cy(&svg, "x1") < cy(&svg, "y1"), "{svg}");
}

/// rank=min のノードへ入る辺は逆にするが、閉路を探すときは書いた向きの辺を先にたどる（Graphviz の絵と同じ）。
#[test]
fn dot_follows_written_edges_before_turned_ones() {
    let svg = render("digraph { e; d; r; e -> r; r -> d; d -> e; r -> e; {rank=min; d} }")
        .unwrap()
        .svg;
    let cy = |id: &str| attr_num(part(&svg, id), "cy");
    assert!(cy("d") < cy("e") && cy("e") < cy("r"), "{svg}");
}

/// 形の大きさは Graphviz の絵で測った大きさ（ラベルの箱 w×h を余白なしで入れたとき）。
#[test]
fn node_shapes_size_like_graphviz() {
    let size = |shape: &str, w: u32, h: u32| {
        let src = format!(
            "digraph {{ pad=0; n [shape={shape} margin=0 penwidth=0 label=<<table fixedsize=\"true\" width=\"{w}\" height=\"{h}\" border=\"0\" cellpadding=\"0\" cellspacing=\"0\"><tr><td></td></tr></table>>] }}"
        );
        let svg = render(&src).unwrap().svg;
        let v: Vec<f64> = svg
            .split("viewBox=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .split(' ')
            .map(|n| n.parse().unwrap())
            .collect();
        (v[2].round(), v[3].round())
    };
    let near = |got: (f64, f64), want: (f64, f64)| {
        (got.0 - want.0).abs() <= 1.0 && (got.1 - want.1).abs() <= 1.0
    };
    for (shape, w, h, want) in [
        ("ellipse", 40, 40, (57.0, 57.0)),
        ("ellipse", 100, 10, (104.0, 36.0)),
        ("triangle", 100, 20, (241.0, 40.0)),
        ("diamond", 40, 40, (80.0, 80.0)),
        ("house", 100, 10, (129.0, 36.0)),
        ("hexagon", 200, 5, (233.0, 36.0)),
        ("Msquare", 40, 40, (40.0, 40.0)),
        ("cylinder", 40, 40, (54.0, 55.0)),
        ("star", 100, 20, (262.0, 249.0)),
    ] {
        let got = size(shape, w, h);
        assert!(near(got, want), "{shape} {w}x{h}: {got:?}, want {want:?}");
    }
}

/// rotate=90・orientation=landscape は絵を左に 90° 回す（幅と高さが入れ替わる）。
#[test]
fn landscape_rotates_the_drawing() {
    let view = |src: &str| {
        let svg = render(src).unwrap().svg;
        let v: Vec<f64> = svg
            .split("viewBox=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .split(' ')
            .map(|n| n.parse().unwrap())
            .collect();
        (v[2], v[3], svg)
    };
    let (w, h, _) = view("digraph { a -> b -> c }");
    for src in [
        "digraph { rotate=90; a -> b -> c }",
        "digraph { orientation=landscape; a -> b -> c }",
    ] {
        let (rw, rh, svg) = view(src);
        assert_eq!((rw, rh), (h, w), "{svg}");
        assert!(svg.contains("rotate(-90)"), "{svg}");
    }
}

/// shape=plain は幅と高さの属性を読まず（fixedsize なら大きさ 0）、塗らない（Graphviz の絵と同じ）。
#[test]
fn plain_nodes_ignore_size_and_fill() {
    let svg = render(
        "digraph { node [shape=rectangle, fixedsize=true, width=2.5, style=filled, fillcolor=red]; a -> e -> b; e [shape=plain, label=\"\"]; t [shape=plain, fixedsize=false, label=\"text\"] }",
    )
    .unwrap()
    .svg;
    let e = part(&svg, "e");
    assert!(!e.contains("<rect") && !e.contains("#ff0000"), "{svg}");
    assert!(!part(&svg, "t").contains("#ff0000"), "{svg}");
}
