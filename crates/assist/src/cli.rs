//! 手元の AI の CLI（Claude Code・Codex）。ツールを使わせない設定で、空の一時ディレクトリで起動し、
//! プロンプトを標準入力で渡して、標準出力に流れてくる JSON の行を読む（macOS 版の `Assistant.stream` と同じ）。

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use amonhen_model::tr;
use serde_json::Value;

/// CLI ごとの違い（起動の引数と、出力の JSON の読み方）。出力の形は CLI の版で変わりやすいので、ここに閉じ込める。
pub trait Assistant: Sync {
    /// 起動の引数（`model` が空なら CLI の既定）。
    fn args(&self, model: &str) -> Vec<String>;
    /// 流れてくる途中の文字。
    fn delta(&self, event: &Value) -> Option<String>;
    /// 最後の全文。
    fn final_text(&self, event: &Value) -> Option<String>;
}

/// Claude Code（`claude -p --output-format stream-json`）。
pub struct ClaudeCode;

impl Assistant for ClaudeCode {
    fn args(&self, model: &str) -> Vec<String> {
        // ツールは使わせない（解説・翻訳でファイルを書いたりコマンドを実行したりする必要はない）。
        let mut args: Vec<String> = [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--tools",
            "",
            "--no-session-persistence",
        ]
        .map(String::from)
        .into();
        if !model.is_empty() {
            args.extend(["--model".into(), model.into()]);
        }
        args
    }

    fn delta(&self, event: &Value) -> Option<String> {
        if event["type"] != "stream_event" {
            return None;
        }
        let e = &event["event"];
        if e["type"] != "content_block_delta" || e["delta"]["type"] != "text_delta" {
            return None;
        }
        e["delta"]["text"].as_str().map(str::to_owned)
    }

    fn final_text(&self, event: &Value) -> Option<String> {
        if event["type"] != "result" {
            return None;
        }
        event["result"].as_str().map(str::to_owned)
    }
}

/// Codex（`codex exec --json`）。途中の文字は流れてこないので、`item.completed` の `agent_message` を読む
/// （古い版は `msg` の `agent_message`）。
pub struct Codex;

impl Assistant for Codex {
    fn args(&self, model: &str) -> Vec<String> {
        // sandbox は読み取りだけ。プロンプトは標準入力（-）。
        let mut args: Vec<String> = [
            "exec",
            "--json",
            "--sandbox",
            "read-only",
            "--skip-git-repo-check",
        ]
        .map(String::from)
        .into();
        if !model.is_empty() {
            args.extend(["-m".into(), model.into()]);
        }
        args.push("-".into());
        args
    }

    fn delta(&self, _event: &Value) -> Option<String> {
        None
    }

    fn final_text(&self, event: &Value) -> Option<String> {
        let item = &event["item"];
        if item["type"].as_str().is_some_and(|t| t.contains("message")) {
            return item["text"].as_str().map(str::to_owned);
        }
        let msg = &event["msg"];
        if msg["type"] == "agent_message" {
            return msg["message"].as_str().map(str::to_owned);
        }
        None
    }
}

/// 選べる CLI。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cli {
    Claude,
    Codex,
}

impl Cli {
    pub const ALL: [Cli; 2] = [Cli::Claude, Cli::Codex];

    /// 設定に書く名前（macOS 版と同じ）。
    pub fn id(self) -> &'static str {
        match self {
            Cli::Claude => "claude",
            Cli::Codex => "codex",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    pub fn title(self) -> &'static str {
        match self {
            Cli::Claude => "Claude Code",
            Cli::Codex => "Codex",
        }
    }

