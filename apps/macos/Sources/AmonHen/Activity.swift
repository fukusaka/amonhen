import AmonhenKit
import AppKit
import SwiftUI

/// 画面の速さの記録（入力から表示まで、選んでから差分まで、など）。項目ごとに直近の値を残す。いつも取る
/// （配列に足すだけ）。Activity の Performance に出す。
enum PerformanceLog {
    private static let keep = 500
    private(set) static var samples: [String: [Double]] = [:]

    static func record(_ metric: String, ms: Double) {
        var values = samples[metric, default: []]
        values.append(ms)
        if values.count > keep { values.removeFirst(values.count - keep) }
        samples[metric] = values
    }
}

/// 内部の出来事（ファイルの変化の知らせ、読み直し）。Activity の Events に出す。
enum EventLog {
    struct Event: Identifiable, Hashable {
        let id: Int
        let time: Date
        let repo: String
        let text: String
    }

    private static let keep = 500
    private static var next = 0
    private(set) static var events: [Event] = []

    static func add(repo: String, _ text: String) {
        next += 1
        events.append(Event(id: next, time: Date(), repo: repo, text: text))
        if events.count > keep { events.removeFirst(events.count - keep) }
    }
}

/// 操作を実行している数（ツールバーの Activity に出す）。
enum RunningOperations {
    private(set) static var count = 0 {
        didSet { NotificationCenter.default.post(name: .runningOperationsChanged, object: nil) }
    }

    static func begin() { count += 1 }
    static func end() { count = max(0, count - 1) }
}

extension Notification.Name {
    static let runningOperationsChanged = Notification.Name("runningOperationsChanged")
}

/// Activity の画面が読む、開いているリポジトリ（操作ログを読み、戻すため）。
struct ActivityRepository: Identifiable, Hashable {
    let name: String
    let repository: Repository
    let actions: Actions
    /// daemon 越しのリポジトリか（SSH 先・手元の共有の daemon。daemon の記録も読む）。
    var isRemote = false
    /// 開いた場所（手元のパス、`daemon:/path`、`ssh://host/path`）。
    var location = ""
    var id: ObjectIdentifier { ObjectIdentifier(repository) }
    static func == (a: Self, b: Self) -> Bool { a.id == b.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
}

/// 操作ログ（`undoLog` の `info`）。
nonisolated struct UndoLogResponse: Decodable, Sendable {
    struct Entry: Decodable, Sendable { let label: String; let time: Double }
    let entries: [Entry]
}

/// Activity の画面の中身。開いている間だけ、0.5 秒ごとに読み直す。
@Observable
final class ActivityModel {
    var records: [ActivityRecord] = []
    var events: [EventLog.Event] = []
    var metrics: [(name: String, values: [Double])] = []
    var repositories: [ActivityRepository] = []
    var undoEntries: [(label: String, time: Date)] = []
    var selectedRepository: ActivityRepository?
    /// AppDelegate が設定する（開いているリポジトリ）。
    @ObservationIgnored var listRepositories: () -> [ActivityRepository] = { [] }
    /// daemon の記録（リポジトリの名前 → 記録）。2 秒ごとに読む。
    @ObservationIgnored private var remoteRecords: [String: [ActivityRecord]] = [:]
    @ObservationIgnored private var remoteLoadedAt = Date.distantPast
    @ObservationIgnored private var loadingRemote = false

    /// 手元の共有の daemon の様子（動いていなければ nil）。2 秒ごとに読む。
    var daemon: SharedDaemon.Status?
    /// daemon を読んだか（まだなら「調べています」と出す）。
    var daemonChecked = false
    @ObservationIgnored private var daemonLoadedAt = Date.distantPast
    @ObservationIgnored private var loadingDaemon = false

    func refresh() {
        let remote = remoteRecords.values.flatMap { $0 }
        records = (ActivityRecord.all() + remote).sorted { $0.started < $1.started }
        loadRemoteRecords()
        loadDaemon()
        events = EventLog.events
        metrics = PerformanceLog.samples.map { ($0.key, $0.value) }.sorted { $0.name < $1.name }
        repositories = listRepositories()
        if let selected = selectedRepository, !repositories.contains(selected) { selectedRepository = nil }
        if selectedRepository == nil { selectedRepository = repositories.first }
    }

