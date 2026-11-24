//! 実在する README・文書で見かける式（`corpus.txt`、1 行 1 式）がすべて描けること、壊れた入力が誤りになること、
//! ゆらした入力で止まらない（panic しない）こと。

use amonhen_math::{Error, render};

const CORPUS: &str = include_str!("corpus.txt");

fn corpus() -> Vec<&'static str> {
    CORPUS.lines().filter(|l| !l.trim().is_empty()).collect()
}

#[test]
fn every_formula_in_the_corpus_renders() {
    let lines = corpus();
    assert!(lines.len() >= 60, "{}", lines.len());
    for line in lines {
        for display in [true, false] {
            let f = render(line, display).unwrap_or_else(|e| panic!("{line}: {e:?}"));
            assert!(f.width > 0.0 && f.height + f.depth > 0.0, "{line}");
            assert!(f.svg.contains("<use "), "{line}");
            assert!(!f.svg.contains("NaN") && !f.svg.contains("inf"), "{line}");
        }
    }
}

#[test]
fn broken_formulas_are_errors() {
    for tex in [
        r"\frac{1}{2",
        r"\begin{pmatrix} a & b",
        r"\unknowncommand{x}",
        "}{",
        r"\left( x",
        "a_",
        r"\end{matrix}",
        // mhchem は入れていない
        r"\ce{H2O}",
    ] {
        match render(tex, true) {
            Err(Error::Parse { message, .. }) => assert!(!message.is_empty(), "{tex}"),
            other => panic!("{tex}: {other:?}"),
        }
    }
}

/// 決まった種から作る乱数（xorshift）。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PIECES: &[&str] = &[
    "{",
    "}",
    "\\",
    "^",
    "_",
    "&",
    "\\\\",
    "$",
    "\\frac",
    "\\sqrt",
    "\\left(",
    "\\right)",
    "\\begin{matrix}",
    "\\end{matrix}",
    "\\begin{cases}",
    "\\end{cases}",
    "\\text{",
    "\\color{red}",
    "x",
    "1",
    " ",
    "[",
    "]",
    "\\left.",
    "\\right",
    "\\over",
    "#",
    "%",
    "~",
    "\\mathbb",
    "\\overbrace",
    "\\binom",
    "\\operatorname",
    "\\hline",
    "\\\\[",
    "é",
    "∑",
    "\u{0}",
    "日",
    "\\def\\a{\\a}",
    "\\newcommand",
    "\\verb",
    "'",
    "''''",
    "\\raisebox{1e9em}",
    "\\rule{1em}",
    "\\begin{array}{:c:}",
    "\\end{array}",
    "\\colorbox{blue}{",
    "\\htmlStyle{",
    "\\tag{",
];

fn mutate(rng: &mut Rng, s: &str) -> String {
    let mut chars: Vec<String> = s.chars().map(String::from).collect();
    for _ in 0..1 + rng.below(4) {
        let at = rng.below(chars.len() + 1);
        match rng.below(3) {
            0 => chars.insert(at, PIECES[rng.below(PIECES.len())].to_owned()),
            1 if !chars.is_empty() => {
                chars.remove(at.min(chars.len() - 1));
            }
            _ if !chars.is_empty() => {
                let start = at.min(chars.len() - 1);
                let end = (start + 1 + rng.below(8)).min(chars.len());
                let copy = chars[start..end].to_vec();
                for (k, c) in copy.into_iter().enumerate() {
                    chars.insert(end + k, c);
                }
            }
            _ => {}
        }
    }
    chars.concat()
}

#[test]
fn fuzzed_formulas_never_panic() {
    let lines = corpus();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut ok, mut errors) = (0, 0);
    for _ in 0..4000 {
        let mut tex = lines[rng.below(lines.len())].to_owned();
        for _ in 0..1 + rng.below(3) {
            tex = mutate(&mut rng, &tex);
        }
        let display = rng.next() & 1 == 0;
        match render(&tex, display) {
            Ok(f) => {
                ok += 1;
                assert!(f.svg.ends_with("</svg>"), "{tex:?}");
            }
            Err(Error::Failed) => panic!("panicked inside RaTeX: {tex:?}"),
            Err(_) => errors += 1,
        }
    }
    // 描けるものも誤りもある（ゆらし方が強すぎも弱すぎもしない）
    assert!(ok > 200 && errors > 200, "ok={ok} errors={errors}");
}
