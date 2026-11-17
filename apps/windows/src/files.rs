//! ファイルとフォルダを選ぶ Windows の標準のダイアログ（`IFileSaveDialog`・`IFileOpenDialog`）と、選んだファイルを
//! エクスプローラーで見せること。GTK4 版の `gtk::FileDialog`・`FileLauncher` に当たる。ダイアログは窓に対して
//! モーダルで、閉じるまで UI のスレッドで待つ。

use std::path::PathBuf;

use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_PICKFOLDERS, FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog,
    SIGDN_FILESYSPATH,
};
use windows::core::{HSTRING, PCWSTR};

/// 選んだ項目のパス。
fn result(dialog: &IFileDialog) -> Option<PathBuf> {
    // SAFETY: COM の呼び出し。返る文字列は CoTaskMemAlloc のもので、読んだら放す。
    unsafe {
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const _));
        path.map(PathBuf::from)
    }
}

/// 保存するファイルを選ぶ。`name` は初めの名前、`kind`・`extension` は種類の名前と拡張子（`mbox` など）。
pub fn save_file(title: &str, name: &str, kind: &str, extension: &str) -> Option<PathBuf> {
    // SAFETY: COM の呼び出し（UI のスレッドは WinUI が STA で初めている）。文字列は呼び出しの間だけ生きていればよい。
    unsafe {
        let dialog: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let title = HSTRING::from(title);
        let name = HSTRING::from(name);
        let kind = HSTRING::from(kind);
        let pattern = HSTRING::from(format!("*.{extension}"));
        let extension = HSTRING::from(extension);
        let filters = [COMDLG_FILTERSPEC {
            pszName: PCWSTR(kind.as_ptr()),
            pszSpec: PCWSTR(pattern.as_ptr()),
        }];
        dialog.SetTitle(&title).ok()?;
        dialog.SetFileName(&name).ok()?;
        dialog.SetFileTypes(&filters).ok()?;
        dialog.SetDefaultExtension(&extension).ok()?;
        dialog.Show(Some(GetActiveWindow())).ok()?;
        result(&dialog.into())
    }
}

/// フォルダを選ぶ。`button` は選ぶボタンの名前。
pub fn pick_folder(title: &str, button: &str) -> Option<PathBuf> {
    // SAFETY: `save_file` と同じ。
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog.SetOptions(options | FOS_PICKFOLDERS).ok()?;
        dialog.SetTitle(&HSTRING::from(title)).ok()?;
        dialog.SetOkButtonLabel(&HSTRING::from(button)).ok()?;
        dialog.Show(Some(GetActiveWindow())).ok()?;
        result(&dialog.into())
    }
}

/// `path` をエクスプローラーで選んだ形で見せる（GTK4 版の `open_containing_folder`）。
// エクスプローラーの窓を出すのが目的なので、process::command（窓を出さない）は使わない。
#[allow(clippy::disallowed_methods)]
pub fn reveal(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}
