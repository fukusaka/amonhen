//! 本物の Svg2Vector（Java と sdk-common）を動かす。svg_oracles だけが使う。
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use amonhen_model::Result;

/// 取り込みを待つ長さ。
const TIMEOUT: Duration = Duration::from_secs(20);

const RUNNER: &str = r#"import java.io.*;
import java.nio.file.Path;

public class Runner {
  public static void main(String[] args) throws Exception {
    PrintStream err = new PrintStream(new FileOutputStream(FileDescriptor.err), true, "UTF-8");
    try {
      ByteArrayOutputStream out = new ByteArrayOutputStream();
      String messages = com.android.ide.common.vectordrawable.Svg2Vector.parseSvgToXml(Path.of(args[0]), out);
      System.out.write(out.toByteArray());
      System.out.flush();
      if (messages != null) err.print(messages);
    } catch (Throwable e) {
      e.printStackTrace(err);
      System.exit(2);
    }
  }
}
"#;

/// 見つけた取り込み。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Converter {
    java: PathBuf,
    classpath: Vec<PathBuf>,
    version: String,
}

/// 取り込みの結果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Converted {
    /// VectorDrawable の XML（取り込めないときは空のこともある）。
    pub xml: String,
    /// 取り込みが出したエラー・警告（取り込みの言葉のまま。なければ空）。
    pub messages: String,
    /// sdk-common の版（Android Studio の jar なら Android Studio の版）。
    pub version: String,
}

/// 手元の取り込み。Java と sdk-common の両方が見つかったときだけ。結果は覚える。
pub fn converter() -> Option<Converter> {
    static FOUND: OnceLock<Option<Converter>> = OnceLock::new();
    FOUND.get_or_init(find).clone()
}

impl Converter {
    /// `svg` を VectorDrawable に変える。
    pub fn convert(&self, svg: &[u8]) -> Result<Converted> {
        let dir = TempDir::new()?;
        let runner = dir.0.join("Runner.java");
        let input = dir.0.join("in.svg");
        std::fs::write(&runner, RUNNER)?;
        std::fs::write(&input, svg)?;
        let classpath = std::env::join_paths(&self.classpath)?;
        let mut child = amonhen_model::process::command(&self.java)
            .arg("-Djava.awt.headless=true")
            .arg("-XX:TieredStopAtLevel=1")
            .arg("-cp")
            .arg(classpath)
            .arg(&runner)
            .arg(&input)
            .current_dir(&dir.0)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Can’t start Java: {error}", error = e))?;
        let out = read_on_thread(child.stdout.take());
        let err = read_on_thread(child.stderr.take());
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() > TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Android Studio’s converter didn’t finish in {seconds} seconds",
                    seconds = TIMEOUT.as_secs()
                )
                .into());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let xml = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
        let messages = String::from_utf8_lossy(&err.join().unwrap_or_default())
            .trim()
            .to_owned();
        if !status.success() {
            return Err(format!(
                "Android Studio’s converter failed: {error}",
                error = messages
            )
            .into());
        }
        Ok(Converted {
            xml,
            messages,
            version: self.version.clone(),
        })
    }
}

fn read_on_thread<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    })
}

