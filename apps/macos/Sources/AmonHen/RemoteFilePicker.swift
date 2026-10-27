import AmonhenKit
import AppKit
import SwiftUI

/// git を動かす機械（SSH 先）のファイルやフォルダを選ぶシート。Finder のカラム表示に近い形: 上にパスの欄（打ち込める。
/// `~` はその機械のホーム）、下に `/` から選んだところまでの列。中身はその機械のエンジンが読む（コアの `listDirectory`）。
/// 呼ぶ所が、実行できるファイルを選ぶか、フォルダを選ぶかを決める（選べないものは薄く出す）。
enum RemoteFilePicker {
    enum Kind {
        case executable
        case folder
    }

    /// 選んだもの: SSH 先と、そこでの絶対パス。
    struct Chosen {
        let host: String
        let path: String
        /// ホームの下なら `~/…`（Places と同じ書き方）。
        let shown: String
    }

    /// `window`（なければ前のウィンドウ）にシートとして出し、選んだものを `done` に渡す。取り消したら nil。
    /// 最初は `initial`（`~` 可）のフォルダ、ファイルならそれを選んだところ（空ならホーム）。`repository` がなければ
    /// `host` のエンジンにつなぐ（数秒かかる）。`hosts` を渡すと、SSH 先も選べる（空の `host` から始めてよい）。
    static func present(title: String, message: String, kind: Kind, host: String, repository: Repository? = nil,
                        hosts: [String]? = nil, initial: String, in window: NSWindow? = nil,
                        done: @escaping (Chosen?) -> Void) {
        let model = RemoteFilePickerModel(kind: kind, hosts: hosts, initial: initial)
        let view = RemoteFilePickerView(model: model, title: title, message: message)
        let sheet = NSWindow(contentViewController: NSHostingController(rootView: view))
        sheet.styleMask = [.titled, .resizable]
        sheet.setContentSize(NSSize(width: 760, height: hosts == nil ? 460 : 490))
        sheet.contentMinSize = NSSize(width: 520, height: 320)
        let parent = window ?? NSApp.keyWindow
        model.close = { [weak parent, weak sheet] chosen in
            guard let sheet else { return }
            if let parent { parent.endSheet(sheet) } else { sheet.close() }
            done(chosen)
        }
        if let parent {
            parent.beginSheet(sheet)
        } else {
            sheet.center()
            sheet.makeKeyAndOrderFront(nil)
        }
        if let repository {
            model.use(repository, host: host)
        } else {
            model.connect(host)
        }
    }
}

/// フォルダの中身（コアの `listDirectory` の `info`）。
nonisolated struct RemoteDirectory: Decodable, Sendable {
    nonisolated struct Entry: Decodable, Sendable, Hashable {
        let name: String
        /// `folder`・`file`・`other`（シンボリックリンクなら、その先の種類）。
        let kind: String
        let link: Bool
        let executable: Bool
        /// `.git` のあるフォルダか。
        let repository: Bool
        var isFolder: Bool { kind == "folder" }
    }

    let path: String
    let home: String?
    let entries: [Entry]
}

@Observable @MainActor
final class RemoteFilePickerModel {
    /// 1 つの列: フォルダと、その中で選んでいるもの。
    struct Column: Identifiable {
        let path: String
        var entries: [RemoteDirectory.Entry]?
        var error: String?
        var selected: String?
        var id: String { path }
    }

    let kind: RemoteFilePicker.Kind
    /// 選べる SSH 先（SSH 先を変えられないなら nil）。
    let hosts: [String]?
    /// 最初に出すところ（つながったら行く）。
    private let initial: String
    /// 今の SSH 先と、そのエンジンにつないだリポジトリ。
    var host = ""
    private var repository: Repository?
    var connecting = false
    /// つながらなかったときの理由（パスを打ち込んで決められる）。
    var connectionError: String?
    var columns: [Column] = []
    var home: String?
    /// パスの欄（選んだもののパス。ホームの下なら `~/…`）。打ち込んで ⏎ でそこへ行く。
    var field = ""
    var showHidden = false
    /// 読めなかったときの理由（打ち込んだパスがないなど）。
    var problem: String?
    var loading = false
    var close: (RemoteFilePicker.Chosen?) -> Void = { _ in }
    /// 今の移動（古い移動の答えは捨てる）。
    private var generation = 0
    /// 今のつなぎ（古いつなぎの答えは捨てる）。
    private var attempt = 0

