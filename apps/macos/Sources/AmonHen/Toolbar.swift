import AppKit

/// ツールバー: 同期（Fetch・Pull・Push）、統合（Merge・Rebase）、退避（Stash・Pop）、状態（Refresh・Undo）。
/// ボタンの action は target を持たず、レスポンダチェーンで `RepositoryWindow` に届く（メニューと同じ操作）。
/// 使えるかどうかも `RepositoryWindow.validateToolbarItem` で決める。
final class MainToolbar: NSObject, NSToolbarDelegate {
    let toolbar = NSToolbar(identifier: "main")
    private let undoItem = NSToolbarItem(itemIdentifier: .undo)
    private let pushItem = NSMenuToolbarItem(itemIdentifier: .push)
    private let pullItem = NSMenuToolbarItem(itemIdentifier: .pull)

    override init() {
        super.init()
        toolbar.delegate = self
        toolbar.displayMode = .iconAndLabel
        toolbar.allowsUserCustomization = true
        toolbar.autosavesConfiguration = true
        setUndo(nil)
    }

    func setUndo(_ label: String?) {
        // ラベルの長さでツールバーの幅が変わらないよう、操作の名前はツールチップに出す。
        undoItem.toolTip = label.map { "Undo: \($0)（⌥⌘Z）" } ?? "Undo できる操作はありません"
    }

    /// 今のブランチが上流より先（push できる）・後ろ（pull できる）にあるコミットの数を、ボタンのバッジに出す。
    func setTracking(ahead: Int, behind: Int) {
        if #available(macOS 26.0, *) {
            pushItem.badge = ahead > 0 ? .count(ahead) : nil
            pullItem.badge = behind > 0 ? .count(behind) : nil
        } else {
            // バッジのない macOS では、ラベルに数を添える。
            pushItem.label = ahead > 0 ? "Push ↑\(ahead)" : "Push"
            pullItem.label = behind > 0 ? "Pull ↓\(behind)" : "Pull"
        }
        pushItem.toolTip = ahead > 0 ? "上流より \(ahead) コミット先にあります（⌃⌘P）" : "Push（⌃⌘P）"
        pullItem.toolTip = behind > 0 ? "上流より \(behind) コミット後ろにあります（⌃⌘L）" : "Pull（⌃⌘L）"
    }

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.fetch, .pull, .push, .space, .merge, .rebase, .space, .stash, .pop, .flexibleSpace, .refresh, .undo]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.fetch, .pull, .push, .merge, .rebase, .stash, .pop, .newBranch, .refresh, .undo, .terminal, .finder,
         .space, .flexibleSpace]
    }

    func toolbar(
        _ toolbar: NSToolbar, itemForItemIdentifier id: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool
    ) -> NSToolbarItem? {
        let item: NSToolbarItem, label: String, symbol: String, action: Selector, tip: String
        switch id {
        case .fetch:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Fetch", "arrow.down.circle",
                                                  #selector(RepositoryWindow.fetch(_:)), "すべてのリモートを fetch（⌃⌘F）")
        case .pull:
            pullItem.menu = Self.menu([
                ("Pull with Merge", #selector(RepositoryWindow.pullMerge(_:))),
                ("Pull with Rebase", #selector(RepositoryWindow.pullRebase(_:))),
                ("Pull Fast-Forward Only", #selector(RepositoryWindow.pullFastForward(_:))),
            ])
            (item, label, symbol, action, tip) = (pullItem, "Pull", "arrow.down.to.line.circle",
                                                  #selector(RepositoryWindow.pull(_:)), "Pull（⌃⌘L）")
        case .push:
            pushItem.menu = Self.menu([
                ("Push", #selector(RepositoryWindow.push(_:))),
                ("Force Push (with Lease)…", #selector(RepositoryWindow.forcePush(_:))),
            ])
            (item, label, symbol, action, tip) = (pushItem, "Push", "arrow.up.circle",
                                                  #selector(RepositoryWindow.push(_:)), "Push（⌃⌘P）")
        case .merge:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Merge", "arrow.triangle.merge",
                                                  #selector(RepositoryWindow.mergeIntoCurrent(_:)),
                                                  "ブランチ・タグ・選んだコミットを今のブランチへ merge（⌥⌘M）")
        case .rebase:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Rebase", "arrow.triangle.pull",
                                                  #selector(RepositoryWindow.rebaseCurrent(_:)),
                                                  "今のブランチを、ブランチ・タグ・選んだコミットの上へ rebase（⌥⌘R）")
        case .stash:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Stash", "tray.and.arrow.down",
                                                  #selector(RepositoryWindow.stashChanges(_:)), "作業中の変更を退避（⌥⌘S）")
        case .pop:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Pop", "tray.and.arrow.up",
                                                  #selector(RepositoryWindow.popStash(_:)), "最新の stash を戻して消す（⌥⌘⇧S）")
        case .newBranch:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Branch", "plus.square.on.square",
                                                  #selector(RepositoryWindow.newBranch(_:)), "HEAD から新しいブランチ（⌥⌘B）")
        case .refresh:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Refresh", "arrow.clockwise",
                                                  #selector(RepositoryWindow.refreshRepository(_:)),
                                                  "History・ブランチ・Working Copy を読み直す（⌘R）")
        case .undo:
            (item, label, symbol, action, tip) = (undoItem, "Undo", "arrow.uturn.backward.circle",
                                                  #selector(RepositoryWindow.undoOperation(_:)), undoItem.toolTip ?? "")
        case .terminal:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Terminal", "terminal",
                                                  #selector(RepositoryWindow.openInTerminal(_:)), "ターミナルで開く")
        case .finder:
            (item, label, symbol, action, tip) = (NSToolbarItem(itemIdentifier: id), "Finder", "folder",
                                                  #selector(RepositoryWindow.revealInFinder(_:)), "Finder で表示")
        default:
            return nil
        }
        if item.label.isEmpty { item.label = label }
        item.paletteLabel = label
        item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
        item.action = action
        if item.toolTip == nil { item.toolTip = tip }
        return item
    }

    private static func menu(_ items: [(String, Selector)]) -> NSMenu {
        let menu = NSMenu()
        for (title, action) in items { menu.addItem(withTitle: title, action: action, keyEquivalent: "") }
        return menu
    }
}

extension NSToolbarItem.Identifier {
    static let fetch = Self("fetch")
    static let pull = Self("pull")
    static let push = Self("push")
    static let merge = Self("merge")
    static let rebase = Self("rebase")
    static let stash = Self("stash")
    static let pop = Self("pop")
    static let newBranch = Self("newBranch")
    static let refresh = Self("refresh")
    static let undo = Self("undo")
    static let terminal = Self("terminal")
    static let finder = Self("finder")
}
