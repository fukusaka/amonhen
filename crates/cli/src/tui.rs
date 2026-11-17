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
    let mut measure = false;
    let mut size = DEFAULT_SIZE;
    let mut a = Args::new(args);
    while let Some(arg) = a.rest.next() {
        match arg.as_str() {
            "-C" => dir = PathBuf::from(a.value("-C")?),
            "--snapshot" => snapshot = Some(a.value("--snapshot")?.to_owned()),
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
    let front: Arc<dyn Front> = Arc::from(open_repository(&dir)?);
    if let Some(file) = snapshot {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let text = amonhen_tui::snapshot(front, size.0, size.1, now)?;
        if file == "-" {
            crate::term::write(&text);
        } else {
            std::fs::write(&file, text)?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    if measure {
        let m =
            amonhen_tui::measure_scroll(front, size.0, size.1, MEASURE_FRAMES, MEASURE_INTERVAL)?;
        println!(
            "{}",
            tr!(
                "{cols}x{rows}, {commits} commits, {frames} frames\nframe time: median {median} µs, 95% {p95} µs, max {max} µs\nframe bytes: mean {mean}, max {max_bytes}\nframes with rows not loaded yet: {incomplete}",
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
            )
        );
        return Ok(ExitCode::SUCCESS);
    }
    amonhen_tui::run(front)?;
    Ok(ExitCode::SUCCESS)
}
