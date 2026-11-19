//! パスワードやパスフレーズを、TUI の中で尋ねる。
//!
//! git と ssh は端末なしで動く（端末は TUI が使っている）ので、尋ねることがあると askpass を呼ぶ。TUI はその
//! askpass を、`amonhen tui --askpass <ソケット> <尋ねる文>` を呼ぶ小さなスクリプトにする（`AMONHEN_ASKPASS`）。
//! そのプロセスは、動いている TUI が待っているソケットにつないで文を送り、TUI が画面で尋ねた答えを受け取る。
//!
//! ソケットとスクリプトは、自分だけが読める一時フォルダに置き、終わるときに消す。Unix だけ。

use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};

use amonhen_model::tr;

use crate::Result;
use crate::app::Event;

/// 尋ねる文の長さの上限（おかしな相手から、際限なく読まない）。
const PROMPT_LIMIT: u64 = 4096;

/// 尋ねられたこと 1 つ。`reply` に答え（やめたら `None`）を返す。
pub struct Question {
    pub prompt: String,
    pub reply: Sender<Option<String>>,
}

impl Question {
    /// 打った文字を隠すか（パスワード・パスフレーズ。ユーザー名や「yes/no」は隠さない）。
    pub fn secret(&self) -> bool {
        let prompt = self.prompt.to_lowercase();
        prompt.contains("password") || prompt.contains("passphrase")
    }
}

/// 待っているソケットと、askpass のスクリプト。
pub struct Askpass {
    dir: PathBuf,
    listener: UnixListener,
}

impl Askpass {
    /// 一時フォルダにソケットとスクリプトを作る。`exe` は、スクリプトが呼ぶ `amonhen`。
    pub fn prepare(exe: &Path) -> Result<Self> {
        // 同じプロセスの中で続けて作っても重ならないよう、時刻に通し番号を足す。
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let dir = std::env::temp_dir().join(format!(
            "amonhen-tui-{}-{nanos}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        // ここから先で失敗したら、`Drop` がフォルダを片づける。
        let pending = Pending(dir.clone());
        let listener = UnixListener::bind(dir.join("socket")).map_err(|e| {
            tr!(
                "Can’t make the socket for password prompts in {dir}: {error}",
                dir = dir.display(),
                error = e
            )
        })?;
        let script = format!(
            "#!/bin/sh\nexec {} tui --askpass {} \"$1\"\n",
            quote(&exe.to_string_lossy()),
            quote(&dir.join("socket").to_string_lossy()),
        );
        let path = dir.join("askpass");
        std::fs::write(&path, script)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        std::mem::forget(pending);
        Ok(Self { dir, listener })
    }

    /// `AMONHEN_ASKPASS` に入れるスクリプト。
    pub fn script(&self) -> PathBuf {
        self.dir.join("askpass")
    }

    /// つないできた相手ごとに、尋ねる文を `events` に送り、答えを返す（スレッドで待ち続ける）。
    pub(crate) fn serve(&self, events: Sender<Event>) -> Result<()> {
        let listener = self.listener.try_clone()?;
        std::thread::Builder::new()
            .name("amonhen-tui-askpass".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { break };
                    let events = events.clone();
                    // 答えを待つ間も、次の相手を受ける。
                    let _ = std::thread::Builder::new()
                        .name("amonhen-tui-askpass-question".into())
                        .spawn(move || answer(stream, &events));
                }
            })?;
        Ok(())
    }
}

impl Drop for Askpass {
    fn drop(&mut self) {
        remove(&self.dir);
    }
}

/// 作りかけの一時フォルダ（途中で失敗したら片づける）。
struct Pending(PathBuf);

impl Drop for Pending {
    fn drop(&mut self) {
        remove(&self.0);
    }
}

/// 自分で作ったソケット・スクリプトと、そのフォルダを消す。
fn remove(dir: &Path) {
    let _ = std::fs::remove_file(dir.join("socket"));
    let _ = std::fs::remove_file(dir.join("askpass"));
    let _ = std::fs::remove_dir(dir);
}

/// 1 つの相手: 文を読み、画面で尋ね、答えを書く。やめたときは、何も書かずに閉じる。
fn answer(mut stream: UnixStream, events: &Sender<Event>) {
    let mut prompt = String::new();
    if (&stream)
        .take(PROMPT_LIMIT)
        .read_to_string(&mut prompt)
        .is_err()
    {
        return;
    }
    let (reply, answered) = mpsc::channel();
    if events.send(Event::Ask(Question { prompt, reply })).is_err() {
        return;
    }
    if let Ok(Some(text)) = answered.recv() {
        // 先頭の 1 バイトは「答えた」の印（空の答えと、やめたのを見分ける）。
        let _ = stream.write_all(b"+");
        let _ = stream.write_all(text.as_bytes());
    }
}

/// askpass として呼ばれた側: ソケットに `prompt` を送り、答えを受け取る。やめたら `None`。
pub fn ask(socket: &Path, prompt: &str) -> Result<Option<String>> {
    let mut stream = UnixStream::connect(socket).map_err(|e| {
        tr!(
            "Can’t reach the terminal front end to ask: {error}",
            error = e
        )
    })?;
    stream.write_all(prompt.as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    Ok(reply.strip_prefix('+').map(str::to_owned))
}

/// sh の 1 つの語にする。
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// スクリプトが呼ぶ側（`ask`）の文が届き、返した答えが戻る。やめたら `None`
    #[test]
    fn a_question_reaches_the_app_and_the_answer_returns() {
        let askpass = Askpass::prepare(Path::new("/usr/bin/amonhen")).unwrap();
        let (tx, rx) = mpsc::channel();
        askpass.serve(tx).unwrap();
        let socket = askpass.dir.join("socket");
        let asking = |prompt: &'static str| {
            let socket = socket.clone();
            std::thread::spawn(move || ask(&socket, prompt).unwrap())
        };
        let next = || match rx.recv().unwrap() {
            Event::Ask(question) => question,
            _ => panic!("expected a question"),
        };

        let client = asking("Password for 'https://example.com': ");
        let question = next();
        assert_eq!(question.prompt, "Password for 'https://example.com': ");
        assert!(question.secret());
        question.reply.send(Some("hunter2".to_owned())).unwrap();
        assert_eq!(client.join().unwrap().as_deref(), Some("hunter2"));

        // 空の答えは、やめたのとは違う。
        let client = asking("Username for 'https://example.com': ");
        let question = next();
        assert!(!question.secret());
        question.reply.send(Some(String::new())).unwrap();
        assert_eq!(client.join().unwrap().as_deref(), Some(""));

        let client = asking("Enter passphrase for key: ");
        next().reply.send(None).unwrap();
        assert_eq!(client.join().unwrap(), None);
    }

    /// スクリプトは自分だけが実行でき、終わると一時フォルダごと消える
    #[test]
    fn the_script_is_private_and_removed_on_drop() {
        let askpass = Askpass::prepare(Path::new("/opt/it's here/amonhen")).unwrap();
        let script = askpass.script();
        let text = std::fs::read_to_string(&script).unwrap();
        assert!(text.starts_with("#!/bin/sh\nexec '/opt/it'\\''s here/amonhen' tui --askpass '"));
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&script), 0o700);
        assert_eq!(mode(script.parent().unwrap()), 0o700);
        let dir = askpass.dir.clone();
        drop(askpass);
        assert!(!dir.exists());
    }
}