    /// 実行ファイルの名前。
    pub fn program(self) -> &'static str {
        match self {
            Cli::Claude => "claude",
            Cli::Codex => "codex",
        }
    }

    pub fn assistant(self) -> &'static dyn Assistant {
        match self {
            Cli::Claude => &ClaudeCode,
            Cli::Codex => &Codex,
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Cli::Claude => "claude-haiku-5-5",
            Cli::Codex => "",
        }
    }

    /// 探す場所（デスクトップから起動したアプリには端末の PATH がないので、よくある場所を順に見る）。
    pub fn candidates(self, home: &Path) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = match self {
            Cli::Claude => vec![
                home.join(".local/bin/claude"),
                home.join(".claude/local/claude"),
                "/opt/homebrew/bin/claude".into(),
                "/usr/local/bin/claude".into(),
            ],
            Cli::Codex => vec![
                "/opt/homebrew/bin/codex".into(),
                "/usr/local/bin/codex".into(),
                home.join(".local/bin/codex"),
                home.join(".npm-global/bin/codex"),
            ],
        };
        out.push(Path::new("/usr/bin").join(self.program()));
        out
    }

    /// 実行ファイルの場所。よくある場所になければ PATH から探す。
    pub fn find(self, home: &Path) -> Option<PathBuf> {
        self.candidates(home)
            .into_iter()
            .chain(
                std::env::var_os("PATH")
                    .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|d| d.join(self.program())),
            )
            .find(|p| is_executable(p))
    }

    /// 選べるモデル（ID と名前）。Claude Code は決まった一覧、Codex は CLI が持っている一覧
    /// （`~/.codex/models_cache.json` のうち、一覧に出すもの）。
    pub fn models(self, home: &Path) -> Vec<(String, String)> {
        match self {
            Cli::Claude => [
                ("claude-haiku-5-5", "Haiku 5.5"),
                ("claude-sonnet-5-5", "Sonnet 5.5"),
                ("claude-opus-5-5", "Opus 5.5"),
                ("claude-fable-5-1", "Fable 5.1"),
            ]
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
            .into(),
            Cli::Codex => std::fs::read(home.join(".codex/models_cache.json"))
                .ok()
                .map(|data| codex_models(&data))
                .unwrap_or_default(),
        }
    }
}

/// `~/.codex/models_cache.json` の中身から、一覧に出すモデル。
pub fn codex_models(data: &[u8]) -> Vec<(String, String)> {
    let Ok(json) = serde_json::from_slice::<Value>(data) else {
        return Vec::new();
    };
    json["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["visibility"] == "list")
        .filter_map(|m| {
            let slug = m["slug"].as_str()?;
            Some((
                slug.to_owned(),
                m["display_name"].as_str().unwrap_or(slug).to_owned(),
            ))
        })
        .collect()
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// 出力の JSON の行を、届いた順に読む（行に分けて、途中の文字と最後の全文を拾う）。
pub struct Parser<'a> {
    assistant: &'a dyn Assistant,
    buffer: Vec<u8>,
    /// 途中の文字をつないだもの。
    pub text: String,
    /// 最後の全文（届いていれば）。
    pub final_text: Option<String>,
}

impl<'a> Parser<'a> {
    pub fn new(assistant: &'a dyn Assistant) -> Self {
        Self {
            assistant,
            buffer: Vec::new(),
            text: String::new(),
            final_text: None,
        }
    }

    /// 読んだ分を足す。途中の文字が増えたら true。
    pub fn push(&mut self, chunk: &[u8]) -> bool {
        self.buffer.extend_from_slice(chunk);
        let mut grew = false;
        while let Some(newline) = self.buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=newline).collect();
            grew |= self.line(&line[..line.len() - 1]);
        }
        grew
    }

    /// 終わり（改行で終わらない最後の行）。
    pub fn finish(&mut self) -> bool {
        let rest = std::mem::take(&mut self.buffer);
        !rest.is_empty() && self.line(&rest)
    }

    fn line(&mut self, line: &[u8]) -> bool {
        let Ok(event) = serde_json::from_slice::<Value>(line) else {
            return false;
        };
        if let Some(done) = self.assistant.final_text(&event) {
            self.final_text = Some(done);
        }
        match self.assistant.delta(&event) {
            Some(delta) => {
                self.text.push_str(&delta);
                true
            }
            None => false,
        }
    }

    /// 結果（最後の全文がなければ、途中の文字をつないだもの）。
    pub fn result(&self) -> String {
        self.final_text
            .as_deref()
            .unwrap_or(&self.text)
            .trim()
            .to_owned()
    }
}

