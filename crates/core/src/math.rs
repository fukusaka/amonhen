//! Markdown の数式（GitHub と同じ書き方: ```` ```math ````・`$$…$$` のブロックと、文中の `$…$`・`` $`…`$ ``）を描く
//! （History の「すべてのファイル」の整形表示）。描くのは `amonhen-math`（RaTeX）で、`math` フィーチャーのときだけ入る
//! （アプリ（FFI・GTK4）と VSCode 拡張の絵の手伝い（amonhen-render）だけが入れ、エンジン（CLI・daemon）には入れない）。
//! 入れないときも書き方は読み、FE は原文をコードとして見せる。
//!
//! 形と位置（SVG）は Markdown を読むときに一度だけ作る。色は CSS の変数 `--math-fg` のままなので、FE は描くときに
//! `colorize` で文字の色を当てる（図と同じ `svgcss` の道。テーマや明るい・暗いを切り替えても、色だけを当て直せばよい）。
//! 大きさは em で、FE は本文の字の大きさ × `FONT_SCALE` を掛けて描く。

use serde::{Serialize, Serializer};

use crate::markdown::Block;
#[cfg(feature = "math")]
use crate::markdown::Run;

/// 数式の色の CSS の変数（FE が文字の色を当てる。`amonhen_math::COLOR_VAR` と同じ）。
pub const COLOR_VAR: &str = "--math-fg";

/// 本文の字の大きさに対する、数式の 1 em の大きさの既定（KaTeX と同じ 1.21 倍。KaTeX のフォントは字が小さめなので、
/// 本文の字とそろえるために大きくする）。FE の設定（Settings > テーマ > 数式 > 大きさ）で変えられる。
pub const FONT_SCALE: f64 = 1.21;

/// 設定で選べる大きさ（本文の字に対する倍率。`FONT_SCALE` が既定）。
pub const FONT_SCALES: [f64; 6] = [1.0, 1.1, FONT_SCALE, 1.3, 1.4, 1.5];

/// 設定で選べる、1 つのファイルで式を描く時間の上限（秒。0 は上限なし。2 が既定）。
pub const TIME_LIMITS: [u64; 5] = [1, 2, 5, 10, 0];

/// 1 つのファイルで式を描く時間の上限（ミリ秒。`u64::MAX` は上限なし）。
static FILE_TIME_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(2_000);

/// 1 つのファイルで式を描く時間の上限を変える（FE の設定。`None` は上限なし）。次に読む Markdown から効く。
/// 上限なしでも、式の数・原文と SVG の大きさの上限は残る。
pub fn set_file_time_limit(limit: Option<std::time::Duration>) {
    let ms = limit.map_or(u64::MAX, |d| {
        u64::try_from(d.as_millis()).unwrap_or(u64::MAX - 1)
    });
    FILE_TIME_MS.store(ms, std::sync::atomic::Ordering::Relaxed);
}

/// 今の、1 つのファイルで式を描く時間の上限。
#[cfg(feature = "math")]
fn file_time_limit() -> std::time::Duration {
    match FILE_TIME_MS.load(std::sync::atomic::Ordering::Relaxed) {
        u64::MAX => std::time::Duration::MAX,
        ms => std::time::Duration::from_millis(ms),
    }
}

/// 描いた数式。大きさは em（本文の字の大きさ × `FONT_SCALE` を 1 とする）。
#[derive(Clone, Debug, Serialize)]
pub struct MathPicture {
    /// SVG（色は `--math-fg` の変数のまま。`colorize` で色を当てる）。
    pub svg: String,
    #[serde(serialize_with = "rounded")]
    pub width: f64,
    /// 基線から上の高さ。
    #[serde(serialize_with = "rounded")]
    pub height: f64,
    /// 基線から下の深さ（文中の式は、これだけ基線より下に出す）。
    #[serde(serialize_with = "rounded")]
    pub depth: f64,
}

impl PartialEq for MathPicture {
    fn eq(&self, other: &Self) -> bool {
        self.svg == other.svg
            && self.width.to_bits() == other.width.to_bits()
            && self.height.to_bits() == other.height.to_bits()
            && self.depth.to_bits() == other.depth.to_bits()
    }
}

