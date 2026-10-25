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

/// 署名に使える鍵（コアの `signingKeys`）。
struct SigningKey: Decodable, Hashable, Identifiable {
    let id: String
    let label: String
    let emails: [String]
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
    /// git が知っている設定の変数の名前（設定の有無は問わない。読めなければ空）。
    private(set) var knownKeys: [String] = []
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
    /// git config の表で「−」などで最後に消したキーと値（表の下の「戻す」の 1 行）。次の書き換えで忘れる。
    var removedConfig: [(key: String, value: String)] = []
    /// このリポジトリ専用のコミットのテンプレート: 中身（ファイルがなければ nil）と、`commit.template` に書くパス。
    private(set) var commitTemplate: (text: String?, path: String)?

    /// 専用のテンプレートを書く（`commit.template` もそれに向く）。
    func saveCommitTemplate(_ text: String) {
        run(["op": "saveCommitTemplate", "text": text])
    }

    /// 署名の形式ごとに集めた鍵（`user.email` の鍵を先に）。コアが集める（SSH 先のリポジトリなら SSH 先で）。
    private(set) var signingKeys: [String: [SigningKey]] = [:]

    /// `format`（`openpgp`・`ssh`・`x509`）の鍵を集める。集められなければ空。
    func loadSigningKeys(_ format: String) {
        let repository = self.repository
        Task {
            struct Keys: Decodable { let keys: [SigningKey] }
            let keys = (try? await Task.detached(priority: .userInitiated) {
                try repository.info(["op": "signingKeys", "format": format], as: Keys.self)
            }.value)?.keys ?? []
            let email = effective("user.email")?.value.lowercased() ?? ""
            signingKeys[format] = keys.sorted { a, b in
                let am = !email.isEmpty && a.emails.contains { $0.lowercased() == email }
                let bm = !email.isEmpty && b.emails.contains { $0.lowercased() == email }
                return am != bm ? am : false
            }
        }
    }

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
            struct Keys: Decodable { let keys: [String] }
            struct Remotes: Decodable { let remotes: [RemoteEntry] }
            struct Ignore: Decodable { let gitignore: String; let exclude: String }
            struct Hooks: Decodable { let hooksPath: String?; let hooks: [HookEntry]; let lfs: Bool; let lfsPatterns: [String] }
            struct Version: Decodable { let version: String?; let updateRefs: Bool }
            struct Template: Decodable { let text: String?; let path: String }
            struct Groups: Decodable { struct Branch: Decodable { let name: String }; let base: String?; let branches: [Branch] }
            do {
                let (e, k, r, i, h, v, g, t) = try await Task.detached(priority: .userInitiated) {
                    (
                        try repository.info(["op": "configList"], as: Entries.self),
                        try? repository.info(["op": "configKeys"], as: Keys.self),
                        try repository.info(["op": "remotes"], as: Remotes.self),
                        try repository.info(["op": "ignoreFiles"], as: Ignore.self),
                        try repository.info(["op": "hooksAndLfs"], as: Hooks.self),
                        try repository.info(["op": "gitVersion"], as: Version.self),
                        try? repository.info(["op": "branchGroups"], as: Groups.self),
                        try? repository.info(["op": "commitTemplate"], as: Template.self)
                    )
                }.value
                commitTemplate = t.map { ($0.text, $0.path) }
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
                knownKeys = k?.keys ?? []
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

    /// 今効いている値（後に出るものほど優先）と、その設定元。
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

/// git config の表の 1 行: 1 つのキーと、設定元（段）ごとの値（docs/ux.md の 4.5）。同じ段に同じキーが
/// 複数あれば（`remote.origin.fetch` など）、改行で並べる。
nonisolated struct ConfigRow: Identifiable, Hashable, Sendable {
    let key: String
    /// 今効いている値（後の段ほど優先）と、その段。どの段にもなければ空。
    let effective: String
    let effectiveScope: String
    /// 段ごとの値と、その段のファイル。
    let values: [String: String]
    let files: [String: String]
    /// 組（section か subsection）の見出しの行。`key` はその組までのキー（`remote`・`remote.origin`）。
    var isGroup = false
    /// 表のキーの列に出す字。葉は組からの相対の名前（`remote.origin.url` なら `url`）、組はその名前。
    var label: String?
    var id: String { key }
    /// キーの 3 つの部分。`section.name` か `section.subsection.name` で、subsection にはドットが入りうる
    /// （`url.https://….insteadOf`）ので、最初と最後のドットで切る。
    var parts: (section: String, subsection: String?, name: String) {
        guard let first = key.firstIndex(of: "."), let last = key.lastIndex(of: ".") else { return (key, nil, "") }
        let section = String(key[..<first]), name = String(key[key.index(after: last)...])
        let subsection = first == last ? nil : String(key[key.index(after: first)..<last])
        return (section, subsection, name)
    }
    var system: String { values["system"] ?? "" }
    var global: String { values["global"] ?? "" }
    var local: String { values["local"] ?? "" }
    var worktree: String { values["worktree"] ?? "" }
}

extension RepositorySettingsModel {
    /// git が知っている変数の綴り（`core.fileMode`）を、小文字のキーから引く。`git config --list` は小文字で出す。
    var canonicalKeys: [String: String] {
        Dictionary(knownKeys.map { ($0.lowercased(), $0) }, uniquingKeysWith: { a, _ in a })
    }

    /// `entries` をキーごとに 1 行にまとめる。順は git が出した順（表はこれを見出しで並べ替える）。
    /// キーは git が知っている綴り（`core.fileMode`）にそろえる。
    var configRows: [ConfigRow] {
        let canonical = canonicalKeys
        var order: [String] = []
        var byKey: [String: [ConfigEntry]] = [:]
        for e in entries {
            let key = canonical[e.key.lowercased()] ?? e.key
            if byKey[key] == nil { order.append(key) }
            byKey[key, default: []].append(e)
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
/// 「+」で新しい行が出て、キーと値を入れる（キーと値がそろわないまま離れたら消える）。「−」と ⌫ で選んだ行の
/// このリポジトリの値を消す。表からの書き換え（編集・追加・削除）はすべてウィンドウの UndoManager に逆の書き換えと
/// 一緒に登録するので、⌘Z／⇧⌘Z で何段でも行き来できる（ページを開いている間）。最後の削除は下の 1 行にも出て、
/// その「戻す」は 1 段の ⌘Z と同じ。⌘C で選んだ行を `key = value` の形でコピーする。
/// 上の切り替えも Xcode と同じで、語も Xcode のまま（ページ名と同じく訳さない）: 行は Basic（ページが尋ねるキー。
/// 未設定でも出す）／Customized（このリポジトリで設定したもの）／All、列は Combined（効いている値と設定元）／Levels。
/// All は git が知っているすべての変数（未設定は空欄で、設定元は git の既定）。
/// 行は Xcode が分類ごとにまとめるのと同じく、キーのドットで開閉できる組にする（section、その下に subsection。
/// `DisclosureTableRow`。初めはすべて開いている）。
private struct AllSettingsTab: View {
    let model: RepositorySettingsModel
    @State private var filter = ""
    @State private var selection = Set<String>()
    /// 見出しで並べ替える。初めはキーの順（`onAppear` で入れる。`@State` の初期値にキーパスは置けない）。
    @State private var sortOrder: [KeyPathComparator<ConfigRow>] = []
    @State private var columns = TableColumnCustomization<ConfigRow>()
    /// 切り替えはアプリ全体で 1 つ（リポジトリをまたいで覚える）。初めは Customized。
    @AppStorage("configTableRows") private var rowsMode = RowsMode.customized
    @AppStorage("configTableColumns") private var columnsMode = ColumnsMode.levels
    /// 「このリポジトリ」の列を編集しているキーと、欄の中身。
    @State private var editing: String?
    @State private var draft = ""
    /// 「+」で足している途中（表の最後に、キーと値の欄だけの行が出る）。
    @State private var adding = false
    @State private var newKey = ""
    /// 閉じた section（初めはすべて開いている）。
    @State private var collapsed = Set<String>()
    @Environment(\.undoManager) private var undoManager
    @FocusState private var focus: Field?

    private enum Field: Hashable { case value, newKey, newValue }
    private enum RowsMode: String { case basic, customized, all }
    private enum ColumnsMode: String { case combined, levels }
    /// 足している途中の行の id（キーは空）。
    private static let newRowID = ""
    /// 「基本」のキー: このページ群が尋ねるもの（docs/ux.md の 4.5）。git が出す形（小文字）で。
    private static let basicKeys = [
        "user.name", "user.email", "commit.gpgsign", "gpg.format", "user.signingkey", "commit.template",
        "amonhen.defaultbranch", "pull.rebase", "pull.ff", "fetch.prune", "rebase.autostash", "rebase.updaterefs",
        "merge.conflictstyle", "core.autocrlf", "core.filemode",
    ]

    /// 組（section か subsection）: 見出しの行と、その中の行と組。
    private struct Group: Identifiable {
        let header: ConfigRow
        var rows: [ConfigRow] = []
        var groups: [Group] = []
        var id: String { header.key }
    }

    /// 出す行を、キーのドットで組にしたもの（section の下に subsection）。組の順はキーの並びと同じ向き、
    /// 中の行の順は並べ替えのまま。
    private var groups: [Group] {
        let f = filter.trimmingCharacters(in: .whitespaces)
        var all = model.configRows
        let blank = { (key: String) in ConfigRow(key: key, effective: "", effectiveScope: "", values: [:], files: [:]) }
        switch rowsMode {
        case .all:
            let set = Set(all.map { $0.key.lowercased() })
            all += model.knownKeys.filter { !set.contains($0.lowercased()) }.map(blank)
        case .customized: all = all.filter { $0.values["local"] != nil || $0.values["worktree"] != nil }
        case .basic:
            let byKey = Dictionary(all.map { ($0.key.lowercased(), $0) }, uniquingKeysWith: { a, _ in a })
            let canonical = model.canonicalKeys
            all = Self.basicKeys.map { byKey[$0] ?? blank(canonical[$0] ?? $0) }
        }
        let sorted = all
            .filter { f.isEmpty || $0.key.localizedCaseInsensitiveContains(f) || $0.values.values.contains { $0.localizedCaseInsensitiveContains(f) } }
            .sorted(using: sortOrder)
        let reversed = sortOrder.first.map { $0.keyPath == \ConfigRow.key && $0.order == .reverse } ?? false
        let header = { (key: String, name: String) in
            ConfigRow(key: key, effective: "", effectiveScope: "", values: [:], files: [:], isGroup: true, label: name)
        }
        var sections: [String: Group] = [:]
        for var row in sorted {
            let p = row.parts
            row.label = p.name
            var section = sections[p.section] ?? Group(header: header(p.section, p.section))
            if let sub = p.subsection {
                let key = "\(p.section).\(sub)"
                if let i = section.groups.firstIndex(where: { $0.id == key }) {
                    section.groups[i].rows.append(row)
                } else {
                    section.groups.append(Group(header: header(key, sub), rows: [row]))
                }
            } else {
                section.rows.append(row)
            }
            sections[p.section] = section
        }
        let order = { (a: String, b: String) in reversed ? a > b : a < b }
        return sections.values.sorted { order($0.id, $1.id) }.map { section in
            var section = section
            section.groups.sort { order($0.id, $1.id) }
            return section
        }
    }

    /// 表に出ている行（見出しも含む。コピーの元）。
    private var rows: [ConfigRow] {
        groups.flatMap { [$0.header] + $0.rows + $0.groups.flatMap { [$0.header] + $0.rows } }
    }

    /// 組の見出しの行の id。
    private var groupIDs: Set<String> {
        Set(groups.flatMap { [$0.id] + $0.groups.map(\.id) })
    }

    var body: some View {
        GroupedPage {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Picker("", selection: $rowsMode) {
                    Text("Basic").tag(RowsMode.basic)
                    Text("Customized").tag(RowsMode.customized)
                    Text("All").tag(RowsMode.all)
                }
                .pickerStyle(.segmented).labelsHidden().fixedSize()
                .help(L("Basic: the settings the pages here ask about, set or not. Customized: the keys set in this repository or this worktree. All: every key git reads."))
                Picker("", selection: $columnsMode) {
                    Text("Combined").tag(ColumnsMode.combined)
                    Text("Levels").tag(ColumnsMode.levels)
                }
                .pickerStyle(.segmented).labelsHidden().fixedSize()
                .help(L("Combined: the value in effect and where it comes from. Levels: a column per source, the one in effect in bold."))
                Spacer()
                TextField(L("Search by key or value"), text: $filter)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 240)
            }
            Table(of: ConfigRow.self, selection: $selection, sortOrder: $sortOrder, columnCustomization: $columns) {
                TableColumn(L("Key"), value: \.key) { row in
                    if row.key == Self.newRowID {
                        TextField(L("Key"), text: $newKey)
                            .textFieldStyle(.plain)
                            .focused($focus, equals: .newKey)
                            .onSubmit { focus = .newValue }
                            .onExitCommand { cancelAdding() }
                    } else if row.isGroup {
                        Text(row.label ?? row.key).fontWeight(.bold)
                    } else {
                        Text(row.label ?? row.key).help(row.key)
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
                        Text(row.isGroup || row.key == Self.newRowID ? "" : row.effectiveScope.isEmpty ? L("git’s default") : scopeLabel(row.effectiveScope, host: model.host))
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
            } rows: {
                ForEach(groups) { section in
                    DisclosureTableRow(section.header, isExpanded: expanded(section.id)) {
                        ForEach(section.rows) { TableRow($0) }
                        ForEach(section.groups) { sub in
                            DisclosureTableRow(sub.header, isExpanded: expanded(sub.id)) {
                                ForEach(sub.rows) { TableRow($0) }
                            }
                        }
                    }
                }
                if adding {
                    TableRow(ConfigRow(key: Self.newRowID, effective: "", effectiveScope: "", values: [:], files: [:]))
                }
            }
            .contextMenu(forSelectionType: String.self) { keys in
                let groupIDs = self.groupIDs
                let keys = keys.filter { $0 != Self.newRowID && !groupIDs.contains($0) }
                Button(L("Edit in This Repository")) { keys.first.map(beginEditing) }.disabled(keys.count != 1)
                Button(L("Remove from This Repository")) { remove(keys) }
                    .disabled(!keys.contains { model.local($0) != nil })
                Divider()
                Button(L("Copy")) { copy(keys) }.disabled(keys.isEmpty)
            } primaryAction: { keys in
                if keys.count == 1, let key = keys.first, key != Self.newRowID, !groupIDs.contains(key) { beginEditing(key) }
            }
            .copyable([copyText(selection)])
            .onDeleteCommand { remove(selection) }
            .onAppear { if sortOrder.isEmpty { sortOrder = [KeyPathComparator(\.key)] } }
            .onChange(of: focus) { _, now in
                // 欄から離れたら、編集中の値は書く。足している途中の行は、キーと値がそろっていれば書き、なければ消す。
                if now != .value, editing != nil { commitEditing() }
                if now != .newKey, now != .newValue, adding { commitAdding() }
            }
            HStack {
                Button { beginAdding() } label: { Image(systemName: "plus") }
                    .help(L("Add a key to this repository"))
                    .disabled(adding)
                Button { remove(selection) } label: { Image(systemName: "minus") }
                    .help(L("Remove the selected keys from this repository (⌫)"))
                    .disabled(!selection.contains { model.local($0) != nil })
                if !model.removedConfig.isEmpty {
                    let r = model.removedConfig
                    Text(r.count == 1 ? L("Removed %@ from this repository", "\(r[0].key) = \(r[0].value)") : L("Removed %ld keys from this repository", r.count))
                        .foregroundStyle(.secondary).lineLimit(1).padding(.leading, 8)
                    Button(L("Restore")) { restore() }
                }
                Spacer()
            }
        }
        }
    }

    /// 段の列の見出し: 4.3 の設定元の言葉を、文の頭だけ大文字に。
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

    /// 組が開いているか（初めはすべて開いている）。
    private func expanded(_ id: String) -> Binding<Bool> {
        Binding(get: { !collapsed.contains(id) }, set: { open in if open { collapsed.remove(id) } else { collapsed.insert(id) } })
    }

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
        let old = model.local(key)
        if value != (old ?? "") {
            write([ConfigChange(key: key, old: old, new: value.isEmpty ? nil : value)],
                  name: value.isEmpty ? L("Remove %@", key) : L("Change %@", key))
        }
    }

    private func beginAdding() {
        if editing != nil { commitEditing() }
        newKey = ""
        draft = ""
        adding = true
        selection = [Self.newRowID]
        DispatchQueue.main.async { focus = .newKey }
    }

    /// 足している行を書く。キーと値がそろっていなければ、行を消すだけ。
    private func commitAdding() {
        let key = newKey.trimmingCharacters(in: .whitespaces), value = draft.trimmingCharacters(in: .whitespaces)
        cancelAdding()
        guard !key.isEmpty, !value.isEmpty else { return }
        write([ConfigChange(key: key, old: model.local(key), new: value)], name: L("Add %@", key))
        selection = [key]
    }

    /// 選んだ行のこのリポジトリの値を消す。最後に消したものは下の 1 行に出す。
    private func remove(_ keys: Set<String>) {
        let gone = keys.sorted().compactMap { k in model.local(k).map { ConfigChange(key: k, old: $0, new: nil) } }
        guard !gone.isEmpty else { return }
        write(gone, name: gone.count == 1 ? L("Remove %@", gone[0].key) : L("Remove %ld keys", gone.count), showRemoved: true)
    }

    /// 下の 1 行の「戻す」: 最後の削除を 1 段戻す（⌘Z と同じ）。
    private func restore() {
        if let undoManager, undoManager.canUndo {
            undoManager.undo()
        } else {
            let back = model.removedConfig.map { ConfigChange(key: $0.key, old: nil, new: $0.value) }
            write(back, name: L("Restore"))
        }
    }

    /// このリポジトリの値の書き換え 1 つ（`new` が nil なら消す）。
    private struct ConfigChange {
        let key: String
        let old: String?
        let new: String?
        var inverse: ConfigChange { ConfigChange(key: key, old: new, new: old) }
    }

    /// 表からの書き換えはすべてここを通す: 書いて、UndoManager に逆の書き換えを登録する。逆の中でまたその逆を
    /// 登録するので、⌘Z のあと ⇧⌘Z でやり直せる。`showRemoved` なら消したものを下の 1 行に出す（それ以外の
    /// 書き換えとやり直しでは消す）。
    private func write(_ changes: [ConfigChange], name: String, showRemoved: Bool = false) {
        Self.apply(changes, to: model, undoManager: undoManager, name: name, showRemoved: showRemoved)
    }

    private static func apply(_ changes: [ConfigChange], to model: RepositorySettingsModel, undoManager: UndoManager?, name: String, showRemoved: Bool) {
        for c in changes { model.set(c.key, c.new) }
        model.removedConfig = showRemoved ? changes.compactMap { c in c.old.map { (key: c.key, value: $0) } } : []
        undoManager?.registerUndo(withTarget: model) { m in
            apply(changes.map(\.inverse), to: m, undoManager: undoManager, name: name, showRemoved: false)
        }
        undoManager?.setActionName(name)
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
    /// `key` の今効いている値の設定元の言い方と、そのファイル。どこにも書かれていなければ「git の既定」。
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

/// 設定元（`git config` の scope）の言い方。行の右と git config の表で同じ言葉を使う。
func scopeLabel(_ scope: String, host: String?) -> String {
    switch scope {
    case "local": L("this repository")
    case "global": host.map { L("all repositories on %@", $0) } ?? L("all repositories")
    case "worktree": L("this worktree")
    case "system": L("system-wide")
    default: scope
    }
}

/// 行の右に出す、値の設定元。
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
/// 最後に設定元。その横の ⓘ で、キーと値とファイル、上書きしなければ効く値が吹き出しに出る。
private struct SettingRow<Control: View>: View {
    let model: RepositorySettingsModel
    let question: String
    let help: String
    /// この行が読み書きするキー。設定元は先頭のキーのもの。↩︎ はすべての local を消す。
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
    /// 設定元に出すのは、取り込み方を決めているほうのキー。
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
                SigningRow(model: model)
                SigningKeyRow(model: model, format: format)
            } header: {
                Text(L("Signing"))
            }
        }
        .formStyle(.grouped)
        .task { for f in SigningRow.formats { model.loadSigningKeys(f) } }
    }
}

/// 署名: 1 つの選択（署名しない／OpenPGP／SSH／X.509）で、`commit.gpgsign` と `gpg.format` を一緒に書く。
private struct SigningRow: View {
    let model: RepositorySettingsModel
    static let formats = ["openpgp", "ssh", "x509"]

    static func name(_ format: String) -> String {
        switch format {
        case "openpgp": "OpenPGP"
        case "ssh": "SSH"
        case "x509": "X.509"
        default: format
        }
    }

    private var signing: Bool { model.effective("commit.gpgsign").map { normalizedBool($0.value) == "true" } ?? false }
    private var format: String {
        let f = model.effective("gpg.format")?.value.lowercased() ?? "openpgp"
        return Self.formats.contains(f) ? f : "openpgp"
    }
    private var current: String { signing ? format : "" }

    private func label(_ value: String) -> String { value.isEmpty ? L("No signing") : Self.name(value) }

    private func choose(_ value: String) {
        if value.isEmpty {
            model.set("commit.gpgsign", "false")
        } else {
            model.set("commit.gpgsign", "true")
            if format != value || model.local("gpg.format") != nil { model.set("gpg.format", value) }
        }
    }

    var body: some View {
        SettingRow(model: model, question: L("Signing"), help: "", keys: ["commit.gpgsign", "gpg.format"],
                   describe: { normalizedBool($0) == "true" ? L("On") : normalizedBool($0) == "false" ? L("Off") : label($0.lowercased()) }) {
            Picker("", selection: Binding(get: { current }, set: { choose($0) })) {
                ForEach([""] + Self.formats, id: \.self) { Text(label($0)).tag($0) }
            }
            .labelsHidden()
            .fixedSize()
        }
    }
}

/// 鍵: コアが集めた鍵（OpenPGP・SSH・X.509 の種類ごと。`user.email` の鍵が先）から選ぶ。git の `user.signingkey` は
/// 1 つなので、鍵を選ぶとその種類を `gpg.format` にも書く。一覧にないものは「その他…」で入れる。
private struct SigningKeyRow: View {
    let model: RepositorySettingsModel
    let format: String
    private let key = "user.signingkey"
    private static let other = "\u{1}other"
    @State private var typing = false
    @State private var text = ""
    @FocusState private var focused: Bool

    private var current: String { model.effective(key)?.value ?? "" }
    /// 値 `id` の鍵の種類（一覧にあれば）。
    private func kind(of id: String) -> String? {
        SigningRow.formats.first { f in (model.signingKeys[f] ?? []).contains { $0.id == id } }
    }

    private func label(_ value: String) -> String {
        if value.isEmpty { return format == "openpgp" ? L("Key for your email") : L("None") }
        return kind(of: value).flatMap { f in model.signingKeys[f]?.first { $0.id == value }?.label } ?? value
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            SettingRow(model: model, question: L("Key"), help: L("The keys gpg, ~/.ssh, ssh-agent and gpgsm know, the ones for your email first. Choosing a key chooses its format too. Empty picks gpg’s key for your email."),
                       keys: [key], describe: label) {
                Picker("", selection: Binding(get: { typing ? Self.other : current }, set: { choose($0) })) {
                    Text(label("")).tag("")
                    ForEach(SigningRow.formats, id: \.self) { f in
                        if let keys = model.signingKeys[f], !keys.isEmpty {
                            Section(SigningRow.name(f)) { ForEach(keys) { Text($0.label).tag($0.id) } }
                        }
                    }
                    if !current.isEmpty, kind(of: current) == nil {
                        Text(current).tag(current)
                    }
                    Divider()
                    Text(L("Other…")).tag(Self.other)
                }
                .labelsHidden()
                .frame(maxWidth: 360)
            }
            if let k = kind(of: current), k != format {
                Text("⚠ " + L("This is an %1$@ key, but the format is %2$@.", SigningRow.name(k), SigningRow.name(format)))
                    .foregroundStyle(.orange).padding(.horizontal, 8)
            }
            if typing {
                TextField("", text: $text, prompt: Text(format == "ssh" ? "~/.ssh/id_ed25519.pub" : L("Fingerprint or key ID")))
                    .focused($focused)
                    .onSubmit { commit() }
                    .onExitCommand { typing = false }
                    .padding(.horizontal, 8)
                    .padding(.bottom, 6)
            }
        }
    }

    private func choose(_ value: String) {
        if value == Self.other {
            text = current
            typing = true
            DispatchQueue.main.async { focused = true }
        } else {
            typing = false
            model.set(key, value.isEmpty ? nil : value)
            // 選んだ鍵の種類が今の形式と違えば、形式も合わせる。
            if let f = kind(of: value), f != format { model.set("gpg.format", f) }
        }
    }

    private func commit() {
        typing = false
        let value = text.trimmingCharacters(in: .whitespaces)
        if value != current { model.set(key, value.isEmpty ? nil : value) }
    }
}

private struct CommitsPage: View {
    let model: RepositorySettingsModel
    @State private var preview: String?
    /// 専用のテンプレートの編集中の文。
    @State private var draft = ""