    private func loadRemoteRecords() {
        guard !loadingRemote, Date().timeIntervalSince(remoteLoadedAt) > 2 else { return }
        let remotes = repositories.filter(\.isRemote)
        guard !remotes.isEmpty else {
            remoteRecords = [:]
            return
        }
        loadingRemote = true
        Task {
            var loaded: [String: [ActivityRecord]] = [:]
            for entry in remotes {
                let repository = entry.repository
                struct Records: Decodable { let records: [ActivityRecord] }
                if let records = try? await Task.detached(priority: .utility, operation: {
                    try repository.info(["op": "remoteActivity"], as: Records.self).records
                }).value {
                    loaded[entry.name] = records.map { var r = $0; r.remote = entry.name; return r }
                }
            }
            remoteRecords = loaded
            remoteLoadedAt = Date()
            loadingRemote = false
        }
    }

    func loadDaemon(force: Bool = false) {
        guard !loadingDaemon, force || Date().timeIntervalSince(daemonLoadedAt) > 2 else { return }
        loadingDaemon = true
        Task {
            let status = await Task.detached(priority: .utility) { SharedDaemon.status() }.value
            daemon = status
            daemonChecked = true
            daemonLoadedAt = Date()
            loadingDaemon = false
        }
    }

    func startDaemon() {
        let path = Daemon.path
        Task {
            do {
                try await Task.detached { try SharedDaemon.start(daemon: path) }.value
            } catch {
                NSAlert(error: error).runModal()
            }
            loadDaemon(force: true)
        }
    }

    func stopDaemon() {
        Task {
            await Task.detached { SharedDaemon.stop() }.value
            loadDaemon(force: true)
        }
    }

    /// 走っている操作を取り消す（daemon の記録なら daemon に頼む）。
    func cancel(_ record: ActivityRecord) {
        guard let name = record.remote else {
            ActivityRecord.cancel(seq: record.seq)
            return
        }
        guard let repository = repositories.first(where: { $0.name == name })?.repository else { return }
        let seq = record.seq
        Task.detached { _ = try? repository.run(["op": "cancelActivity", "seq": seq, "remote": true]) }
    }

    func loadUndoLog() {
        guard let repository = selectedRepository?.repository else {
            undoEntries = []
            return
        }
        Task {
            let log = try? await Task.detached { try repository.info(["op": "undoLog"], as: UndoLogResponse.self) }.value
            undoEntries = (log?.entries ?? []).map { ($0.label, Date(timeIntervalSince1970: $0.time / 1000)) }
        }
    }

    /// 操作ログの `index` 番目（0 が一番新しい）まで戻す（新しいものから順に Undo する）。
    func undo(through index: Int) {
        guard let actions = selectedRepository?.actions, index < undoEntries.count else { return }
        let title = index == 0 ? L("Undo “%@”?", undoEntries[0].0) : L("Undo the last %ld operations?", index + 1)
        Task { @MainActor in
            guard await actions.confirm(title, message: L("Returns HEAD and the branches to where they were."), ok: "Undo",
                                        in: ActivityWindowController.shared.window) else { return }
            actions.perform(sequence: Array(repeating: ["op": "undo"], count: index + 1))
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in self?.loadUndoLog() }
        }
    }
}

private let timeFormatter: DateFormatter = {
    let f = DateFormatter()
    f.dateFormat = "HH:mm:ss.SSS"
    return f
}()

private func repoName(_ path: String) -> String { path.isEmpty ? "" : (path as NSString).lastPathComponent }

private func ms(_ value: Double) -> String { value < 10 ? String(format: "%.2f", value) : String(format: "%.0f", value) }

private struct OperationsTab: View {
    let model: ActivityModel
    @State private var kind = "all"
    @State private var failuresOnly = false
    @State private var filter = ""
    @State private var selection: String?