/// 作ったばかりの一時ディレクトリ。捨てるときに中身ごと消す。
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        loop {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let name = format!(
                "amonhen-svg2vector-{}-{}-{nanos}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            );
            let path = base.join(name);
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- 探す ----

fn find() -> Option<Converter> {
    let studios = android_studios();
    let java = find_java(&studios)?;
    if let Some((classpath, version)) = gradle_cache() {
        return Some(Converter {
            java,
            classpath,
            version,
        });
    }
    studios.iter().find_map(|studio| {
        let (classpath, version) = studio_classpath(studio)?;
        Some(Converter {
            java: java.clone(),
            classpath,
            version,
        })
    })
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

fn java_name() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

/// Java: `JAVA_HOME`、PATH、よくある置き場所、Android Studio に同梱の JBR の順。
fn find_java(studios: &[PathBuf]) -> Option<PathBuf> {
    let name = java_name();
    let mut candidates = Vec::new();
    if let Some(home) = std::env::var_os("JAVA_HOME").filter(|h| !h.is_empty()) {
        candidates.push(PathBuf::from(home).join("bin").join(name));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let java = dir.join(name);
            // macOS の /usr/bin/java は、JDK がないと入れる案内の窓を出すだけ
            if cfg!(target_os = "macos") && java == Path::new("/usr/bin/java") && !mac_has_jdk() {
                continue;
            }
            candidates.push(java);
        }
    }
    if let Some(home) = home() {
        candidates.push(home.join(".sdkman/candidates/java/current/bin").join(name));
    }
    if cfg!(target_os = "macos") {
        candidates.push("/opt/homebrew/opt/openjdk/bin/java".into());
        candidates.push("/usr/local/opt/openjdk/bin/java".into());
    }
    for studio in studios {
        candidates.push(studio.join("jbr/Contents/Home/bin").join(name));
        candidates.push(studio.join("jbr/bin").join(name));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn mac_has_jdk() -> bool {
    let mut dirs = vec![PathBuf::from("/Library/Java/JavaVirtualMachines")];
    if let Some(home) = home() {
        dirs.push(home.join("Library/Java/JavaVirtualMachines"));
    }
    dirs.iter()
        .any(|d| std::fs::read_dir(d).is_ok_and(|mut entries| entries.any(|e| e.is_ok())))
}

/// 版の並べ方: プレリリース（alpha・beta・rc など）より正式版、数の大きいほうを先に。
fn version_key(version: &str) -> (bool, Vec<u64>) {
    let (numbers, suffix) = version.split_once('-').unwrap_or((version, ""));
    let release = suffix.is_empty() || matches!(suffix, "jre" | "android");
    let numbers = numbers
        .split('.')
        .map_while(|part| part.parse().ok())
        .collect();
    (release, numbers)
}

/// Gradle のキャッシュの `<group>/<artifact>` の版を、新しい順に。
fn cached_versions(cache: &Path, group: &str, artifact: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(cache.join(group).join(artifact)) else {
        return Vec::new();
    };
    let mut versions: Vec<String> = entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    versions.sort_by_key(|v| std::cmp::Reverse(version_key(v)));
    versions
}

fn cached_jar(cache: &Path, group: &str, artifact: &str, version: &str) -> Option<PathBuf> {
    let file = format!("{artifact}-{version}.jar");
    std::fs::read_dir(cache.join(group).join(artifact).join(version))
        .ok()?
        .filter_map(|e| Some(e.ok()?.path().join(&file)))
        .find(|p| p.is_file())
}

fn newest_jar(cache: &Path, group: &str, artifact: &str) -> Option<PathBuf> {
    cached_versions(cache, group, artifact)
        .iter()
        .find_map(|v| cached_jar(cache, group, artifact, v))
}

/// Gradle のキャッシュの sdk-common（いちばん新しい版で、要る jar がそろうもの）。
fn gradle_cache() -> Option<(Vec<PathBuf>, String)> {
    let gradle = std::env::var_os("GRADLE_USER_HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(home()?.join(".gradle")))?;
    let cache = gradle.join("caches/modules-2/files-2.1");
    let guava = newest_jar(&cache, "com.google.guava", "guava")?;
    let kotlin = newest_jar(&cache, "org.jetbrains.kotlin", "kotlin-stdlib")?;
    cached_versions(&cache, "com.android.tools", "sdk-common")
        .into_iter()
        .find_map(|version| {
            let sdk = cached_jar(&cache, "com.android.tools", "sdk-common", &version)?;
            let common = cached_jar(&cache, "com.android.tools", "common", &version)?;
            Some((vec![sdk, common, guava.clone(), kotlin.clone()], version))
        })
}

/// 入っている Android Studio（`.app` やインストール先のディレクトリ）。
fn android_studios() -> Vec<PathBuf> {
    let prefixed = |dir: &Path, prefix: &str| -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
            .map(|e| e.path())
            .collect();
        names.sort();
        names
    };
    let mut found = Vec::new();
    let home = home();
    if cfg!(target_os = "macos") {
        found.extend(prefixed(Path::new("/Applications"), "Android Studio"));
        if let Some(home) = &home {
            found.extend(prefixed(&home.join("Applications"), "Android Studio"));
        }
        found = found.into_iter().map(|p| p.join("Contents")).collect();
    } else if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(var).filter(|v| !v.is_empty()) {
                let base = PathBuf::from(base);
                found.extend(prefixed(&base.join("Android"), "Android Studio"));
                found.extend(prefixed(&base.join("Programs"), "Android Studio"));
            }
        }
    } else {
        if let Some(home) = &home {
            found.push(home.join("android-studio"));
            found.extend(prefixed(
                &home.join(".local/share/JetBrains/Toolbox/apps"),
                "android-studio",
            ));
        }
        found.push("/opt/android-studio".into());
        found.push("/usr/local/android-studio".into());
    }
    found.retain(|p| p.is_dir());
    found
}

const SVG2VECTOR_CLASS: &str = "com/android/ide/common/vectordrawable/Svg2Vector.class";

/// Android Studio の jar の置き場所。Svg2Vector の jar があれば、置き場所すべて（`dir/*`）を classpath にする。
fn studio_classpath(studio: &Path) -> Option<(Vec<PathBuf>, String)> {
    let dirs: Vec<PathBuf> = [
        "lib",
        "lib/modules",
        "plugins/android/lib",
        "plugins/android/lib/modules",
    ]
    .iter()
    .map(|d| studio.join(d))
    .filter(|d| d.is_dir())
    .collect();
    let has_converter = dirs.iter().any(|dir| {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries.filter_map(|e| e.ok()).any(|e| {
                let path = e.path();
                path.extension().is_some_and(|x| x == "jar")
                    && jar_has_entry(&path, SVG2VECTOR_CLASS).unwrap_or(false)
            })
        })
    });
    if !has_converter {
        return None;
    }
    let version = ["Resources/build.txt", "build.txt"]
        .iter()
        .find_map(|f| std::fs::read_to_string(studio.join(f)).ok())
        .map_or_else(
            || "Android Studio".to_owned(),
            |b| format!("Android Studio {}", b.trim()),
        );
    Some((dirs.iter().map(|d| d.join("*")).collect(), version))
}

