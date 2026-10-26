//! この機械にあるプログラム（git・gpg・gpgsm・ssh-keygen）を探す（設定の「プログラム」の候補）。PATH で最初に
//! 見つかるものを先頭に、よく置かれる場所と PATH の残りから、同じ実体は 1 つにして集める。版は `--version` で読む。

use std::path::{Path, PathBuf};

use amonhen_model::Program;

/// `name` の候補。先頭は PATH で最初に見つかるもの（何も設定しなければ使われるもの）。
pub fn programs(name: &str) -> Vec<Program> {
    let usual: &[&str] = if cfg!(windows) {
        &[]
    } else {
        &[
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/local/MacGPG2/bin",
            "/opt/local/bin",
            "/home/linuxbrew/.linuxbrew/bin",
            "/usr/bin",
        ]
    };
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let first = path_dirs
        .iter()
        .map(|d| d.join(&file))
        .find(|p| executable(p));
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut found: Vec<Program> = Vec::new();
    for candidate in first
        .into_iter()
        .chain(usual.iter().map(|d| Path::new(d).join(&file)))
        .chain(path_dirs.iter().map(|d| d.join(&file)))
    {
        if !executable(&candidate) {
            continue;
        }
        let real = std::fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
        if seen.contains(&real) {
            continue;
        }
        seen.push(real);
        found.push(Program {
            version: version(name, &candidate),
            path: candidate.to_string_lossy().into_owned(),
        });
    }
    found
}

fn executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// 版。git・gpg・gpgsm は `--version` の 1 行目（git は `git version ` の後、ほかは最後の語）。ssh-keygen は
/// 同じ場所の `ssh -V`（`OpenSSH_9.9p2, …`。標準エラー出力に出る）。
fn version(name: &str, path: &Path) -> Option<String> {
    if name == "ssh-keygen" {
        let ssh = path.with_file_name(if cfg!(windows) { "ssh.exe" } else { "ssh" });
        let out = amonhen_model::process::command(ssh)
            .arg("-V")
            .output()
            .ok()?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        return parse_ssh_version(&text);
    }
    let out = amonhen_model::process::command(path)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    parse_version(name, &String::from_utf8_lossy(&out.stdout))
}

fn parse_version(name: &str, text: &str) -> Option<String> {
    let line = text.lines().next()?.trim();
    if name == "git" {
        return line
            .strip_prefix("git version ")?
            .split_whitespace()
            .next()
            .map(str::to_owned);
    }
    line.split_whitespace().last().map(str::to_owned)
}

fn parse_ssh_version(text: &str) -> Option<String> {
    let rest = &text[text.find("OpenSSH_")? + "OpenSSH_".len()..];
    let v: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != ',')
        .collect();
    (!v.is_empty()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_versions_from_the_first_line() {
        assert_eq!(
            parse_version("git", "git version 2.54.0 (Apple Git-157)\n").as_deref(),
            Some("2.54.0")
        );
        assert_eq!(
            parse_version("git", "git version 2.45.1.windows.1").as_deref(),
            Some("2.45.1.windows.1")
        );
        assert_eq!(
            parse_version("gpg", "gpg (GnuPG) 2.4.5\nlibgcrypt 1.10.3\n").as_deref(),
            Some("2.4.5")
        );
        assert_eq!(
            parse_version("gpgsm", "gpgsm (GnuPG) 2.4.5").as_deref(),
            Some("2.4.5")
        );
        assert_eq!(parse_version("git", "not git"), None);
        assert_eq!(
            parse_ssh_version("OpenSSH_9.9p2, LibreSSL 3.3.6\n").as_deref(),
            Some("9.9p2")
        );
        assert_eq!(
            parse_ssh_version("OpenSSH_for_Windows_9.5p1"),
            Some("for_Windows_9.5p1".to_owned())
        );
        assert_eq!(parse_ssh_version("ssh: not found"), None);
    }

    #[test]
    fn finds_git_first_on_path() {
        let found = programs("git");
        assert!(!found.is_empty(), "git は PATH にあるはず");
        assert!(
            found[0]
                .version
                .as_deref()
                .is_some_and(|v| v.starts_with(char::is_numeric))
        );
        let mut paths: Vec<&str> = found.iter().map(|p| p.path.as_str()).collect();
        paths.dedup();
        assert_eq!(paths.len(), found.len());
    }
}
