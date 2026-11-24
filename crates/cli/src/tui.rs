//! `amonhen tui`: 端末の中の画面（`amonhen-tui`、docs/tui.md）。ここでは、ほかのサブコマンドと同じ開き方で
//! リポジトリを開いて渡すだけ。

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use amonhen_front::Front;
use amonhen_model::tr;

use crate::{Args, Result, open_repository, usage};

/// `--snapshot`・`--measure-scroll` で描く大きさ（`--size` で替える）。
const DEFAULT_SIZE: (u16, u16) = (120, 40);
/// `--measure-scroll` で描くフレームの数と、その間隔（120 Hz）。
const MEASURE_FRAMES: usize = 600;
const MEASURE_INTERVAL: Duration = Duration::from_micros(8333);

pub fn run(args: &[String]) -> Result<ExitCode> {
    let mut dir = PathBuf::from(".");
    let mut snapshot: Option<String> = None;
    let mut keys = String::new();
    let mut measure = false;
    let mut step = amonhen_tui::Step::Wheel;
    let mut size = DEFAULT_SIZE;
    // askpass として呼ばれた（`amonhen-tui` が作るスクリプトから。隠した引数）。
    #[cfg(unix)]
    if let [flag, socket, rest @ ..] = args
        && flag == "--askpass"
    {
        let prompt = rest.first().map_or("", String::as_str);
        return Ok(
            match amonhen_tui::askpass::ask(std::path::Path::new(socket), prompt)? {
                Some(answer) => {
                    println!("{answer}");
                    ExitCode::SUCCESS
                }
                // やめた。
                None => ExitCode::FAILURE,
            },
        );
    }
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--snapshot" => snapshot = Some(a.value("--snapshot")?.to_owned()),
            "--keys" => keys = a.value("--keys")?.to_owned(),
            "--measure-scroll" => measure = true,
            "--measure-move" => (measure, step) = (true, amonhen_tui::Step::Move),
            "--measure-diff" => (measure, step) = (true, amonhen_tui::Step::Diff),
            "--measure-changes" => (measure, step) = (true, amonhen_tui::Step::Changes),
            "--measure-diff-pan" => (measure, step) = (true, amonhen_tui::Step::DiffPan),
            "--measure-changes-pan" => (measure, step) = (true, amonhen_tui::Step::ChangesPan),
            "--size" => {
                let value = a.value("--size")?;
                size = value
                    .split_once('x')
                    .and_then(|(c, r)| Some((c.parse().ok()?, r.parse().ok()?)))
                    .filter(|&(c, r)| c > 0 && r > 0)
                    .ok_or_else(|| tr!("--size takes <cols>x<rows>, like 120x40"))?;
            }
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!(
                    "{}\n\n{}",
                    tr!("Unknown option: {option}", option = s),
                    usage()
                )
                .into());
            }
            _ => dir = PathBuf::from(arg),
        }
    }
    // 画面を出すなら、git や ssh が尋ねることを TUI の中で聞けるよう、askpass を用意する。リポジトリを開く前に
    // 環境変数に入れる（共有の daemon には、開くときの環境が渡る）。
    #[cfg(unix)]
    let askpass = match snapshot.is_none() && !measure {
        true => {
            let askpass = amonhen_tui::askpass::Askpass::prepare(&std::env::current_exe()?)?;
            // SAFETY: まだほかのスレッドを始めていない。
            unsafe { std::env::set_var("AMONHEN_ASKPASS", askpass.script()) };
            Some(askpass)
        }
        false => None,
    };
    // `ssh://`（`daemon:ssh://` も）の先のファイルは、この機械にない（TUI のエディタで開けない）。
    let given = dir.to_string_lossy();
    let files_here = !given.starts_with("ssh://") && !given.starts_with("daemon:ssh://");
    let front: Arc<dyn Front> = Arc::from(open_repository(&dir)?);
    if let Some(file) = snapshot {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let clock = amonhen_tui::Clock { now, local };
        let text = amonhen_tui::snapshot(front, size.0, size.1, clock, &keys)?;
        if file == "-" {
            crate::term::write(&text);
        } else {
            std::fs::write(&file, text)?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    if measure {
        let m = amonhen_tui::measure_scroll(
            front,
            size.0,
            size.1,
            MEASURE_FRAMES,
            MEASURE_INTERVAL,
            step,
            &keys,
            local,
        )?;
        println!(
            "{}",
            tr!(
                "{cols}x{rows}, {commits} commits, {frames} frames\nframe time: median {median} µs, 95% {p95} µs, max {max} µs\nframe bytes: mean {mean}, max {max_bytes}\nframes with rows not loaded yet: {incomplete}\nparts (events / build / diff and write): median {m0} / {m1} / {m2} µs, slowest frame {s0} / {s1} / {s2} µs with {replies} replies",
                cols = size.0,
                rows = size.1,
                commits = m.rows,
                frames = m.frames,
                median = m.median_us,
                p95 = m.p95_us,
                max = m.max_us,
                mean = m.mean_bytes,
                max_bytes = m.max_bytes,
                incomplete = m.incomplete_frames,
                m0 = m.median_parts_us[0],
                m1 = m.median_parts_us[1],
                m2 = m.median_parts_us[2],
                s0 = m.slowest_parts_us[0],
                s1 = m.slowest_parts_us[1],
                s2 = m.slowest_parts_us[2],
                replies = m.slowest_replies,
            )
        );
        // かかった時間よりずっと短ければ、その間スレッドは止められていた。
        if let Some(cpu) = m.slowest_cpu_us {
            println!(
                "{}",
                tr!(
                    "slowest frame: {cpu} µs of CPU time in {max} µs",
                    cpu = cpu,
                    max = m.max_us
                )
            );
        }
        if !matches!(step, amonhen_tui::Step::Wheel | amonhen_tui::Step::Move) {
            println!("{}", tr!("diff: {lines} lines", lines = m.details_lines));
        }
        return Ok(ExitCode::SUCCESS);
    }
    // サイドバーでほかの worktree を選ぶと、TUI が終わってそのパスを返す。同じ開き方（手元・daemon・SSH 先）で
    // 開き直す。Quick Open でブックマークを選んだときは、その場所を開く。
    // Activity の Daemon から共有の daemon を起動できるよう、`daemon start` と同じエンジンを渡す。
    if let Ok(program) = super::daemon_program() {
        amonhen_tui::set_daemon_program(program);
    }
    let mut front = front;
    let mut given = given.into_owned();
    let mut files_here = files_here;
    loop {
        // 開いたことを覚える（アプリ・GTK4 と同じ recent.json。Quick Open・アプリの一覧に出る）。書けなくても開く。
        let _ = amonhen_model::recent::touch(&recent_location(&given));
        #[cfg(unix)]
        let next = match &askpass {
            Some(askpass) => amonhen_tui::run_asking(front, local, files_here, askpass)?,
            None => amonhen_tui::run(front, local, files_here)?,
        };
        #[cfg(not(unix))]
        let next = amonhen_tui::run(front, local, files_here)?;
        // Quick Open で選んだリポジトリは、その場所のまま開く（次の worktree はその開き方で）。
        given = match next {
            None => return Ok(ExitCode::SUCCESS),
            Some(amonhen_tui::Next::Worktree(path)) => worktree_location(&given, &path),
            Some(amonhen_tui::Next::Location(location)) => location,
        };
        files_here = !given.starts_with("ssh://") && !given.starts_with("daemon:ssh://");
        front = Arc::from(open_repository(std::path::Path::new(&given))?);
    }
}

/// 最近開いたものに覚える形。手元のパスは絶対パスにして、リポジトリの中のフォルダなら作業ツリーの一番上にする
/// （アプリと同じく、リンクは解かない）。`ssh://`・`daemon:` の場所はそのまま。
fn recent_location(given: &str) -> String {
    if given.starts_with("ssh://") || given.starts_with("daemon:") {
        return given.to_owned();
    }
    let Ok(full) = std::path::absolute(given) else {
        return given.to_owned();
    };
    let mut clean = PathBuf::new();
    for part in full.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other),
        }
    }
    clean
        .ancestors()
        .find(|d| d.join(".git").exists())
        .unwrap_or(&clean)
        .to_string_lossy()
        .into_owned()
}

