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
        guard let actions = selectedRepository?.actions else { return }
        actions.perform(sequence: Array(repeating: ["op": "undo"], count: index + 1))
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in self?.loadUndoLog() }
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
                    Text("すべて").tag("all")
                    Text("git").tag("git")
                    Text("コマンド").tag("command")
                }
                .pickerStyle(.segmented).frame(width: 220)
                Toggle("失敗だけ", isOn: $failuresOnly)
                TextField("絞り込む", text: $filter)
                Button("JSON に書き出す…") { export() }
            }
            Table(shown, selection: $selection) {
                TableColumn("時刻") { r in
                    Text(timeFormatter.string(from: Date(timeIntervalSince1970: Double(r.started) / 1000))).monospacedDigit()
                }
                .width(90)
                TableColumn("") { r in
                    Text(r.ok == nil ? "●" : r.ok! ? "✓" : "✗")
                        .foregroundStyle(r.ok == nil ? .orange : r.ok! ? .green : .red)
                }
                .width(16)
                TableColumn("リポジトリ") { r in
                    Text(r.remote.map { "\($0)（daemon）" } ?? repoName(r.repo))
                }
                .width(min: 60, ideal: 110)
                TableColumn("内容") { r in
                    HStack {
                        Text(r.label).lineLimit(1).truncationMode(.middle).help(r.label)
                        if r.ok == nil, let progress = r.progress {
                            Text(progress).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                        if r.ok == nil, r.cancellable {
                            Spacer()
                            Button("取り消す") { model.cancel(r) }.controlSize(.small)
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
            Text("git の起動とコアの命令を、新しい順に最大 1000 件（daemon 越しのリポジトリは daemon で起動した git も）。● は実行中で、fetch・pull・push・clone は進み具合を出し、取り消せる。")
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
            Picker("リポジトリ", selection: $model.selectedRepository) {
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
                        Button(i == 0 ? "Undo" : "ここまで戻す") { model.undo(through: i) }
                            .help(i == 0 ? "この操作を戻す" : "この操作を含め、これより新しい \(i + 1) 個の操作を順に戻す")
                    }
                }
            }
            Text("Undo できる操作（新しい順）。ほかのツールで ref が動いていると、そこで止まります。").font(.caption).foregroundStyle(.secondary)
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
                TableColumn("項目", value: \.id)
                TableColumn("回数") { Text("\($0.count)").monospacedDigit() }.width(50)
                TableColumn("p50 ms") { Text(ms($0.p50)).monospacedDigit() }.width(60)
                TableColumn("p95 ms") { r in Text(ms(r.p95)).monospacedDigit().foregroundStyle(r.p95 > 16.7 ? .orange : .primary) }.width(60)
                TableColumn("最大 ms") { Text(ms($0.max)).monospacedDigit() }.width(60)
                TableColumn("直近 ms") { Text(ms($0.last)).monospacedDigit() }.width(60)
            }
            Text("アプリを起動してからの直近 500 回ずつ。p95 が 1 フレームの倍（16.7 ms）を超えたら橙色。").font(.caption).foregroundStyle(.secondary)
        }
        .padding()
    }
}

private struct EventsTab: View {
    let model: ActivityModel
    var body: some View {
        VStack(alignment: .leading) {
            Table(model.events.reversed()) {
                TableColumn("時刻") { Text(timeFormatter.string(from: $0.time)).monospacedDigit() }.width(90)
                TableColumn("リポジトリ") { Text($0.repo) }.width(min: 60, ideal: 120)
                TableColumn("内容") { Text($0.text) }
            }
            Text("ファイルの変化の知らせと、それによる読み直し（新しい順に最大 500 件）。").font(.caption).foregroundStyle(.secondary)
        }
        .padding()
    }
}

/// 手元の共有の daemon の様子と、このアプリの daemon とのつながり。
private struct DaemonTab: View {
    let model: ActivityModel