impl Eq for MathPicture {}

fn rounded<S: Serializer>(value: &f64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64((value * 1e4).round() / 1e4)
}

/// 数式の SVG `svg` に文字の色 `fg`（`#rrggbb`）を当て、librsvg で描ける SVG にする。読めなければ `None`。
pub fn colorize(svg: &str, fg: &str) -> Option<String> {
    crate::svgcss::inline_with(svg, &[(COLOR_VAR, fg)])
}

/// `colorize` と同じで、CoreSVG（macOS）に渡すとき（`diagram::colorize_for_coresvg` と同じ道）。
pub fn colorize_for_coresvg(svg: &str, fg: &str) -> Option<String> {
    crate::svgcss::inline_slanted(svg, &[(COLOR_VAR, fg)])
}

/// 1 つの Markdown のファイルで描く式の数・原文と SVG の合計の大きさの上限（時間の上限は `set_file_time_limit`）。超えた
/// 分は描かずに原文を見せる（1 つのファイルで整形表示が止まらないように）。
#[cfg(feature = "math")]
const MAX_FILE_FORMULAS: usize = 10_000;
#[cfg(feature = "math")]
const MAX_FILE_SOURCE: usize = 512 * 1024;
/// SVG の合計（FE は式ごとに色を当てて画像にするので、JSON とその手間が大きくなりすぎないように）。小さな式でも字形を
/// 持つので 1 式あたり 2〜4 KB になり、上の 1 万式が入る大きさにする（1 万式で約 44 MB）。
#[cfg(feature = "math")]
const MAX_FILE_SVG: usize = 48 << 20;

/// 1 つのファイルの上限の組（試験では小さくする）。
#[cfg(feature = "math")]
#[derive(Clone, Copy)]
struct FileLimits {
    formulas: usize,
    source: usize,
    svg: usize,
    time: std::time::Duration,
}

#[cfg(feature = "math")]
const FILE_LIMITS: FileLimits = FileLimits {
    formulas: MAX_FILE_FORMULAS,
    source: MAX_FILE_SOURCE,
    svg: MAX_FILE_SVG,
    time: std::time::Duration::from_secs(2),
};
/// 描くスレッドのスタック（RaTeX の入れ子の上限まで足りる大きさ。使った分だけ取られる）。
#[cfg(feature = "math")]
const STACK: usize = 64 << 20;

/// 描く式 1 つの在りか。
#[cfg(feature = "math")]
enum Slot<'a> {
    Block {
        text: &'a str,
        math: &'a mut Option<MathPicture>,
        error: &'a mut Option<String>,
    },
    Run(&'a mut Run),
}

