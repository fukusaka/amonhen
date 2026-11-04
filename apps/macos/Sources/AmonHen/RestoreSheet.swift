import AppKit

/// 作業ツリーのファイルを別のコミットの版に戻す前の確認。index にも入れるかを選べる（既定は git restore と同じく
/// 作業ツリーだけ）。
@MainActor
enum RestoreSheet {
    /// `path` を `rev` の版（`before` なら `rev` の前の版）に戻す。`paths` は実際に戻すパス。
    static func ask(in window: NSWindow, path: String, paths: [String], rev: String, before: Bool,
                    perform: @escaping ([String: Any]) -> Void) {
        let short = String(rev.prefix(Titles.shortIDLength))
        let alert = NSAlert()
        alert.messageText = before ? L("Restore %@ as It Was Before %@?", path, short) : L("Restore %@ as It Is in %@?", path, short)
        alert.informativeText = L("Replaces the file in the working tree with that version (git restore); a file that version doesn’t have is deleted. Undo brings back the file as it is now.")
        let staged = NSButton(checkboxWithTitle: L("Stage it too"), target: nil, action: nil)
        alert.accessoryView = staged
        alert.addButton(withTitle: L("Restore"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            perform(["op": "restoreFiles", "rev": rev, "paths": paths, "staged": staged.state == .on])
        }
    }
}