/// CLI を起動して `prompt` を送り、返事を流しながら `on_text(それまでの全文)` に渡す。`cancel` が立ったら
/// 止める。裏のスレッドで呼ぶ。
pub fn stream(
    cli: Cli,
    executable: &Path,
    model: &str,
    prompt: &str,
    cancel: &AtomicBool,
    mut on_text: impl FnMut(&str),
) -> Result<String, String> {
    let assistant = cli.assistant();
    // リポジトリの CLAUDE.md などを読ませないよう、空の場所で動かす。
    let dir = EmptyDir::new().map_err(|e| e.to_string())?;
    let mut command = amonhen_model::process::command(executable);
    command
        .args(assistant.args(model))
        .current_dir(&dir.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        // デスクトップから起動したアプリには端末の PATH がない（CLI が node などを探せるように）。
        let home = std::env::var("HOME").unwrap_or_default();
        let path = std::env::var("PATH").unwrap_or_default();
        command.env(
            "PATH",
            format!("{home}/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:{path}"),
        );
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("{}: {e}", executable.display()))?;
    // 書く・エラーを読むのは別のスレッドで（どちらかが詰まって止まらないよう）。
    let mut stdin = child.stdin.take().expect("stdin");
    let input = prompt.to_owned();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let mut stderr = child.stderr.take().expect("stderr");
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let mut parser = Parser::new(assistant);
    // 取り消したら止める（読むのを待っている間も気づけるよう、別のスレッドで見張る）。
    let child = Arc::new(Mutex::new(child));
    let done = AtomicBool::new(false);
    let cancelled = std::thread::scope(|scope| {
        let (c, done) = (child.clone(), &done);
        let watcher = scope.spawn(move || {
            while !done.load(Ordering::Relaxed) {
                if cancel.load(Ordering::Relaxed) {
                    let _ = c.lock().map(|mut c| c.kill());
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            false
        });
        read_all(&mut reader, &mut parser, &mut on_text);
        done.store(true, Ordering::Relaxed);
        watcher.join().unwrap_or(false)
    });
    let status = child
        .lock()
        .map_err(|e| e.to_string())?
        .wait()
        .map_err(|e| e.to_string())?;
    let _ = writer.join();
    let error_text = errors.join().unwrap_or_default();
    if cancelled {
        return Err(amonhen_model::i18n::cancelled());
    }
    let result = parser.result();
    if !status.success() || result.is_empty() {
        let error_text = error_text.trim();
        return Err(if error_text.is_empty() {
            tr!(
                "{cli} failed (exit code {code}).",
                cli = cli.title(),
                code = status.code().unwrap_or(-1)
            )
        } else {
            error_text.to_owned()
        });
    }
    on_text(&result);
    Ok(result)
}

/// 標準出力を終わりまで読み、途中の文字が増えるたびに `on_text(それまでの全文)` を呼ぶ。
fn read_all(reader: &mut impl BufRead, parser: &mut Parser<'_>, on_text: &mut impl FnMut(&str)) {
    loop {
        let chunk = match reader.fill_buf() {
            Ok([]) | Err(_) => break,
            Ok(chunk) => chunk.to_vec(),
        };
        reader.consume(chunk.len());
        if parser.push(&chunk) {
            on_text(&parser.text);
        }
    }
    if parser.finish() {
        on_text(&parser.text);
    }
}

/// CLI の版（Settings に出す）。動かなければ None。
pub fn version(executable: &Path) -> Option<String> {
    let out = amonhen_model::process::command(executable)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// 使い終わったら消す、空の一時ディレクトリ。
struct EmptyDir(PathBuf);

impl EmptyDir {
    fn new() -> std::io::Result<Self> {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("amonhen-assist-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for EmptyDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Claude Code 2.x の `--output-format stream-json --include-partial-messages` の行（縮めたもの）。
    const CLAUDE_LINES: &str = r#"{"type":"system","subtype":"init","cwd":"/tmp/x","session_id":"s","tools":[],"model":"claude-haiku-5-5"}
{"type":"stream_event","event":{"type":"message_start","message":{"id":"m","role":"assistant","content":[]}},"session_id":"s"}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}},"session_id":"s"}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"キャッシュの"}},"session_id":"s"}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"当たりを数える"}},"session_id":"s"}
{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"s"}
{"type":"assistant","message":{"content":[{"type":"text","text":"キャッシュの当たりを数える"}]},"session_id":"s"}
{"type":"result","subtype":"success","is_error":false,"result":"キャッシュの当たりを数える\n","session_id":"s"}
"#;

    /// Codex の `exec --json` の行（0.4x の形）。
    const CODEX_LINES: &str = r#"{"type":"thread.started","thread_id":"t"}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"item_0","type":"reasoning","text":"**Translating**"}}
{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"Count cache hits"}}
{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":3}}
"#;

    #[test]
    fn claude_streams_deltas_and_takes_the_result() {
        let mut p = Parser::new(&ClaudeCode);
        let mut seen = Vec::new();
        // 行の途中で切れて届いても読める。
        for chunk in CLAUDE_LINES.as_bytes().chunks(37) {
            if p.push(chunk) {
                seen.push(p.text.clone());
            }
        }
        p.finish();
        assert_eq!(seen, ["キャッシュの", "キャッシュの当たりを数える"]);
        assert_eq!(p.result(), "キャッシュの当たりを数える");
    }

    #[test]
    fn codex_takes_the_agent_message_but_not_reasoning() {
        let mut p = Parser::new(&Codex);
        assert!(!p.push(CODEX_LINES.as_bytes()));
        assert_eq!(p.result(), "Count cache hits");
        // 古い版の形。
        let mut p = Parser::new(&Codex);
        p.push(br#"{"id":"0","msg":{"type":"agent_message","message":"old"}}"#);
        p.finish();
        assert_eq!(p.result(), "old");
    }

    #[test]
    fn args_match_the_macos_app() {
        assert_eq!(
            ClaudeCode.args("claude-haiku-5-5"),
            [
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--tools",
                "",
                "--no-session-persistence",
                "--model",
                "claude-haiku-5-5"
            ]
        );
        assert_eq!(
            Codex.args(""),
            [
                "exec",
                "--json",
                "--sandbox",
                "read-only",
                "--skip-git-repo-check",
                "-"
            ]
        );
        assert_eq!(Codex.args("gpt-5")[5..], ["-m", "gpt-5", "-"]);
    }

    #[test]
    fn lists_codex_models_shown_in_its_picker() {
        let data = br#"{"models":[{"slug":"gpt-5","display_name":"GPT-5","visibility":"list"},{"slug":"hidden","visibility":"hide"},{"slug":"bare","visibility":"list"}]}"#;
        assert_eq!(
            codex_models(data),
            [
                ("gpt-5".to_owned(), "GPT-5".to_owned()),
                ("bare".to_owned(), "bare".to_owned())
            ]
        );
        assert!(codex_models(b"nope").is_empty());
    }

    #[cfg(unix)]
    fn fake_cli(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("claude");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// 書いたばかりの偽の CLI は、並んで走るほかの試験が子プロセスを作った瞬間にその子へ書き込みの記述子が渡り、
    /// Linux では実行すると「Text file busy」で断られることがある。そのときは少し待って実行し直す。
    #[cfg(unix)]
    fn stream_fake(
        cli: &Path,
        prompt: &str,
        cancel: &AtomicBool,
        mut on_text: impl FnMut(&str),
    ) -> Result<String, String> {
        for _ in 0..20 {
            match stream(Cli::Claude, cli, "", prompt, cancel, &mut on_text) {
                Err(e) if e.contains("os error 26") => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                other => return other,
            }
        }
        panic!("still busy after retries");
    }

    #[cfg(unix)]
    #[test]
    fn streams_from_a_fake_cli_in_an_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let lines = dir.path().join("lines");
        std::fs::write(&lines, CLAUDE_LINES).unwrap();
        // 標準入力を読み、空の場所で動いているかを返事に混ぜる。
        let cli = fake_cli(
            dir.path(),
            &format!(
                "input=$(cat)\n[ -z \"$(ls -A)\" ] && [ \"$input\" = prompt ] || exit 3\ncat '{}'",
                lines.display()
            ),
        );
        let mut seen = Vec::new();
        let out = stream_fake(&cli, "prompt", &AtomicBool::new(false), |t| {
            seen.push(t.to_owned())
        })
        .unwrap();
        assert_eq!(out, "キャッシュの当たりを数える");
        assert_eq!(seen.last().unwrap(), &out);
        assert!(seen.len() >= 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_cli_reports_its_error_output() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_cli(
            dir.path(),
            "cat >/dev/null; echo 'Not logged in' >&2; exit 1",
        );
        let e = stream_fake(&cli, "p", &AtomicBool::new(false), |_| {}).unwrap_err();
        assert_eq!(e, "Not logged in");
        let cli = fake_cli(dir.path(), "cat >/dev/null; exit 2");
        let e = stream_fake(&cli, "p", &AtomicBool::new(false), |_| {}).unwrap_err();
        assert!(e.contains("Claude Code") && e.contains('2'), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn cancelling_stops_a_cli_that_is_still_thinking() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_cli(dir.path(), "cat >/dev/null; exec sleep 30");
        let started = std::time::Instant::now();
        let e = stream_fake(&cli, "p", &AtomicBool::new(true), |_| {}).unwrap_err();
        assert!(amonhen_model::i18n::is_cancelled(&e), "{e}");
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
