import AppKit

/// History の行に出す項目を選ぶメニュー。View メニューの「History Items」と、History のヘッダーの右クリックに同じものを出す。
final class HistoryItemsMenu: NSObject, NSMenuItemValidation {
    static let shared = HistoryItemsMenu()

    /// 項目と見出し（出す順）。
    static let entries: [(HistoryItems, String)] = [
        (.refs, "Branches and Tags"), (.author, "Author"), (.date, "Date"), (.id, "Commit ID"),
        (.signature, "Signature"), (.ci, "CI Status"),
    ]

    func make() -> NSMenu {
        let menu = NSMenu(title: L("History Items"))
        for (item, title) in Self.entries {
            let entry = NSMenuItem(title: L(title), action: #selector(toggle(_:)), keyEquivalent: "")
            entry.target = self
            entry.tag = item.rawValue
            menu.addItem(entry)
        }
        return menu
    }

    @objc private func toggle(_ sender: NSMenuItem) {
        Settings.historyItems.formSymmetricDifference(HistoryItems(rawValue: sender.tag))
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        item.state = Settings.historyItems.contains(HistoryItems(rawValue: item.tag)) ? .on : .off
        return true
    }
}
