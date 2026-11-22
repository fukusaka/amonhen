//! 開いていた窓の記録（macOS 版・GTK4 版の、終了したときに開いていた窓とタブを起動したときに戻す）。
//!
//! 窓はどれも 1 つのプロセスにあり、閉じる窓がそれぞれ自分のタブを `windows\<pid>-<窓>.json` に書く。
//! File > Amon Hen を終了（Ctrl+Q）は、先に印（`quit`）を置いてからすべての窓を閉じる。印があれば、閉じる窓は
//! みな書き足す。印がなく、ほかに窓がなければ（最後の 1 つを閉じた）、前の記録を消して自分だけを書く。ほかに窓が
//! 残っていれば何も書かない（1 つずつ閉じた窓は戻さない）。引数なしで起動したら、記録をすべて読んで消し、
//! 1 つ目を最初の窓に、残りをそれぞれ同じプロセスの新しい窓で開く。

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// 終了の印。これより古ければ、終了の途中ではない（印を置いた後に開いた窓を 1 つずつ閉じた）。
const QUIT: &str = "quit";
const QUIT_FRESH: Duration = Duration::from_secs(30);

/// 記録を置くフォルダ（エンジンの `settings.json` の隣）。
fn dir() -> Option<PathBuf> {
    amonhen_model::settings::path().map(|p| p.with_file_name("windows"))
}

/// フォルダの中の窓の記録（古い順。作った順に窓を開く）。
fn records(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut found: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .map(|p| {
            let at = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (at, p)
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

fn quitting(dir: &std::path::Path) -> bool {
    std::fs::metadata(dir.join(QUIT))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age < QUIT_FRESH)
}

/// 窓 `window` を閉じるときに、その窓のタブ（`tabs::saved` の形）を覚える。`last` はこのプロセスの最後の窓。
pub fn remember(window: u64, saved: &str, last: bool) {
    let Some(dir) = dir() else { return };
    let quit = quitting(&dir);
    if !quit {
        if !last || others_open() {
            return;
        }
        // 最後の 1 つを閉じた: 前の記録は捨てて、この窓だけを戻す。
        for old in records(&dir) {
            let _ = std::fs::remove_file(old);
        }
    }
    let written = std::fs::create_dir_all(&dir).and_then(|()| {
        std::fs::write(
            dir.join(format!("{}-{window}.json", std::process::id())),
            saved,
        )
    });
    if let Err(e) = written {
        eprintln!("窓のタブを覚えられない: {e}");
    }
}

/// 起動したときに戻す窓（それぞれ `tabs::saved` の形）。読んだら記録と終了の印は消す。
pub fn take() -> Vec<String> {
    let Some(dir) = dir() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for path in records(&dir) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            out.push(text);
        }
        let _ = std::fs::remove_file(path);
    }
    let _ = std::fs::remove_file(dir.join(QUIT));
    out
}

/// File > Amon Hen を終了: 印を置き、前の記録を捨ててから、この実行ファイルの窓をすべて閉じる（それぞれが
/// 閉じるときに自分のタブを書く）。
pub fn quit_all() {
    if let Some(dir) = dir() {
        for old in records(&dir) {
            let _ = std::fs::remove_file(old);
        }
        let marked =
            std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(QUIT), ""));
        if let Err(e) = marked {
            eprintln!("終了の印を置けない: {e}");
        }
    }
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    for window in app_windows(true) {
        let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }
}

/// ほかのプロセスに、この実行ファイルの窓が開いているか。
fn others_open() -> bool {
    !app_windows(false).is_empty()
}

/// この実行ファイルの WinUI の窓（見えているもの。`own` ならこのプロセスのものも）。
fn app_windows(own: bool) -> Vec<windows::Win32::Foundation::HWND> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible,
    };
    unsafe extern "system" fn found(window: HWND, out: LPARAM) -> windows::core::BOOL {
        let mut class = [0u16; 64];
        let n = unsafe { GetClassNameW(window, &mut class) } as usize;
        if unsafe { IsWindowVisible(window) }.as_bool()
            && String::from_utf16_lossy(&class[..n]) == "WinUIDesktopWin32WindowClass"
        {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
            let list = unsafe { &mut *(out.0 as *mut Vec<(HWND, u32)>) };
            list.push((window, pid));
        }
        true.into()
    }
    let mut all: Vec<(HWND, u32)> = Vec::new();
    let _ = unsafe { EnumWindows(Some(found), LPARAM(&mut all as *mut _ as isize)) };
    let me = std::process::id();
    let exe = std::env::current_exe().ok();
    let mut same: std::collections::HashMap<u32, bool> = std::collections::HashMap::new();
    all.into_iter()
        .filter(|(_, pid)| {
            if *pid == me {
                return own;
            }
            *same
                .entry(*pid)
                .or_insert_with(|| image_of(*pid).is_some_and(|p| same_path(&p, exe.as_deref())))
        })
        .map(|(w, _)| w)
        .collect()
}

fn same_path(a: &std::path::Path, b: Option<&std::path::Path>) -> bool {
    b.is_some_and(|b| {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    })
}

/// プロセスの実行ファイルのパス。
fn image_of(pid: u32) -> Option<PathBuf> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = [0u16; 1024];
    let mut size = buffer.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    ok.ok()?;
    Some(PathBuf::from(String::from_utf16_lossy(
        &buffer[..size as usize],
    )))
}