    private var choice: TemplateRow.Choice { TemplateRow.choice(model) }
    private var saved: String { model.commitTemplate?.text ?? "" }

    var body: some View {
        // 専用のテンプレートの編集欄は窓の高さいっぱいに（行と説明、ボタンの分を引いた残り）。
        GeometryReader { geometry in
        Form {
            Section {
                TemplateRow(model: model)
                switch choice {
                case .own:
                    VStack(alignment: .leading, spacing: 6) {
                        TextEditor(text: $draft)
                            .font(.system(.body, design: .monospaced))
                            .scrollContentBackground(.hidden)
                            .frame(height: max(160, geometry.size.height - 250))
                        HStack {
                            Spacer()
                            Button(L("Revert")) { draft = saved }.disabled(draft == saved)
                            Button(L("Save")) { model.saveCommitTemplate(draft) }.disabled(draft == saved).keyboardShortcut("s")
                        }
                    }
                    .padding(.vertical, 6)
                    .onAppear { draft = saved }
                    .onChange(of: saved) { _, new in draft = new }
                case .shared(let name):
                    let text = Settings.commitTemplates.first { $0.name == name }?.text ?? ""
                    VStack(alignment: .leading, spacing: 6) {
                        Text(text.trimmingCharacters(in: .whitespacesAndNewlines))
                            .font(.system(.body, design: .monospaced)).foregroundStyle(.secondary).lineLimit(12).textSelection(.enabled)
                        HStack {
                            Spacer()
                            Button(L("Edit in Settings > Git…")) { SettingsWindowController.shared.show() }
                            Button(L("Copy to This Repository")) {
                                model.saveCommitTemplate(text)
                                model.set(TemplateRow.libraryKey, nil)
                            }
                            .help(L("Makes a copy of this template for this repository alone, which git in a terminal uses too."))
                        }
                    }
                    .padding(.vertical, 6)
                case .file:
                    if let preview, !preview.isEmpty {
                        Text(preview).font(.system(.body, design: .monospaced)).foregroundStyle(.secondary).lineLimit(12).textSelection(.enabled)
                    }
                case .none:
                    EmptyView()
                }
            } header: {
                Text(L("Message"))
            }
        }
        .formStyle(.grouped)
        .task(id: model.effective(TemplateRow.fileKey)?.value ?? "") { preview = await filePreview() }
        }
    }

