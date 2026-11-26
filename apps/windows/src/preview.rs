//! コミットの中のファイル（blob）をアプリの外で見せる（PDF・動画・音声などアプリで描けないもの。macOS 版の
//! `QuickLook`、GTK4 版の `preview`）。Windows には Quick Look に当たる標準のプレビューがないので、GTK4 版が
//! プレビューのないときにするのと同じく、既定のアプリで開く。中身は一時フォルダに元の名前で書き出す（拡張子で
//! 種類が決まるため）。書き出したものは、次に開くときに消す。

use std::path::PathBuf;

/// 書き出す場所（アプリごと）。
fn folder() -> PathBuf {
    std::env::temp_dir().join("io.github.fukusaka.amonhen.preview")
}

/// `data` を `name`（パスの最後の部分）で書き出して、既定のアプリで開く。開けたら true。
pub fn show(data: &[u8], name: &str) -> bool {
    match write(data, name) {
        Ok(file) => crate::files::open_default(&file),
        Err(e) => {
            eprintln!("プレビューに書き出せない: {e}");
            false
        }
    }
}

/// 前に書き出したものを消し、新しいフォルダに書き出す（同じ名前を続けて開いても、古い中身を見せ続けないように、
/// 開くたびに別のフォルダへ）。
fn write(data: &[u8], name: &str) -> std::io::Result<PathBuf> {
    let root = folder();
    let _ = std::fs::remove_dir_all(&root);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = root.join(format!("{stamp:x}"));
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(file_name(name));
    std::fs::write(&file, data)?;
    Ok(file)
}

/// パスの最後の部分（空なら `file`。`..` などは使わない）。
fn file_name(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or_default();
    if name.is_empty() || name == "." || name == ".." {
        "file"
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_under_the_last_part_of_the_path() {
        assert_eq!(file_name("docs/guide.pdf"), "guide.pdf");
        assert_eq!(file_name("a\\b.mp4"), "b.mp4");
        assert_eq!(file_name("dir/.."), "file");
        assert_eq!(file_name(""), "file");
    }
}
