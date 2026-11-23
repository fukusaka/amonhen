import AppKit
import QuickLookUI

/// コミットの中のファイル（blob）を Quick Look の窓で見せる（PDF・動画・音声などアプリで描けないもの）。
/// 中身は一時フォルダに元の名前で書き出して渡す（拡張子で種類が決まるため）。書き出したものは、次に開くときと
/// アプリを終えるときに消す。
@MainActor
final class QuickLook: NSObject, QLPreviewPanelDataSource {
    static let shared = QuickLook()
    private var url: URL?

    /// 書き出す場所（アプリごと）。
    private static let folder = FileManager.default.temporaryDirectory
        .appendingPathComponent("io.github.fukusaka.amonhen.preview", isDirectory: true)

    /// `data` を `name`（パスの最後の部分）で書き出して、Quick Look の窓で開く。
    func show(_ data: Data, name: String) {
        let fm = FileManager.default
        try? fm.removeItem(at: Self.folder)
        // 同じ名前を続けて開いても、窓が古い中身を見せ続けないように、開くたびに別のフォルダへ。
        let dir = Self.folder.appendingPathComponent(UUID().uuidString, isDirectory: true)
        let file = dir.appendingPathComponent(name.isEmpty ? "file" : name)
        do {
            try fm.createDirectory(at: dir, withIntermediateDirectories: true)
            try data.write(to: file)
        } catch {
            NSSound.beep()
            return
        }
        url = file
        guard let panel = QLPreviewPanel.shared() else { return }
        panel.dataSource = self
        panel.reloadData()
        panel.makeKeyAndOrderFront(nil)
    }

    /// 書き出したものを消す（アプリを終えるとき）。
    static func cleanUp() {
        try? FileManager.default.removeItem(at: folder)
    }

    nonisolated func numberOfPreviewItems(in panel: QLPreviewPanel!) -> Int {
        MainActor.assumeIsolated { url == nil ? 0 : 1 }
    }

    nonisolated func previewPanel(_ panel: QLPreviewPanel!, previewItemAt index: Int) -> (any QLPreviewItem)! {
        MainActor.assumeIsolated { url as NSURL? }
    }
}