/// `blocks` の中の式を、出てくる順にたどる。
#[cfg(feature = "math")]
fn each(blocks: &mut [Block], f: &mut dyn FnMut(Slot<'_>)) {
    let runs = |runs: &mut [Run], f: &mut dyn FnMut(Slot<'_>)| {
        for run in runs.iter_mut().filter(|r| r.math_tex.is_some()) {
            f(Slot::Run(run));
        }
    };
    for block in blocks {
        match block {
            Block::Math { text, math, error } => f(Slot::Block { text, math, error }),
            Block::Heading { runs: r, .. } | Block::Paragraph { runs: r, .. } => runs(r, f),
            Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => each(blocks, f),
            Block::Details {
                summary, blocks, ..
            } => {
                runs(summary, f);
                each(blocks, f);
            }
            Block::List { items, .. } => {
                for item in items {
                    each(&mut item.blocks, f);
                }
            }
            Block::Table { head, rows, .. } => {
                for cell in head.iter_mut().chain(rows.iter_mut().flatten()) {
                    runs(cell, f);
                }
            }
            Block::HtmlTable { cells, .. } => {
                for cell in cells {
                    runs(&mut cell.runs, f);
                }
            }
            Block::Code { .. } | Block::Diagram { .. } | Block::Rule => {}
        }
    }
}

/// `blocks` の中の式をすべて描いて、絵（描けなければ理由）を入れる。`math` フィーチャーがなければ何もしない
/// （FE は原文をコードとして見せる）。
pub(crate) fn render_all(blocks: &mut [Block]) {
    #[cfg(feature = "math")]
    {
        let mut jobs: Vec<(String, bool)> = Vec::new();
        each(blocks, &mut |slot| match slot {
            Slot::Block { text, .. } => jobs.push((text.to_owned(), true)),
            Slot::Run(run) => {
                if let Some(tex) = &run.math_tex {
                    jobs.push((tex.clone(), run.math_display));
                }
            }
        });
        if jobs.is_empty() {
            return;
        }
        let results = render_jobs(jobs);
        let mut results = results.into_iter();
        each(blocks, &mut |slot| {
            let Some(result) = results.next() else {
                return;
            };
            match (slot, result) {
                (Slot::Block { math, .. }, Ok(picture)) => *math = Some(picture),
                (Slot::Block { error, .. }, Err(e)) => *error = Some(e),
                (Slot::Run(run), Ok(picture)) => run.math = Some(picture),
                (Slot::Run(run), Err(e)) => run.math_error = Some(e),
            }
        });
    }
    #[cfg(not(feature = "math"))]
    let _ = blocks;
}

/// 1 つの Markdown のファイルの式（TeX と、ブロックの組み方で描くか）を、出てくる順に描く（ファイルごとの上限つき。
/// 色は `--math-fg` のまま）。Markdown を読まずに式だけを描く FE（VSCode 拡張の `amonhen-render`）が使う。
#[cfg(feature = "math")]
pub fn render_formulas(jobs: Vec<(String, bool)>) -> Vec<Result<MathPicture, String>> {
    render_jobs(jobs)
}

/// 式を順に描く（大きなスタックの別スレッドで。上限を超えた分は描かない）。
#[cfg(feature = "math")]
fn render_jobs(jobs: Vec<(String, bool)>) -> Vec<Result<MathPicture, String>> {
    render_jobs_with(
        jobs,
        FileLimits {
            time: file_time_limit(),
            ..FILE_LIMITS
        },
    )
}

/// `render_jobs` を上限 `limits` で。
#[cfg(feature = "math")]
fn render_jobs_with(
    jobs: Vec<(String, bool)>,
    limits: FileLimits,
) -> Vec<Result<MathPicture, String>> {
    let count = jobs.len();
    let worker = std::thread::Builder::new()
        .name("math".into())
        .stack_size(STACK)
        .spawn(move || {
            let started = std::time::Instant::now();
            let (mut source, mut svg) = (0usize, 0usize);
            jobs.iter()
                .enumerate()
                .map(|(i, (tex, display))| {
                    source += tex.len();
                    if i >= limits.formulas
                        || source > limits.source
                        || svg > limits.svg
                        || started.elapsed() > limits.time
                    {
                        return Err(amonhen_model::tr!(
                            "This file has too many formulas to draw them all"
                        ));
                    }
                    let drawn = render_one(tex, *display);
                    if let Ok(picture) = &drawn {
                        svg += picture.svg.len();
                    }
                    drawn
                })
                .collect::<Vec<_>>()
        });
    match worker.map(|w| w.join()) {
        Ok(Ok(results)) => results,
        _ => vec![Err(amonhen_model::tr!("This formula couldn’t be drawn")); count],
    }
}

#[cfg(feature = "math")]
fn render_one(tex: &str, display: bool) -> Result<MathPicture, String> {
    use amonhen_math::Error;
    match amonhen_math::render(tex, display) {
        Ok(f) => Ok(MathPicture {
            svg: f.svg.clone(),
            width: f.width,
            height: f.height,
            depth: f.depth,
        }),
        // 原文は FE が見せるので、理由と位置だけ
        Err(e @ Error::Parse { .. }) => {
            let message = e.message().unwrap_or_default();
            Err(match e.position(tex) {
                Some((line, column)) if tex.contains('\n') => amonhen_model::tr!(
                    "{message} (line {line}, column {column})",
                    message = message,
                    line = line,
                    column = column
                ),
                Some((_, column)) => amonhen_model::tr!(
                    "{message} (column {column})",
                    message = message,
                    column = column
                ),
                None => message.to_owned(),
            })
        }
        Err(Error::TooLong) => Err(amonhen_model::tr!(
            "This formula is too long to draw ({limit} bytes at most)",
            limit = amonhen_math::MAX_SOURCE
        )),
        Err(Error::TooLarge) => Err(amonhen_model::tr!("This formula is too large to draw")),
        Err(Error::Failed) => Err(amonhen_model::tr!("This formula couldn’t be drawn")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{Item, Run, parse};

    /// 段落の文字（数式は `[tex]`、コードは `` `text` ``、硬い改行は `\n`）。
    fn line(runs: &[Run]) -> String {
        runs.iter()
            .map(|r| match &r.math_tex {
                Some(tex) => format!("[{tex}]"),
                None if r.code => format!("`{}`", r.text),
                None if r.line_break => "\n".to_owned(),
                None => r.text.clone(),
            })
            .collect()
    }

    fn paragraphs(md: &str) -> Vec<String> {
        paragraphs_of(&parse(md))
    }

    fn paragraphs_of(blocks: &[Block]) -> Vec<String> {
        blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph { runs, .. } => line(runs),
                Block::Math { text, .. } => format!("MATH[{text}]"),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn inline_math_follows_github() {
        assert_eq!(paragraphs("a $x^2$ b"), ["a [x^2] b"]);
        // 開きの前は頭・空白・`(` だけ、閉じの後に英数字・`_` は続かない（GitHub の Markdown API で確かめた）
        assert_eq!(
            paragraphs("$a$$b$ and foo$y=x$bar"),
            ["[a]$b$ and foo$y=x$bar"]
        );
        assert_eq!(paragraphs("($x$), $y$. $z$-$w$"), ["([x]), [y]. [z]-$w$"]);
        assert_eq!(paragraphs("$x$a $x$_ $x$です"), ["$x$a $x$_ [x]です"]);
        assert_eq!(paragraphs("日本語の$x$ と $y$"), ["日本語の$x$ と [y]"]);
        assert_eq!(
            paragraphs(r#"a$x$b 5$x$ "$x$" [$x$]"#),
            [r#"a$x$b 5$x$ "$x$" [$x$]"#]
        );
        assert_eq!(paragraphs("$x$y$ and $x$$$y$"), ["$x$y$ and [x]$$y$"]);
        // 閉じの前の空白は、後が終わりか記号のときだけ
        assert_eq!(paragraphs("$x $."), ["[x ]."]);
        assert_eq!(paragraphs("a $y $ b and $x $a"), ["a $y $ b and $x $a"]);
        // `\$` は `$` として組を探す
        assert_eq!(paragraphs(r"\$x$ and $x\$"), ["[x] and [x]"]);
        assert_eq!(paragraphs(r"$x\$ y$"), ["[x] y$"]);
        assert_eq!(paragraphs(r"$\$$ and $\$x$"), ["$$$ and $$x$"]);
        // `\\$` は文字の `\$`（閉じにならず、式の中なら文字の `$`）
        assert_eq!(paragraphs(r"$x \\$ and $y$"), [r"$x \$ and [y]"]);
        assert_eq!(paragraphs(r"$a\$ and $b \\$ c$"), [r"[a] and [b \$ c]"]);
        assert_eq!(paragraphs(r"$a \\ b$ \\$x$"), [r"[a \\ b] \$x$"]);
        // 斜体・リンクの中は数式にしない。太字の中はする
        assert_eq!(
            paragraphs("*$x$* [$y$](https://example.com) **$z$**"),
            ["$x$ $y$ [z]"]
        );
        // 脚注の中身も数式にしない
        let blocks = parse("a $x$[^1]\n\n[^1]: $y$\n");
        let Some(Block::Footnote { blocks: inner, .. }) = blocks.last() else {
            panic!("{blocks:?}")
        };
        assert_eq!(paragraphs_of(inner), ["$y$"]);
        // `` $`…`$ `` の中には `$` を書け、閉じの後は見ない
        assert_eq!(paragraphs("$`a$b`$ and more"), ["[a$b] and more"]);
        assert_eq!(paragraphs("a $`x`$b"), ["a [x]b"]);
        // GitHub の $`…`$（中は文字どおり。pulldown-cmark は中の $ で閉じるので、中に $ は書けない）
        assert_eq!(paragraphs("a $`x+y`$ b"), ["a [x+y] b"]);
        assert_eq!(paragraphs("a $`` x`y ``$ b"), ["a [x`y] b"]);
        // 原文は書いたまま・コードの印つき（描けない FE はそれを見せる）
        let blocks = parse("a $x^2$ b");
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(runs[1].text, "$x^2$");
        assert!(runs[1].code && !runs[1].math_display);
    }

    #[test]
    fn inline_math_across_lines_follows_github() {
        // 行の境をまたいでよい。次の行の頭の閉じは、前が空白でも閉じる（GitHub で確かめた）
        assert_eq!(paragraphs("a $x +\ny$ b"), ["a [x +\ny] b"]);
        assert_eq!(paragraphs("a $x\n$ b"), ["a [x\n] b"]);
        assert_eq!(
            paragraphs("a $\nx$ b and $5 and\n$6"),
            ["a $ x$ b and $5 and $6"]
        );
        assert_eq!(paragraphs("$x$\nb and a\n$y$"), ["[x] b and a [y]"]);
        // 硬い改行はまたがない
        let blocks = parse("a $x +  \ny$ b\n\nc $x +\\\ny$ d");
        assert_eq!(paragraphs_of(&blocks), ["a $x +\ny$ b", "c $x +\ny$ d"]);
        // 引用・リストの中でも、TeX に入れ物の印や字下げを入れない
        let quote = parse("> a $x +\n> y$ b");
        let Some(Block::Quote { blocks, .. }) = quote.first() else {
            panic!("{quote:?}")
        };
        assert_eq!(paragraphs_of(blocks), ["a [x +\ny] b"]);
        let list = parse("- a $x +\n  y$ b");
        let Some(Block::List { items, .. }) = list.first() else {
            panic!("{list:?}")
        };
        assert_eq!(paragraphs_of(&items[0].blocks), ["a [x +\ny] b"]);
    }

    #[test]
    fn dollars_that_are_not_math() {
        // 値段
        assert_eq!(paragraphs("It costs $5 and $6."), ["It costs $5 and $6."]);
        assert_eq!(paragraphs("$20,000 and $30,000"), ["$20,000 and $30,000"]);
        assert_eq!(paragraphs("price: $3.50 or $4"), ["price: $3.50 or $4"]);
        // 閉じの $ の後が数字
        assert_eq!(paragraphs("$5/$10 each"), ["$5/$10 each"]);
        assert_eq!(
            paragraphs("from $5 to $10, $5k-$10k"),
            ["from $5 to $10, $5k-$10k"]
        );
        // 開きの後が空白
        assert_eq!(paragraphs("$ x$ and $ x $"), ["$ x$ and $ x $"]);
        // \$ は文字
        assert_eq!(paragraphs(r"pay \$5 and \$6"), ["pay $5 and $6"]);
        // コードの中
        assert_eq!(paragraphs("`$x$` code"), ["`$x$` code"]);
        // 閉じない
        assert_eq!(paragraphs("Hello $world."), ["Hello $world."]);
    }

    #[test]
    fn block_math() {
        assert_eq!(paragraphs("$$\nx^2\n$$"), ["MATH[x^2]"]);
        assert_eq!(
            paragraphs("```math\n\\frac{a}{b}\n```"),
            ["MATH[\\frac{a}{b}]"]
        );
        assert_eq!(paragraphs("```MATH\nx\n```"), ["MATH[x]"]);
        // 段落の中の $$…$$ は段落を分ける
        assert_eq!(
            paragraphs("text\n$$\nx\n$$\nmore"),
            ["text", "MATH[x]", "more"]
        );
        assert_eq!(
            paragraphs("before $$y$$ after"),
            ["before", "MATH[y]", "after"]
        );
        // 空の $$ は文字
        assert_eq!(
            paragraphs("Oops empty $$ expression."),
            ["Oops empty $$ expression."]
        );
        // 引用・リストの中
        let blocks = parse("> $$a$$\n\n- $$b$$\n- c $d$");
        let Block::Quote { blocks: inner, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert!(matches!(&inner[0], Block::Math { text, .. } if text == "a"));
        let Block::List { items, .. } = &blocks[1] else {
            panic!("{blocks:?}")
        };
        let Item { blocks: first, .. } = &items[0];
        assert!(
            matches!(&first[0], Block::Math { text, .. } if text == "b"),
            "{first:?}"
        );
        let Block::Paragraph { runs, .. } = &items[1].blocks[0] else {
            panic!("{items:?}")
        };
        assert_eq!(line(runs), "c [d]");
    }

    #[test]
    fn display_math_in_headings_and_cells_stays_inline() {
        let blocks = parse("# Title $$x$$\n\n| a | b |\n|-|-|\n| $y$ | $$z$$ |\n");
        let Block::Heading { runs, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(line(runs), "Title [x]");
        assert!(runs[1].math_display);
        let Block::Table { rows, .. } = &blocks[1] else {
            panic!("{blocks:?}")
        };
        assert_eq!(line(&rows[0][0]), "[y]");
        assert_eq!(line(&rows[0][1]), "[z]");
        assert!(rows[0][1][0].math_display && !rows[0][0][0].math_display);
    }

    /// 時間の上限は設定で変えられ、上限なしにもできる（ほかの試験に響かないよう、上限なしと既定だけを使う）。
    #[cfg(feature = "math")]
    #[test]
    fn the_time_limit_can_be_set() {
        set_file_time_limit(None);
        assert_eq!(file_time_limit(), std::time::Duration::MAX);
        set_file_time_limit(Some(std::time::Duration::from_secs(2)));
        assert_eq!(file_time_limit(), std::time::Duration::from_secs(2));
        assert!(FONT_SCALES.contains(&FONT_SCALE) && TIME_LIMITS.contains(&2));
    }

    #[test]
    fn math_runs_do_not_merge_with_code() {
        let blocks = parse("$x$`code`");
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(line(runs), "[x]`code`");
    }

    /// `$`・`` ` ``・`\\`・数字・表・リスト・引用を混ぜた文で止まらず、どの式にも絵か理由のどちらかが入る。
    #[test]
    fn mixed_dollars_never_panic() {
        const PIECES: [&str; 16] = [
            "$", "$$", "`", "\\", "5", "x", " ", "\n", "| ", "- ", "> ", "{", "}", "\\frac", "# ",
            "\n\n",
        ];
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        for _ in 0..400 {
            let mut md = String::new();
            for _ in 0..40 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                md.push_str(PIECES[(seed % PIECES.len() as u64) as usize]);
            }
            let mut blocks = parse(&md);
            #[cfg(feature = "math")]
            each(&mut blocks, &mut |slot| match slot {
                Slot::Block { math, error, .. } => {
                    assert!(math.is_some() != error.is_some(), "{md:?}")
                }
                Slot::Run(run) => assert!(run.math.is_some() != run.math_error.is_some(), "{md:?}"),
            });
            blocks.clear();
        }
    }

    #[cfg(not(feature = "math"))]
    #[test]
    fn without_the_feature_math_is_left_as_source() {
        let json = crate::markdown::parse_json("a $x$ b\n\n$$y$$", None);
        assert!(json.contains(r#""text":"$x$","code":true"#), "{json}");
        assert!(json.contains(r#"{"kind":"math","text":"y"}"#), "{json}");
    }

    #[cfg(feature = "math")]
    #[test]
    fn draws_inline_and_block_math() {
        let blocks = parse("Euler: $e^{i\\pi}+1=0$.\n\n$$\n\\sum_{i=1}^n i\n$$\n");
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        let math = runs[1].math.as_ref().expect("inline picture");
        assert!(math.svg.contains("var(--math-fg"), "{}", math.svg);
        assert!(
            math.width > 1.0 && math.height > 0.5 && math.depth >= 0.0,
            "{math:?}"
        );
        let Block::Math { math, error, .. } = &blocks[1] else {
            panic!("{blocks:?}")
        };
        assert!(error.is_none());
        let math = math.as_ref().expect("block picture");
        // 大きな演算子の上下の添え字（ブロックの組み方）で、文中より背が高い
        assert!(math.height + math.depth > 2.0, "{math:?}");
        let json = crate::markdown::parse_json("$$x$$\n\na $y$", None);
        assert!(
            json.contains(r#""kind":"math","text":"x","svg":"<svg"#),
            "{json}"
        );
        assert!(json.contains(r#""math":{"svg":"<svg"#), "{json}");
        assert!(json.contains(r#""depth":"#), "{json}");
    }

    #[cfg(feature = "math")]
    #[test]
    fn errors_show_the_source() {
        let blocks = parse("bad $\\nope{x}$ here\n\n```math\n\\frac{1}{2\n```\n");
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert!(runs[1].math.is_none() && runs[1].code);
        assert_eq!(runs[1].text, "$\\nope{x}$");
        let inline = runs[1].math_error.as_deref().unwrap();
        assert!(
            inline.contains("nope") && inline.ends_with("(column 1)"),
            "{inline}"
        );
        let Block::Math { text, math, error } = &blocks[1] else {
            panic!("{blocks:?}")
        };
        assert_eq!(text, "\\frac{1}{2");
        assert!(math.is_none());
        let error = error.as_deref().unwrap();
        // 原文はもう一度見せない（FE がコードとして見せる）
        assert!(
            !error.contains("\\frac") && !error.contains('\n'),
            "{error}"
        );
        // 長すぎる式
        let long = format!("$$\n{}\n$$", "x+".repeat(amonhen_math::MAX_SOURCE));
        let blocks = parse(&long);
        assert!(
            matches!(&blocks[0], Block::Math { error: Some(e), .. } if e.contains("8192")),
            "{:?}",
            blocks[0]
        );
    }

    #[cfg(feature = "math")]
    #[test]
    fn a_file_has_a_limit() {
        // 上限を小さくして、上限を超えた式は描かずに断ることを見る（速さに左右されないように時間は長く）
        let limits = FileLimits {
            formulas: 3,
            time: std::time::Duration::from_secs(600),
            ..FILE_LIMITS
        };
        let jobs: Vec<(String, bool)> = (0..5).map(|i| (format!("x_{{{i}}}"), false)).collect();
        let results = render_jobs_with(jobs, limits);
        assert_eq!(results.len(), 5);
        assert!(results[..3].iter().all(Result::is_ok));
        assert!(results[3..].iter().all(Result::is_err));
        // SVG の合計の上限も同じく止める
        let limits = FileLimits {
            svg: 1,
            time: std::time::Duration::from_secs(600),
            ..FILE_LIMITS
        };
        let jobs: Vec<(String, bool)> = (0..3).map(|i| (format!("x_{{{i}}}"), false)).collect();
        let results = render_jobs_with(jobs, limits);
        assert!(results[0].is_ok() && results[1..].iter().all(Result::is_err));
        // Markdown の中の文中の式も、上限を超えた分は原文と理由になる
        let md: String = (0..5).map(|i| format!("$x_{{{i}}}$ ")).collect();
        let blocks = parse(&md);
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(runs.iter().filter(|r| r.math.is_some()).count(), 5);
    }

    #[cfg(feature = "math")]
    #[test]
    fn colorize_sets_the_text_color_and_keeps_explicit_ones() {
        let blocks = parse("$x + {\\color{red} y}$");
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("{blocks:?}")
        };
        let svg = &runs[0].math.as_ref().unwrap().svg;
        for out in [
            colorize(svg, "#123456").unwrap(),
            colorize_for_coresvg(svg, "#123456").unwrap(),
        ] {
            assert!(!out.contains("var("), "{out}");
            assert!(out.contains("#123456"), "{out}");
            assert!(out.contains("#ff0000"), "{out}");
            assert!(out.contains("<use ") && out.contains("<defs>"), "{out}");
        }
        assert_eq!(COLOR_VAR, amonhen_math::COLOR_VAR);
    }
}
