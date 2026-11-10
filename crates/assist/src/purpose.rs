//! AI に頼むこと（用途）と、用途ごとの組み込みのプロンプト・変数・返事の形（macOS 版の `AssistPurpose` と同じ）。

use std::collections::BTreeMap;

use amonhen_model::tr;

/// AI に頼むこと。用途ごとにプロンプトを持ち、利用者が書き換えられる。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Purpose {
    Translate,
    ExplainCommit,
    ExplainHunk,
    SuggestMessage,
    ResolveConflict,
    ExplainConflict,
}

/// プロンプトに差し込む値（変数の名前と値）。
pub type Variables = BTreeMap<String, String>;

impl Purpose {
    pub const ALL: [Purpose; 6] = [
        Purpose::Translate,
        Purpose::ExplainCommit,
        Purpose::ExplainHunk,
        Purpose::SuggestMessage,
        Purpose::ResolveConflict,
        Purpose::ExplainConflict,
    ];

    /// 名前（プロンプトのファイル名 `<id>.md`。macOS 版と同じ）。
    pub fn id(self) -> &'static str {
        match self {
            Purpose::Translate => "translate",
            Purpose::ExplainCommit => "explainCommit",
            Purpose::ExplainHunk => "explainHunk",
            Purpose::SuggestMessage => "suggestMessage",
            Purpose::ResolveConflict => "resolveConflict",
            Purpose::ExplainConflict => "explainConflict",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn title(self) -> String {
        match self {
            Purpose::Translate => tr!("Translate Commit Message"),
            Purpose::ExplainCommit => tr!("Explain Commit"),
            Purpose::ExplainHunk => tr!("Explain Hunk"),
            Purpose::SuggestMessage => tr!("Suggest Commit Message"),
            Purpose::ResolveConflict => tr!("Resolve Conflict"),
            Purpose::ExplainConflict => tr!("Explain Conflict"),
        }
    }

    /// プロンプトで使える変数。
    pub fn variables(self) -> &'static [&'static str] {
        match self {
            Purpose::Translate => &["message", "language"],
            Purpose::ExplainCommit => &["message", "diff", "files", "language"],
            Purpose::ExplainHunk => &["path", "hunk", "message", "language"],
            Purpose::SuggestMessage => &["diff", "files", "recent_messages", "language"],
            Purpose::ResolveConflict | Purpose::ExplainConflict => {
                &["path", "ours", "theirs", "base", "language"]
            }
        }
    }

