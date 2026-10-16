import AmonhenKit
import AppKit
import SwiftUI

/// 利用者の設定。
enum Settings {
    /// commit-graph がないリポジトリで `git commit-graph write --reachable` を実行するか。
    /// リポジトリの `.git` に書き込むので、既定では行わない。
    static var writeCommitGraph: Bool {
        get { UserDefaults.standard.bool(forKey: "writeCommitGraph") }
        set { UserDefaults.standard.set(newValue, forKey: "writeCommitGraph") }
    }

    /// 手元のリポジトリの status で git の fsmonitor を使うか（既定は使う）。リポジトリごとに
    /// `git fsmonitor--daemon` が常駐し、status が速くなる。
    static var useFSMonitor: Bool {
        get { UserDefaults.standard.object(forKey: "useFSMonitor") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "useFSMonitor")
            Repository.setFSMonitor(newValue)
        }
    }

    /// SSH 先のリポジトリのコミットの署名を、手元の鍵で検証するか（既定は手元）。false なら SSH 先の git と鍵で
    /// 検証する。変えたら `signatureCheckChanged` を知らせる。
    static var checkRemoteSignaturesLocally: Bool {
        get { UserDefaults.standard.object(forKey: "checkRemoteSignaturesLocally") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "checkRemoteSignaturesLocally")
            NotificationCenter.default.post(name: .signatureCheckChanged, object: nil)
        }
    }

    /// コミットの詳細に作者・コミッターの Gravatar のアバターを出すか（既定は出す。メールアドレスの MD5 を
    /// gravatar.com に送る）。変えたら `commitMessageStyleChanged` を知らせる。
    static var showAvatars: Bool {
        get { UserDefaults.standard.object(forKey: "showAvatars") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "showAvatars")
            NotificationCenter.default.post(name: .commitMessageStyleChanged, object: nil)
        }
    }

    /// コミットの本文を等幅で出すか（既定は等幅）。変えたら `commitMessageStyleChanged` を知らせる。
    static var monospacedCommitMessage: Bool {
        get { UserDefaults.standard.object(forKey: "monospacedCommitMessage") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "monospacedCommitMessage")
            NotificationCenter.default.post(name: .commitMessageStyleChanged, object: nil)
        }
    }

    /// 選んだコミットの祖先を強調するか（既定はする）。変えたら `highlightAncestorsChanged` を知らせる。
    static var highlightAncestors: Bool {
        get { UserDefaults.standard.object(forKey: "highlightAncestors") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "highlightAncestors")
            NotificationCenter.default.post(name: .highlightAncestorsChanged, object: nil)
        }
    }

    /// Pull（ツールバーのボタン・⌃⌘L）で上流を取り込む既定の方法。`pullMode` の命令に渡す値。
    static var pullMode: PullMode {
        get { UserDefaults.standard.string(forKey: "pullMode").flatMap(PullMode.init) ?? .merge }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "pullMode") }
    }

    /// History のコミットの差分で、全ファイルを縦に並べるか（既定は並べる。オフなら選んだファイルだけ）。
    /// 変えたら `stackedCommitDiffChanged` を知らせる。
    static var stackedCommitDiff: Bool {
        get { UserDefaults.standard.object(forKey: "stackedCommitDiff") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "stackedCommitDiff")
            NotificationCenter.default.post(name: .stackedCommitDiffChanged, object: nil)
        }
    }

    /// History の行を 1 行にするか（既定は件名と作者・日時の 2 行）。変えたら `historyRowsChanged` を知らせる。
    static var compactHistoryRows: Bool {
        get { UserDefaults.standard.bool(forKey: "compactHistoryRows") }
        set {
            UserDefaults.standard.set(newValue, forKey: "compactHistoryRows")
            NotificationCenter.default.post(name: .historyRowsChanged, object: nil)
        }
    }
}

extension Notification.Name {
    static let historyRowsChanged = Notification.Name("historyRowsChanged")
    static let highlightAncestorsChanged = Notification.Name("highlightAncestorsChanged")
    static let signatureCheckChanged = Notification.Name("signatureCheckChanged")
    static let commitMessageStyleChanged = Notification.Name("commitMessageStyleChanged")
    static let stackedCommitDiffChanged = Notification.Name("stackedCommitDiffChanged")
}


/// pull での上流の取り込み方（`crates/model` の `PullMode`）。
enum PullMode: String, CaseIterable {
    case merge, rebase, fastForwardOnly

    var title: String {
        switch self {
        case .merge: "Merge"
        case .rebase: "Rebase"
        case .fastForwardOnly: "Fast-Forward Only"
        }
    }
}

/// Settings ウィンドウ（⌘,）の中身。開くたびに `Settings` から読み直し、変えたらすぐ書き戻す。
@Observable
final class SettingsModel {
    var pullMode = Settings.pullMode { didSet { Settings.pullMode = pullMode } }
    var compactHistoryRows = Settings.compactHistoryRows { didSet { Settings.compactHistoryRows = compactHistoryRows } }
    var highlightAncestors = Settings.highlightAncestors { didSet { Settings.highlightAncestors = highlightAncestors } }
    var monospacedCommitMessage = Settings.monospacedCommitMessage {
        didSet { Settings.monospacedCommitMessage = monospacedCommitMessage }
    }
    var showAvatars = Settings.showAvatars { didSet { Settings.showAvatars = showAvatars } }
    var stackedCommitDiff = Settings.stackedCommitDiff { didSet { Settings.stackedCommitDiff = stackedCommitDiff } }
    var writeCommitGraph = Settings.writeCommitGraph { didSet { Settings.writeCommitGraph = writeCommitGraph } }
    var useFSMonitor = Settings.useFSMonitor { didSet { Settings.useFSMonitor = useFSMonitor } }
    var checkRemoteSignaturesLocally = Settings.checkRemoteSignaturesLocally {
        didSet { Settings.checkRemoteSignaturesLocally = checkRemoteSignaturesLocally }
    }