    init(kind: RemoteFilePicker.Kind, hosts: [String]?, initial: String) {
        self.kind = kind
        self.hosts = hosts
        let initial = initial.trimmingCharacters(in: .whitespaces)
        self.initial = initial.isEmpty ? "~" : initial
        field = self.initial
    }

    /// つないであるリポジトリで読む。
    func use(_ repository: Repository, host: String) {
        self.host = host
        self.repository = repository
        go(initial, orHome: true)
    }

    /// `host` のエンジンにつなぐ（リポジトリなしで動かす。共有の daemon を使う設定なら daemon 越し）。
    func connect(_ host: String) {
        let host = host.trimmingCharacters(in: .whitespaces)
        attempt += 1
        let attempt = attempt
        self.host = host
        repository = nil
        columns = []
        connectionError = nil
        problem = nil
        guard !host.isEmpty, !host.contains(where: { $0 == "/" || $0.isWhitespace }) else {
            connecting = false
            return
        }
        connecting = true
        let location = (Settings.openThroughDaemon ? "daemon:" : "") + "ssh://\(host)"
        let daemon = Daemon.path, daemons = Daemon.bundledDirectory
        // 欄に打ち込んであれば、そこへ行く（つないでいる間に打ち込み直したもの）。
        Task {
            let result = await Task.detached(priority: .userInitiated) {
                Result { try RepositoryWindow.openRepository(location: location, daemon: daemon, daemons: daemons) }
            }.value
            guard attempt == self.attempt else { return }
            connecting = false
            switch result {
            case let .success(repository):
                self.repository = repository
                go(field, orHome: true)
            case let .failure(error):
                connectionError = "\(error)"
            }
        }
    }

    /// フォルダの中身を、つないだエンジンで読む（裏のスレッドで呼ぶ）。
    nonisolated private static func list(_ repository: Repository, _ path: String) throws -> RemoteDirectory {
        try repository.info(["op": "listDirectory", "path": path], as: RemoteDirectory.self)
    }

    /// 打ち込んだパスへ行く: フォルダならその中、ファイルならそれのあるフォルダでそれを選ぶ。`orHome` なら、
    /// 読めないときはホームへ（最初に出すところがもうないとき）。
    func go(_ typed: String, orHome: Bool = false) {
        let typed = typed.trimmingCharacters(in: .whitespaces).isEmpty ? "~" : typed.trimmingCharacters(in: .whitespaces)
        guard let repository else { return }
        generation += 1
        let generation = generation
        loading = true
        problem = nil
        Task {
            let result = await Task.detached(priority: .userInitiated) { () -> Result<(RemoteDirectory, String?), Error> in
                let list = { (path: String) throws -> RemoteDirectory in try Self.list(repository, path) }
                do { return .success((try list(typed), nil)) } catch {
                    // フォルダとして読めなければ、上のフォルダでそれを選ぶ。
                    let parent = (typed as NSString).deletingLastPathComponent
                    let name = (typed as NSString).lastPathComponent
                    guard !parent.isEmpty, !name.isEmpty, name != "~",
                          let dir = try? list(parent), dir.entries.contains(where: { $0.name == name })
                    else { return .failure(error) }
                    return .success((dir, name))
                }
            }.value
            guard generation == self.generation else { return }
            loading = false
            switch result {
            case let .success((dir, name)):
                show(dir, selecting: name)
            case .failure where orHome && typed != "~":
                go("~")
            case let .failure(error):
                problem = "\(error)"
            }
        }
    }

