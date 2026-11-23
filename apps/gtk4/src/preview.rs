//! コミットの中のファイル（blob）をプレビューで見せる（PDF・動画・音声などアプリで描けないもの。macOS 版の
//! `QuickLook`）。中身は一時フォルダに元の名前で書き出して渡す（拡張子で種類が決まるため）。GNOME の Sushi
//! （`org.gnome.NautilusPreviewer`）があればそれで、なければ既定のアプリで開く。書き出したものは、次に開くときと
//! アプリを終えるときに消す。

use gtk::prelude::*;
use gtk::{gio, glib};

const SUSHI: &str = "org.gnome.NautilusPreviewer";
const SUSHI_PATH: &str = "/org/gnome/NautilusPreviewer";

/// 書き出す場所（アプリごと）。
fn folder() -> std::path::PathBuf {
    glib::tmp_dir().join("io.github.fukusaka.amonhen.preview")
}

/// `data` を `name`（パスの最後の部分）で書き出して、プレビューで開く（裏で。UI は止めない）。書き出せなければ
/// エラー音。
pub fn show(widget: &impl IsA<gtk::Widget>, data: &[u8], name: &str) {
    let file = match write(data, name) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("プレビューに書き出せない: {e}");
            widget.error_bell();
            return;
        }
    };
    let window = widget
        .root()
        .and_downcast::<gtk::Window>()
        .map(|w| w.downgrade());
    glib::spawn_future_local(async move {
        let file = gio::File::for_path(&file);
        if sushi(&file.uri()).await {
            return;
        }
        let window = window.and_then(|w| w.upgrade());
        gtk::FileLauncher::new(Some(&file)).launch(
            window.as_ref(),
            gio::Cancellable::NONE,
            |result| {
                if let Err(e) = result {
                    eprintln!("プレビューを開けない: {e}");
                }
            },
        );
    });
}

/// 前に書き出したものを消し、新しいフォルダに書き出す（同じ名前を続けて開いても、古い中身を見せ続けないように、
/// 開くたびに別のフォルダへ）。
fn write(data: &[u8], name: &str) -> std::io::Result<std::path::PathBuf> {
    let root = folder();
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join(glib::uuid_string_random().as_str());
    std::fs::create_dir_all(&dir)?;
    let name = std::path::Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("file");
    let file = dir.join(name);
    std::fs::write(&file, data)?;
    Ok(file)
}

/// Sushi で開く（開けたら true）。新しい `org.gnome.NautilusPreviewer2`（窓のハンドルは文字）を先に、だめなら
/// 古い `org.gnome.NautilusPreviewer`（X の窓の番号）で。
async fn sushi(uri: &str) -> bool {
    let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else {
        return false;
    };
    let calls = [
        (
            "org.gnome.NautilusPreviewer2",
            (uri, "", false).to_variant(),
        ),
        (SUSHI, (uri, 0i32, false).to_variant()),
    ];
    for (interface, args) in calls {
        let result = bus
            .call_future(
                Some(SUSHI),
                SUSHI_PATH,
                interface,
                "ShowFile",
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                5000,
            )
            .await;
        match result {
            Ok(_) => return true,
            Err(e) => eprintln!("Sushi で開けない（{interface}）: {e}"),
        }
    }
    false
}

/// 書き出したものを消す（アプリを終えるとき）。
pub fn clean_up() {
    let _ = std::fs::remove_dir_all(folder());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_under_the_file_name_in_a_fresh_folder() {
        let first = write(b"a", "docs/manual.pdf").unwrap();
        assert_eq!(first.file_name().unwrap(), "manual.pdf");
        assert_eq!(std::fs::read(&first).unwrap(), b"a");
        let second = write(b"b", "manual.pdf").unwrap();
        assert_ne!(first.parent(), second.parent());
        // 前のものは消える。
        assert!(!first.exists());
        let empty = write(b"", "").unwrap();
        assert_eq!(empty.file_name().unwrap(), "file");
        clean_up();
        assert!(!empty.exists());
    }
}
