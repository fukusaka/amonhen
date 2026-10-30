import AmonhenKit
import AppKit
import SwiftUI

/// ファイルやフォルダを選ぶシート。この Mac のものも、git を動かす機械（SSH 先）のものも同じ形で選ぶ。Finder のカラム表示に
/// 近い形: 上にパスの欄（打ち込める。`~` はその機械のホーム）、下に `/` から選んだところまでの列。中身はコアが読む（この Mac は
/// `LocalFolders`、SSH 先はそのエンジンの `listDirectory`）。リポジトリは緑の印で出す。呼ぶ所が、何を選ぶかを決める
/// （選べないものは薄く出す）。パスは機械の形のまま（Windows なら `C:\…`。`HostPath`）。列は Finder と同じくキーボードでも
/// 動かせる。
enum FilePicker {
    enum Kind {
        /// 実行できるファイル。
        case executable
        /// フォルダ（開いているフォルダを選ぶ）。
        case folder
        /// ファイル。
        case file
        /// リポジトリ（`.git` のあるフォルダ。開かずに選ぶ）。
        case repository
        /// アプリ（`.app`。開かずに選ぶ）。
        case application
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
    /// `places` は、すぐ行けるフォルダ（`/usr/local/bin` など、ふだん見えない所）。
    static func present(title: String, message: String, kind: Kind, host: String, repository: Repository? = nil,
                        hosts: [String]? = nil, initial: String, places: [String] = [], in window: NSWindow? = nil,
                        done: @escaping (Chosen?) -> Void) {
        let model = FilePickerModel(kind: kind, hosts: hosts, initial: initial, places: places)
        show(model, title: title, message: message, height: hosts == nil ? 460 : 490, in: window) { paths in
            done(paths?.first.map { Chosen(host: model.host, path: $0, shown: model.shown($0)) })
        }
        if let repository {
            model.use(repository, host: host)
        } else {
            model.connect(host)
        }
    }

    /// この Mac のものを選ぶ。選んだものの絶対パスを `done` に渡す（`multiple` なら ⌘ クリックでいくつでも）。取り消したら nil。
    /// `canCreateFolders` なら、開いているフォルダの中にフォルダを作れる。
    static func presentLocal(title: String, message: String, kind: Kind, initial: String, places: [String] = [],
                             multiple: Bool = false, canCreateFolders: Bool = false, in window: NSWindow? = nil,
                             done: @escaping ([String]?) -> Void) {
        let model = FilePickerModel(kind: kind, hosts: nil, initial: initial, places: places, multiple: multiple,
                                    canCreateFolders: canCreateFolders)
        show(model, title: title, message: message, height: 460, in: window, done: done)
        model.useLocal()
    }

    /// リポジトリの HEAD の木のフォルダを選ぶ（まだ取り出していないフォルダも。sparse checkout で取り出すフォルダ）。
    /// パスは `/` を根にした形（`/crates/core`）で、選んだものを根からの相対パスにして `done` に渡す。根は選べない。
    /// 取り消したら nil。
    static func presentTree(title: String, message: String, repository: Repository, initial: String = "/",
                            in window: NSWindow? = nil, done: @escaping (String?) -> Void) {
        let model = FilePickerModel(kind: .folder, hosts: nil, initial: initial)
        show(model, title: title, message: message, height: 460, in: window) { paths in
            let path = paths?.first.map { $0.trimmingCharacters(in: CharacterSet(charactersIn: "/")) }
            done(path?.isEmpty == false ? path : nil)
        }
        model.useTree(repository)
    }