    /// メニューで変えた分を拾う（同じ値なら書き戻さない）。
    func reload() {
        if pullMode != Settings.pullMode { pullMode = Settings.pullMode }
        if compactHistoryRows != Settings.compactHistoryRows { compactHistoryRows = Settings.compactHistoryRows }
        if highlightAncestors != Settings.highlightAncestors { highlightAncestors = Settings.highlightAncestors }
        if monospacedCommitMessage != Settings.monospacedCommitMessage {
            monospacedCommitMessage = Settings.monospacedCommitMessage
        }
        if showAvatars != Settings.showAvatars { showAvatars = Settings.showAvatars }
        if stackedCommitDiff != Settings.stackedCommitDiff { stackedCommitDiff = Settings.stackedCommitDiff }
        if writeCommitGraph != Settings.writeCommitGraph { writeCommitGraph = Settings.writeCommitGraph }
        if useFSMonitor != Settings.useFSMonitor { useFSMonitor = Settings.useFSMonitor }
        if checkRemoteSignaturesLocally != Settings.checkRemoteSignaturesLocally {
            checkRemoteSignaturesLocally = Settings.checkRemoteSignaturesLocally
        }
    }
}

/// 項目の下に添える説明。
private struct Note: View {
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }
}

private struct GitSettings: View {
    @Bindable var model: SettingsModel
    var body: some View {
        Form {
            Section {
                Picker("Pull の既定", selection: $model.pullMode) {
                    ForEach(PullMode.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                Note("ツールバーの Pull と ⌃⌘L で使う。Pull ボタンの ▾ や Repository > Pull With でその都度も選べる。")
            }
        }
        .formStyle(.grouped)
    }
}

private struct AppearanceSettings: View {
    @Bindable var model: SettingsModel
    var body: some View {
        Form {
            Section("History") {
                Toggle("1 コミットを 1 行で表示する", isOn: $model.compactHistoryRows)
                Toggle("選んだコミットの祖先を強調する", isOn: $model.highlightAncestors)
                Note("選んだコミットの祖先と、それをつなぐ線だけを濃く描き、ほかを薄くする。")
            }
            Section("コミットの詳細") {
                Toggle("本文を等幅の字体で表示する", isOn: $model.monospacedCommitMessage)
                Toggle("アバターを表示する", isOn: $model.showAvatars)
                Note("Gravatar から取る（メールアドレスのハッシュを gravatar.com に送る）。")
                Toggle("全ファイルの差分を縦に並べる", isOn: $model.stackedCommitDiff)
                Note("GitHub のプルリクエストのように、変更ファイルの差分を続けて見せる。ファイルの見出しで畳め、Viewed で見たものを畳む。変更ファイルが 200 を超えるコミットでは 1 ファイルずつ。")
            }
        }
        .formStyle(.grouped)
    }
}

private struct PerformanceSettings: View {
    @Bindable var model: SettingsModel
    var body: some View {
        Form {
            Section {
                Toggle("commit-graph を自動で書き出す", isOn: $model.writeCommitGraph)
                Note("commit-graph がないリポジトリを開いたとき、裏で git commit-graph write --reachable --changed-paths を実行する（.git に書き込む）。履歴の読み込みと path: での検索が速くなる。")
                Toggle("fsmonitor で status を速くする", isOn: $model.useFSMonitor)
                Note("この Mac のリポジトリの status で git の fsmonitor を使う（リポジトリごとに git fsmonitor--daemon が常駐し、.git にソケットを作る）。")
            }
        }
        .formStyle(.grouped)
    }
}

private struct AdvancedSettings: View {
    @Bindable var model: SettingsModel
    var body: some View {
        Form {
            Section("SSH 先のリポジトリ") {
                Toggle("署名をこの Mac の鍵で検証する", isOn: $model.checkRemoteSignaturesLocally)
                Note("この Mac の gpg・allowed_signers・信用度で検証する。外すと SSH 先の git と鍵で検証する。この Mac にあるリポジトリは、いつもこの Mac で検証する。")
            }
        }
        .formStyle(.grouped)
    }
}

/// Settings ウィンドウ。上に項目の分類のタブを並べる（macOS 標準の形）。
final class SettingsWindowController: NSWindowController {
    static let shared = SettingsWindowController()
    private let model = SettingsModel()

    private init() {
        let tabs = NSTabViewController()
        tabs.tabStyle = .toolbar
        func add(_ title: String, _ symbol: String, _ view: some View) {
            let host = NSHostingController(rootView: view.frame(width: 520).fixedSize())
            host.sizingOptions = .preferredContentSize
            // タブを選ぶと、ウィンドウのタイトルがタブの名前になる（macOS 標準の設定画面と同じ）。
            host.title = title
            let item = NSTabViewItem(viewController: host)
            item.label = title
            item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: title)
            tabs.addTabViewItem(item)
        }
        add("Git", "arrow.triangle.branch", GitSettings(model: model))
        add("Appearance", "paintbrush", AppearanceSettings(model: model))
        add("Performance", "speedometer", PerformanceSettings(model: model))
        add("Advanced", "gearshape.2", AdvancedSettings(model: model))
        let window = NSWindow(contentViewController: tabs)
        window.styleMask = [.titled, .closable]
        window.title = "Settings"
        window.setFrameAutosaveName("Settings")
        super.init(window: window)
    }

    required init?(coder: NSCoder) { fatalError() }

    func show() {
        model.reload()
        if window?.isVisible != true { window?.center() }
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
    }
}
