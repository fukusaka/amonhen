//! 本物の vector_graphics_compiler（Dart）を動かす。svg_oracles だけが使う。

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use amonhen_model::{Result, process::command};

/// vgc の 1 回の変換（初回は pub get と実行ファイルの組み立てを含む）の時間の上限。
const TIMEOUT: Duration = Duration::from_secs(30);

/// 手元で見つけた Dart と vector_graphics_compiler。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Converter {
    dart: PathBuf,
    pub_cache: PathBuf,
    version: String,
}

/// vgc で変換した結果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Converted {
    /// vector_graphics のバイナリ（`.vec`）。
    pub vec: Vec<u8>,
    /// vgc の警告（`Warning: Unsupported image format svg+xml` など、vgc の英語のまま）。
    pub warnings: Vec<String>,
    /// 使った vector_graphics_compiler の版。
    pub version: String,
}

/// 手元の Dart と、pub のキャッシュの vector_graphics_compiler（いちばん新しい版）。なければ `None`。一度だけ探す。
pub fn converter() -> Option<Converter> {
    static FOUND: OnceLock<Option<Converter>> = OnceLock::new();
    FOUND.get_or_init(find).clone()
}

fn find() -> Option<Converter> {
    let dart = find_dart()?;
    let pub_cache = pub_cache_dirs()
        .into_iter()
        .find_map(|cache| highest_vgc(&cache.join("hosted").join("pub.dev")).map(|v| (cache, v)));
    let (pub_cache, version) = pub_cache?;
    Some(Converter {
        dart,
        pub_cache,
        version,
    })
}

fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// `dart`: PATH の上、PATH の上の Flutter の中（`flutter/bin/cache/dart-sdk/bin/dart`）、よくある置き場所の順に。
/// Finder から起動したアプリはシェルの PATH を持たないので、Homebrew などの置き場所も見る。
fn find_dart() -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let darts: &[&str] = if cfg!(windows) {
        &["dart.exe", "dart.bat"]
    } else {
        &["dart"]
    };
    let flutters: &[&str] = if cfg!(windows) {
        &["flutter.bat", "flutter.exe"]
    } else {
        &["flutter"]
    };
    if let Some(dart) = dirs
        .iter()
        .flat_map(|d| darts.iter().map(move |n| d.join(n)))
        .find(|p| p.is_file())
    {
        return Some(dart);
    }
    if let Some(dart) = dirs
        .iter()
        .flat_map(|d| flutters.iter().map(move |n| d.join(n)))
        .filter(|p| p.is_file())
        .find_map(|p| flutter_dart(&p))
    {
        return Some(dart);
    }
    let mut known: Vec<PathBuf> = Vec::new();
    if cfg!(unix) {
        known.extend(
            [
                "/opt/homebrew/bin/dart",
                "/usr/local/bin/dart",
                "/usr/lib/dart/bin/dart",
            ]
            .map(PathBuf::from),
        );
    }
    if let Some(dart) = known.into_iter().find(|p| p.is_file()) {
        return Some(dart);
    }
    let home = home()?;
    ["flutter", "development/flutter", "fvm/default"]
        .iter()
        .map(|d| home.join(d).join("bin").join(flutters[0]))
        .filter(|p| p.is_file())
        .find_map(|p| flutter_dart(&p))
}

/// Flutter の `bin/flutter` から、同梱の Dart。
fn flutter_dart(flutter: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(flutter).ok()?;
    let dart = real
        .parent()?
        .join("cache")
        .join("dart-sdk")
        .join("bin")
        .join(exe("dart"));
    dart.is_file().then_some(dart)
}

/// pub のキャッシュの候補（`$PUB_CACHE`、なければ OS の既定）。
fn pub_cache_dirs() -> Vec<PathBuf> {
    if let Some(dir) = std::env::var_os("PUB_CACHE").filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(dir)];
    }
    let mut dirs = Vec::new();
    if cfg!(windows) {
        for var in ["LOCALAPPDATA", "APPDATA"] {
            if let Some(base) = std::env::var_os(var).filter(|v| !v.is_empty()) {
                dirs.push(PathBuf::from(base).join("Pub").join("Cache"));
            }
        }
    } else if let Some(home) = home() {
        dirs.push(home.join(".pub-cache"));
    }
    dirs
}

/// `hosted/pub.dev` の中の、いちばん新しい vector_graphics_compiler の版。
fn highest_vgc(hosted: &Path) -> Option<String> {
    std::fs::read_dir(hosted)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let version = name.strip_prefix("vector_graphics_compiler-")?.to_owned();
            e.path()
                .join("bin")
                .join("vector_graphics_compiler.dart")
                .is_file()
                .then_some(version)
        })
        .max_by(|a, b| version_key(a).cmp(&version_key(b)))
}

/// semver の並び（数の部分、正式版はプレリリースより後、プレリリースの文字列）。
fn version_key(v: &str) -> (Vec<u64>, bool, String) {
    let v = v.split('+').next().unwrap_or(v);
    let (core, pre) = match v.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (v, None),
    };
    let nums = core.split('.').map(|n| n.parse().unwrap_or(0)).collect();
    (nums, pre.is_none(), pre.unwrap_or("").to_owned())
}

