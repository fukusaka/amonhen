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

    // MARK: GitHub（gh で読む。変えたら `gitHubSettingsChanged` を知らせる）

    private static func gitHubBool(_ key: String, default value: Bool) -> Bool {
        UserDefaults.standard.object(forKey: key) as? Bool ?? value
    }

    private static func setGitHub(_ value: Any, _ key: String) {
        UserDefaults.standard.set(value, forKey: key)
        NotificationCenter.default.post(name: .gitHubSettingsChanged, object: nil)
    }

    /// History のコミットごとに CI の状態を読んで出すか（既定は出す）。
    static var gitHubCIStatus: Bool {
        get { gitHubBool("gitHubCIStatus", default: true) }
        set { setGitHub(newValue, "gitHubCIStatus") }
    }

    /// コミットの作者の GitHub のアバターを使うか（既定は使う。使わなければ Gravatar だけ）。
    static var gitHubAvatars: Bool {
        get { gitHubBool("gitHubAvatars", default: true) }
        set { setGitHub(newValue, "gitHubAvatars") }
    }

    /// CI を読むブランチに、既定のブランチも含めるか（既定は含める。外すと今のブランチだけ）。
    static var gitHubCIDefaultBranch: Bool {
        get { gitHubBool("gitHubCIDefaultBranch", default: true) }
        set { setGitHub(newValue, "gitHubCIDefaultBranch") }
    }

    /// ブランチごとに読む直近のコミットの数（既定は 100）。
    static var gitHubCICommits: Int {
        get { [30, 50, 100].contains(UserDefaults.standard.integer(forKey: "gitHubCICommits")) ? UserDefaults.standard.integer(forKey: "gitHubCICommits") : 100 }
        set { setGitHub(newValue, "gitHubCICommits") }
    }

    /// アバターの URL は、ウィンドウを開いて最初の 1 回だけ読むか（既定はそうする。2 回目からは CI の状態だけを読む）。
    static var gitHubAvatarsOnce: Bool {
        get { gitHubBool("gitHubAvatarsOnce", default: true) }
        set { setGitHub(newValue, "gitHubAvatarsOnce") }
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

    /// History の変更ファイルを、差分の左に木で出すか（既定はメッセージと差分の間に一覧）。
    /// 変えたら `historyFileTreeChanged` を知らせる。
    static var historyFileTree: Bool {
        get { UserDefaults.standard.bool(forKey: "historyFileTree") }
        set {
            UserDefaults.standard.set(newValue, forKey: "historyFileTree")
            NotificationCenter.default.post(name: .historyFileTreeChanged, object: nil)
        }
    }

    /// History でマージ済みブランチを折りたたむか（既定は折りたたまない）。変えたら `foldMergedBranchesChanged` を知らせる。
    static var foldMergedBranches: Bool {
        get { UserDefaults.standard.bool(forKey: "foldMergedBranches") }
        set {
            UserDefaults.standard.set(newValue, forKey: "foldMergedBranches")
            NotificationCenter.default.post(name: .foldMergedBranchesChanged, object: nil)
        }
    }

    /// File History にマージのコミットも出すか（既定は出さない）。変えたら `fileHistoryMergesChanged` を知らせる。
    static var fileHistoryMerges: Bool {
        get { UserDefaults.standard.bool(forKey: "fileHistoryMerges") }
        set {
            UserDefaults.standard.set(newValue, forKey: "fileHistoryMerges")
            NotificationCenter.default.post(name: .fileHistoryMergesChanged, object: nil)
        }
    }

    /// リポジトリ（手元も SSH 先も）を、共有の daemon 越しに開くか（既定は開かない。このアプリの中で開く）。
    static var openThroughDaemon: Bool {
        get { UserDefaults.standard.bool(forKey: "openThroughDaemon") }
        set { UserDefaults.standard.set(newValue, forKey: "openThroughDaemon") }
    }

    /// 起動時にログインシェルの環境（PATH・SSH_AUTH_SOCK・GIT_* など）を取り込むか（既定は取り込む。
    /// `ShellEnvironment`）。
    static var importShellEnvironment: Bool {
        get { UserDefaults.standard.object(forKey: "importShellEnvironment") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "importShellEnvironment") }
    }

    /// 開いたリポジトリを、まだ一覧のどこにもなければブックマークに入れるか（既定は入れる）。
    static var autoBookmark: Bool {
        get { UserDefaults.standard.object(forKey: "autoBookmark") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "autoBookmark") }
    }

    /// マージエディタを開いたときに、間違えようのないコンフリクトを Magic Merge で解くか（既定は解く）。
    static var autoMagicMerge: Bool {
        get { UserDefaults.standard.object(forKey: "autoMagicMerge") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "autoMagicMerge") }
    }

    /// History の行を 1 行にするか（既定は件名と作者・日時の 2 行）。変えたら `historyRowsChanged` を知らせる。
    static var compactHistoryRows: Bool {
        get { UserDefaults.standard.bool(forKey: "compactHistoryRows") }
        set {
            UserDefaults.standard.set(newValue, forKey: "compactHistoryRows")
            NotificationCenter.default.post(name: .historyRowsChanged, object: nil)
        }
    }

    /// History の行に出す項目（既定はすべて）。変えたら `historyRowsChanged` を知らせる。
    static var historyItems: HistoryItems {
        get {
            // 値がなければすべて（0 は「何も出さない」）。`integer(forKey:)` は引数（`-historyItems 3`）の文字列も数にする。
            let defaults = UserDefaults.standard
            return defaults.object(forKey: "historyItems") == nil ? .all : HistoryItems(rawValue: defaults.integer(forKey: "historyItems"))
        }
        set {
            UserDefaults.standard.set(newValue.rawValue, forKey: "historyItems")
            NotificationCenter.default.post(name: .historyRowsChanged, object: nil)
        }
    }

    /// History の行を実際に 1 行で出すか: 設定で 1 行にしたか、作者・日時の行に出す項目が 1 つもないとき。
    static var oneLineHistoryRows: Bool { compactHistoryRows || !historyItems.hasMeta }

    /// History の並び順（既定は日付順）。変えたら `historyOrderChanged` を知らせる。
    static var historyOrder: HistoryOrder {
        get { UserDefaults.standard.string(forKey: "historyOrder").flatMap(HistoryOrder.init(name:)) ?? .date }
        set {
            UserDefaults.standard.set(newValue.name, forKey: "historyOrder")
            NotificationCenter.default.post(name: .historyOrderChanged, object: nil)
        }
    }
}

