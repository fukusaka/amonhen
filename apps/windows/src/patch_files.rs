//! パッチのファイル（GTK4 版の `patches.rs` の拡張子と並べ方）。

/// パッチとして受け取る拡張子（GTK4 版・macOS 版と同じ）。
pub const EXTENSIONS: &[&str] = &["patch", "diff", "mbox", "eml"];

/// `path` がパッチのファイルの拡張子か。
fn is_patch(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// 落とされたファイルがすべてパッチか（1 つもなければ false）。
pub fn all_patches(paths: &[String]) -> bool {
    !paths.is_empty() && paths.iter().all(|p| is_patch(p))
}

/// 数字を数として比べる（`2.patch` が `10.patch` より前。GTK4 版の `natural` と同じ）。
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    fn chunks(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, text)) if *d == digit => text.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    }
    let (a, b) = (chunks(a), chunks(b));
    for (x, y) in a.iter().zip(&b) {
        let order = match (x.0, y.0) {
            (true, true) => {
                let (p, q) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
                p.len().cmp(&q.len()).then_with(|| p.cmp(q))
            }
            _ => x.1.to_lowercase().cmp(&y.1.to_lowercase()),
        };
        if order.is_ne() {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_a_drop_only_when_every_file_is_a_patch() {
        let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(all_patches(&p(&["a.patch", "b.DIFF", "c.mbox", "d.eml"])));
        assert!(!all_patches(&p(&["a.patch", "notes.txt"])));
        assert!(!all_patches(&[]));
    }

    #[test]
    fn numbers_sort_as_numbers() {
        let mut names = vec!["10-a.patch", "2-b.patch", "0001-c.patch"];
        names.sort_by(|a, b| natural(a, b));
        assert_eq!(names, ["0001-c.patch", "2-b.patch", "10-a.patch"]);
    }
}