/// 開いている場所 `given` と同じ開き方で、worktree の `path`（エンジンが返す、その機械の絶対パス）を開く場所。
fn worktree_location(given: &str, path: &str) -> String {
    for scheme in ["daemon:ssh://", "ssh://"] {
        if let Some(rest) = given.strip_prefix(scheme) {
            let authority = rest.split('/').next().unwrap_or(rest);
            let slash = if path.starts_with('/') { "" } else { "/" };
            return format!("{scheme}{authority}{slash}{path}");
        }
    }
    match given.starts_with("daemon:") {
        true => format!("daemon:{path}"),
        false => path.to_owned(),
    }
}

/// 手元の日付と時刻。
fn local(time: i64) -> String {
    crate::term::local_time(time, true)
}

#[cfg(test)]
mod tests {
    use super::{recent_location, worktree_location};

    /// 最近開いたものには、リポジトリの一番上の絶対パスで覚える（SSH 先・daemon 越しはそのまま）。
    #[test]
    fn recents_keep_the_top_of_the_repository() {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().join("repo");
        std::fs::create_dir_all(top.join(".git")).unwrap();
        std::fs::create_dir_all(top.join("src/deep")).unwrap();
        let inside = top.join("src/deep/..").to_string_lossy().into_owned();
        assert_eq!(recent_location(&inside), top.to_string_lossy());
        assert_eq!(recent_location("ssh://host/w/a"), "ssh://host/w/a");
        assert_eq!(recent_location("daemon:/w/a"), "daemon:/w/a");
    }

    /// worktree は、開いているリポジトリと同じ開き方で開く。
    #[test]
    fn worktrees_open_the_same_way() {
        assert_eq!(worktree_location(".", "/w/other"), "/w/other");
        assert_eq!(
            worktree_location("daemon:/w/main", "/w/other"),
            "daemon:/w/other"
        );
        assert_eq!(
            worktree_location("ssh://me@host:22/w/main", "/w/other"),
            "ssh://me@host:22/w/other"
        );
        assert_eq!(
            worktree_location("daemon:ssh://host/~/main", "/home/me/other"),
            "daemon:ssh://host/home/me/other"
        );
        assert_eq!(
            worktree_location("ssh://host/C:/w/main", "C:/w/other"),
            "ssh://host/C:/w/other"
        );
    }
}