    private func duration(_ ms: UInt64) -> String {
        let s = ms / 1000
        if s < 60 { return "\(s) 秒" }
        if s < 3600 { return s % 60 == 0 ? "\(s / 60) 分" : "\(s / 60) 分 \(s % 60) 秒" }
        return "\(s / 3600) 時間 \(s % 3600 / 60) 分"
    }

    private func bytes(_ n: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(n), countStyle: .file)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top) {
                if let d = model.daemon {
                    VStack(alignment: .leading, spacing: 2) {
                        Label("共有の daemon は動いています", systemImage: "circle.fill").foregroundStyle(.green).font(.headline)
                        Text("pid \(String(d.pid)) · 版 \(d.version)（通信の形 \(d.protocol)） · 動いて \(duration(d.uptimeMs)) · つながり \(d.connections)")
                            .font(.caption).monospacedDigit()
                        Text(d.socket).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                } else {
                    VStack(alignment: .leading, spacing: 2) {
                        Label(model.daemonChecked ? "共有の daemon は動いていません" : "調べています…", systemImage: "circle")
                            .foregroundStyle(.secondary).font(.headline)
                        Text("daemon 越しにリポジトリを開くと（リポジトリの一覧の右クリック、または Settings > Advanced）、自動で起動します。")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                Spacer()
                if model.daemon == nil {
                    Button("起動") { model.startDaemon() }
                } else {
                    Button("止める") { model.stopDaemon() }
                        .help("daemon 越しに開いているリポジトリは、つながりが切れる（「つなぎ直す」でまた起動する）")
                }
            }
            Text("開いているリポジトリ").font(.subheadline.bold())
            Table(model.daemon?.repos ?? []) {
                TableColumn("リポジトリ") { r in Text(r.path).lineLimit(1).truncationMode(.head).help(r.path) }
                TableColumn("つながり") { r in
                    Text(r.clients > 0 ? "\(r.clients)" : "なし（\(duration(r.idleMs))前から）").monospacedDigit()
                }
                .width(min: 60, ideal: 130)
                TableColumn("History") { r in Text(r.history ? "あり" : "—") }.width(60)
                TableColumn("監視") { r in Text(r.watching ? "あり" : "—") }.width(40)
                TableColumn("検索の索引") { r in
                    Text(r.indexRows.map { "\($0) 行・\(bytes(UInt64(r.indexBytes ?? 0)))" } ?? "—").monospacedDigit()
                }
                .width(min: 80, ideal: 150)
            }
            .frame(minHeight: 110)
            Text("このアプリの daemon とのつながり").font(.subheadline.bold())
            let remotes = model.repositories.filter(\.isRemote)
            Table(remotes) {
                TableColumn("リポジトリ") { r in Text(r.name) }
                TableColumn("場所") { r in
                    Text(r.location.hasPrefix("ssh://") ? "SSH 先" : "手元の共有の daemon").foregroundStyle(.secondary)
                }
                .width(min: 80, ideal: 130)
                TableColumn("状態") { r in
                    switch r.repository.connectionState {
                    case .closed(let reason): Text("切れた").foregroundStyle(.red).help(reason)
                    default: Text("つながっている").foregroundStyle(.green)
                    }
                }
                .width(90)
                TableColumn("送った・受け取った") { r in
                    let t = r.repository.traffic
                    Text(t.map { "\(bytes($0.sent)) · \(bytes($0.received))" } ?? "—").monospacedDigit()
                }
                .width(min: 100, ideal: 150)
            }
            .frame(minHeight: 80)
            Text("daemon はだれもつながらなくなって \(duration(model.daemon?.idleExitMs ?? 600_000))たつと終わる。端末からは amonhen daemon status で見られる。")
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
            OperationsTab(model: model).tabItem { Text("Operations") }
            HistoryTab(model: model).tabItem { Text("History") }
            PerformanceTab(model: model).tabItem { Text("Performance") }
            EventsTab(model: model).tabItem { Text("Events") }
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
