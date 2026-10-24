import AmonhenKit
import AppKit
import SwiftUI

/// git の設定の 1 項目と、どこで決まっているか。
struct ConfigEntry: Decodable, Hashable, Identifiable {
    let scope: String
    let origin: String
    let key: String
    let value: String
    var id: String { "\(scope)|\(origin)|\(key)|\(value)" }
}

struct RemoteEntry: Decodable, Hashable, Identifiable {
    let name: String
    let fetchUrl: String
    let pushUrl: String
    var id: String { name }
}



/// リポジトリの設定（サイドバーの Workspace の Settings、⌘;）の中身。読み書きはコアの命令を裏で呼ぶ。
@Observable
final class RepositorySettingsModel {
    let repository: Repository
    /// リポジトリの場所。
    let location: String
    private(set) var hooksPath: String?
    private(set) var hooks: [HookEntry] = []
    private(set) var lfs = false
    private(set) var lfsPatterns: [String] = []
    private(set) var entries: [ConfigEntry] = []
    private(set) var remotes: [RemoteEntry] = []
    var gitignore = ""
    var exclude = ""
    private(set) var savedIgnore = ("", "")
    var error: String?
    /// このリポジトリを扱う git の版（SSH 先なら SSH 先の git）。読めなければ nil。
    private(set) var gitVersion: String?
    /// rebase で積み重ねたブランチを一緒に動かせるか（git 2.38 から）。
    private(set) var updateRefsSupported = true
    /// ローカルブランチの名前と、判定した既定のブランチ（`branchGroups` の `base`。`amonhen.defaultBranch` も効いた後）。
    private(set) var localBranches: [String] = []
    private(set) var detectedBase: String?
    /// git の設定・リモートなどを変えた（画面を読み直す）。
    var onRepositoryChange: (() -> Void)?

    /// SSH 先のリポジトリなら、その SSH 先の名前。
    var host: String? { RepositoryLocation.isRemote(location) ? RepositoryLocation.host(location) : nil }

    init(repository: Repository, location: String) {
        self.repository = repository
        self.location = location
    }

    func load() {
        let repository = self.repository
        Task {
            struct Entries: Decodable { let entries: [ConfigEntry] }
            struct Remotes: Decodable { let remotes: [RemoteEntry] }
            struct Ignore: Decodable { let gitignore: String; let exclude: String }
            struct Hooks: Decodable { let hooksPath: String?; let hooks: [HookEntry]; let lfs: Bool; let lfsPatterns: [String] }
            struct Version: Decodable { let version: String?; let updateRefs: Bool }
            struct Groups: Decodable { struct Branch: Decodable { let name: String }; let base: String?; let branches: [Branch] }
            do {
                let (e, r, i, h, v, g) = try await Task.detached(priority: .userInitiated) {
                    (
                        try repository.info(["op": "configList"], as: Entries.self),
                        try repository.info(["op": "remotes"], as: Remotes.self),
                        try repository.info(["op": "ignoreFiles"], as: Ignore.self),
                        try repository.info(["op": "hooksAndLfs"], as: Hooks.self),
                        try repository.info(["op": "gitVersion"], as: Version.self),
                        try? repository.info(["op": "branchGroups"], as: Groups.self)
                    )
                }.value
                gitVersion = v.version
                updateRefsSupported = v.updateRefs
                localBranches = g?.branches.map(\.name).sorted() ?? []
                detectedBase = g?.base
                hooksPath = h.hooksPath
                hooks = h.hooks
                lfs = h.lfs
                lfsPatterns = h.lfsPatterns
                // `command` は Amon Hen が git を起動するときの `-c`（色を切るなど）で、リポジトリの設定ではない。
                // `unknown` は git が組み込みで読むファイル（Apple の git の Xcode の gitconfig）で、機械全体のもの。
                entries = e.entries.filter { $0.scope != "command" }.map {
                    $0.scope == "unknown" ? ConfigEntry(scope: "system", origin: $0.origin, key: $0.key, value: $0.value) : $0
                }
                remotes = r.remotes
                if (gitignore, exclude) == savedIgnore {
                    gitignore = i.gitignore
                    exclude = i.exclude
                }
                savedIgnore = (i.gitignore, i.exclude)
            } catch {
                self.error = "\(error)"
            }
        }
    }

    /// 今効いている値（後に出るものほど優先）と、その出どころ。
    func effective(_ key: String) -> ConfigEntry? {
        entries.last { $0.key.lowercased() == key.lowercased() }
    }

    /// このリポジトリ（local）での値。
    func local(_ key: String) -> String? {
        entries.last { $0.key.lowercased() == key.lowercased() && $0.scope == "local" }?.value
    }

    /// local 以外で決まっている値（local を消したときに効く値）。
    func inherited(_ key: String) -> ConfigEntry? {
        entries.last { $0.key.lowercased() == key.lowercased() && $0.scope != "local" }
    }

    /// ユーザー全体（global）での値。
    func global(_ key: String) -> String? {
        entries.last { $0.key.lowercased() == key.lowercased() && $0.scope == "global" }?.value
    }

    /// local の値を変える。nil か空なら消す。
    func set(_ key: String, _ value: String?) {
        let value = value?.trimmingCharacters(in: .whitespaces)
        run(["op": "configSet", "key": key, "value": (value?.isEmpty ?? true) ? NSNull() : value!])
    }

    /// ユーザー全体（global。SSH 先のリポジトリなら SSH 先のユーザー）の値を変える。nil か空なら消す。
    func setGlobal(_ key: String, _ value: String?) {
        let value = value?.trimmingCharacters(in: .whitespaces)
        run(["op": "configSet", "key": key, "value": (value?.isEmpty ?? true) ? NSNull() : value!, "scope": "global"])
    }

    func changeRemote(_ change: [String: Any]) {
        run(["op": "changeRemote", "change": change])
    }

