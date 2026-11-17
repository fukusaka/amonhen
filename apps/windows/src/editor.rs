//! 外部のエディタ（GTK4 版の `editor.rs`、macOS 版の `ExternalEditor`）。アプリのどこから「エディタで開く」ときも、
//! Settings > General で選んだものを使う。選んでいなければ、入っているコードエディタのうち最初のもの、それもなければ
//! メモ帳。設定（`externalEditor`）には実行ファイルのパスを置く（Other… で選んだものも同じ）。
//!
//! 入っているかは、決まったインストール先（ユーザーごと・すべてのユーザー）と、PATH にある起動用のスクリプト
//! （`<インストール先>\bin\code.cmd` など）から探す。

use std::path::{Path, PathBuf};

use amonhen_model::tr;

/// 設定の鍵（GTK4 版・macOS 版と同じ）。
pub const KEY: &str = "externalEditor";

/// 知っているエディタ（この順に探す。macOS 版の `known` と同じ順）: 名前、インストール先のフォルダ（`%LOCALAPPDATA%\
/// Programs` か `%ProgramFiles%` の下）、実行ファイル、PATH にある起動用のスクリプト、フォルダを開けるか。
const KNOWN: &[(&str, &str, &str, &str, bool)] = &[
    ("VS Code", "Microsoft VS Code", "Code.exe", "code.cmd", true),
    (
        "VS Code Insiders",
        "Microsoft VS Code Insiders",
        "Code - Insiders.exe",
        "code-insiders.cmd",
        true,
    ),
    ("Cursor", "cursor", "Cursor.exe", "cursor.cmd", true),
    ("Zed", "Zed", "Zed.exe", "zed.exe", true),
    (
        "Sublime Text",
        "Sublime Text",
        "sublime_text.exe",
        "subl.exe",
        true,
    ),
    ("VSCodium", "VSCodium", "VSCodium.exe", "codium.cmd", true),
    (
        "Notepad++",
        "Notepad++",
        "notepad++.exe",
        "notepad++.exe",
        false,
    ),
];

/// エディタのアプリ。
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    /// 設定に置く値（実行ファイルのパス）。
    pub id: String,
    pub name: String,
    /// フォルダを開けるか（知らない実行ファイルは開けるものとする。GTK4 版と同じ）。
    pub opens_folders: bool,
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

/// 知っているエディタの実行ファイルを探す。
fn find(folder: &str, exe: &str, script: &str) -> Option<PathBuf> {
    let installs = [
        env_dir("LOCALAPPDATA").map(|d| d.join("Programs")),
        env_dir("ProgramFiles"),
        env_dir("ProgramFiles(x86)"),
    ];
    if let Some(path) = installs
        .into_iter()
        .flatten()
        .map(|d| d.join(folder).join(exe))
        .find(|p| p.is_file())
    {
        return Some(path);
    }
    // PATH の起動用のスクリプト（`<インストール先>\bin\code.cmd`）か、実行ファイルそのもの。
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let found = dir.join(script);
        if !found.is_file() {
            return None;
        }
        if found
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            return Some(found);
        }
        let exe = dir.parent()?.join(exe);
        exe.is_file().then_some(exe)
    })
}

/// メモ帳（いつもある。フォルダは開けない）。
fn notepad() -> App {
    let path = env_dir("SystemRoot")
        .map(|d| d.join("System32").join("notepad.exe"))
        .filter(|p| p.is_file())
        .map_or_else(|| "notepad.exe".to_owned(), |p| p.display().to_string());
    App {
        id: path,
        name: "Notepad".to_owned(),
        opens_folders: false,
    }
}

/// 入っているエディタ（KNOWN の順）。最後にメモ帳。
pub fn installed() -> Vec<App> {
    let mut apps: Vec<App> = KNOWN
        .iter()
        .filter_map(|(name, folder, exe, script, folders)| {
            find(folder, exe, script).map(|p| App {
                id: p.display().to_string(),
                name: (*name).to_owned(),
                opens_folders: *folders,
            })
        })
        .collect();
    apps.push(notepad());
    apps
}

/// 選んでいないときに使うもの（入っているもののうち最初）。
pub fn automatic() -> App {
    installed().into_iter().next().unwrap_or_else(notepad)
}

/// 設定の値（実行ファイルのパス）のアプリ。知っているものなら名前を、知らなければファイルの名前を出す。
pub fn app_of(value: &str) -> Option<App> {
    let path = Path::new(value);
    if !path.is_file() {
        return None;
    }
    let file = path.file_name()?.to_string_lossy().into_owned();
    if let Some((name, .., folders)) = KNOWN
        .iter()
        .find(|(_, _, exe, _, _)| exe.eq_ignore_ascii_case(&file))
    {
        return Some(App {
            id: value.to_owned(),
            name: (*name).to_owned(),
            opens_folders: *folders,
        });
    }
    let notepad = file.eq_ignore_ascii_case("notepad.exe");
    Some(App {
        id: value.to_owned(),
        name: if notepad { "Notepad".to_owned() } else { file },
        opens_folders: !notepad,
    })
}

/// 選んでいるもの（なければ自動）。
pub fn chosen() -> App {
    crate::prefs::string(KEY)
        .filter(|v| !v.is_empty())
        .and_then(|v| app_of(&v))
        .unwrap_or_else(automatic)
}

/// フォルダ（リポジトリ・worktree）を選んだエディタで開く。開けなければ、そのわけ（GTK4 版の `open_folder`）。
// エディタの窓を出すのが目的なので、process::command（窓を出さない）は使わない。
#[allow(clippy::disallowed_methods)]
pub fn open_folder(dir: &Path) -> Result<(), String> {
    let app = chosen();
    if !app.opens_folders {
        return Err(tr!(
            "{app} can’t open folders. Choose a code editor in Settings > General.",
            app = app.name
        ));
    }
    std::process::Command::new(&app.id)
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notepad_is_always_there_and_never_opens_folders() {
        let apps = installed();
        let last = apps.last().expect("メモ帳");
        assert_eq!(last.name, "Notepad");
        assert!(!last.opens_folders);
    }

    #[test]
    fn a_known_executable_gets_its_name() {
        let exe = std::env::current_exe().expect("実行ファイル");
        let app = app_of(&exe.display().to_string()).expect("ある");
        assert!(app.opens_folders);
        assert!(app_of(r"C:\no\such\Code.exe").is_none());
    }
}