    var body: some View {
        let shown = model.records.reversed().filter {
            (kind == "all" || $0.kind == kind) && (!failuresOnly || $0.ok == false)
                && (filter.isEmpty || $0.label.localizedCaseInsensitiveContains(filter)
                    || $0.repo.localizedCaseInsensitiveContains(filter))
        }
        VStack(alignment: .leading) {
            HStack {
                Picker("", selection: $kind) {
                    Text(L("All")).tag("all")
                    Text("git").tag("git")
                    Text(L("Commands")).tag("command")
                }
                .pickerStyle(.segmented).frame(width: 220)
                Toggle(L("Failures Only"), isOn: $failuresOnly)
                TextField(L("Filter"), text: $filter)
                Button(L("Export JSON…")) { export() }
            }
            Table(shown, selection: $selection) {
                TableColumn(L("Time")) { r in
                    Text(timeFormatter.string(from: Date(timeIntervalSince1970: Double(r.started) / 1000))).monospacedDigit()
                }
                .width(90)
                TableColumn("") { r in
                    Text(r.ok == nil ? "●" : r.ok! ? "✓" : "✗")
                        .foregroundStyle(r.ok == nil ? .orange : r.ok! ? .green : .red)
                }
                .width(16)
                TableColumn(L("Repository")) { r in
                    Text(r.remote.map { L("%@ (daemon)", $0) } ?? repoName(r.repo))
                }
                .width(min: 60, ideal: 110)
                TableColumn(L("Content")) { r in
                    HStack {
                        Text(r.label).lineLimit(1).truncationMode(.middle).help(r.label)
                        if r.ok == nil, let progress = r.progress {
                            Text(progress).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                        if r.ok == nil, r.cancellable {
                            Spacer()
                            Button(L("Cancel Operation")) { model.cancel(r) }.controlSize(.small)
                        }
                    }
                }
                TableColumn("ms") { r in Text(r.duration.map(ms) ?? "…").monospacedDigit() }.width(60)
            }
            if let selection, let record = model.records.first(where: { $0.id == selection }) {
                ScrollView {
                    Text("\(record.repo)\n\(record.label)" + (record.error.map { "\n\n\($0)" } ?? ""))
                        .font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(height: 90)
            }
            Text(L("git runs and core commands, newest first, up to 1,000 (for repositories through the daemon, also the git it starts). ● means running; fetch, pull, push and clone show their progress and can be canceled."))
                .font(.caption).foregroundStyle(.secondary)
        }
        .padding()
    }

    private func export() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "amonhen-activity.json"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        guard let out = amonhenActivityJSON() else { return }
        try? out.write(to: url, atomically: true, encoding: .utf8)
    }
}

/// 記録をそのまま JSON で（書き出し用）。
private func amonhenActivityJSON() -> String? {
    let records = ActivityRecord.all()
    struct Out: Encodable {
        let seq: UInt64, kind: String, repo: String, label: String, started: UInt64
        let duration: Double?, ok: Bool?, error: String?
    }
    let out = records.map {
        Out(seq: $0.seq, kind: $0.kind, repo: $0.repo, label: $0.label, started: $0.started,
            duration: $0.duration, ok: $0.ok, error: $0.error)
    }
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted]
    return (try? encoder.encode(out)).map { String(decoding: $0, as: UTF8.self) }
}

private struct HistoryTab: View {
    @Bindable var model: ActivityModel

    var body: some View {
        VStack(alignment: .leading) {
            Picker(L("Repository"), selection: $model.selectedRepository) {
                ForEach(model.repositories) { Text($0.name).tag(Optional($0)) }
            }
            .frame(width: 320)
            .onChange(of: model.selectedRepository) { model.loadUndoLog() }
            List {
                ForEach(Array(model.undoEntries.enumerated()), id: \.offset) { i, entry in
                    HStack {
                        Text(timeFormatter.string(from: entry.time)).monospacedDigit().foregroundStyle(.secondary)
                        Text(entry.label)
                        Spacer()
                        Button(i == 0 ? "Undo" : L("Undo to Here")) { model.undo(through: i) }
                            .help(i == 0 ? L("Undo this operation.") : L("Undo the %ld latest operations, down to and including this one.", i + 1))
                    }
                }
            }
            Text(L("Operations you can Undo (newest first). It stops where another tool has moved a ref.")).font(.caption).foregroundStyle(.secondary)
        }
        .padding()
        .onAppear { model.loadUndoLog() }
    }
}

private struct PerformanceTab: View {
    let model: ActivityModel

    private struct Row: Identifiable {
        let id: String
        let count: Int, p50: Double, p95: Double, max: Double, last: Double
    }