    func saveIgnore() {
        run(["op": "saveIgnoreFiles", "gitignore": gitignore, "exclude": exclude])
    }

    var ignoreChanged: Bool { (gitignore, exclude) != savedIgnore }

    private func run(_ command: [String: Any]) {
        let repository = self.repository
        guard let json = try? Repository.json(command) else { return }
        Task {
            do {
                try await Task.detached(priority: .userInitiated) { try repository.run(json: json) }.value
                error = nil
            } catch {
                self.error = "\(error)"
            }
            load()
            onRepositoryChange?()
        }
    }
}

struct HookEntry: Decodable, Hashable, Identifiable {
    let name: String
    let enabled: Bool
    var id: String { name }
}

/// 設定が書かれているファイル（`file:` を取り、ホームは ~ にする）。
private func configFile(_ entry: ConfigEntry) -> String {
    var origin = entry.origin
    if origin.hasPrefix("file:") { origin = String(origin.dropFirst(5)) }
    return (origin as NSString).abbreviatingWithTildeInPath
}

/// 表のページ（リモート・git config）に、ほかのページのグループ化した Form と同じ地を敷く。Form の地は素材
/// （単色では同じにならない）なので、空の Form を後ろに置く。
private struct GroupedPage<Content: View>: View {
    @ViewBuilder let content: Content
    var body: some View {
        ZStack {
            Form { EmptyView() }.formStyle(.grouped)
            content.padding()
        }
    }
}

private struct RemotesTab: View {
    let model: RepositorySettingsModel
    @State private var selection: String?

    var body: some View {
        GroupedPage {
        VStack(alignment: .leading) {
            Table(model.remotes, selection: $selection) {
                TableColumn(L("Name"), value: \.name).width(min: 80, ideal: 100)
                TableColumn("URL", value: \.fetchUrl)
                TableColumn(L("Push URL")) { r in
                    Text(r.pushUrl == r.fetchUrl ? L("(same)") : r.pushUrl).foregroundStyle(r.pushUrl == r.fetchUrl ? .secondary : .primary)
                }
            }
            HStack {
                Button(L("Add…")) { add() }
                Button(L("Rename…")) { rename() }.disabled(selection == nil)
                Button(L("Edit URL…")) { editURL() }.disabled(selection == nil)
                Button("Prune") { selection.map { model.changeRemote(["kind": "prune", "name": $0]) } }
                    .disabled(selection == nil)
                    .help(L("Remove remote-tracking branches that no longer exist on the remote."))
                Spacer()
                Button(L("Delete"), role: .destructive) { remove() }.disabled(selection == nil)
            }
        }
        }
    }

    private func add() {
        guard let (name, url) = askPair(L("Add Remote"), first: (L("Name"), "origin"), second: ("URL", "")) else { return }
        model.changeRemote(["kind": "add", "name": name, "url": url])
    }

    private func rename() {
        guard let from = selection, let to = ask(L("Rename Remote"), label: L("New name"), initial: from) else { return }
        model.changeRemote(["kind": "rename", "from": from, "to": to])
    }

    private func editURL() {
        guard let name = selection, let remote = model.remotes.first(where: { $0.name == name }),
              let url = ask(L("Edit URL"), label: L("URL for %@", name), initial: remote.fetchUrl) else { return }
        model.changeRemote(["kind": "setUrl", "name": name, "url": url])
    }

    private func remove() {
        guard let name = selection else { return }
        let alert = NSAlert()
        alert.messageText = L("Remove %@?", name)
        alert.informativeText = L("This removes the remote’s settings and its remote-tracking branches (%@/…). The remote repository isn’t touched.", name)
        alert.addButton(withTitle: L("Delete"))
        alert.addButton(withTitle: L("Cancel"))
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        model.changeRemote(["kind": "remove", "name": name])
        selection = nil
    }

    private func ask(_ title: String, label: String, initial: String) -> String? {
        askPair(title, first: (label, initial), second: nil).map(\.0)
    }

    /// 1 つか 2 つの欄に入れてもらう。キャンセルか空なら nil。
    private func askPair(_ title: String, first: (String, String), second: (String, String)?) -> (String, String)? {
        let alert = NSAlert()
        alert.messageText = title
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: L("Cancel"))
        let a = NSTextField(string: first.1), b = NSTextField(string: second?.1 ?? "")
        var rows: [[NSView]] = [[NSTextField(labelWithString: first.0 + ":"), a]]
        if let second { rows.append([NSTextField(labelWithString: second.0 + ":"), b]) }
        let grid = NSGridView(views: rows)
        grid.column(at: 1).width = 320
        grid.rowSpacing = 8
        grid.frame = NSRect(x: 0, y: 0, width: 380, height: CGFloat(rows.count) * 30)
        alert.accessoryView = grid
        alert.window.initialFirstResponder = a
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let x = a.stringValue.trimmingCharacters(in: .whitespaces), y = b.stringValue.trimmingCharacters(in: .whitespaces)
        guard !x.isEmpty, second == nil || !y.isEmpty else { return nil }
        return (x, y)
    }
}

/// git config の表の 1 行: 1 つのキーと、出どころ（段）ごとの値（docs/ux.md の 4.5）。同じ段に同じキーが
/// 複数あれば（`remote.origin.fetch` など）、改行で並べる。
nonisolated struct ConfigRow: Identifiable, Hashable, Sendable {
    let key: String
    /// 今効いている値（後の段ほど優先）と、その段。どの段にもなければ空。
    let effective: String
    let effectiveScope: String
    /// 段ごとの値と、その段のファイル。
    let values: [String: String]
    let files: [String: String]
    var id: String { key }
    var system: String { values["system"] ?? "" }
    var global: String { values["global"] ?? "" }
    var local: String { values["local"] ?? "" }
    var worktree: String { values["worktree"] ?? "" }
}