/// jar（zip）の中央ディレクトリに `name` があるか。中身は読まない。
fn jar_has_entry(path: &Path, name: &str) -> std::io::Result<bool> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    // 終わりのレコード（EOCD）は末尾の 22 バイト＋コメント（最大 64 KiB）の中
    let tail_len = len.min(22 + 65_535);
    file.seek(SeekFrom::Start(len - tail_len))?;
    let mut tail = vec![0; tail_len as usize];
    file.read_exact(&mut tail)?;
    let Some(eocd) = tail.windows(4).rposition(|w| w == [0x50, 0x4b, 0x05, 0x06]) else {
        return Ok(false);
    };
    let rec = &tail[eocd..];
    if rec.len() < 22 {
        return Ok(false);
    }
    let size = u64::from(u32::from_le_bytes([rec[12], rec[13], rec[14], rec[15]]));
    let offset = u64::from(u32::from_le_bytes([rec[16], rec[17], rec[18], rec[19]]));
    // ZIP64 なら 0xFFFFFFFF。そのときは中央ディレクトリの位置がわからないので、EOCD の前を大きめに読む
    let (start, size) = if offset == u64::from(u32::MAX) || size == u64::from(u32::MAX) {
        let end = len - tail_len + eocd as u64;
        let size = end.min(64 << 20);
        (end - size, size)
    } else {
        (offset, size.min(len.saturating_sub(offset)))
    };
    file.seek(SeekFrom::Start(start))?;
    let mut dir = vec![0; size as usize];
    file.read_exact(&mut dir)?;
    let needle = name.as_bytes();
    Ok(dir.windows(needle.len()).any(|w| w == needle))
}