    private static func show(_ model: FilePickerModel, title: String, message: String, height: CGFloat,
                             in window: NSWindow?, done: @escaping ([String]?) -> Void) {
        let view = FilePickerView(model: model, title: title, message: message)
        let sheet = NSWindow(contentViewController: NSHostingController(rootView: view))
        sheet.styleMask = [.titled, .resizable]
        sheet.setContentSize(NSSize(width: 760, height: height))
        sheet.contentMinSize = NSSize(width: 520, height: 320)
        sheet.title = title
        // アプリが前にいないとき（キーのウィンドウがないとき）も、いちばん前のウィンドウに付ける。
        let parent = window ?? NSApp.keyWindow ?? NSApp.orderedWindows.first { $0.isVisible && !($0 is NSPanel) }
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
    }
}

/// フォルダの中身（コアの `listDirectory` の `info`、この Mac なら `LocalFolders`）。
nonisolated struct FolderContents: Decodable, Sendable {
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
final class FilePickerModel {
    /// 1 つの列: フォルダと、その中で選んでいるもの。
    struct Column: Identifiable {
        let path: String
        var entries: [FolderContents.Entry]?
        var error: String?
        var selected: String?
        /// ⌘ クリックで足したもの（いくつも選べるとき。`selected` は含まない）。
        var marked: Set<String> = []
        var id: String { path }
        func isSelected(_ name: String) -> Bool { selected == name || marked.contains(name) }
    }

    /// 中身を読む所: SSH 先のエンジン（つないだリポジトリ）か、この Mac。
    nonisolated enum Reader: @unchecked Sendable {
        case engine(Repository)
        case local
        /// リポジトリの HEAD の木（フォルダだけ。パスは `/` を根にした形）。
        case tree(Repository)
    }

    let kind: FilePicker.Kind
    /// 選べる SSH 先（SSH 先を変えられないなら nil）。
    let hosts: [String]?
    /// すぐ行けるフォルダ。
    let places: [String]
    /// ⌘ クリックでいくつも選べるか（開かずに選ぶもの）。
    let multiple: Bool
    /// 開いているフォルダの中にフォルダを作れるか（この Mac だけ）。
    let canCreateFolders: Bool
    /// 最初に出すところ（つながったら行く）。
    private let initial: String
    /// 今の SSH 先（この Mac なら空）と、中身を読む所。
    var host = ""
    private(set) var reader: Reader?
    var isLocal: Bool { if case .local = reader { true } else { false } }
    /// リポジトリの木を見ている（ホームも SSH 先もない）。
    var isTree: Bool { if case .tree = reader { true } else { false } }
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
    /// 打ち込んだパスへ行ったたびに増える（列を、選んだものが見えるところまで送る）。
    var reveal = 0
    var close: ([String]?) -> Void = { _ in }
    /// 今の移動（古い移動の答えは捨てる）。
    private var generation = 0
    /// 今のつなぎ（古いつなぎの答えは捨てる）。
    private var attempt = 0
    /// → で入ったが、まだ読んでいるフォルダ（読めたら最初のものを選ぶ）。
    private var entering: String?
    /// 打った名前（type-select）と、最後に打った時刻。
    private var typed = ""
    private var typedAt = Date.distantPast

    init(kind: FilePicker.Kind, hosts: [String]?, initial: String, places: [String] = [], multiple: Bool = false,
         canCreateFolders: Bool = false) {
        self.kind = kind
        self.hosts = hosts
        self.places = places
        self.multiple = multiple
        self.canCreateFolders = canCreateFolders
        let initial = initial.trimmingCharacters(in: .whitespaces)
        self.initial = initial.isEmpty ? "~" : initial
        field = self.initial
    }

    /// つないであるリポジトリで読む。
    func use(_ repository: Repository, host: String) {
        self.host = host
        reader = .engine(repository)
        go(initial, orHome: true)
    }

    /// リポジトリの HEAD の木を読む。
    func useTree(_ repository: Repository) {
        host = ""
        reader = .tree(repository)
        go(initial, orHome: true)
    }

    /// この Mac のものを読む。
    func useLocal() {
        host = ""
        reader = .local
        go(initial, orHome: true)
    }

    /// `host` のエンジンにつなぐ（リポジトリなしで動かす。共有の daemon を使う設定なら daemon 越し）。
    func connect(_ host: String) {
        let host = host.trimmingCharacters(in: .whitespaces)
        attempt += 1
        let attempt = attempt
        self.host = host
        reader = nil
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
                reader = .engine(repository)
                go(field, orHome: true)
            case let .failure(error):
                connectionError = "\(error)"
            }
        }
    }