    /// Settings の「試す」で差し込む見本の値（プロンプトの書き換えを、実際のリポジトリなしで確かめる）。
    pub fn sample_variables(self) -> Variables {
        let diff = "--- src/cache.rs\n\
@@ -10,7 +10,9 @@ impl Cache {\n     pub fn get(&self, key: &str) -> Option<&Value> {\n\
-        self.map.get(key)\n\
+        let value = self.map.get(key)?;\n\
+        self.hits.fetch_add(1, Ordering::Relaxed);\n\
+        Some(value)\n     }";
        let message = "Count cache hits\n\nThe hit rate shows whether the cache size is right.";
        let pairs: Vec<(&str, &str)> = match self {
            Purpose::Translate => vec![("message", message)],
            Purpose::ExplainCommit => vec![
                ("message", message),
                ("diff", diff),
                ("files", "src/cache.rs"),
            ],
            Purpose::ExplainHunk => vec![
                ("path", "src/cache.rs"),
                ("hunk", diff),
                ("message", message),
            ],
            Purpose::SuggestMessage => vec![
                ("diff", diff),
                ("files", "src/cache.rs"),
                (
                    "recent_messages",
                    "Add a size limit to the cache\n---\nFix eviction order",
                ),
            ],
            Purpose::ResolveConflict | Purpose::ExplainConflict => vec![
                ("path", "src/cache.rs"),
                ("ours", "let limit = 100;"),
                ("theirs", "let limit = config.limit;"),
                ("base", "let limit = 64;"),
            ],
        };
        pairs
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    /// 組み込みのプロンプト（macOS 版と同じ文）。
    pub fn default_prompt(self) -> &'static str {
        match self {
            Purpose::Translate => {
                "次の git のコミットメッセージを{{language}}に翻訳してください。件名が短くても、件名を含めて文章はすべて訳してください。\
コード・識別子・パス・コミット ID・URL は訳さずそのまま残し、構成（空行、箇条書き、トレーラー）も保ってください。\
訳文だけを出力してください。

{{message}}"
            }
            Purpose::ExplainCommit => {
                "次の git のコミットを{{language}}で解説してください。
1. 何を変えたか（要点を箇条書きで）
2. なぜ変えたと考えられるか
3. 影響しそうな箇所・注意点
推測は推測と書いてください。

# メッセージ
{{message}}

# 変更ファイル
{{files}}

# 差分
{{diff}}"
            }
            Purpose::ExplainHunk => {
                "次の差分の一部（hunk）が何をしているかを{{language}}で短く解説してください。変更の意図として考えられること、\
気をつけるべき点があれば添えてください。推測は推測と書いてください。

# ファイル
{{path}}

# コミットのメッセージ（あれば）
{{message}}

# hunk
{{hunk}}"
            }
            Purpose::SuggestMessage => {
                "次の stage 済みの変更に合う git のコミットメッセージを書いてください。1 行目は 50 文字前後の件名（命令形）、\
空行のあと必要なら本文（何をなぜ変えたか、72 桁で改行）。このリポジトリの最近のメッセージの書き方（言語・形式）に\
合わせてください。メッセージだけを出力し、説明や囲みは付けないでください。

# 最近のメッセージ
{{recent_messages}}

# 変更ファイル
{{files}}

# 差分
{{diff}}"
            }
            Purpose::ResolveConflict => {
                "{{path}} のマージのコンフリクトを解消してください。Current（今のブランチ）と Incoming（取り込む側）の両方の\
意図を活かした内容にしてください。どちらかを捨てるべきならそうしてください。

# 共通の祖先（なければ空）
{{base}}

# Current
{{ours}}

# Incoming
{{theirs}}"
            }
            Purpose::ExplainConflict => {
                "{{path}} のマージのコンフリクトについて、Current と Incoming がそれぞれ何をしようとしているか、\
どう解消するのがよさそうかを{{language}}で簡潔に説明してください。

# 共通の祖先（なければ空）
{{base}}

# Current
{{ours}}

# Incoming
{{theirs}}"
            }
        }
    }

    /// 返させる形（アプリが読むので、プロンプトの編集では変えられない。末尾に自動で足す）。
    pub fn output_contract(self) -> Option<&'static str> {
        match self {
            Purpose::ResolveConflict => Some(
                "\n\n出力は次の形の JSON だけにしてください（前後に文章やコードの囲みを付けない）:
{\"text\": \"解消した後の内容（コンフリクトの範囲を置き換える行。マーカーは含めない）\", \"reason\": \"そうした理由（短く、{{language}}で）\"}",
            ),
            _ => None,
        }
    }
}

/// テンプレートの中の `{{name}}`（前後の空白は許す）の位置と名前。名前でなければ 1 字先から探し直す（macOS 版の
/// 正規表現 `\{\{\s*([a-z_]+)\s*\}\}` と同じく、`{{{diff}}}` は内側の `{{diff}}`）。
fn placeholders(template: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(open) = template[at..].find("{{").map(|i| at + i) {
        let after = open + 2;
        let Some(close) = template[after..].find("}}").map(|i| after + i) else {
            break;
        };
        let name = template[after..close].trim();
        if !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
            out.push((open..close + 2, name));
            at = close + 2;
        } else {
            at = open + 1;
        }
    }
    out
}

/// `{{name}}` を差し込む。知らない変数があれば失敗にする（どの変数が使えるかの文）。
pub fn fill(template: &str, variables: &Variables, purpose: Purpose) -> Result<String, String> {
    let found = placeholders(template);
    if let Some((_, unknown)) = found.iter().find(|(_, n)| !purpose.variables().contains(n)) {
        return Err(tr!(
            "The prompt variable {{{name}}} can’t be used for “{purpose}” (available: {available})",
            name = unknown,
            purpose = purpose.title(),
            available = purpose.variables().join(", ")
        ));
    }
    let mut out = String::with_capacity(template.len());
    let mut last = 0;
    for (range, name) in found {
        out.push_str(&template[last..range.start]);
        out.push_str(variables.get(name).map_or("", String::as_str));
        last = range.end;
    }
    out.push_str(&template[last..]);
    Ok(out)
}