extension HistoryOrder {
    /// 設定・メニューで使う名前。
    var name: String {
        switch self {
        case .date: "date"
        case .topo: "topo"
        }
    }

    init?(name: String) {
        guard let order = Self.allCases.first(where: { $0.name == name }) else { return nil }
        self = order
    }

    var title: String {
        switch self {
        case .date: L("Date Order")
        case .topo: L("Topological Order")
        }
    }

    var toolTip: String {
        switch self {
        case .date: L("Newest first wherever the graph allows (like git log --date-order)")
        case .topo: L("Keep a branch’s commits together under the merge that brought them in (like git log --topo-order)")
        }
    }
}

/// History の行に出す項目。件名はいつも出す。
nonisolated struct HistoryItems: OptionSet, Sendable, Hashable {
    let rawValue: Int
    /// ブランチ・タグ（と worktree）のラベル。
    static let refs = HistoryItems(rawValue: 1)
    static let author = HistoryItems(rawValue: 2)
    static let date = HistoryItems(rawValue: 4)
    /// 短いコミット ID。
    static let id = HistoryItems(rawValue: 8)
    /// 署名の検証結果の印。
    static let signature = HistoryItems(rawValue: 16)
    /// GitHub の CI の状態の印。
    static let ci = HistoryItems(rawValue: 32)
    static let all: HistoryItems = [.refs, .author, .date, .id, .signature, .ci]
    /// 作者・日時の行に出すものがあるか。
    var hasMeta: Bool { !isDisjoint(with: [.author, .date, .id]) }
}