extension RepositorySettingsModel {
    /// `entries` をキーごとに 1 行にまとめる。順は git が出した順（表はこれを見出しで並べ替える）。
    var configRows: [ConfigRow] {
        var order: [String] = []
        var byKey: [String: [ConfigEntry]] = [:]
        for e in entries {
            if byKey[e.key] == nil { order.append(e.key) }
            byKey[e.key, default: []].append(e)
        }
        return order.map { key in
            let es = byKey[key] ?? []
            var values: [String: String] = [:], files: [String: String] = [:]
            for e in es {
                values[e.scope] = values[e.scope].map { $0 + "\n" + e.value } ?? e.value
                files[e.scope] = configFile(e)
            }
            let scope = es.last?.scope ?? ""
            return ConfigRow(key: key, effective: values[scope] ?? "", effectiveScope: scope, values: values, files: files)
        }
    }
}

/// `git config` のページ: Xcode の Build Settings のような表。列はキー、効いている値、そして段ごと（システム全体・
/// すべてのリポジトリ・このリポジトリ・この worktree）の値。効いている段の値は太字。「このリポジトリ」の列だけ
/// 書ける: 行をダブルクリック（か右クリックの「編集」）で欄になり、Return で書く。空にするとここでは消える。
/// 「+」で新しい行が出て、キーと値を入れる。⌘C で選んだ行を `key = value` の形でコピーする。
/// 上の切り替えも Xcode と同じ: 行は「基本（ページが尋ねるキー。未設定でも出す）／このリポジトリ／すべて」、
/// 列は「まとめて（効いている値と出どころ）／段ごと」。
private struct AllSettingsTab: View {
    let model: RepositorySettingsModel
    @State private var filter = ""
    @State private var selection = Set<String>()
    /// 見出しで並べ替える。初めはキーの順（`onAppear` で入れる。`@State` の初期値にキーパスは置けない）。
    @State private var sortOrder: [KeyPathComparator<ConfigRow>] = []
    @State private var columns = TableColumnCustomization<ConfigRow>()
    @AppStorage("configTableRows") private var rowsMode = RowsMode.all
    @AppStorage("configTableColumns") private var columnsMode = ColumnsMode.levels
    /// 「このリポジトリ」の列を編集しているキーと、欄の中身。
    @State private var editing: String?
    @State private var draft = ""
    /// 「+」で足している途中（表の最後に、キーと値の欄だけの行が出る）。
    @State private var adding = false
    @State private var newKey = ""
    @FocusState private var focus: Field?

    private enum Field: Hashable { case value, newKey, newValue }
    private enum RowsMode: String { case basic, thisRepository, all }
    private enum ColumnsMode: String { case combined, levels }
    /// 足している途中の行の id（キーは空）。
    private static let newRowID = ""
    /// 「基本」のキー: このページ群が尋ねるもの（docs/ux.md の 4.5）。git が出す形（小文字）で。
    private static let basicKeys = [
        "user.name", "user.email", "commit.gpgsign", "gpg.format", "user.signingkey", "commit.template",
        "amonhen.defaultbranch", "pull.rebase", "pull.ff", "fetch.prune", "rebase.autostash", "rebase.updaterefs",
        "merge.conflictstyle", "core.autocrlf", "core.filemode",
    ]

    private var rows: [ConfigRow] {
        let f = filter.trimmingCharacters(in: .whitespaces)
        var all = model.configRows
        switch rowsMode {
        case .all: break
        case .thisRepository: all = all.filter { $0.values["local"] != nil || $0.values["worktree"] != nil }
        case .basic:
            let byKey = Dictionary(all.map { ($0.key.lowercased(), $0) }, uniquingKeysWith: { a, _ in a })
            all = Self.basicKeys.map { byKey[$0] ?? ConfigRow(key: $0, effective: "", effectiveScope: "", values: [:], files: [:]) }
        }
        var shown = all
            .filter { f.isEmpty || $0.key.localizedCaseInsensitiveContains(f) || $0.values.values.contains { $0.localizedCaseInsensitiveContains(f) } }
            .sorted(using: sortOrder)
        if adding {
            shown.append(ConfigRow(key: Self.newRowID, effective: "", effectiveScope: "", values: [:], files: [:]))
        }
        return shown
    }

