//! `git range-diff` の結果（同じ一連のコミットの、2 つの版の比べ合わせ）。

/// 組の種類（`git range-diff` の見出しの記号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum RangeStatus {
    /// `=`: 中身が同じ。
    Same,
    /// `!`: 対応するが、中身（メッセージ・差分）が変わった。
    Changed,
    /// `<`: 古い側にだけある（消えた）。
    Removed,
    /// `>`: 新しい側にだけある（足された）。
    Added,
}

/// 組の片側のコミット。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RangeCommit {
    /// 範囲の中の番号（1 始まり。古い順）。
    pub index: u32,
    /// コミット ID（`git range-diff` が短くしたものは、読んだ後で完全な形に直す）。
    pub oid: String,
}

/// 組 1 つ（`git range-diff` の見出しの 1 行と、その下の差分の差分）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RangePair {
    pub status: RangeStatus,
    pub old: Option<RangeCommit>,
    pub new: Option<RangeCommit>,
    /// 件名（新しい側があれば新しい側の）。
    pub subject: String,
    /// 差分の差分（`Changed` のときだけ）。1 行ずつ、字下げを除いたもの。先頭の 1 文字が外側の記号
    /// （` `・`-`・`+`、見出しは `@`）、残りが内側（それぞれの版のパッチの行）。
    pub diff: Vec<String>,
}

/// `git range-diff --no-color` の出力を組に分ける。見出しでない行は、前の組の差分の差分。
pub fn parse_range_diff(text: &str) -> Vec<RangePair> {
    let mut pairs: Vec<RangePair> = Vec::new();
    for line in text.lines() {
        if let Some(body) = line.strip_prefix("    ") {
            if let Some(pair) = pairs.last_mut() {
                pair.diff.push(body.to_owned());
            }
            continue;
        }
        if let Some(pair) = parse_header(line) {
            pairs.push(pair);
        }
    }
    pairs
}

/// `1:  abc1234 ! 1:  def5678 subject`・`-:  ------- > 2:  def5678 subject` の 1 行。番号は幅をそろえるために
/// 前に空白が付くことがある。
fn parse_header(line: &str) -> Option<RangePair> {
    let mut rest = line;
    let mut token = || {
        let trimmed = rest.trim_start_matches(' ');
        let end = trimmed.find(' ').unwrap_or(trimmed.len());
        let (t, r) = trimmed.split_at(end);
        rest = r;
        (!t.is_empty()).then_some(t)
    };
    let side = |index: &str, oid: &str| -> Option<Option<RangeCommit>> {
        let index = index.strip_suffix(':')?;
        if index == "-" {
            return oid.bytes().all(|b| b == b'-').then_some(None);
        }
        let index = index.parse().ok()?;
        oid.bytes().all(|b| b.is_ascii_hexdigit()).then(|| {
            Some(RangeCommit {
                index,
                oid: oid.to_owned(),
            })
        })
    };
    let old = side(token()?, token()?)?;
    let status = match token()? {
        "=" => RangeStatus::Same,
        "!" => RangeStatus::Changed,
        "<" => RangeStatus::Removed,
        ">" => RangeStatus::Added,
        _ => return None,
    };
    let new = side(token()?, token()?)?;
    let subject = rest.strip_prefix(' ').unwrap_or(rest).to_owned();
    let expected = match status {
        RangeStatus::Same | RangeStatus::Changed => old.is_some() && new.is_some(),
        RangeStatus::Removed => old.is_some() && new.is_none(),
        RangeStatus::Added => old.is_none() && new.is_some(),
    };
    expected.then_some(RangePair {
        status,
        old,
        new,
        subject,
        diff: Vec::new(),
    })
}

/// reflog の行（`rebase (finish): refs/heads/topic onto <oid>`）から、rebase の載せ先。
pub fn rebase_onto(message: &str) -> Option<&str> {
    if !message.starts_with("rebase") {
        return None;
    }
    let (_, onto) = message.rsplit_once(" onto ")?;
    let onto = onto.trim();
    (onto.len() >= 7 && onto.bytes().all(|b| b.is_ascii_hexdigit())).then_some(onto)
}

/// 先を書き換えない reflog の行（コミット・作成・fast-forward など）。これ以外の行は、前のコミットが
/// 新しいコミットの祖先かどうかで書き換えたかを決める。
pub fn reflog_keeps_history(message: &str) -> bool {
    [
        "commit:",
        "commit (initial):",
        "commit (merge):",
        "branch:",
        "merge ",
        "pull: Fast-forward",
        "cherry-pick:",
        "revert:",
    ]
    .iter()
    .any(|p| message.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: &str = "\
1:  ef98ed8 ! 1:  040a1df five
    @@ Commit message
         five

      ## f ##
    -@@
    +@@ f: one
      2
2:  618be3e = 2:  b7fc003 ten
3:  fa6b06d < -:  ------- add z
-:  ------- > 3:  e373123 new: with  spaces
";

    #[test]
    fn splits_headers_and_bodies() {
        let pairs = parse_range_diff(OUTPUT);
        assert_eq!(pairs.len(), 4);
        assert_eq!(pairs[0].status, RangeStatus::Changed);
        assert_eq!(
            pairs[0].old,
            Some(RangeCommit {
                index: 1,
                oid: "ef98ed8".into()
            })
        );
        assert_eq!(pairs[0].new.as_ref().unwrap().oid, "040a1df");
        assert_eq!(pairs[0].subject, "five");
        assert_eq!(
            pairs[0].diff,
            [
                "@@ Commit message",
                "     five",
                " ",
                "  ## f ##",
                "-@@",
                "+@@ f: one",
                "  2"
            ]
        );
        assert_eq!(pairs[1].status, RangeStatus::Same);
        assert!(pairs[1].diff.is_empty());
        assert_eq!(pairs[2].status, RangeStatus::Removed);
        assert_eq!(pairs[2].new, None);
        assert_eq!(pairs[3].status, RangeStatus::Added);
        assert_eq!(pairs[3].old, None);
        assert_eq!(pairs[3].subject, "new: with  spaces");
    }

    #[test]
    fn reads_padded_numbers() {
        let pairs = parse_range_diff(
            " 9:  aaaaaaa = 9:  bbbbbbb nine\n10:  ccccccc ! 10:  ddddddd ten\n -:  ------- > 11:  eeeeeee x\n",
        );
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0].old.as_ref().unwrap().index, 9);
        assert_eq!(pairs[1].new.as_ref().unwrap().index, 10);
        assert_eq!(pairs[2].new.as_ref().unwrap().index, 11);
    }

    #[test]
    fn finds_the_rebase_base() {
        assert_eq!(
            rebase_onto(
                "rebase (finish): refs/heads/topic onto 68ca011997ede02ea11e2e016ff1d297a5f00ff4"
            ),
            Some("68ca011997ede02ea11e2e016ff1d297a5f00ff4")
        );
        assert_eq!(
            rebase_onto("rebase -i (finish): refs/heads/topic onto 68ca011"),
            Some("68ca011")
        );
        assert_eq!(rebase_onto("commit: onto 68ca011"), None);
        assert_eq!(
            rebase_onto("rebase (finish): returning to refs/heads/topic"),
            None
        );
        assert!(reflog_keeps_history("commit: x"));
        assert!(!reflog_keeps_history("commit (amend): x"));
        assert!(!reflog_keeps_history("reset: moving to HEAD~1"));
    }
}