    /// ファイルのテンプレートの中身（手元のリポジトリだけ。SSH 先は読まない）。
    private func filePreview() async -> String? {
        guard model.host == nil, var path = model.effective(TemplateRow.fileKey)?.value, !path.isEmpty else { return nil }
        path = (path as NSString).expandingTildeInPath
        if !path.hasPrefix("/") { path = (RepositoryLocation.path(model.location) as NSString).appendingPathComponent(path) }
        return (try? String(contentsOfFile: path, encoding: .utf8)).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
    }
}

/// メッセージのテンプレート: なし／Settings > Git の共有のテンプレート（`amonhen.commitTemplate` に名前。Amon Hen の
/// コミット欄だけが使う）／このリポジトリ専用（`<git dir>/amonhen/commit-template` に置き、git の `commit.template` を
/// 向ける。端末の git も使う）／ファイル…（`commit.template`）。
private struct TemplateRow: View {
    let model: RepositorySettingsModel
    static let libraryKey = "amonhen.commitTemplate", fileKey = "commit.template"
    private static let pickFile = "\u{1}file", own = "\u{1}own"

    /// 今の選び。
    enum Choice: Equatable { case none, shared(String), own, file(String) }

    static func choice(_ model: RepositorySettingsModel) -> Choice {
        let templates = Settings.commitTemplates
        if let n = model.effective(libraryKey)?.value.nonEmpty, templates.contains(where: { $0.name == n }) { return .shared(n) }
        guard let file = model.effective(fileKey)?.value.nonEmpty else { return .none }
        return isOwn(file, model) ? .own : .file(file)
    }