    var body: some View {
        GroupedPage {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Picker("", selection: $rowsMode) {
                    Text(L("Basic")).tag(RowsMode.basic)
                    Text(L("This Repository")).tag(RowsMode.thisRepository)
                    Text(L("All")).tag(RowsMode.all)
                }
                .pickerStyle(.segmented).labelsHidden().fixedSize()
                .help(L("Basic: the settings the pages here ask about, set or not. This Repository: the keys set in this repository or this worktree. All: every key git reads."))
                Picker("", selection: $columnsMode) {
                    Text(L("Combined")).tag(ColumnsMode.combined)
                    Text(L("Levels")).tag(ColumnsMode.levels)
                }
                .pickerStyle(.segmented).labelsHidden().fixedSize()
                .help(L("Combined: the value in effect and where it comes from. Levels: a column per source, the one in effect in bold."))
                Spacer()
                TextField(L("Search by key or value"), text: $filter)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 240)
            }
            Table(rows, selection: $selection, sortOrder: $sortOrder, columnCustomization: $columns) {
                TableColumn(L("Key"), value: \.key) { row in
                    if row.key == Self.newRowID {
                        TextField(L("Key"), text: $newKey)
                            .textFieldStyle(.plain)
                            .focused($focus, equals: .newKey)
                            .onSubmit { focus = .newValue }
                            .onExitCommand { cancelAdding() }
                    } else {
                        Text(row.key)
                    }
                }
                .width(min: 140, ideal: 220)
                .customizationID("key")
                .disabledCustomizationBehavior(.all)
                TableColumn(L("Value in effect"), value: \.effective) { row in
                    // まとめての表では、ここが「このリポジトリ」の値の欄にもなる。
                    if columnsMode == .combined && isEditing(row) {
                        valueField(row)
                    } else if row.effectiveScope.isEmpty {
                        // どこにも設定がない（「基本」の未設定のキー）: 空の値とは違うので、何も出さない。
                        Text("")
                    } else {
                        valueText(row.effective)
                            .help(row.effectiveScope.isEmpty ? "" : "\(scopeLabel(row.effectiveScope, host: model.host)) · \(row.files[row.effectiveScope] ?? "")")
                    }
                }
                .width(min: 100, ideal: 160)
                .customizationID("effective")
                .disabledCustomizationBehavior(.all)
                if columnsMode == .combined {
                    TableColumn(L("Source"), value: \.effectiveScope) { row in
                        Text(row.effectiveScope.isEmpty ? L("git’s default") : scopeLabel(row.effectiveScope, host: model.host))
                            .foregroundStyle(row.effectiveScope.isEmpty ? .secondary : .primary)
                            .help(row.effectiveScope.isEmpty ? "" : "\(row.effectiveScope) · \(row.files[row.effectiveScope] ?? "")")
                    }
                    .width(min: 110, ideal: 170)
                    .customizationID("source")
                    .disabledCustomizationBehavior(.all)
                } else {
                    TableColumn(header("system"), value: \.system) { row in cell(row, scope: "system") }
                        .width(min: 80, ideal: 120)
                        .customizationID("system")
                    TableColumn(header("global"), value: \.global) { row in cell(row, scope: "global") }
                        .width(min: 80, ideal: 140)
                        .customizationID("global")
                    TableColumn(header("local"), value: \.local) { row in
                        if isEditing(row) { valueField(row) } else { cell(row, scope: "local") }
                    }
                    .width(min: 80, ideal: 140)
                    .customizationID("local")
                    .disabledCustomizationBehavior(.all)
                    TableColumn(header("worktree"), value: \.worktree) { row in cell(row, scope: "worktree") }
                        .width(min: 80, ideal: 120)
                        .customizationID("worktree")
                }
            }
            .contextMenu(forSelectionType: String.self) { keys in
                let keys = keys.filter { $0 != Self.newRowID }
                Button(L("Edit in This Repository")) { keys.first.map(beginEditing) }.disabled(keys.count != 1)
                Button(L("Remove from This Repository")) { for k in keys where model.local(k) != nil { model.set(k, nil) } }
                    .disabled(!keys.contains { model.local($0) != nil })
                Divider()
                Button(L("Copy")) { copy(keys) }.disabled(keys.isEmpty)
            } primaryAction: { keys in
                if keys.count == 1, let key = keys.first, key != Self.newRowID { beginEditing(key) }
            }
            .copyable([copyText(selection)])
            .onAppear { if sortOrder.isEmpty { sortOrder = [KeyPathComparator(\.key)] } }
            .onChange(of: focus) { _, now in
                // 欄から離れたら、編集中の値は書き、足している途中の行はそのまま（また戻って続けられる）。
                if now != .value, editing != nil { commitEditing() }
            }
            HStack {
                Button { beginAdding() } label: { Image(systemName: "plus") }
                    .help(L("Add a key to this repository"))
                    .disabled(adding)
                Spacer()
            }
        }
        }
    }

    /// 段の列の見出し: 4.3 の出どころの言葉を、文の頭だけ大文字に。
    private func header(_ scope: String) -> String {
        let label = scopeLabel(scope, host: model.host)
        return label.prefix(1).uppercased() + label.dropFirst()
    }

    /// 段の列のセル。効いている段なら太字。ツールチップに git の呼び名（global など）とファイル。
    @ViewBuilder private func cell(_ row: ConfigRow, scope: String) -> some View {
        if let value = row.values[scope] {
            valueText(value)
                .fontWeight(row.effectiveScope == scope ? .bold : .regular)
                .help("\(scope) · \(row.files[scope] ?? "")")
        } else {
            Text("")
        }
    }

    /// 値の見せ方: 同じ段に複数あれば 1 行ずつ（行の高さが伸びる）、明示的に空の値は「(empty)」。
    private func valueText(_ value: String) -> Text {
        let lines = value.split(separator: "\n", omittingEmptySubsequences: false)
        return lines.map { $0.isEmpty ? Text(L("(empty)")).foregroundStyle(.secondary) : Text($0) }
            .reduce(nil) { (acc: Text?, t: Text) in acc.map { $0 + Text("\n") + t } ?? t } ?? Text("")
    }

    /// 足している途中の行か、編集中の行か。
    private func isEditing(_ row: ConfigRow) -> Bool { row.key == Self.newRowID || editing == row.key }

    /// 「このリポジトリ」の値の欄（足している途中の行と、編集中の行）。
    @ViewBuilder private func valueField(_ row: ConfigRow) -> some View {
        if row.key == Self.newRowID {
            TextField(L("Value"), text: $draft)
                .textFieldStyle(.plain)
                .focused($focus, equals: .newValue)
                .onSubmit { commitAdding() }
                .onExitCommand { cancelAdding() }
        } else {
            TextField("", text: $draft)
                .textFieldStyle(.plain)
                .focused($focus, equals: .value)
                .onSubmit { commitEditing() }
                .onExitCommand { editing = nil }
        }
    }

    private func beginEditing(_ key: String) {
        cancelAdding()
        if editing != nil, editing != key { commitEditing() }
        draft = model.local(key) ?? ""
        editing = key
        selection = [key]
        DispatchQueue.main.async { focus = .value }
    }

    private func commitEditing() {
        guard let key = editing else { return }
        editing = nil
        let value = draft.trimmingCharacters(in: .whitespaces)
        if value != (model.local(key) ?? "") { model.set(key, value) }
    }

    private func beginAdding() {
        if editing != nil { commitEditing() }
        newKey = ""
        draft = ""
        adding = true
        selection = [Self.newRowID]
        DispatchQueue.main.async { focus = .newKey }
    }

    private func commitAdding() {
        let key = newKey.trimmingCharacters(in: .whitespaces), value = draft.trimmingCharacters(in: .whitespaces)
        cancelAdding()
        guard !key.isEmpty, !value.isEmpty else { return }
        model.set(key, value)
        selection = [key]
    }

    private func cancelAdding() {
        adding = false
        newKey = ""
        if focus == .newKey || focus == .newValue { focus = nil }
    }

    /// 選んだ行を `key = value` の形に（効いている値。複数あれば 1 行ずつ。どこにも設定のない行は飛ばす）。
    private func copyText(_ keys: Set<String>) -> String {
        rows.filter { keys.contains($0.key) && $0.key != Self.newRowID && !$0.effectiveScope.isEmpty }
            .flatMap { row in row.effective.split(separator: "\n", omittingEmptySubsequences: false).map { "\(row.key) = \($0)" } }
            .joined(separator: "\n")
    }

    private func copy(_ keys: Set<String>) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(copyText(keys), forType: .string)
    }
}