/// 送るプロンプト: `template` に差し込み、返事の形を末尾に足す。
pub fn build(template: &str, variables: &Variables, purpose: Purpose) -> Result<String, String> {
    let mut prompt = fill(template, variables, purpose)?;
    if let Some(contract) = purpose.output_contract() {
        prompt.push_str(&fill(contract, variables, purpose)?);
    }
    Ok(prompt)
}

/// コンフリクトの解消の提案（`resolveConflict` の JSON）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub text: String,
    pub reason: Option<String>,
}

impl Proposal {
    /// 返事から JSON を取り出して読む（コードの囲みや前後の文は除く）。
    pub fn parse(raw: &str) -> Option<Self> {
        let start = raw.find('{')?;
        let end = raw.rfind('}')?;
        if end < start {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(&raw[start..=end]).ok()?;
        Some(Self {
            text: v["text"].as_str()?.to_owned(),
            reason: v["reason"].as_str().map(str::to_owned),
        })
    }

    /// 提案にコンフリクトのマーカーが残っているか（残っていれば受け入れない）。
    pub fn has_markers(&self) -> bool {
        const MARKERS: [&str; 4] = ["<<<<<<<", "=======", ">>>>>>>", "|||||||"];
        self.text
            .lines()
            .any(|line| MARKERS.iter().any(|m| line.starts_with(m)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Variables {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn fills_known_variables_and_allows_spaces() {
        let out = fill(
            "A {{ message }} in {{language}}; {{message}}",
            &vars(&[("message", "m"), ("language", "日本語")]),
            Purpose::Translate,
        )
        .unwrap();
        assert_eq!(out, "A m in 日本語; m");
    }

    #[test]
    fn unknown_variables_are_an_error() {
        let e = fill("{{diff}}", &Variables::new(), Purpose::Translate).unwrap_err();
        assert!(e.contains("{{diff}}"), "{e}");
        assert!(e.contains("message, language"), "{e}");
    }

    #[test]
    fn missing_values_become_empty_and_other_braces_stay() {
        let out = fill(
            "{\"a\": {{path}}} {{X}}",
            &Variables::new(),
            Purpose::ExplainHunk,
        )
        .unwrap();
        assert_eq!(out, "{\"a\": } {{X}}");
    }

    #[test]
    fn triple_braces_fill_the_inner_variable() {
        let v = vars(&[("diff", "D")]);
        assert_eq!(
            fill("{{{diff}}} {{{ diff }}}", &v, Purpose::ExplainCommit).unwrap(),
            "{D} {D}"
        );
        let e = fill("{{{foo}}}", &v, Purpose::ExplainCommit).unwrap_err();
        assert!(e.contains("{{foo}}"), "{e}");
    }

    #[test]
    fn every_default_prompt_uses_only_its_variables() {
        for p in Purpose::ALL {
            build(p.default_prompt(), &p.sample_variables(), p).unwrap();
            assert_eq!(Purpose::from_id(p.id()), Some(p));
        }
    }

    #[test]
    fn the_resolution_prompt_ends_with_the_reply_format() {
        let p = Purpose::ResolveConflict;
        let mut v = p.sample_variables();
        v.insert("language".into(), "English".into());
        let prompt = build(p.default_prompt(), &v, p).unwrap();
        assert!(prompt.ends_with("\"reason\": \"そうした理由（短く、Englishで）\"}"));
        assert!(prompt.contains("let limit = config.limit;"));
    }

    #[test]
    fn reads_a_proposal_out_of_a_fenced_reply() {
        let raw = "```json\n{\"text\": \"a\\nb\", \"reason\": \"both\"}\n```";
        let p = Proposal::parse(raw).unwrap();
        assert_eq!(p.text, "a\nb");
        assert_eq!(p.reason.as_deref(), Some("both"));
        assert!(!p.has_markers());
        assert!(Proposal::parse("no json").is_none());
        assert!(Proposal::parse("{\"reason\": \"x\"}").is_none());
        let marked = Proposal {
            text: "a\n=======\nb".into(),
            reason: None,
        };
        assert!(marked.has_markers());
    }
}