    var body: some View {
        let rows = model.metrics.map { m -> Row in
            let sorted = m.values.sorted()
            let q = { (p: Double) in sorted[min(sorted.count - 1, Int(Double(sorted.count) * p))] }
            return Row(id: m.name, count: sorted.count, p50: q(0.5), p95: q(0.95), max: sorted.last ?? 0, last: m.values.last ?? 0)
        }
        VStack(alignment: .leading) {
            Table(rows) {
                TableColumn(L("Item"), value: \.id)
                TableColumn(L("Count")) { Text("\($0.count)").monospacedDigit() }.width(50)
                TableColumn("p50 ms") { Text(ms($0.p50)).monospacedDigit() }.width(60)
                TableColumn("p95 ms") { r in Text(ms(r.p95)).monospacedDigit().foregroundStyle(r.p95 > 16.7 ? .orange : .primary) }.width(60)
                TableColumn(L("Max ms")) { Text(ms($0.max)).monospacedDigit() }.width(60)
                TableColumn(L("Last ms")) { Text(ms($0.last)).monospacedDigit() }.width(60)
            }
            Text(L("The last 500 of each since the app started. Orange when p95 exceeds twice a frame (16.7 ms).")).font(.caption).foregroundStyle(.secondary)
        }
        .padding()
    }
}

private struct EventsTab: View {
    let model: ActivityModel
    var body: some View {
        VStack(alignment: .leading) {
            Table(model.events.reversed()) {
                TableColumn(L("Time")) { Text(timeFormatter.string(from: $0.time)).monospacedDigit() }.width(90)
                TableColumn(L("Repository")) { Text($0.repo) }.width(min: 60, ideal: 120)
                TableColumn(L("Content")) { Text($0.text) }
            }
            Text(L("File change notices and the reloads they cause (newest first, up to 500).")).font(.caption).foregroundStyle(.secondary)
        }
        .padding()
    }
}

/// 手元の共有の daemon の様子と、このアプリの daemon とのつながり。
private struct DaemonTab: View {
    let model: ActivityModel

    private func duration(_ ms: UInt64) -> String {
        let s = ms / 1000
        if s < 60 { return L("%ld s", Int(s)) }
        if s < 3600 { return s % 60 == 0 ? L("%ld min", Int(s / 60)) : L("%ld min %ld s", Int(s / 60), Int(s % 60)) }
        return L("%ld h %ld min", Int(s / 3600), Int(s % 3600 / 60))
    }