// MARK: - 行の部品（docs/ux.md の 4 章）

/// git の真偽値を `true`・`false` にそろえる（`yes`・`on`・`1` も true）。
private func normalizedBool(_ value: String) -> String? {
    switch value.lowercased() {
    case "true", "yes", "on", "1", "": "true"
    case "false", "no", "off", "0": "false"
    default: nil
    }
}

extension RepositorySettingsModel {
    /// `key` の今効いている値の出どころの言い方と、そのファイル。どこにも書かれていなければ「git の既定」。
    func origin(of key: String) -> (label: String, file: String?) {
        guard let e = effective(key) else { return (L("git’s default"), nil) }
        var file = e.origin
        if file.hasPrefix("file:") { file = String(file.dropFirst(5)) }
        file = (file as NSString).abbreviatingWithTildeInPath
        return (scopeLabel(e.scope, host: host), file)
    }

    /// local の値を消す（上の値に戻る）。
    func revert(_ keys: [String]) {
        for key in keys where local(key) != nil { set(key, nil) }
    }
}

/// 出どころ（`git config` の scope）の言い方。行の右と git config の表で同じ言葉を使う。
func scopeLabel(_ scope: String, host: String?) -> String {
    switch scope {
    case "local": L("this repository")
    case "global": host.map { L("all repositories on %@", $0) } ?? L("all repositories")
    case "worktree": L("this worktree")
    case "system": L("system-wide")
    default: scope
    }
}

/// 行の右に出す、値の出どころ。
private struct SourceLabel: View {
    let model: RepositorySettingsModel
    let key: String
    var body: some View {
        let o = model.origin(of: key)
        Text(o.label)
            .font(.caption)
            .foregroundStyle(.secondary)
            .frame(width: 118, alignment: .trailing)
            .help(o.file ?? L("Nothing is set anywhere, so git’s own default applies."))
    }
}

/// 行の共通の形: 左に質問とその下に 1 文の説明、右に効いている値の欄、このリポジトリが上書きしている間だけ ↩︎、
/// 最後に出どころ。その横の ⓘ で、キーと値とファイル、上書きしなければ効く値が吹き出しに出る。
private struct SettingRow<Control: View>: View {
    let model: RepositorySettingsModel
    let question: String
    let help: String
    /// この行が読み書きするキー。出どころは先頭のキーのもの。↩︎ はすべての local を消す。
    let keys: [String]
    /// 生の値を人の言葉にする（吹き出しの表示に使う）。
    var describe: (String) -> String = { $0 }
    /// 使えないときの理由（古い git など）。あれば欄を灰色にして、下に出す。
    var disabledReason: String?
    @ViewBuilder let control: () -> Control
    @State private var showingDetails = false

    private var overriding: Bool { keys.contains { model.local($0) != nil } }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(question)
                Spacer()
                control().disabled(disabledReason != nil)
                if overriding {
                    Button { model.revert(keys) } label: { Image(systemName: "arrow.uturn.backward") }
                        .buttonStyle(.borderless)
                        .help(L("Stop overriding here. The value above takes over."))
                } else {
                    Color.clear.frame(width: 20, height: 1)
                }
                SourceLabel(model: model, key: keys[0])
                Button { showingDetails.toggle() } label: { Image(systemName: "info.circle") }
                    .buttonStyle(.borderless)
                    .help(L("Where the value comes from"))
                    .popover(isPresented: $showingDetails, arrowEdge: .trailing) { details }
            }
            if !help.isEmpty {
                Text(help).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.horizontal, 8)
            }
            if let disabledReason {
                Text("⚠ " + disabledReason).foregroundStyle(.orange).padding(.horizontal, 8)
            }
        }
        .padding(.vertical, 6)
    }

    private var details: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(keys, id: \.self) { key in
                VStack(alignment: .leading, spacing: 2) {
                    if let e = model.effective(key) {
                        Text("\(key) = \(e.value)").font(.system(.body, design: .monospaced))
                        Text(model.origin(of: key).file.map { L("%@ (%@)", model.origin(of: key).label, $0) } ?? model.origin(of: key).label)
                            .foregroundStyle(.secondary)
                    } else {
                        Text(L("%@ is not set", key)).font(.system(.body, design: .monospaced))
                        Text(L("git’s default")).foregroundStyle(.secondary)
                    }
                    if model.local(key) != nil, let above = model.inherited(key) {
                        Text(L("Without the override here: %@", describe(above.value))).foregroundStyle(.secondary)
                    }
                }
            }
        }
        .font(.callout)
        .textSelection(.enabled)
        .padding(12)
        .frame(minWidth: 240, alignment: .leading)
    }
}

