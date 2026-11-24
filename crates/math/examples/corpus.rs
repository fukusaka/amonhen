//! 数式の描き手を測る・目で確かめるための道具（テストではない）。
//!
//! - `cargo run --release -p amonhen-math --example corpus -- corpus <file> <outdir>`: 1 行 1 式を描いて
//!   `<outdir>/NN.svg` に書き、式ごとの時間（15 回の中央値）を出す。
//! - `… -- one <tex>`: 1 つの式を描いて、時間と結果を出す。
//! - `… -- stress`: 原文の上限（`MAX_SOURCE`）の中で重い入力を描いて、時間と結果を出す。

use std::time::Instant;

fn time(tex: &str, display: bool) -> (f64, String) {
    let t = Instant::now();
    let r = amonhen_math::render(tex, display);
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let status = match r {
        Ok(f) => format!(
            "OK svg={}B items~{} w={:.2} h={:.2} d={:.2}",
            f.svg.len(),
            f.svg.matches("<path").count()
                + f.svg.matches("<rect").count()
                + f.svg.matches("<text").count(),
            f.width,
            f.height,
            f.depth
        ),
        Err(e) => format!("ERR {e:?}").chars().take(120).collect(),
    };
    (ms, status)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("corpus") => {
            let text = std::fs::read_to_string(&args[2]).unwrap();
            let out = &args[3];
            std::fs::create_dir_all(out).unwrap();
            let mut medians = Vec::new();
            for (i, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
                let mut times = Vec::new();
                let mut result = None;
                for round in 0..15 {
                    // 覚えたものを使わないよう、回ごとに後ろの空白を変える
                    let tex = format!("{line}{}", " ".repeat(round));
                    let t = Instant::now();
                    let r = amonhen_math::render(&tex, true);
                    times.push(t.elapsed().as_secs_f64() * 1e3);
                    result.get_or_insert(r);
                }
                times.sort_by(f64::total_cmp);
                let m = times[times.len() / 2];
                match result.unwrap() {
                    Ok(f) => {
                        std::fs::write(format!("{out}/{:02}.svg", i + 1), &f.svg).unwrap();
                        medians.push(m);
                        println!("{:02} OK  {m:7.3} ms {:6} B | {line}", i + 1, f.svg.len());
                    }
                    Err(e) => println!("{:02} ERR {m:7.3} ms {e:?} | {line}", i + 1),
                }
            }
            medians.sort_by(f64::total_cmp);
            println!(
                "median {:.3} ms, max {:.3} ms, n={}",
                medians[medians.len() / 2],
                medians.last().unwrap(),
                medians.len()
            );
        }
        Some("stress") => {
            let max = amonhen_math::MAX_SOURCE;
            let fit = |unit: &str| max / unit.len();
            let side = (1..)
                .take_while(|n| n * n * 2 + n * 2 + 30 < max)
                .last()
                .unwrap();
            let row = vec!["x"; side].join("&");
            let cases: Vec<(String, String)> = vec![
                ("x+ repeated".into(), "x+".repeat(fit("x+") - 1)),
                (format!("matrix {side}x{side}"), format!(r"\begin{{matrix}}{}\end{{matrix}}", vec![row.clone(); side].join(r"\\"))),
                ("frac 30 deep".into(), format!("{}x{}", r"\frac{1}{".repeat(30), "}".repeat(30))),
                ("sqrt nested 200".into(), format!("{}x{}", r"\sqrt{".repeat(200), "}".repeat(200))),
                ("left( 1000".into(), format!("{}x{}", r"\left(".repeat(1000), r"\right)".repeat(1000))),
                ("text CJK".into(), format!(r"\text{{{}}}", "日".repeat(fit("日") - 10))),
                ("overbrace chain".into(), r"\overbrace{x+x}^{a}".repeat(fit(r"\overbrace{x+x}^{a}"))),
                ("macro 10^4".into(), r"\def\a{xxxxxxxxxx}\def\b{\a\a\a\a\a\a\a\a\a\a}\def\c{\b\b\b\b\b\b\b\b\b\b}\def\d{\c\c\c\c\c\c\c\c\c\c}\d".into()),
                ("macro 10^5".into(), r"\def\a{xxxxxxxxxx}\def\b{\a\a\a\a\a\a\a\a\a\a}\def\c{\b\b\b\b\b\b\b\b\b\b}\def\d{\c\c\c\c\c\c\c\c\c\c}\def\e{\d\d\d\d\d\d\d\d\d\d}\e".into()),
                ("macro bomb".into(), r"\def\a{\a\a}\a".into()),
                ("array hline".into(), format!(r"\begin{{array}}{{|c|c|}}{}\end{{array}}", r"\hline x&y\\".repeat(fit(r"\hline x&y\\") - 4))),
                ("dashed tall".into(), r"\begin{array}{:c:}\rule{1em}{1000em}\end{array}".into()),
                ("color many".into(), r"\color{red}x\color{blue}y".repeat(fit(r"\color{red}x\color{blue}y"))),
                ("sum big".into(), r"\sum_{i=1}^{n}\int_0^1".repeat(fit(r"\sum_{i=1}^{n}\int_0^1"))),
            ];
            for (name, tex) in cases {
                let (ms, status) = time(&tex, true);
                println!("{name:16} {:5} B {ms:8.2} ms {status}", tex.len());
            }
        }
        Some("one") => {
            let (ms, status) = time(&args[2], true);
            println!("{ms:.2} ms {status}");
        }
        _ => eprintln!("usage: corpus <file> <outdir> | stress | one <tex>"),
    }
}