    /// フォルダの中身を読む（裏のスレッドで呼ぶ）。
    nonisolated private static func list(_ reader: Reader, _ path: String) throws -> FolderContents {
        switch reader {
        case let .engine(repository):
            try repository.info(["op": "listDirectory", "path": path], as: FolderContents.self)
        case .local:
            try JSONDecoder().decode(FolderContents.self, from: LocalFolders.listJSON(path))
        case let .tree(repository):
            try repository.info(["op": "treeFolders", "path": path], as: FolderContents.self)
        }
    }

    /// 打ち込んだパスへ行く: フォルダならその中、ファイルならそれのあるフォルダでそれを選ぶ。`orHome` なら、
    /// 読めないときはホームへ（最初に出すところがもうないとき）。
    func go(_ typed: String, orHome: Bool = false) {
        let typed = typed.trimmingCharacters(in: .whitespaces).isEmpty ? "~" : typed.trimmingCharacters(in: .whitespaces)
        guard let reader else { return }
        generation += 1
        let generation = generation
        loading = true
        problem = nil
        entering = nil
        // Windows の機械なら `\` も区切り（`~\bin\git.exe` は形で見分けられない）。
        let windows = HostPath.isWindows(home ?? columns.first?.path ?? "")
        let kind = kind
        Task {
            let result = await Task.detached(priority: .userInitiated) { () -> Result<(FolderContents, String?), Error> in
                let list = { (path: String) throws -> FolderContents in try Self.list(reader, path) }
                // 上のフォルダで、それを選ぶ。
                let inParent = { (path: String) -> (FolderContents, String?)? in
                    guard let split = HostPath.split(path, windows: windows), split.name != "~",
                          let dir = try? list(split.parent), dir.entries.contains(where: { $0.name == split.name })
                    else { return nil }
                    return (dir, split.name)
                }
                do {
                    let dir = try list(typed)
                    // リポジトリやアプリを選ぶなら、それは開かずに選ぶ。
                    let leaf = switch kind {
                    case .repository: dir.entries.contains { $0.name == ".git" }
                    case .application: dir.path.hasSuffix(".app")
                    case .folder, .executable, .file: false
                    }
                    if leaf, let found = inParent(dir.path) { return .success(found) }
                    return .success((dir, nil))
                } catch {
                    // フォルダとして読めなければ、上のフォルダでそれを選ぶ。
                    guard let found = inParent(typed) else { return .failure(error) }
                    return .success(found)
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

    /// `dir` を最後の列にし、その上のフォルダを根（`/`・`C:\`）から並べる（中身はあとで読む）。
    private func show(_ dir: FolderContents, selecting name: String?) {
        home = dir.home ?? home
        let parts = HostPath.ancestors(of: dir.path)
        var columns: [Column] = []
        for (i, path) in parts.enumerated() {
            let next = i + 1 < parts.count ? parts[i + 1] : dir.path
            columns.append(Column(path: path, selected: HostPath.name(next)))
        }
        columns.append(Column(path: dir.path, entries: dir.entries, selected: name))
        self.columns = columns
        reveal += 1
        updateField()
        for column in columns where column.entries == nil { load(column.path) }
    }

    /// 列の中のものを選ぶ。開くフォルダなら、その中を次の列に出す。`extend`（⌘ クリック）なら、いくつも選べるとき、
    /// 選んだものに足す（選んであれば外す）。
    func select(_ entry: FolderContents.Entry, in index: Int, extend: Bool = false) {
        guard columns.indices.contains(index), selectable(entry) || opens(entry) else { return }
        generation += 1
        problem = nil
        loading = false
        entering = nil
        if extend, multiple, selectable(entry), !opens(entry), index == columns.count - 1 {
            var column = columns[index]
            if column.isSelected(entry.name) {
                column.marked.remove(entry.name)
                if column.selected == entry.name { column.selected = column.marked.popFirst() }
            } else {
                if let selected = column.selected, column.entries?.first(where: { $0.name == selected }).map(selectable) == true {
                    column.marked.insert(selected)
                }
                column.selected = entry.name
            }
            columns[index] = column
            updateField()
            return
        }
        columns.removeSubrange((index + 1)...)
        columns[index].selected = entry.name
        columns[index].marked = []
        if opens(entry) {
            let path = HostPath.join(columns[index].path, entry.name)
            columns.append(Column(path: path))
            load(path)
        }
        updateField()
    }

    /// 列の中身を読む。
    private func load(_ path: String) {
        guard let reader else { return }
        Task {
            let result = await Task.detached(priority: .userInitiated) { Result { try Self.list(reader, path) } }.value
            guard let index = columns.firstIndex(where: { $0.path == path }) else { return }
            switch result {
            case let .success(dir):
                home = dir.home ?? home
                columns[index].entries = dir.entries
                if entering == path {
                    entering = nil
                    selectFirst(in: index)
                }
            case let .failure(error):
                columns[index].entries = []
                columns[index].error = "\(error)"
            }
        }
    }

    /// キーボードで動かす列: いちばん奥で選んでいるものの列（フォルダを選んで、その中でまだ何も選んでいなければ、
    /// そのフォルダの列）。
    var activeIndex: Int? {
        guard let last = columns.indices.last else { return nil }
        return columns[last].selected == nil && last > 0 ? last - 1 : last
    }

    /// 列の中で選べるもの（キーボードで動く先）。
    private func candidates(_ index: Int) -> [FolderContents.Entry] {
        visible(columns[index]).filter { selectable($0) || opens($0) }
    }

    /// ↑↓: 列の中で前後のものを選ぶ。何も選んでいなければ、端のもの。
    func move(_ step: Int) {
        guard let index = activeIndex else { return }
        let entries = candidates(index)
        guard !entries.isEmpty else { return }
        let current = columns[index].selected.flatMap { name in entries.firstIndex { $0.name == name } }
        let next = current.map { min(max($0 + step, 0), entries.count - 1) } ?? (step > 0 ? 0 : entries.count - 1)
        if next != current { select(entries[next], in: index) }
    }

    /// →: 選んでいるフォルダに入り、その最初のものを選ぶ（まだ読んでいれば、読めたら）。
    func enter() {
        guard let index = activeIndex, index + 1 < columns.count else { return }
        if columns[index + 1].entries == nil {
            entering = columns[index + 1].path
        } else {
            selectFirst(in: index + 1)
        }
    }

    /// ←: 上の列へ戻る（今の列の選びを外す。上の列では、このフォルダを選んだまま）。
    func back() {
        guard let index = activeIndex, index > 0 else { return }
        generation += 1
        problem = nil
        loading = false
        entering = nil
        columns.removeSubrange((index + 1)...)
        columns[index].selected = nil
        updateField()
    }

    /// ⏎: 選べるものなら選ぶ。選べなければ、選んでいるフォルダに入る。
    func activate() {
        if !chosen.isEmpty { choose() } else { enter() }
    }

    /// 名前を打つと、その名前の行を選ぶ（続けて打てば続きで。なければ、並びでその後にあるもの）。
    func typeSelect(_ characters: String) {
        let now = Date()
        typed = now.timeIntervalSince(typedAt) > 1 ? characters : typed + characters
        typedAt = now
        guard let index = activeIndex else { return }
        let entries = candidates(index)
        // エンジンは小文字にした名前で並べる。
        let key = typed.lowercased()
        guard let entry = entries.first(where: { $0.name.lowercased() >= key }) ?? entries.last,
              entry.name != columns[index].selected else { return }
        select(entry, in: index)
    }

    private func selectFirst(in index: Int) {
        if let first = candidates(index).first { select(first, in: index) }
    }

    /// 選べるものか（フォルダを選ぶならフォルダ、実行ファイルを選ぶなら実行できるファイルなど）。
    func selectable(_ entry: FolderContents.Entry) -> Bool {
        switch kind {
        case .folder: entry.isFolder
        case .executable: entry.kind == "file" && entry.executable
        case .file: entry.kind == "file"
        case .repository: entry.repository
        case .application: Self.isApplication(entry)
        }
    }

    /// クリックで中を次の列に出すか（フォルダ。リポジトリやアプリを選ぶときは、それは開かずに選ぶ）。
    func opens(_ entry: FolderContents.Entry) -> Bool {
        guard entry.isFolder else { return false }
        switch kind {
        case .repository: return !entry.repository
        case .application: return !Self.isApplication(entry)
        case .folder, .executable, .file: return true
        }
    }

    private static func isApplication(_ entry: FolderContents.Entry) -> Bool {
        entry.isFolder && entry.name.hasSuffix(".app")
    }

    /// 出すもの: 隠しファイル（`.` で始まるもの）は切り替えで。選んでいるものはいつも出す。
    func visible(_ column: Column) -> [FolderContents.Entry] {
        (column.entries ?? []).filter { showHidden || !$0.name.hasPrefix(".") || column.isSelected($0.name) }
    }

    /// 選んだもの（決めるボタンで返すパス。並びは列の順）。選べなければ空。つながらなければ、打ち込んだパス。
    var chosen: [String] {
        if connectionError != nil {
            let typed = field.trimmingCharacters(in: .whitespaces)
            switch kind {
            case .folder: return typed.isEmpty ? [] : [typed]
            case .executable, .file, .repository, .application: return HostPath.isAbsolute(typed) ? [typed] : []
            }
        }
        guard let last = columns.last else { return [] }
        switch kind {
        case .folder:
            // 木の根は選べない（すべてのフォルダになる）。
            return last.error == nil && !(isTree && last.path == "/") ? [last.path] : []
        case .executable, .file, .repository, .application:
            return (last.entries ?? []).filter { last.isSelected($0.name) && selectable($0) }.map { HostPath.join(last.path, $0.name) }
        }
    }

    /// 開いているフォルダ（最後の列）。
    var openFolder: String? { columns.last.flatMap { $0.error == nil && $0.entries != nil ? $0.path : nil } }

    /// 開いているフォルダの中に `name` のフォルダを作り、そこへ行く（この Mac だけ）。
    func createFolder(_ name: String) {
        let name = name.trimmingCharacters(in: .whitespaces)
        guard isLocal, let folder = openFolder, !name.isEmpty else { return }
        guard !name.contains("/"), name != ".", name != ".." else {
            problem = L("A folder name can’t contain “/”.")
            return
        }
        let path = HostPath.join(folder, name)
        do {
            try FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: false)
            go(path)
        } catch {
            problem = error.localizedDescription
        }
    }

    func choose() {
        let chosen = chosen
        guard !chosen.isEmpty else { return }
        close(chosen)
    }

    /// 欄に、選んだもの（なければ最後のフォルダ）を出す。
    private func updateField() {
        guard let last = columns.last else { return }
        let path = last.selected.map { HostPath.join(last.path, $0) } ?? last.path
        field = shown(path)
    }

    /// ホームの下なら `~/…`。
    func shown(_ path: String) -> String {
        HostPath.shown(path, home: home)
    }
}

struct FilePickerView: View {
    @Bindable var model: FilePickerModel
    let title: String
    let message: String
    private let columnWidth: CGFloat = 210
    @State private var naming = false
    @State private var newName = ""

    private enum Focus: Hashable {
        case field
        case columns
    }

    @FocusState private var focus: Focus?
    /// パスの欄に打ち込んだか（打ち込んでいれば、列が出ても欄に残す）。
    @State private var typing = false

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
                TextField("", text: Binding(get: { model.field }, set: {
                    // 欄は同じ値も書き戻すので、変わったときだけ。
                    if $0 != model.field { typing = true }
                    model.field = $0
                }), prompt: Text(model.isTree ? "/src" : model.kind == .executable ? "/usr/local/bin/git" : "~/prj"))
                    .textFieldStyle(.roundedBorder)
                    .font(.system(.body, design: .monospaced))
                    .focused($focus, equals: .field)
                    .onSubmit { model.go(model.field) }
                if !model.isTree {
                    Button { model.go("~") } label: { Image(systemName: "house") }
                        .help(L("Home folder"))
                        .disabled(model.connecting || model.connectionError != nil)
                }
                if !model.places.isEmpty {
                    PlacesButton(places: model.places) { model.go($0) }
                        .fixedSize()
                        .disabled(model.connecting || model.connectionError != nil)
                }
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
            } else if model.host.isEmpty, !model.isLocal, !model.isTree {
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
                if model.canCreateFolders {
                    Button(L("New Folder…")) { newName = ""; naming = true }
                        .keyboardShortcut("n", modifiers: [.command, .shift])
                        .disabled(model.openFolder == nil)
                }
                Spacer()
                Button(L("Cancel")) { model.close(nil) }
                    .keyboardShortcut(.cancelAction)
                Button(L("Choose")) { model.choose() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(model.chosen.isEmpty)
            }
        }
        .padding(16)
        .frame(minWidth: 520, minHeight: 320)
        .background(Color(nsColor: .windowBackgroundColor))
        .alert(L("New Folder"), isPresented: $naming) {
            TextField(L("Folder name"), text: $newName)
            Button(L("Create")) { model.createFolder(newName) }
            Button(L("Cancel"), role: .cancel) {}
        } message: {
            Text(L("In %@", model.openFolder.map(model.shown) ?? ""))
        }
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
            // キーボードでは、列をまとめて 1 つとして（Finder のカラム表示と同じキー）。パスの欄で打っているときは届かない。
            .focusable()
            .focusEffectDisabled()
            .focused($focus, equals: .columns)
            .onKeyPress(phases: .down) { key($0) }
            .onChange(of: model.columns.map(\.id)) { _, ids in
                if let last = ids.last { withAnimation(.easeOut(duration: 0.15)) { proxy.scrollTo(last, anchor: .trailing) } }
            }
            .onAppear {
                if let last = model.columns.last { proxy.scrollTo(last.id, anchor: .trailing) }
                // パスの欄に打ち込んでいなければ、列で始める（すぐ矢印で動ける。シートを出すと欄に入るので、そのあとで）。
                if !typing { Task { focus = .columns } }
            }
        }
    }