/// 選ぶ設定。欄には効いている値（なければ git の既定）を出す。
private struct ChoiceRow: View {
    let model: RepositorySettingsModel
    let question: String
    let help: String
    let key: String
    /// 生の値と、その言い方。
    let options: [(value: String, label: String)]
    /// git の既定（生の値）。
    let defaultValue: String
    var isBool = false
    var disabledReason: String?

    private func label(_ value: String) -> String {
        let v = isBool ? (normalizedBool(value) ?? value) : value.lowercased()
        return options.first { $0.value == v }?.label ?? value
    }

    private var current: String {
        guard let value = model.effective(key)?.value else { return defaultValue }
        return isBool ? (normalizedBool(value) ?? value) : value.lowercased()
    }

    var body: some View {
        SettingRow(model: model, question: question, help: help, keys: [key], describe: label, disabledReason: disabledReason) {
            Picker("", selection: Binding(get: { current }, set: { model.set(key, $0) })) {
                ForEach(options, id: \.value) { Text($0.label).tag($0.value) }
                if !options.contains(where: { $0.value == current }) {
                    Text(current).tag(current)
                }
            }
            .labelsHidden()
            .fixedSize()
        }
    }
}

/// はい／いいえの設定。
private struct SwitchRow: View {
    let model: RepositorySettingsModel
    let question: String
    let help: String
    let key: String
    let defaultOn: Bool
    var disabledReason: String?

    var body: some View {
        SettingRow(model: model, question: question, help: help, keys: [key],
                   describe: { normalizedBool($0) == "true" ? L("On") : L("Off") }, disabledReason: disabledReason) {
            Toggle("", isOn: Binding(
                get: { model.effective(key).map { normalizedBool($0.value) == "true" } ?? defaultOn },
                set: { model.set(key, $0 ? "true" : "false") }
            ))
            .labelsHidden()
            .toggleStyle(.switch)
            .controlSize(.small)
        }
    }
}

/// 文字の設定。欄から離れたときか Return で書く。空にすると上書きを消す。
private struct TextRow: View {
    let model: RepositorySettingsModel
    let question: String
    let help: String
    let key: String
    var placeholder = ""
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        SettingRow(model: model, question: question, help: help, keys: [key]) {
            TextField("", text: $text, prompt: Text(model.effective(key)?.value ?? placeholder))
                .labelsHidden()
                .frame(width: 260)
                .focused($focused)
                .onSubmit { commit() }
                .onChange(of: focused) { _, now in if !now { commit() } }
                .onAppear { text = model.local(key) ?? "" }
                .onChange(of: model.local(key)) { _, new in text = new ?? "" }
        }
    }

    private func commit() {
        let value = text.trimmingCharacters(in: .whitespaces)
        if value != (model.local(key) ?? "") { model.set(key, value.isEmpty ? nil : value) }
    }
}

/// pull のとき、どう取り込むか。`pull.rebase` と `pull.ff` の 2 つで決まる（コアの `syncDefaults` と同じ読み方）。
private struct PullMethodRow: View {
    let model: RepositorySettingsModel

    private enum Method: String { case merge, rebase, ffOnly }

    private func method(rebase: String?, ff: String?) -> Method {
        if ff?.lowercased() == "only" { return .ffOnly }
        if let r = rebase?.lowercased(), ["true", "yes", "on", "1", "merges", "interactive"].contains(r) { return .rebase }
        return .merge
    }

    private var current: Method { method(rebase: model.effective("pull.rebase")?.value, ff: model.effective("pull.ff")?.value) }
    /// 出どころに出すのは、取り込み方を決めているほうのキー。
    private var decidingKey: String { model.effective("pull.ff")?.value.lowercased() == "only" ? "pull.ff" : "pull.rebase" }

    private func label(_ m: Method) -> String {
        switch m {
        case .merge: L("Merge")
        case .rebase: L("Rebase")
        case .ffOnly: L("Fast-forward")
        }
    }

    private func choose(_ m: Method) {
        // fast-forward だけ → pull.ff=only。それ以外は pull.ff の only を消してから pull.rebase を書く。
        if model.local("pull.ff")?.lowercased() == "only" { model.set("pull.ff", nil) }
        switch m {
        case .merge: model.set("pull.rebase", "false")
        case .rebase: model.set("pull.rebase", "true")
        case .ffOnly:
            // pull.rebase=true が残っていると、どちらが勝つかが git の版で変わる。両方書いて曖昧さをなくす。
            model.set("pull.rebase", "false")
            model.set("pull.ff", "only")
        }
    }

    var body: some View {
        SettingRow(
            model: model, question: L("When pulling, how are the changes brought in?"),
            help: "",
            keys: [decidingKey, decidingKey == "pull.ff" ? "pull.rebase" : "pull.ff"]
        ) {
            Picker("", selection: Binding(get: { current }, set: { choose($0) })) {
                ForEach([Method.merge, .rebase, .ffOnly], id: \.self) { Text(label($0)).tag($0) }
            }
            .labelsHidden()
            .fixedSize()
        }
    }
}

// MARK: - ページ（docs/ux.md の 4.5）

/// 左の一覧の項目。組はなく、よく変えるものを先に、たまに見る 2 つを最後に。
enum SettingsPage: String, CaseIterable, Identifiable {
    case author, commits, operations, branches, workingCopy, remotes, hooks, config
    var id: String { rawValue }

    var title: String {
        switch self {
        case .author: L("Author and Signing")
        case .commits: L("Commits")
        case .operations: L("Operations")
        case .branches: L("Branches")
        case .workingCopy: "Working Copy"
        case .remotes: L("Remotes")
        case .hooks: "Hooks · LFS"
        case .config: "git config"
        }
    }
}

