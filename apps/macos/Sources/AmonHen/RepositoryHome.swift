import AppKit

/// リポジトリのページ: リポジトリを開いていないタブ（⌘T・⌘N・⌘O・⌘⇧R、何も開いていないとき）。リポジトリの
/// 一覧を列に分けて見渡し（ブランチ・状態・最後のコミット・最後に開いた日時）、選ぶとこのタブがそのリポジトリに
/// なる。リポジトリのウィンドウと同じタブの並びに入る。
final class RepositoryHomeWindow: NSObject, NSWindowDelegate {
    let window: NSWindow
    let browser = RepositoryBrowser(style: .page)
    /// リポジトリのタブと同じツールバー（タブを切り替えてもタブの並びの位置が動かないように）。リポジトリへの操作は
    /// 届け先がないので押せない。
    private let toolbar = MainToolbar()
    /// 閉じた（AppDelegate が手放す）。
    var onClose: (() -> Void)?

    init(tabWith: NSWindow?) {
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1100, height: 700),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        super.init()
        window.title = "Repositories"
        window.subtitle = L("Choose a repository to open")
        window.isReleasedWhenClosed = false
        window.delegate = self
        window.contentViewController = browser
        window.toolbar = toolbar.toolbar
        toolbar.showNoRepository()
        window.setContentSize(NSSize(width: 1100, height: 700))
        if !window.setFrameUsingName("RepositoryHome") { window.center() }
        window.setFrameAutosaveName("RepositoryHome")
        window.tabbingIdentifier = RepositoryWindow.tabbingIdentifier
        if let tabWith {
            // タブに入るときは、並びのウィンドウの大きさに合わせる。
            tabWith.addTabbedWindow(window, ordered: .above)
            window.makeKeyAndOrderFront(nil)
        } else {
            window.tabbingMode = .disallowed
            window.makeKeyAndOrderFront(nil)
            window.tabbingMode = .preferred
        }
        browser.prepare()
    }

    /// 前に出して、絞り込みの欄に入る。
    func show() {
        window.makeKeyAndOrderFront(nil)
        browser.focusFilter()
    }

    func windowWillClose(_ notification: Notification) {
        onClose?()
    }
}