extension Notification.Name {
    static let historyRowsChanged = Notification.Name("historyRowsChanged")
    static let historyOrderChanged = Notification.Name("historyOrderChanged")
    static let highlightAncestorsChanged = Notification.Name("highlightAncestorsChanged")
    static let signatureCheckChanged = Notification.Name("signatureCheckChanged")
    static let commitMessageStyleChanged = Notification.Name("commitMessageStyleChanged")
    static let stackedCommitDiffChanged = Notification.Name("stackedCommitDiffChanged")
    static let historyFileTreeChanged = Notification.Name("historyFileTreeChanged")
    static let foldMergedBranchesChanged = Notification.Name("foldMergedBranchesChanged")
    static let fileHistoryMergesChanged = Notification.Name("fileHistoryMergesChanged")
    static let gitHubSettingsChanged = Notification.Name("gitHubSettingsChanged")
    /// AI の CLI の選択が変わった（AI のボタンを出す・隠す）。
    static let assistantChanged = Notification.Name("assistantChanged")
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
    var historyOrder = Settings.historyOrder { didSet { Settings.historyOrder = historyOrder } }
    var historyItems = Settings.historyItems { didSet { Settings.historyItems = historyItems } }
    var highlightAncestors = Settings.highlightAncestors { didSet { Settings.highlightAncestors = highlightAncestors } }
    var monospacedCommitMessage = Settings.monospacedCommitMessage {
        didSet { Settings.monospacedCommitMessage = monospacedCommitMessage }
    }
    var showAvatars = Settings.showAvatars { didSet { Settings.showAvatars = showAvatars } }
    var gitHubCIStatus = Settings.gitHubCIStatus { didSet { Settings.gitHubCIStatus = gitHubCIStatus } }
    var gitHubAvatars = Settings.gitHubAvatars { didSet { Settings.gitHubAvatars = gitHubAvatars } }
    var gitHubCIDefaultBranch = Settings.gitHubCIDefaultBranch { didSet { Settings.gitHubCIDefaultBranch = gitHubCIDefaultBranch } }
    var gitHubCICommits = Settings.gitHubCICommits { didSet { Settings.gitHubCICommits = gitHubCICommits } }
    var gitHubAvatarsOnce = Settings.gitHubAvatarsOnce { didSet { Settings.gitHubAvatarsOnce = gitHubAvatarsOnce } }
    var autoBookmark = Settings.autoBookmark { didSet { Settings.autoBookmark = autoBookmark } }
    var openThroughDaemon = Settings.openThroughDaemon { didSet { Settings.openThroughDaemon = openThroughDaemon } }
    var importShellEnvironment = Settings.importShellEnvironment { didSet { Settings.importShellEnvironment = importShellEnvironment } }
    var stackedCommitDiff = Settings.stackedCommitDiff { didSet { Settings.stackedCommitDiff = stackedCommitDiff } }
    var historyFileTree = Settings.historyFileTree { didSet { Settings.historyFileTree = historyFileTree } }
    var fileHistoryMerges = Settings.fileHistoryMerges { didSet { Settings.fileHistoryMerges = fileHistoryMerges } }
    var autoMagicMerge = Settings.autoMagicMerge { didSet { Settings.autoMagicMerge = autoMagicMerge } }
    var writeCommitGraph = Settings.writeCommitGraph { didSet { Settings.writeCommitGraph = writeCommitGraph } }
    var useFSMonitor = Settings.useFSMonitor { didSet { Settings.useFSMonitor = useFSMonitor } }
    var assistant = Settings.assistant {
        didSet {
            Settings.assistant = assistant
            assistantModel = Settings.assistantModel(assistant)
            NotificationCenter.default.post(name: .assistantChanged, object: nil)
        }
    }
    var assistantModel = Settings.assistantModel(Settings.assistant) {
        didSet { Settings.setAssistantModel(assistantModel, for: assistant) }
    }
    var assistantLanguage = Settings.assistantLanguage { didSet { Settings.assistantLanguage = assistantLanguage } }
    var assistantVersion: String?
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
        if openThroughDaemon != Settings.openThroughDaemon { openThroughDaemon = Settings.openThroughDaemon }
        if gitHubCIStatus != Settings.gitHubCIStatus { gitHubCIStatus = Settings.gitHubCIStatus }
        if gitHubAvatars != Settings.gitHubAvatars { gitHubAvatars = Settings.gitHubAvatars }
        if gitHubCIDefaultBranch != Settings.gitHubCIDefaultBranch { gitHubCIDefaultBranch = Settings.gitHubCIDefaultBranch }
        if gitHubCICommits != Settings.gitHubCICommits { gitHubCICommits = Settings.gitHubCICommits }
        if gitHubAvatarsOnce != Settings.gitHubAvatarsOnce { gitHubAvatarsOnce = Settings.gitHubAvatarsOnce }
        if stackedCommitDiff != Settings.stackedCommitDiff { stackedCommitDiff = Settings.stackedCommitDiff }
        if historyFileTree != Settings.historyFileTree { historyFileTree = Settings.historyFileTree }
        if autoMagicMerge != Settings.autoMagicMerge { autoMagicMerge = Settings.autoMagicMerge }
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
                Picker(L("Default Pull"), selection: $model.pullMode) {
                    ForEach(PullMode.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                Note(L("The choice preselected in the Pull sheet (toolbar and ⌃⌘L). You can change it there each time, or use Repository > Pull With."))
            }
            Section(L("Repository List")) {
                Toggle(L("Bookmark repositories you open"), isOn: $model.autoBookmark)
                Note(L("Repositories already found in folders to scan aren’t added."))
            }
            Section("GitHub") {
                Toggle(L("Show each commit’s CI status in History"), isOn: $model.gitHubCIStatus)
                Toggle(L("Use commit authors’ GitHub avatars"), isOn: $model.gitHubAvatars)
                Note(L("Read with gh on open, on Refresh, and after a fetch or push (at least 30 seconds apart). Each read is about 40 KB for both with 2 branches × 100 commits, 17 KB for CI only, and 3 KB for the current branch only with 30 commits (measured on a medium-sized repository with CI). Nothing is read when both are off."))
                Picker(L("Branches to read CI for"), selection: $model.gitHubCIDefaultBranch) {
                    Text(L("Current and default branch")).tag(true)
                    Text(L("Current branch only")).tag(false)
                }
                .disabled(!model.gitHubCIStatus && !model.gitHubAvatars)
                Picker(L("Commits to read per branch"), selection: $model.gitHubCICommits) {
                    ForEach([100, 50, 30], id: \.self) { Text(L("Last %ld", $0)).tag($0) }
                }
                .disabled(!model.gitHubCIStatus && !model.gitHubAvatars)
                Toggle(L("Read avatar URLs only once per window"), isOn: $model.gitHubAvatarsOnce)
                    .disabled(!model.gitHubAvatars)
                Note(L("After the first read, only the CI status is read (40 KB → 17 KB)."))
            }
            Section(L("Merging")) {
                Toggle(L("Resolve unambiguous conflicts when the merge editor opens"), isOn: $model.autoMagicMerge)
                Note(L("Conflicts where both sides are the same, or only one side changed. ⌘Z undoes it. Use ✨ Magic Merge for the rest."))
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
                Picker(L("Order"), selection: $model.historyOrder) {
                    ForEach(HistoryOrder.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                Note(L("Topological order keeps a branch’s commits together under the merge that brought them in."))
                Toggle(L("Show each commit on one line"), isOn: $model.compactHistoryRows)
                Toggle(L("Highlight the selected commit’s ancestors"), isOn: $model.highlightAncestors)
                Note(L("Draws only the selected commit’s ancestors and the lines between them at full strength, and dims the rest."))
            }
            Section(L("Items")) {
                ForEach(HistoryDisplayMenu.entries, id: \.0.rawValue) { item, title in
                    Toggle(L(title), isOn: Binding(
                        get: { model.historyItems.contains(item) },
                        set: { on in if on { model.historyItems.insert(item) } else { model.historyItems.remove(item) } }))
                }
                Note(L("What each commit’s row shows besides the subject. With none of author, date and ID, rows are one line."))
            }
            Section(L("Commit Details")) {
                Toggle(L("Show the message in a monospaced font"), isOn: $model.monospacedCommitMessage)
                Toggle(L("Show avatars"), isOn: $model.showAvatars)
                Note(L("Fetched from Gravatar (sends a hash of the email address to gravatar.com)."))
                Toggle(L("Stack the diffs of all files"), isOn: $model.stackedCommitDiff)
                Toggle(L("Show changed files as a tree left of the diff"), isOn: $model.historyFileTree)
                Note(L("Shows the diffs of the changed files one after another, like a GitHub pull request. Click a file header to collapse it; Viewed collapses files you’ve seen. Commits with more than 200 changed files show one file at a time."))
            }
            Section("File History") {
                Toggle(L("Show merge commits"), isOn: $model.fileHistoryMerges)
                Note(L("Also lists the merges that changed the file, such as one that resolved a conflict or brought in changes made under an earlier name. Also from the list’s context menu."))
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
                Toggle(L("Write commit-graph automatically"), isOn: $model.writeCommitGraph)
                Note(L("Keeps the commit-graph up to date in the background (writes to .git): runs git commit-graph write --reachable --changed-paths when you open a repository without one, and adds just the new commits to it (--split) when you open a repository or fetch. Loading history and searching with path: get faster."))
                Toggle(L("Speed up status with fsmonitor"), isOn: $model.useFSMonitor)
                Note(L("Uses git’s fsmonitor for the status of repositories on this Mac (a git fsmonitor--daemon runs for each repository and creates a socket in .git)."))
            }
        }
        .formStyle(.grouped)
    }
}

private struct AISettings: View {
    @Bindable var model: SettingsModel
    @State private var customized: Set<String> = []
    /// モデルの「その他…」を選んだ（名前を書く欄を出す）。
    @State private var otherModel = false