/// 既定のブランチ（Amon Hen の `amonhen.defaultBranch`。なければ判定したもの）。
private struct DefaultBranchRow: View {
    let model: RepositorySettingsModel
    private let key = "amonhen.defaultBranch"

    var body: some View {
        let chosen = model.local(key)
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(L("Default branch"))
                Spacer()
                Picker("", selection: Binding(get: { chosen ?? "" }, set: { model.set(key, $0.isEmpty ? nil : $0) })) {
                    Text(model.detectedBase.map { L("Detected (%@)", $0) } ?? L("Detected (none)")).tag("")
                    ForEach(model.localBranches, id: \.self) { Text($0).tag($0) }
                    if let chosen, !model.localBranches.contains(chosen) { Text(chosen).tag(chosen) }
                }
                .labelsHidden().fixedSize()
                if chosen != nil {
                    Button { model.set(key, nil) } label: { Image(systemName: "arrow.uturn.backward") }
                        .buttonStyle(.borderless).help(L("Go back to the detected branch."))
                } else {
                    Color.clear.frame(width: 20, height: 1)
                }
                Text(chosen == nil ? L("detected") : L("this repository"))
                    .font(.caption).foregroundStyle(.secondary).frame(width: 118, alignment: .trailing)
                Color.clear.frame(width: 16, height: 1)
            }
            Text(L("Branches measures the others against it. Detected from origin/HEAD, main or master; choose one when there is none."))
                .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.horizontal, 8)
            if let chosen, !model.localBranches.contains(chosen) {
                Text("⚠ " + L("There is no branch named %@, so the detected branch is used.", chosen)).foregroundStyle(.orange).padding(.horizontal, 8)
            }
        }
        .padding(.vertical, 6)
    }
}

private struct BranchesPage: View {
    let model: RepositorySettingsModel
    var body: some View {
        Form {
            Section {
                DefaultBranchRow(model: model)
            }
        }
        .formStyle(.grouped)
    }
}

private struct AuthorSigningPage: View {
    let model: RepositorySettingsModel
    var body: some View {
        let format = model.effective("gpg.format")?.value.lowercased() ?? "openpgp"
        Form {
            Section {
                TextRow(model: model, question: L("Name"), help: "", key: "user.name")
                TextRow(model: model, question: L("Email"), help: "", key: "user.email")
            } header: {
                Text(L("Author"))
            }
            Section {
                SwitchRow(model: model, question: L("Sign commits"), help: "", key: "commit.gpgsign", defaultOn: false)
                ChoiceRow(model: model, question: L("Signature format"), help: "", key: "gpg.format",
                          options: [("openpgp", "OpenPGP"), ("ssh", "SSH"), ("x509", "X.509")], defaultValue: "openpgp")
                TextRow(model: model, question: L("Key"),
                        help: format == "ssh" ? L("The SSH public key’s path, or the key itself.") : L("The key’s fingerprint or ID; empty picks the key for your email."),
                        key: "user.signingkey", placeholder: format == "ssh" ? "~/.ssh/id_ed25519.pub" : L("Key for your email"))
            } header: {
                Text(L("Signing"))
            }
        }
        .formStyle(.grouped)
    }
}

private struct CommitsPage: View {
    let model: RepositorySettingsModel
    @State private var preview: String?

    var body: some View {
        Form {
            Section {
                TextRow(model: model, question: L("Message template"), help: "", key: "commit.template", placeholder: L("None"))
                if let preview, !preview.isEmpty {
                    Text(preview).font(.system(.body, design: .monospaced)).foregroundStyle(.secondary).lineLimit(8).textSelection(.enabled)
                }
            } header: {
                Text(L("Message"))
            }
        }
        .formStyle(.grouped)
        .task(id: model.effective("commit.template")?.value) { preview = await templatePreview() }
    }

    /// テンプレートの中身（手元のリポジトリだけ。SSH 先は読まない）。
    private func templatePreview() async -> String? {
        guard model.host == nil, var path = model.effective("commit.template")?.value, !path.isEmpty else { return nil }
        path = (path as NSString).expandingTildeInPath
        if !path.hasPrefix("/") { path = (RepositoryLocation.path(model.location) as NSString).appendingPathComponent(path) }
        return (try? String(contentsOfFile: path, encoding: .utf8)).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
    }
}

/// Pull・Fetch・Rebase・Merge の設定。
private struct OperationsPage: View {
    let model: RepositorySettingsModel
    var body: some View {
        let tooOld = model.updateRefsSupported ? nil : L("Needs git 2.38 or later. This repository’s git is %@%@.", model.gitVersion ?? "?", model.host.map { " (\($0))" } ?? "")
        Form {
            Section("Pull") {
                PullMethodRow(model: model)
            }
            Section("Fetch") {
                SwitchRow(model: model, question: L("Remove remote-tracking branches deleted on the remote"), help: "", key: "fetch.prune", defaultOn: false)
            }
            Section("Rebase") {
                SwitchRow(model: model, question: L("Stash changes before rebasing"), help: L("History edits always stash, whatever this says."), key: "rebase.autoStash", defaultOn: false)
                ChoiceRow(model: model, question: L("Move stacked branches along when rebasing or editing History"),
                          help: L("Branches whose tips are among the rewritten commits. Undo puts them back."),
                          key: "rebase.updateRefs", options: [("false", L("Leave them")), ("true", L("Move them along"))], defaultValue: "false", isBool: true, disabledReason: tooOld)
            }
            Section("Merge") {
                ChoiceRow(model: model, question: L("How conflicts are written in files"), help: L("With the base (diff3, zdiff3) the merge editor resolves one-sided conflicts by itself. Amon Hen recommends zdiff3."), key: "merge.conflictStyle",
                          options: [("merge", L("Both sides")), ("diff3", L("Both sides and the common base")), ("zdiff3", L("Both sides and the base, shortened"))], defaultValue: "merge")
            }
        }
        .formStyle(.grouped)
    }
}