    private func bytes(_ n: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(n), countStyle: .file)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top) {
                if let d = model.daemon {
                    VStack(alignment: .leading, spacing: 2) {
                        Label(L("The shared daemon is running"), systemImage: "circle.fill").foregroundStyle(.green).font(.headline)
                        Text(L("pid %@ · version %@ (build %@) · up %@ · connections %ld", String(d.pid), "\(d.version)", d.build, duration(d.uptimeMs), d.connections))
                            .font(.caption).monospacedDigit()
                        Text(d.socket).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                } else {
                    VStack(alignment: .leading, spacing: 2) {
                        Label(model.daemonChecked ? L("The shared daemon isn’t running") : L("Checking…"), systemImage: "circle")
                            .foregroundStyle(.secondary).font(.headline)
                        Text(L("It starts automatically when you open a repository through the daemon (right-click in the repository list, or Settings > Performance)."))
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                Spacer()
                if model.daemon == nil {
                    Button(L("Start")) { model.startDaemon() }
                } else {
                    Button(L("Stop")) { model.stopDaemon() }
                        .help(L("Repositories opened through the daemon get disconnected (Reconnect starts it again)."))
                }
            }
            Text(L("Open Repositories")).font(.subheadline.bold())
            Table(model.daemon?.repos ?? []) {
                TableColumn(L("Repository")) { r in Text(r.path).lineLimit(1).truncationMode(.head).help(r.path) }
                TableColumn("SSH") { r in
                    if r.ssh == true {
                        if let reason = r.closed {
                            Text(L("Disconnected")).foregroundStyle(.red).help(reason)
                        } else {
                            Text("\(bytes(r.sent ?? 0)) · \(bytes(r.received ?? 0))").monospacedDigit()
                                .help(L("Sent to and received from the SSH host") + (r.engine.map { "\n" + L("Engine on the SSH host: %@", $0) } ?? ""))
                        }
                    } else {
                        Text("—").foregroundStyle(.secondary)
                    }
                }
                .width(min: 40, ideal: 120)
                TableColumn(L("Connections")) { r in
                    Text(r.clients > 0 ? "\(r.clients)" : L("None (for %@)", duration(r.idleMs))).monospacedDigit()
                }
                .width(min: 60, ideal: 130)
                TableColumn("History") { r in Text(r.history ? L("Yes") : "—") }.width(60)
                TableColumn(L("Watching")) { r in Text(r.watching ? L("Yes") : "—") }.width(40)
                TableColumn(L("Search Index")) { r in
                    Text(r.indexRows.map { L("%ld rows · %@", $0, bytes(UInt64(r.indexBytes ?? 0))) } ?? "—").monospacedDigit()
                }
                .width(min: 80, ideal: 150)
            }
            .frame(minHeight: 110)
            Text(L("This App’s Connections to the Daemon")).font(.subheadline.bold())
            let remotes = model.repositories.filter(\.isRemote)
            Table(remotes) {
                TableColumn(L("Repository")) { r in Text(r.name) }
                TableColumn(L("Location")) { r in
                    Text(r.location.hasPrefix("ssh://") ? L("SSH host (direct)")
                        : r.location.hasPrefix("daemon:ssh://") ? L("Shared daemon → SSH host") : L("Local shared daemon"))
                        .foregroundStyle(.secondary)
                }
                .width(min: 80, ideal: 130)
                TableColumn(L("Status")) { r in
                    switch r.repository.connectionState {
                    case .closed(let reason): Text(L("Disconnected")).foregroundStyle(.red).help(reason)
                    default: Text(L("Connected")).foregroundStyle(.green)
                    }
                }
                .width(90)
                TableColumn(L("Sent · Received")) { r in
                    let t = r.repository.traffic
                    Text(t.map { "\(bytes($0.sent)) · \(bytes($0.received))" } ?? "—").monospacedDigit()
                }
                .width(min: 100, ideal: 150)
                TableColumn(L("Engine on the SSH Host")) { r in
                    let engine = r.repository.remoteEngine
                    Text(engine ?? "—").lineLimit(1).truncationMode(.head).foregroundStyle(.secondary).help(engine ?? "")
                }
                .width(min: 80, ideal: 220)
            }
            .frame(minHeight: 80)
            Text(L("The daemon exits after %@ with no connections. From a terminal, run amonhen daemon status to see it.", duration(model.daemon?.idleExitMs ?? 600_000)))
                .font(.caption).foregroundStyle(.secondary)
        }
        .padding()
        .onAppear { model.loadDaemon(force: true) }
    }
}

private struct ActivityView: View {
    let model: ActivityModel
    var body: some View {
        TabView {
            OperationsTab(model: model).tabItem { Text(L("Operations")) }
            HistoryTab(model: model).tabItem { Text("History") }
            PerformanceTab(model: model).tabItem { Text(L("Performance")) }
            EventsTab(model: model).tabItem { Text(L("Events")) }
            DaemonTab(model: model).tabItem { Text("Daemon") }
        }
        .padding(.top, 8)
        .frame(minWidth: 760, minHeight: 460)
    }
}

/// Activity（Window > Activity、⌥⌘A）: 実行した git とコマンド、操作ログ、画面の速さ、内部の出来事。
final class ActivityWindowController: NSWindowController, NSWindowDelegate {
    static let shared = ActivityWindowController()
    let model = ActivityModel()
    private var timer: Timer?

    private init() {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 900, height: 560),
            styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "Activity"
        window.setFrameAutosaveName("Activity")
        window.contentViewController = NSHostingController(rootView: ActivityView(model: model))
        super.init(window: window)
        window.delegate = self
    }

    required init?(coder: NSCoder) { fatalError() }

    func show() {
        model.refresh()
        if window?.isVisible != true { window?.center() }
        showWindow(nil)
        // 開いている間だけ読み直す（閉じていれば何もしない）。
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.model.refresh() }
        }
    }

    func windowWillClose(_ notification: Notification) {
        timer?.invalidate()
        timer = nil
    }
}