    var body: some View {
        Form {
            Section {
                Picker(L("AI to use"), selection: $model.assistant) {
                    ForEach(AssistantCLI.allCases) { Text($0.title).tag($0) }
                }
                if model.assistant == .apple {
                    if let reason = AppleModel.unavailableReason {
                        Note(reason).foregroundStyle(.red)
                    } else {
                        Note(L("Runs on this Mac and sends nothing out. It reads about 4,000 tokens at a time, so long diffs are cut short; for large commits or resolving conflicts, Claude Code or Codex does better."))
                    }
                    TextField(L("Language for translations and explanations"), text: $model.assistantLanguage)
                } else if model.assistant != .none {
                    if let path = model.assistant.path {
                        Note(model.assistantVersion.map { L("%@ (%@)", path, $0) } ?? path)
                    } else {
                        Note(model.assistant == .claude
                             ? L("Not found. Install %@ (%@) and sign in with the CLI.", "Claude Code", "claude")
                             : L("Not found. Install %@ (%@) and sign in with the CLI.", "Codex", "codex"))
                            .foregroundStyle(.red)
                    }
                    // 一覧から選ぶ。一覧にない名前は「その他…」で書く。
                    let models = model.assistant.models
                    let custom = !model.assistantModel.isEmpty && !models.contains { $0.id == model.assistantModel }
                    Picker(L("Model"), selection: Binding(
                        get: { custom || otherModel ? "\u{0}other" : model.assistantModel },
                        set: { value in
                            otherModel = value == "\u{0}other"
                            if !otherModel { model.assistantModel = value }
                        })) {
                        Text(L("CLI default")).tag("")
                        ForEach(models, id: \.id) { Text("\($0.title)  (\($0.id))").tag($0.id) }
                        Divider()
                        Text(L("Other…")).tag("\u{0}other")
                    }
                    if custom || otherModel {
                        TextField(L("Model name"), text: $model.assistantModel, prompt: Text(L("CLI default")))
                    }
                    TextField(L("Language for translations and explanations"), text: $model.assistantLanguage)
                }
            } footer: {
                if model.assistant.sendsOut {
                    Note(L("Runs the local CLI with tools disabled (the app holds no keys; it uses the CLI’s sign-in). For each repository, asks before sending anything the first time."))
                }
            }
            Section(L("Prompts by Purpose")) {
                ForEach(AssistPurpose.allCases) { purpose in
                    HStack {
                        VStack(alignment: .leading) {
                            Text(purpose.title)
                            Note(L("Variables: %@", purpose.variables.map { "{{\($0)}}" }.joined(separator: " ")))
                        }
                        Spacer()
                        if customized.contains(purpose.rawValue) { Text(L("Customized")).font(.caption).foregroundStyle(.orange) }
                        Button(L("Edit…")) {
                            if let file = try? Assistant.editablePromptFile(purpose) { NSWorkspace.shared.open(file) }
                            refresh()
                        }
                        Button(L("Restore Default")) {
                            Assistant.resetPrompt(purpose)
                            refresh()
                        }
                        .disabled(!customized.contains(purpose.rawValue))
                        Button(L("Try")) {
                            // 見本の値で、今のプロンプト（書き換えていればそれ）を送る。キャッシュは使わない。
                            AssistantPanel.show(title: L("%@ (Try with Sample)", purpose.title), over: NSApp.keyWindow) { _, onText in
                                try await Assistant.run(
                                    purpose, variables: purpose.sampleVariables, workdir: nil, useCache: false, onText: onText)
                            }
                        }
                        .disabled(!model.assistant.isAvailable)
                        .help(L("Send the current prompt with a sample diff and message, and see the reply."))
                    }
                }
                Note(L("Stored in ~/Library/Application Support/Amon Hen/prompts/<purpose>.md. A repository’s .amonhen/prompts/<purpose>.md takes precedence (you’re asked to check it the first time). The app appends the reply format (the JSON for resolving conflicts)."))
            }
            Section {
                Button(L("Forget Per-Repository Permissions")) { Assistant.forgetRepositoryDecisions() }
                Note(L("Forgets the answers to “Use AI in this repository?” and asks again next time."))
            }
        }
        .formStyle(.grouped)
        .onAppear { refresh() }
        .task(id: model.assistant) {
            otherModel = false
            model.assistantVersion = await Assistant.version(model.assistant)
        }
    }

