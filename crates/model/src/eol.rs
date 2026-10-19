//! ファイルの改行の種類（`git ls-files --eol` の `i/`・`w/` と同じ見方）。

/// ファイルの改行の種類。
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Eol {
    /// 改行がない（空・1 行だけで改行なし）。
    #[default]
    None = 0,
    Lf = 1,
    Crlf = 2,
    /// LF と CRLF が混ざっている。
    Mixed = 3,
}

impl Eol {
    /// `data` の改行の種類。NUL を含む（バイナリ）なら `None`。
    pub fn of(data: &[u8]) -> Eol {
        if data[..data.len().min(8000)].contains(&0) {
            return Eol::None;
        }
        let (mut lf, mut crlf) = (false, false);
        for (i, &b) in data.iter().enumerate() {
            if b == b'\n' {
                if i > 0 && data[i - 1] == b'\r' {
                    crlf = true;
                } else {
                    lf = true;
                }
                if lf && crlf {
                    return Eol::Mixed;
                }
            }
        }
        match (lf, crlf) {
            (true, _) => Eol::Lf,
            (_, true) => Eol::Crlf,
            _ => Eol::None,
        }
    }

    /// `none`・`lf`・`crlf`・`mixed`（FE 向けの名前）。
    pub fn name(self) -> &'static str {
        match self {
            Eol::None => "none",
            Eol::Lf => "lf",
            Eol::Crlf => "crlf",
            Eol::Mixed => "mixed",
        }
    }
}

/// Working Copy の 1 ファイルの改行。git（index）に入っている形・作業ツリーの形・checkout したときに付く形と、
/// かかっている変換。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineEndings {
    pub index: Eol,
    pub worktree: Eol,
    /// index の内容を checkout したときの改行。
    pub checkout: Eol,
    /// かかっている変換（`text=auto eol=crlf`・`core.autocrlf=true`・`-text` など。なければ空）。
    pub rule: String,
}

impl LineEndings {
    /// 気をつけた方がよいか。改行が混ざっている、または作業ツリーが CRLF なのに checkout すると LF になる
    /// （`eol=lf` など。次の commit や checkout で改行がそろえられ、ファイル全体が差分に出ることがある）。
    /// 作業ツリーが LF で checkout すると CRLF になるだけ（Windows の `core.autocrlf=true` で、ツールが LF で書いた
    /// ファイル）は、git に入る形が変わらないので気にしない。
    pub fn warning(&self) -> bool {
        self.worktree == Eol::Mixed
            || self.index == Eol::Mixed
            || (self.worktree == Eol::Crlf && self.checkout == Eol::Lf)
    }

    /// 見せる値打ちがあるか。改行を変換している（作業ツリーと git で違う）か、気をつけた方がよいとき。
    /// どちらも LF で変換がないような普段のファイルでは見せない。
    pub fn notable(&self) -> bool {
        let differs =
            self.index != self.worktree && self.index != Eol::None && self.worktree != Eol::None;
        differs || self.warning()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_line_endings_apart() {
        assert_eq!(Eol::of(b""), Eol::None);
        assert_eq!(Eol::of(b"no newline"), Eol::None);
        assert_eq!(Eol::of(b"a\nb\n"), Eol::Lf);
        assert_eq!(Eol::of(b"a\r\nb\r\n"), Eol::Crlf);
        assert_eq!(Eol::of(b"a\r\nb\n"), Eol::Mixed);
        assert_eq!(Eol::of(b"a\0\r\nb\n"), Eol::None);
    }

    #[test]
    fn shows_only_conversions_and_surprises() {
        let le = |index, worktree, checkout| LineEndings {
            index,
            worktree,
            checkout,
            rule: String::new(),
        };
        // 普段のファイル（どちらも LF、変換なし）は見せない。
        assert!(!le(Eol::Lf, Eol::Lf, Eol::Lf).notable());
        // core.autocrlf=true: 作業ツリーは CRLF、git には LF。
        let autocrlf = le(Eol::Lf, Eol::Crlf, Eol::Crlf);
        assert!(autocrlf.notable() && !autocrlf.warning());
        // 改行が混ざっている。
        assert!(le(Eol::Lf, Eol::Mixed, Eol::Crlf).warning());
        // eol=lf なのに作業ツリーが CRLF。
        assert!(le(Eol::Lf, Eol::Crlf, Eol::Lf).warning());
        // core.autocrlf=true で、ツールが LF で書いたファイル（checkout すると CRLF になるだけ）。見せない。
        assert!(!le(Eol::Lf, Eol::Lf, Eol::Crlf).notable());
        // 新しい・空のファイル。
        assert!(!le(Eol::None, Eol::Lf, Eol::None).notable());
    }
}