    /// `dir` を最後の列にし、その上のフォルダを `/` から並べる（中身はあとで読む）。
    private func show(_ dir: RemoteDirectory, selecting name: String?) {
        home = dir.home ?? home
        let parts = Self.ancestors(of: dir.path)
        var columns: [Column] = []
        for (i, path) in parts.enumerated() {
            let next = i + 1 < parts.count ? parts[i + 1] : dir.path
            columns.append(Column(path: path, selected: (next as NSString).lastPathComponent))
        }
        columns.append(Column(path: dir.path, entries: dir.entries, selected: name))
        self.columns = columns
        updateField()
        for column in columns where column.entries == nil { load(column.path) }
    }

    /// 列の中のものを選ぶ。フォルダなら、その中を次の列に出す。
    func select(_ entry: RemoteDirectory.Entry, in index: Int) {
        guard columns.indices.contains(index), selectable(entry) || entry.isFolder else { return }
        generation += 1
        problem = nil
        loading = false
        columns.removeSubrange((index + 1)...)
        columns[index].selected = entry.name
        if entry.isFolder {
            let path = Self.join(columns[index].path, entry.name)
            columns.append(Column(path: path))
            load(path)
        }
        updateField()
    }

    /// 列の中身を読む。
    private func load(_ path: String) {
        guard let repository else { return }
        Task {
            let result = await Task.detached(priority: .userInitiated) { Result { try Self.list(repository, path) } }.value
            guard let index = columns.firstIndex(where: { $0.path == path }) else { return }
            switch result {
            case let .success(dir):
                home = dir.home ?? home
                columns[index].entries = dir.entries
            case let .failure(error):
                columns[index].entries = []
                columns[index].error = "\(error)"
            }
        }
    }

    /// 選べるものか（フォルダを選ぶならフォルダ、実行ファイルを選ぶなら実行できるファイル）。
    func selectable(_ entry: RemoteDirectory.Entry) -> Bool {
        switch kind {
        case .folder: entry.isFolder
        case .executable: entry.kind == "file" && entry.executable
        }
    }

    /// 出すもの: 隠しファイル（`.` で始まるもの）は切り替えで。選んでいるものはいつも出す。
    func visible(_ column: Column) -> [RemoteDirectory.Entry] {
        (column.entries ?? []).filter { showHidden || !$0.name.hasPrefix(".") || $0.name == column.selected }
    }

    /// 選んだもの（決めるボタンで返すパス）。選べなければ nil。つながらなければ、打ち込んだパス。
    var chosen: String? {
        if connectionError != nil {
            let typed = field.trimmingCharacters(in: .whitespaces)
            switch kind {
            case .folder: return typed.isEmpty ? nil : typed
            case .executable: return typed.hasPrefix("/") ? typed : nil
            }
        }
        guard let last = columns.last else { return nil }
        switch kind {
        case .folder:
            return last.error == nil ? last.path : nil
        case .executable:
            guard let name = last.selected, let entry = last.entries?.first(where: { $0.name == name }),
                  selectable(entry) else { return nil }
            return Self.join(last.path, name)
        }
    }

    func choose() {
        guard let chosen else { return }
        close(RemoteFilePicker.Chosen(host: host, path: chosen, shown: shown(chosen)))
    }

    /// 欄に、選んだもの（なければ最後のフォルダ）を出す。
    private func updateField() {
        guard let last = columns.last else { return }
        let path = last.selected.map { Self.join(last.path, $0) } ?? last.path
        field = shown(path)
    }

    /// ホームの下なら `~/…`。
    func shown(_ path: String) -> String {
        guard let home, !home.isEmpty, home != "/" else { return path }
        if path == home { return "~" }
        if path.hasPrefix(home + "/") { return "~" + path.dropFirst(home.count) }
        return path
    }

    /// `/a/b/c` の上のフォルダ: `/`・`/a`・`/a/b`。
    static func ancestors(of path: String) -> [String] {
        guard path.hasPrefix("/"), path != "/" else { return [] }
        var out = ["/"]
        var current = ""
        for part in path.split(separator: "/").dropLast() {
            current += "/" + part
            out.append(current)
        }
        return out
    }

