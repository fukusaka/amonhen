import AmonhenKit
import AppKit

/// History の見せ方を選ぶメニュー。View メニューの「History Items」「History Order」、History の上のバーの右端の
/// ボタン、History のヘッダーの右クリックに同じものを出す（ボタンとヘッダーには 1 行表示・折りたたみ・祖先の強調も）。
final class HistoryDisplayMenu: NSObject, NSMenuItemValidation {
    static let shared = HistoryDisplayMenu()

    /// 行に出す項目と見出し（出す順）。
    static let entries: [(HistoryItems, String)] = [
        (.refs, "Branches and Tags"), (.author, "Author"), (.date, "Date"), (.id, "Commit ID"),
        (.signature, "Signature"), (.ci, "CI Status"),
    ]

    /// 行に出す項目のメニュー（View メニューの「History Items」）。
    func itemsMenu() -> NSMenu {
        let menu = NSMenu(title: L("History Items"))
        addItems(to: menu)
        return menu
    }

    /// 並び順のメニュー（View メニューの「History Order」）。
    func orderMenu() -> NSMenu {
        let menu = NSMenu(title: L("History Order"))
        addOrders(to: menu)
        return menu
    }

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
            (L("Fold Merged Branches"), #selector(toggleFold(_:)),
             L("In History, hide the commits brought in by a merge and show their count on the merge row (right-click to expand)")),
            (L("Highlight Ancestors"), #selector(toggleAncestors(_:)),
             L("Draw only the selected commit’s ancestors and the lines between them in full color, and dim the rest")),
        ] as [(String, Selector, String?)] {
            let entry = NSMenuItem(title: title, action: action, keyEquivalent: "")
            entry.target = self
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
            (L("All Files in One Diff"), #selector(toggleStacked(_:)),
             L("Stack every file in a commit’s diff (when off, only the selected file)")),
            (L("Changed Files as Tree"), #selector(toggleTree(_:)),
             L("Show a commit’s changed files as a tree to the left of the diff (when off, as a list between the message and the diff)")),
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
    @objc private func toggleFold(_ sender: NSMenuItem) { Settings.foldMergedBranches.toggle() }
    @objc private func toggleAncestors(_ sender: NSMenuItem) { Settings.highlightAncestors.toggle() }
    @objc private func toggleMonospaced(_ sender: NSMenuItem) { Settings.monospacedCommitMessage.toggle() }
    @objc private func toggleAvatars(_ sender: NSMenuItem) { Settings.showAvatars.toggle() }
    @objc private func toggleStacked(_ sender: NSMenuItem) { Settings.stackedCommitDiff.toggle() }
    @objc private func toggleTree(_ sender: NSMenuItem) { Settings.historyFileTree.toggle() }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        let on = switch item.action {
        case #selector(toggleItem(_:)): Settings.historyItems.contains(HistoryItems(rawValue: item.tag))
        case #selector(setOrder(_:)): Settings.historyOrder.rawValue == UInt32(item.tag)
        case #selector(toggleOneLine(_:)): Settings.compactHistoryRows
        case #selector(toggleFold(_:)): Settings.foldMergedBranches
        case #selector(toggleAncestors(_:)): Settings.highlightAncestors
        case #selector(toggleMonospaced(_:)): Settings.monospacedCommitMessage
        case #selector(toggleAvatars(_:)): Settings.showAvatars
        case #selector(toggleStacked(_:)): Settings.stackedCommitDiff
        case #selector(toggleTree(_:)): Settings.historyFileTree
        default: false
        }
        item.state = on ? .on : .off
        return true
    }
}