private struct WorkingCopyPage: View {
    @Bindable var model: RepositorySettingsModel
    var body: some View {
        Form {
            Section {
                ChoiceRow(model: model, question: L("Line endings"), help: "", key: "core.autocrlf",
                          options: [("false", L("Keep as committed")), ("true", L("CRLF in the working tree, LF in commits")), ("input", L("LF in commits, no change on checkout"))],
                          defaultValue: "false", isBool: false)
                SwitchRow(model: model, question: L("Track changes to the executable bit"), help: "", key: "core.fileMode", defaultOn: true)
            }
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    Text(L(".git/info/exclude, one pattern per line. Shared patterns go in .gitignore."))
                        .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.horizontal, 8)
                    TextEditor(text: $model.exclude)
                        .font(.system(.body, design: .monospaced))
                        .scrollContentBackground(.hidden)
                        .frame(minHeight: 320)
                    HStack {
                        Spacer()
                        Button("Revert") { model.load() }.disabled(!model.ignoreChanged)
                        Button(L("Save")) { model.saveIgnore() }.disabled(!model.ignoreChanged).keyboardShortcut("s")
                    }
                }
                .padding(.vertical, 6)
            } header: {
                Text(L("Ignore in this repository only"))
            }
        }
        .formStyle(.grouped)
    }
}

/// hooks と LFS。
private struct HooksLFSPage: View {
    let model: RepositorySettingsModel
    var body: some View {
        Form {
            Section {
                if model.hooks.isEmpty {
                    Text(model.hooksPath.map { $0.hasPrefix("/") || $0.hasPrefix("~") ? L("core.hooksPath (%@) is outside the working tree, so its contents can’t be shown.", $0) : L("None") } ?? L("None"))
                        .foregroundStyle(.secondary)
                }
                ForEach(model.hooks) { hook in
                    HStack {
                        Text(hook.name).font(.system(.body, design: .monospaced))
                        Spacer()
                        Text(hook.enabled ? L("Enabled") : L("Not executable (no chmod +x)"))
                            .foregroundStyle(hook.enabled ? Color.green : Color.secondary)
                    }
                }
            } header: {
                Text("Hooks")
            } footer: {
                Text(L("Location: %@. .sample files aren’t counted. git runs these on commit, push and so on.", model.hooksPath.map { "core.hooksPath = \($0)" } ?? ".git/hooks"))
                    .foregroundStyle(.secondary)
            }
            Section {
                LabeledContent("Git LFS", value: model.lfs ? L("Installed (filter.lfs is set)") : L("Not installed"))
                if model.lfsPatterns.isEmpty {
                    Text(L("No LFS patterns in .gitattributes.")).foregroundStyle(.secondary)
                }
                ForEach(model.lfsPatterns, id: \.self) { Text($0).font(.system(.body, design: .monospaced)) }
            } header: {
                Text("LFS")
            } footer: {
                if !model.lfs && !model.lfsPatterns.isEmpty {
                    Text(L("There are LFS patterns, but git lfs isn’t installed. Install it with brew install git-lfs and git lfs install."))
                        .foregroundStyle(.orange)
                }
            }
        }
        .formStyle(.grouped)
    }
}

private struct RepositorySettingsView: View {
    let model: RepositorySettingsModel
    @State var page: SettingsPage

    var body: some View {
        HStack(spacing: 0) {
            VStack(spacing: 0) {
                List(selection: Binding(get: { Optional(page) }, set: { if let p = $0 { page = p } })) {
                    ForEach(SettingsPage.allCases) { Text($0.title).tag($0) }
                }
                .listStyle(.sidebar)
                // このリポジトリを動かしている git。機械（この Mac か SSH 先）のものなので、変えるのはその機械の設定で。
                Button {
                    if model.host == nil { SettingsWindowController.shared.show() }
                } label: {
                    Text("git \(model.gitVersion ?? "?") · \(model.host ?? L("This Mac"))")
                        .font(.caption).foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(model.host.map { L("The git on %@ runs this repository. Every repository on that host uses it.", $0) }
                    ?? L("This Mac’s git runs this repository. Choose it in Settings > Git."))
                .padding(.horizontal, 14).padding(.vertical, 8)
            }
            .frame(width: 200)
            Divider()
            VStack(spacing: 0) {
                content
                if let error = model.error {
                    HStack {
                        Text(error).foregroundStyle(.red).lineLimit(2)
                        Spacer()
                    }
                    .padding([.horizontal, .bottom])
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        }
        .background(Color(nsColor: .windowBackgroundColor))
    }

    @ViewBuilder private var content: some View {
        switch page {
        case .author: AuthorSigningPage(model: model)
        case .commits: CommitsPage(model: model)
        case .operations: OperationsPage(model: model)
        case .branches: BranchesPage(model: model)
        case .workingCopy: WorkingCopyPage(model: model)
        case .remotes: RemotesTab(model: model)
        case .hooks: HooksLFSPage(model: model)
        case .config: AllSettingsTab(model: model)
        }
    }
}

/// リポジトリの設定の画面（サイドバーの Workspace の Settings）。
final class RepositorySettingsPage: NSHostingController<AnyView> {
    let model: RepositorySettingsModel

    init(repository: Repository, location: String) {
        model = RepositorySettingsModel(repository: repository, location: location)
        let page = AppDelegate.option("--settings-page").flatMap(SettingsPage.init) ?? .author
        super.init(rootView: AnyView(RepositorySettingsView(model: model, page: page)))
    }

    @MainActor required dynamic init?(coder: NSCoder) { fatalError() }

    /// 開くたびに読み直す（端末などで変えたかもしれない）。
    override func viewWillAppear() {
        super.viewWillAppear()
        model.load()
    }
}