    private func refresh() {
        customized = Set(AssistPurpose.allCases.filter(Assistant.isCustomized).map(\.rawValue))
    }
}

private struct AdvancedSettings: View {
    @Bindable var model: SettingsModel
    var body: some View {
        Form {
            Section(L("Shared daemon")) {
                Toggle(L("Open repositories through the shared daemon (SSH hosts too)"), isOn: $model.openThroughDaemon)
                Note(L("Shares History, the search index, the operation log and SSH connections among the app, the CLI (amonhen -C daemon:… or -C ssh://…) and the upcoming VSCode extension. Applies to repositories opened from now on. You can also open a single repository through the daemon by right-clicking it in the repository list. See Daemon in Window > Activity for its status."))
            }
            Section(L("Repositories on SSH Hosts")) {
                Toggle(L("Verify signatures with this Mac’s keys"), isOn: $model.checkRemoteSignaturesLocally)
                Note(L("Verifies with this Mac’s gpg, allowed_signers and trust levels. When off, verifies with the git and keys on the SSH host. Repositories on this Mac are always verified on this Mac."))
            }
            Section(L("Terminal Environment")) {
                Toggle(L("Take the login shell’s environment at launch"), isOn: $model.importShellEnvironment)
                Note(L("Launched from the Finder, the app does not get the terminal’s PATH, SSH agent or GIT_* settings, so git could miss Homebrew’s git, credential helpers and agents such as 1Password. The app runs your login shell once at launch and gives git, the shared daemon and gh what it found. Changes apply at the next launch."))
                ShellEnvironmentView()
            }
        }
        .formStyle(.grouped)
    }
}

/// 取り込んだ変数とその値。読み直すボタン付き（シェルの設定を変えたあとに）。
private struct ShellEnvironmentView: View {
    @State private var values = ShellEnvironment.values
    @State private var shell = ShellEnvironment.shell
    @State private var lastRead = ShellEnvironment.lastRead
    @State private var reloading = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(status).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button(L("Read Again")) {
                    reloading = true
                    ShellEnvironment.reload()
                }
                .disabled(reloading || !ShellEnvironment.isEnabled)
            }
            if values.isEmpty {
                Text(L("Nothing taken from the shell.")).font(.caption).foregroundStyle(.secondary)
            } else {
                ForEach(values.keys.sorted(), id: \.self) { key in
                    VStack(alignment: .leading, spacing: 1) {
                        Text(key).font(.caption.weight(.medium))
                        // PATH は 1 行に 1 つ（どの git が選ばれるかは、この順で決まる）。
                        Text(key == "PATH" ? values[key, default: ""].replacingOccurrences(of: ":", with: "\n") : values[key, default: ""])
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: ShellEnvironment.didChange)) { _ in
            values = ShellEnvironment.values
            shell = ShellEnvironment.shell
            lastRead = ShellEnvironment.lastRead
            reloading = false
        }
    }

    private var status: String {
        if !ShellEnvironment.isEnabled { return L("Off") }
        guard let lastRead else { return shell.isEmpty ? L("Not read yet") : L("Couldn’t read %@", shell) }
        return L("Read from %@ at %@", shell, lastRead.formatted(date: .omitted, time: .shortened))
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
        add(L("Appearance"), "paintbrush", AppearanceSettings(model: model))
        add(L("Performance"), "speedometer", PerformanceSettings(model: model))
        add("AI", "sparkles", AISettings(model: model))
        add(L("Advanced"), "gearshape.2", AdvancedSettings(model: model))
        let window = NSWindow(contentViewController: tabs)
        window.styleMask = [.titled, .closable]
        window.title = L("Settings")
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
