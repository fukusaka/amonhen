//! Mermaid の architecture がアイコンの組なしで知っているアイコンの名前の絵（pele の
//! `src/diagrams/architecture/icons.ts`）。24×24 の格子に描く。ほかの名前はホストの解き手に任せる。

const GLYPHS: [(&str, &str); 5] = [
    (
        "database",
        "M4 6a8 3 0 1 0 16 0a8 3 0 1 0-16 0M4 6v12a8 3 0 0 0 16 0V6M4 12a8 3 0 0 0 16 0",
    ),
    (
        "server",
        "M3 4h18v7H3zM3 13h18v7H3zM7 7.5h.01M7 16.5h.01M11 7.5h6M11 16.5h6",
    ),
    (
        "disk",
        "M5 3h14v18H5zM12 6.5a4 4 0 1 0 0 8a4 4 0 1 0 0-8M12 10.5h.01M8 18h3",
    ),
    (
        "internet",
        "M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0M3 12h18M12 3a13 13 0 0 0 0 18M12 3a13 13 0 0 1 0 18",
    ),
    (
        "cloud",
        "M7 18a4 4 0 0 1-.5-7.97A6 6 0 0 1 18 9.5a4.25 4.25 0 0 1-.5 8.5z",
    ),
];

pub fn builtin_icon(name: &str) -> String {
    match GLYPHS.iter().find(|(n, _)| *n == name) {
        Some((_, d)) => format!("<path d=\"{d}\" stroke=\"var(--_fg)\" stroke-width=\"1.5\"/>"),
        None => String::new(),
    }
}
