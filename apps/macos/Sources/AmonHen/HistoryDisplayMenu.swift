import AmonhenKit
import AppKit

/// History の見せ方を選ぶメニュー。History の上のバーの右端のボタンと History のヘッダーの右クリックに出す
/// （並び順・項目のほか、1 行表示・祖先の強調・折りたたみも）。
final class HistoryDisplayMenu: NSObject, NSMenuItemValidation {
    static let shared = HistoryDisplayMenu()

    /// 行に出す項目と見出し（出す順）。
    static let entries: [(HistoryItems, String)] = [
        (.refs, "Branches and Tags"), (.author, "Author"), (.date, "Date"), (.id, "Commit ID"),
        (.signature, "Signature"), (.ci, "CI Status"),
    ]

    /// 並び順・項目と、1 行表示・折りたたみ・祖先の強調をまとめたメニュー（バーのボタンとヘッダーの右クリック）。
    func displayMenu() -> NSMenu {
        let menu = NSMenu(title: L("History Display"))
        menu.addItem(.sectionHeader(title: L("Order")))
        addOrders(to: menu)
        menu.addItem(.separator())
        menu.addItem(.sectionHeader(title: L("Items")))
        addItems(to: menu)
        menu.addItem(.separator())
        for (title, action, tip) in [
            (L("One Line per Commit"), #selector(toggleOneLine(_:)), nil),
            (L("Highlight Ancestors"), #selector(toggleAncestors(_:)),
             L("Draw only the selected commit’s ancestors and the lines between them in full color, and dim the rest")),
        ] as [(String, Selector, String?)] {
            let entry = NSMenuItem(title: title, action: action, keyEquivalent: "")
            entry.target = self
            entry.toolTip = tip
            menu.addItem(entry)
        }
        // merge の取り込んだ側を、すべてたたむ・すべて表示する（窓の History に。個々の merge は「+N」・右クリック・
        // ←/→ で開閉し、リポジトリごとに覚える）。
        menu.addItem(.separator())
        for (title, action, tip) in [
            (L("Fold All Merged Commits"), #selector(RepositoryWindow.foldAllMerges(_:)),
             L("Hide the commits every merge brought in; each merge shows their count (click it to expand)")),
            (L("Show All Merged Commits"), #selector(RepositoryWindow.showAllMerges(_:)),
             L("Show the commits every merge brought in")),
        ] {
            let entry = NSMenuItem(title: title, action: action, keyEquivalent: "")
            entry.toolTip = tip
            menu.addItem(entry)
        }
        // コミットの詳細の見せ方（Settings > Appearance と同じもの。ウィンドウからも変えられるように）。
        menu.addItem(.separator())
        menu.addItem(.sectionHeader(title: L("Commit Details")))
        for (title, action, tip) in [
            (L("Monospaced Commit Message"), #selector(toggleMonospaced(_:)),
             L("Show commit messages in a monospaced font (keeps bodies wrapped at 72 columns aligned)")),
            (L("Show Avatars"), #selector(toggleAvatars(_:)),
             L("Show Gravatar avatars in commit details (sends a hash of the email address to gravatar.com)")),
            (L("Changed Files as Tree"), #selector(toggleTree(_:)),
             L("Show a commit’s changed files as a tree of folders (when off, as a list of paths)")),
        ] as [(String, Selector, String)] {
            let entry = NSMenuItem(title: title, action: action, keyEquivalent: "")
            entry.target = self
            entry.toolTip = tip
            menu.addItem(entry)
        }
        return menu
    }

    private func addItems(to menu: NSMenu) {
        for (item, title) in Self.entries {
            let entry = NSMenuItem(title: L(title), action: #selector(toggleItem(_:)), keyEquivalent: "")
            entry.target = self
            entry.tag = item.rawValue
            menu.addItem(entry)
        }
    }

    private func addOrders(to menu: NSMenu) {
        for order in HistoryOrder.allCases {
            let entry = NSMenuItem(title: order.title, action: #selector(setOrder(_:)), keyEquivalent: "")
            entry.toolTip = order.toolTip
            entry.target = self
            entry.tag = Int(order.rawValue)
            menu.addItem(entry)
        }
    }

    @objc private func toggleItem(_ sender: NSMenuItem) {
        Settings.historyItems.formSymmetricDifference(HistoryItems(rawValue: sender.tag))
    }

    @objc private func setOrder(_ sender: NSMenuItem) {
        guard let order = HistoryOrder(rawValue: UInt32(sender.tag)), order != Settings.historyOrder else { return }
        Settings.historyOrder = order
    }

    @objc private func toggleOneLine(_ sender: NSMenuItem) { Settings.compactHistoryRows.toggle() }
    @objc private func toggleAncestors(_ sender: NSMenuItem) { Settings.highlightAncestors.toggle() }
    @objc private func toggleMonospaced(_ sender: NSMenuItem) { Settings.monospacedCommitMessage.toggle() }
    @objc private func toggleAvatars(_ sender: NSMenuItem) { Settings.showAvatars.toggle() }
    @objc private func toggleTree(_ sender: NSMenuItem) { Settings.historyFileTree.toggle() }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        let on = switch item.action {
        case #selector(toggleItem(_:)): Settings.historyItems.contains(HistoryItems(rawValue: item.tag))
        case #selector(setOrder(_:)): Settings.historyOrder.rawValue == UInt32(item.tag)
        case #selector(toggleOneLine(_:)): Settings.compactHistoryRows
        case #selector(toggleAncestors(_:)): Settings.highlightAncestors
        case #selector(toggleMonospaced(_:)): Settings.monospacedCommitMessage
        case #selector(toggleAvatars(_:)): Settings.showAvatars
        case #selector(toggleTree(_:)): Settings.historyFileTree
        default: false
        }
        item.state = on ? .on : .off
        return true
    }
}