    /// `path` がこのリポジトリ専用のテンプレートのファイルか。
    private static func isOwn(_ path: String, _ model: RepositorySettingsModel) -> Bool {
        path == model.commitTemplate?.path || path.hasSuffix("/amonhen/commit-template")
    }

    private var templates: [CommitTemplate] { Settings.commitTemplates }
    private var choice: Choice { Self.choice(model) }
    private var current: String {
        switch choice {
        case .none: ""
        case .shared(let n): "lib:" + n
        case .own: Self.own
        case .file(let f): "file:" + f
        }
    }
    private var decidingKey: String { if case .shared = choice { Self.libraryKey } else { Self.fileKey } }

    private func label(_ value: String) -> String {
        if value.hasPrefix("lib:") { return String(value.dropFirst(4)) }
        if value == Self.own { return L("This repository’s own") }
        if value.hasPrefix("file:") {
            let path = String(value.dropFirst(5))
            return Self.isOwn(path, model) ? L("This repository’s own") : L("File: %@", (path as NSString).abbreviatingWithTildeInPath)
        }
        return L("None")
    }

    var body: some View {
        SettingRow(model: model, question: L("Message template"),
                   help: L("Shared templates live in Settings > Git and are Amon Hen’s. This repository’s own is a file under .git/amonhen that commit.template points to, so git in a terminal uses it too."),
                   keys: [decidingKey, decidingKey == Self.libraryKey ? Self.fileKey : Self.libraryKey], describe: label) {
            Picker("", selection: Binding(get: { current }, set: { choose($0) })) {
                Text(L("None")).tag("")
                if !templates.isEmpty {
                    Section(L("Shared")) { ForEach(templates) { Text($0.name).tag("lib:" + $0.name) } }
                }
                Text(L("This repository’s own")).tag(Self.own)
                if case .file(let f) = choice { Text(label("file:" + f)).tag("file:" + f) }
                Divider()
                Text(L("A file…")).tag(Self.pickFile)
            }
            .labelsHidden()
            .frame(maxWidth: 320)
        }
    }