    static func join(_ folder: String, _ name: String) -> String {
        folder.hasSuffix("/") ? folder + name : folder + "/" + name
    }
}

struct RemoteFilePickerView: View {
    @Bindable var model: RemoteFilePickerModel
    let title: String
    let message: String
    private let columnWidth: CGFloat = 210

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.headline)
            if !message.isEmpty {
                Text(message).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            if let hosts = model.hosts {
                HStack(spacing: 6) {
                    Text(L("Host:"))
                    HostComboBox(hosts: hosts, host: model.host) { model.connect($0) }
                }
            }
            HStack(spacing: 6) {
                TextField("", text: $model.field, prompt: Text(model.kind == .folder ? "~/prj" : "/usr/local/bin/git"))
                    .textFieldStyle(.roundedBorder)
                    .font(.system(.body, design: .monospaced))
                    .onSubmit { model.go(model.field) }
                Button { model.go("~") } label: { Image(systemName: "house") }
                    .help(L("Home folder"))
                    .disabled(model.connecting || model.connectionError != nil)
                if model.loading { ProgressView().controlSize(.small) }
            }
            if model.connecting {
                ProgressView(L("Connecting to %@…", model.host))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let error = model.connectionError {
                VStack(alignment: .leading, spacing: 8) {
                    Text(L("Couldn’t connect to %@", model.host))
                    Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled)
                    Text(L("You can still type the path above and choose it."))
                        .font(.caption).foregroundStyle(.secondary)
                    Button(L("Check Connection")) { model.connect(model.host) }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            } else if model.host.isEmpty {
                Text(L("Choose a host to see its folders."))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                columns
            }
            if let problem = model.problem {
                Text(problem).font(.caption).foregroundStyle(.red).lineLimit(2).textSelection(.enabled)
            }
            HStack {
                Toggle(L("Show hidden files"), isOn: $model.showHidden)
                    .keyboardShortcut(".", modifiers: [.command, .shift])
                Spacer()
                Button(L("Cancel")) { model.close(nil) }
                    .keyboardShortcut(.cancelAction)
                Button(L("Choose")) { model.choose() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(model.chosen == nil)
            }
        }
        .padding(16)
        .frame(minWidth: 520, minHeight: 320)
        .background(Color(nsColor: .windowBackgroundColor))
    }

    /// `/` から選んだところまでの列（右端が見えるように送る）。
    private var columns: some View {
        ScrollViewReader { proxy in
            ScrollView(.horizontal) {
                HStack(spacing: 0) {
                    ForEach(Array(model.columns.enumerated()), id: \.element.id) { index, column in
                        columnView(column, index: index)
                            .frame(width: columnWidth)
                            .id(column.id)
                        Divider()
                    }
                }
            }
            .background(Color(nsColor: .textBackgroundColor))
            .overlay(RoundedRectangle(cornerRadius: 4).stroke(Color(nsColor: .separatorColor)))
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .onChange(of: model.columns.map(\.id)) { _, ids in
                if let last = ids.last { withAnimation(.easeOut(duration: 0.15)) { proxy.scrollTo(last, anchor: .trailing) } }
            }
            .onAppear {
                if let last = model.columns.last { proxy.scrollTo(last.id, anchor: .trailing) }
            }
        }
    }

    @ViewBuilder private func columnView(_ column: RemoteFilePickerModel.Column, index: Int) -> some View {
        // 最後に選んだ列だけ強い色で（Finder と同じ）。
        let active = index == model.columns.count - 1 || (index == model.columns.count - 2 && model.columns.last?.selected == nil)
        if column.entries == nil {
            ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let error = column.error {
            Text(error).font(.caption).foregroundStyle(.secondary).padding(8)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        } else {
            ScrollViewReader { proxy in
                ScrollView(.vertical) {
                    LazyVStack(spacing: 0) {
                        ForEach(model.visible(column), id: \.name) { entry in
                            row(entry, selected: column.selected == entry.name, active: active)
                                .id(entry.name)
                                .onTapGesture { model.select(entry, in: index) }
                                .simultaneousGesture(TapGesture(count: 2).onEnded {
                                    if model.selectable(entry) { model.select(entry, in: index); model.choose() }
                                })
                        }
                    }
                    .padding(.vertical, 2)
                }
                .onAppear {
                    if let selected = column.selected { proxy.scrollTo(selected, anchor: .center) }
                }
            }
        }
    }

    private func row(_ entry: RemoteDirectory.Entry, selected: Bool, active: Bool) -> some View {
        let enabled = model.selectable(entry) || entry.isFolder
        return HStack(spacing: 6) {
            Image(systemName: icon(entry))
                .foregroundStyle(selected && active ? Color.white
                                 : entry.repository ? Color.green : entry.isFolder ? Color.accentColor : .secondary)
                .frame(width: 16)
                .help(entry.repository ? L("Git repository") : "")
            Text(entry.name).lineLimit(1).truncationMode(.middle)
            if entry.link {
                Image(systemName: "arrow.turn.up.right").font(.caption2).foregroundStyle(.secondary)
                    .help(L("Symbolic link"))
            }
            Spacer(minLength: 4)
            if entry.isFolder {
                Image(systemName: "chevron.right").font(.caption2).foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 6)
        .frame(height: 20)
        .foregroundStyle(selected && active ? Color.white : enabled ? Color.primary : Color.secondary.opacity(0.6))
        .background(
            RoundedRectangle(cornerRadius: 4)
                .fill(selected ? (active ? Color(nsColor: .selectedContentBackgroundColor) : Color(nsColor: .unemphasizedSelectedContentBackgroundColor)) : .clear)
        )
        .padding(.horizontal, 4)
        .contentShape(Rectangle())
    }

    private func icon(_ entry: RemoteDirectory.Entry) -> String {
        if entry.repository { return "arrow.triangle.branch" }
        if entry.isFolder { return "folder.fill" }
        if entry.kind == "file" { return entry.executable ? "apple.terminal" : "doc" }
        return "questionmark.square.dashed"
    }
}

/// SSH 先の欄（候補は ~/.ssh/config などの名前）。選ぶか ⏎ でつなぐ。
private struct HostComboBox: NSViewRepresentable {
    let hosts: [String]
    let host: String
    let commit: (String) -> Void

    func makeNSView(context: Context) -> NSComboBox {
        let box = NSComboBox()
        box.addItems(withObjectValues: hosts)
        box.numberOfVisibleItems = 12
        box.completes = true
        box.placeholderString = L("user@host:port (or a name in ~/.ssh/config)")
        box.stringValue = host
        box.target = context.coordinator
        box.action = #selector(Coordinator.committed(_:))
        box.delegate = context.coordinator
        return box
    }

    func updateNSView(_ box: NSComboBox, context: Context) {
        context.coordinator.commit = commit
        // つなぐ SSH 先が変わったときだけ書く（打ち込んでいるものは消さない）。
        if context.coordinator.shown != host {
            context.coordinator.shown = host
            box.stringValue = host
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(commit: commit) }

    final class Coordinator: NSObject, NSComboBoxDelegate {
        var commit: (String) -> Void
        /// 欄に書いた SSH 先。
        var shown: String?
        /// 最後につないだ名前（同じ名前で何度もつながない）。
        private var last: String?

        init(commit: @escaping (String) -> Void) { self.commit = commit }

        @objc func committed(_ box: NSComboBox) { send(box.stringValue) }

        func comboBoxSelectionDidChange(_ notification: Notification) {
            guard let box = notification.object as? NSComboBox, box.indexOfSelectedItem >= 0,
                  let value = box.objectValueOfSelectedItem as? String else { return }
            send(value)
        }

        private func send(_ value: String) {
            let value = value.trimmingCharacters(in: .whitespaces)
            guard value != last else { return }
            last = value
            commit(value)
        }
    }
}
