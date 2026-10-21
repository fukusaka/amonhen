import AmonhenKit
import AppKit

/// History の見せ方を選ぶメニュー。View メニューの「History Items」「History Order」、History の上のバーの右端の
/// ボタン、History のヘッダーの右クリックに同じものを出す。
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

    /// 並び順・項目・1 行表示をまとめたメニュー（バーのボタンとヘッダーの右クリック）。
    func displayMenu() -> NSMenu {
        let menu = NSMenu(title: L("History Display"))
        menu.addItem(.sectionHeader(title: L("Order")))
        addOrders(to: menu)
        menu.addItem(.separator())
        menu.addItem(.sectionHeader(title: L("Items")))
        addItems(to: menu)
        menu.addItem(.separator())
        let oneLine = NSMenuItem(title: L("One Line per Commit"), action: #selector(toggleOneLine(_:)), keyEquivalent: "")
        oneLine.target = self
        menu.addItem(oneLine)
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

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        let on = switch item.action {
        case #selector(toggleItem(_:)): Settings.historyItems.contains(HistoryItems(rawValue: item.tag))
        case #selector(setOrder(_:)): Settings.historyOrder.rawValue == UInt32(item.tag)
        case #selector(toggleOneLine(_:)): Settings.compactHistoryRows
        default: false
        }
        item.state = on ? .on : .off
        return true
    }
}
