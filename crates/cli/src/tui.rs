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
        return Ok(ExitCode::SUCCESS);
    }
    #[cfg(unix)]
    if let Some(askpass) = askpass {
        amonhen_tui::run_asking(front, local, &askpass)?;
        return Ok(ExitCode::SUCCESS);
    }
    amonhen_tui::run(front, local)?;
    Ok(ExitCode::SUCCESS)
}

/// 手元の日付と時刻。
fn local(time: i64) -> String {
    crate::term::local_time(time, true)
}