    private func choose(_ value: String) {
        // 今と同じ値なら何もしない（選び直しでなく、表示の都合で呼ばれることがある）。
        if value == current { return }
        if value == Self.own {
            // 専用のファイルがまだなければ空で作る（commit.template もそれを向く）。あれば向け直すだけ。
            if let t = model.commitTemplate, t.text != nil {
                model.set(Self.fileKey, t.path)
            } else {
                model.saveCommitTemplate("")
            }
            if model.local(Self.libraryKey) != nil { model.set(Self.libraryKey, nil) }
        } else if value == Self.pickFile {
            let panel = NSOpenPanel()
            panel.canChooseDirectories = false
            panel.allowsMultipleSelection = false
            panel.message = L("Choose a commit message template file")
            guard panel.runModal() == .OK, let url = panel.url else { return }
            model.set(Self.fileKey, url.path)
            if model.local(Self.libraryKey) != nil { model.set(Self.libraryKey, nil) }
        } else if value.hasPrefix("lib:") {
            model.set(Self.libraryKey, String(value.dropFirst(4)))
            if model.local(Self.fileKey) != nil { model.set(Self.fileKey, nil) }
        } else if value.hasPrefix("file:") {
            if model.local(Self.libraryKey) != nil { model.set(Self.libraryKey, nil) }
        } else {
            for key in [Self.libraryKey, Self.fileKey] where model.local(key) != nil { model.set(key, nil) }
        }
    }
}

private extension String {
    var nonEmpty: String? { isEmpty ? nil : self }
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
        // exclude の欄は窓の高さいっぱいに（上の 2 行と見出し・ボタンの分を引いた残り）。
        GeometryReader { geometry in
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
                        .frame(height: max(160, geometry.size.height - 330))
                    HStack {
                        Spacer()
                        Button(L("Revert")) { model.load() }.disabled(!model.ignoreChanged)
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
