//! 差分を、AI に渡す unified diff 風の文字にする（大きければ切る。macOS 版の `DiffText` と同じ形）。

/// 差分の行の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    /// ファイルの見出し（パス）。
    File,
    /// hunk の見出し（`@@ … @@`）。
    Hunk,
    Added,
    Removed,
    Context,
}

/// 差分の文字を組み立てる。`LIMIT` バイトを超えたら、それより後ろは省く。
pub struct DiffText {
    out: String,
    full: bool,
}

impl DiffText {
    pub const LIMIT: usize = 60_000;

    pub fn new() -> Self {
        Self {
            out: String::new(),
            full: false,
        }
    }

    /// 上限に届いたか（届いたら、それ以上足しても何も起きない）。
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// 1 ファイルの差分の前に、そのパスの見出しを置く（全ファイルを並べた差分には見出しの行がある）。
    pub fn path(&mut self, path: &str) {
        if !self.full {
            self.out.push_str(&format!("--- {path}\n"));
        }
    }

    /// 1 行足す。上限に届いたら false。
    pub fn push(&mut self, kind: Line, text: &str) -> bool {
        if self.full {
            return false;
        }
        match kind {
            Line::File => self.out.push_str(&format!("\n--- {text}\n")),
            Line::Hunk => self.out.push_str(&format!("{text}\n")),
            Line::Added => self.out.push_str(&format!("+{text}\n")),
            Line::Removed => self.out.push_str(&format!("-{text}\n")),
            Line::Context => self.out.push_str(&format!(" {text}\n")),
        }
        if self.out.len() > Self::LIMIT {
            self.out.push_str("\n…（長いので以下は省略）\n");
            self.full = true;
        }
        !self.full
    }

    pub fn finish(self) -> String {
        self.out
    }
}

impl Default for DiffText {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_unified_lines_and_stops_at_the_limit() {
        let mut d = DiffText::new();
        d.push(Line::File, "a.rs");
        d.push(Line::Hunk, "@@ -1 +1 @@");
        d.push(Line::Removed, "old");
        d.push(Line::Added, "new");
        d.push(Line::Context, "same");
        assert_eq!(d.finish(), "\n--- a.rs\n@@ -1 +1 @@\n-old\n+new\n same\n");
        let mut d = DiffText::new();
        d.path("b.rs");
        let line = "x".repeat(1000);
        let mut n = 0;
        while d.push(Line::Added, &line) {
            n += 1;
        }
        assert!(d.is_full());
        assert_eq!(n, 59);
        let text = d.finish();
        assert!(text.starts_with("--- b.rs\n+x"));
        assert!(text.ends_with("省略）\n"));
    }
}