impl Converter {
    /// SVG を vgc で `.vec` に変える。vgc を使うだけのパッケージ（pubspec と `pub get --offline` の結果、
    /// `dart run` が組み立てた実行ファイル）は一時フォルダに残して次から使い回す（初回は約 2 秒、2 回目からは
    /// 約 0.15 秒）。SVG と `.vec` は呼ぶたびの一時フォルダに置き、終わったら消す。
    pub fn convert(&self, svg: &[u8]) -> Result<Converted> {
        // 同じパッケージで `dart run` が実行ファイルを組み立てるのを重ねない
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let package = self.prepare()?;
        let work = TempDir::new("amonhen-vgc-run")?;
        let input = work.0.join("in.svg");
        let output = work.0.join("out.vec");
        std::fs::write(&input, svg)?;
        let mut cmd = self.dart_command(&package);
        cmd.args(["run", "vector_graphics_compiler", "--input"])
            .arg(&input)
            .arg("--output")
            .arg(&output)
            .args([
                "--no-optimize-masks",
                "--no-optimize-clips",
                "--no-optimize-overdraw",
                "--concurrency",
                "1",
            ]);
        let run = run(cmd)?;
        if !run.success || !output.is_file() {
            let message = failure_message(&run.stdout, &run.stderr);
            return Err(format!(
                "vector_graphics_compiler failed: {message}",
                message = message
            )
            .into());
        }
        Ok(Converted {
            vec: std::fs::read(&output)?,
            warnings: warnings(&run.stdout, &run.stderr),
            version: self.version.clone(),
        })
    }

    fn dart_command(&self, dir: &Path) -> std::process::Command {
        let mut cmd = command(&self.dart);
        cmd.current_dir(dir)
            .env("PUB_CACHE", &self.pub_cache)
            .arg("--suppress-analytics");
        cmd
    }

    /// vgc だけに頼るパッケージ（`<temp>/amonhen-vgc/<版>`）。なければ作り、`pub get --offline` をかける（網には
    /// 出ない。キャッシュに足りない依存があれば失敗する）。
    fn prepare(&self) -> Result<PathBuf> {
        let root = std::env::temp_dir().join("amonhen-vgc");
        let dir = root.join(&self.version);
        let pubspec = format!(
            "name: amonhen_vgc\npublish_to: none\nenvironment:\n  sdk: \">=3.0.0 <4.0.0\"\n\
             dependencies:\n  vector_graphics_compiler: {}\n",
            self.version
        );
        let ready = |dir: &Path| {
            std::fs::read_to_string(dir.join("pubspec.yaml")).is_ok_and(|p| p == pubspec)
                && dir.join(".dart_tool").join("package_config.json").is_file()
        };
        if ready(&dir) {
            return Ok(dir);
        }
        std::fs::create_dir_all(&root)?;
        let staging = TempDir::new_in(&root, "prepare")?;
        std::fs::write(staging.0.join("pubspec.yaml"), &pubspec)?;
        let mut cmd = self.dart_command(&staging.0);
        cmd.args(["pub", "get", "--offline"]);
        let run = run(cmd)?;
        if !run.success {
            let message = last_lines(&format!("{}\n{}", run.stdout, run.stderr), 5);
            return Err(format!("dart pub get failed: {message}", message = message).into());
        }
        if std::fs::rename(&staging.0, &dir).is_err() && !ready(&dir) {
            // 壊れた（途中で止まった）古いもの
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::rename(&staging.0, &dir)?;
        }
        Ok(dir)
    }
}

/// 消える一時フォルダ。
struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Result<Self> {
        Self::new_in(&std::env::temp_dir(), prefix)
    }

    fn new_in(parent: &Path, prefix: &str) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let name = format!(
            "{prefix}-{}-{}-{nanos}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = parent.join(name);
        std::fs::create_dir(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Run {
    success: bool,
    stdout: String,
    stderr: String,
}

/// `cmd` を `TIMEOUT` まで待って走らせる。
fn run(mut cmd: std::process::Command) -> Result<Run> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let reader = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut out);
            }
            String::from_utf8_lossy(&out).into_owned()
        })
    };
    let stdout = reader(child.stdout.take().map(|p| Box::new(p) as _));
    let stderr = reader(child.stderr.take().map(|p| Box::new(p) as _));
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "vector_graphics_compiler didn’t finish in {seconds} seconds",
                seconds = TIMEOUT.as_secs()
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Ok(Run {
        success: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// 成功したときの出力のうち、進み具合と `dart run` の組み立ての知らせ以外（vgc は警告を stdout に書く）。
fn warnings(stdout: &str, stderr: &str) -> Vec<String> {
    stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with("Progress:")
                && !l.starts_with("Building package executable")
                && !l.starts_with("Built ")
        })
        .map(str::to_owned)
        .collect()
}

/// vgc が失敗したときの理由。vgc は `XXXXXXXXXXX <入力> XXXXXXXXXXXXX` の次の行から例外とスタックを stdout に書く。
fn failure_message(stdout: &str, stderr: &str) -> String {
    let mut lines = stdout.lines();
    if lines.any(|l| l.starts_with("XXXXXXXXXXX")) {
        let message: Vec<&str> = lines
            .take_while(|l| !l.starts_with("#0") && !l.starts_with("Some targets failed."))
            .map(str::trim_end)
            .collect();
        let message = message.join("\n").trim().to_owned();
        if !message.is_empty() {
            return message;
        }
    }
    last_lines(&format!("{stdout}\n{stderr}"), 5)
}

fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}