    /// 列でのキー: ↑↓ で列の中、→ でフォルダの中、← で上の列、⏎ で選ぶ、名前を打つとその行へ。
    private func key(_ press: KeyPress) -> KeyPress.Result {
        guard press.modifiers.isDisjoint(with: [.command, .control, .option]) else { return .ignored }
        switch press.key {
        case .upArrow: model.move(-1)
        case .downArrow: model.move(1)
        case .rightArrow: model.enter()
        case .leftArrow: model.back()
        case .return: model.activate()
        default:
            let characters = press.characters
            guard !characters.isEmpty, !characters.unicodeScalars.contains(where: {
                CharacterSet.controlCharacters.contains($0) || (0xF700...0xF8FF).contains($0.value)
            }), !(characters == " ") else { return .ignored }
            model.typeSelect(characters)
        }
        return .handled
    }

    @ViewBuilder private func columnView(_ column: FilePickerModel.Column, index: Int) -> some View {
        // 最後に選んだ列だけ強い色で（Finder と同じ）。パスの欄で打っているときは薄い色で。
        let active = index == model.activeIndex && focus != .field
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
                            row(entry, in: column.path, selected: column.isSelected(entry.name), active: active)
                                .id(entry.name)
                                .onTapGesture {
                                    model.select(entry, in: index, extend: NSEvent.modifierFlags.contains(.command))
                                    focus = .columns
                                }
                                .simultaneousGesture(TapGesture(count: 2).onEnded {
                                    if model.selectable(entry) { model.select(entry, in: index); model.choose() }
                                })
                                // VoiceOver などからは、1 行を 1 つのボタンとして（押すと選ぶ）。
                                .accessibilityElement(children: .ignore)
                                .accessibilityLabel(accessibilityLabel(entry))
                                .accessibilityAddTraits(column.isSelected(entry.name) ? [.isButton, .isSelected] : .isButton)
                                .accessibilityAction { model.select(entry, in: index) }
                        }
                    }
                    .padding(.vertical, 2)
                }
                .onAppear {
                    if let selected = column.selected { proxy.scrollTo(selected, anchor: .center) }
                }
                // 前から出ていた列でも（クリックで選んだときは動かさない）。
                .onChange(of: model.reveal) {
                    if let selected = column.selected { proxy.scrollTo(selected, anchor: .center) }
                }
                // キーボードで選んだものが見えるように（見えていれば動かさない）。
                .onChange(of: column.selected) { _, selected in
                    if let selected { proxy.scrollTo(selected) }
                }
            }
        }
    }

    private func row(_ entry: FolderContents.Entry, in folder: String, selected: Bool, active: Bool) -> some View {
        let enabled = model.selectable(entry) || model.opens(entry)
        return HStack(spacing: 6) {
            // この Mac のアプリは、そのアイコンで。
            if model.isLocal, model.kind == .application, entry.isFolder, !model.opens(entry) {
                Image(nsImage: NSWorkspace.shared.icon(forFile: HostPath.join(folder, entry.name)))
                    .resizable()
                    .frame(width: 16, height: 16)
            } else {
                Image(systemName: icon(entry))
                    .foregroundStyle(selected && active ? Color.white
                                     : entry.repository ? Color.green : entry.isFolder ? Color.accentColor : .secondary)
                    .frame(width: 16)
                    .help(entry.repository ? L("Git repository") : "")
            }
            Text(entry.name).lineLimit(1).truncationMode(.middle)
            if entry.link {
                Image(systemName: "arrow.turn.up.right").font(.caption2).foregroundStyle(.secondary)
                    .help(L("Symbolic link"))
            }
            Spacer(minLength: 4)
            if model.opens(entry) {
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

    /// 名前と種類（リポジトリ、フォルダ、実行ファイル、シンボリックリンク）。
    private func accessibilityLabel(_ entry: FolderContents.Entry) -> String {
        var kinds: [String] = []
        if entry.repository { kinds.append(L("Git repository")) }
        if entry.link { kinds.append(L("Symbolic link")) }
        return ([entry.name] + kinds).joined(separator: ", ")
    }

    private func icon(_ entry: FolderContents.Entry) -> String {
        if entry.repository { return "arrow.triangle.branch" }
        if model.kind == .application, !model.opens(entry), entry.isFolder { return "app.dashed" }
        if entry.isFolder { return "folder.fill" }
        if entry.kind == "file" { return entry.executable ? "apple.terminal" : "doc" }
        return "questionmark.square.dashed"
    }
}

/// すぐ行けるフォルダのメニュー（AppKit のプルダウン。SwiftUI の Menu はシートの中だとアクセシビリティに出ない）。
private struct PlacesButton: NSViewRepresentable {
    let places: [String]
    let go: (String) -> Void

    func makeNSView(context: Context) -> NSPopUpButton {
        let button = NSPopUpButton(frame: .zero, pullsDown: true)
        button.bezelStyle = .push
        (button.cell as? NSPopUpButtonCell)?.arrowPosition = .noArrow
        button.toolTip = L("Go to a folder")
        button.setAccessibilityLabel(L("Go to a folder"))
        update(button, context: context)
        return button
    }

    func updateNSView(_ button: NSPopUpButton, context: Context) {
        update(button, context: context)
    }

    private func update(_ button: NSPopUpButton, context: Context) {
        context.coordinator.go = go
        guard context.coordinator.places != places else { return }
        context.coordinator.places = places
        button.removeAllItems()
        // プルダウンの最初の項目はボタンの顔。
        button.addItem(withTitle: "")
        button.item(at: 0)?.image = NSImage(systemSymbolName: "folder.badge.gearshape", accessibilityDescription: L("Go to a folder"))
        for place in places {
            let item = NSMenuItem(title: place, action: #selector(Coordinator.picked(_:)), keyEquivalent: "")
            item.target = context.coordinator
            button.menu?.addItem(item)
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator: NSObject {
        var places: [String] = []
        var go: (String) -> Void = { _ in }
        @objc func picked(_ item: NSMenuItem) { go(item.title) }
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
