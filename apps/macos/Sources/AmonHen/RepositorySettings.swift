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

/// リポジトリの設定（Repository > Repository Settings…、⌘;）の中身。読み書きはコアの命令を裏で呼ぶ。
@Observable
final class RepositorySettingsModel {
    let repository: Repository
    /// リポジトリの場所（表示名・色ラベル・自動 fetch はブックマークに置く）。
    let location: String
    var displayName: String
    var color: String?
    var autoFetch: Int
    private(set) var hooksPath: String?
    private(set) var hooks: [HookEntry] = []
    private(set) var lfs = false
    private(set) var lfsPatterns: [String] = []
    /// Ignore の各行に合うファイル（「合うファイルを見る」を押したとき）。
    private(set) var ignoreMatches: [IgnoreMatch]?
    private(set) var excludeMatches: [IgnoreMatch]?
    private(set) var matching = false
    private(set) var entries: [ConfigEntry] = []
    private(set) var remotes: [RemoteEntry] = []
    var gitignore = ""
    var exclude = ""
    private(set) var savedIgnore = ("", "")
    var error: String?

    init(repository: Repository, location: String) {
        self.repository = repository
        self.location = location
        let bookmark = Bookmarks.shared.bookmark(location)
        displayName = bookmark?.name ?? ""
        color = bookmark?.color
        autoFetch = bookmark?.autoFetch ?? 0
    }

    /// 表示名・色ラベル・自動 fetch をブックマークに書く（ブックマークになければ加える）。
    func saveGeneral() {
        let bookmarks = Bookmarks.shared
        let name = displayName.trimmingCharacters(in: .whitespaces)
        if bookmarks.bookmark(location)?.name ?? "" != name { bookmarks.rename(location, to: name.isEmpty ? nil : name) }
        if bookmarks.bookmark(location)?.color != color { bookmarks.setColor(location, color) }
        let minutes = autoFetch > 0 ? autoFetch : nil
        if bookmarks.bookmark(location)?.autoFetch != minutes { bookmarks.setAutoFetch(location, minutes: minutes) }
    }

    /// .gitignore と exclude の各行に合うファイルを探す（git に聞くので裏で）。
    func findIgnoreMatches() {
        let repository = self.repository
        let texts = (gitignore, exclude)
        matching = true
        Task {
            struct Lines: Decodable { let lines: [IgnoreMatch] }
            do {
                let (a, b) = try await Task.detached(priority: .userInitiated) {
                    (
                        try repository.info(["op": "ignoreMatches", "text": texts.0], as: Lines.self).lines,
                        try repository.info(["op": "ignoreMatches", "text": texts.1], as: Lines.self).lines
                    )
                }.value
                ignoreMatches = a
                excludeMatches = b
            } catch {
                self.error = "\(error)"
            }
            matching = false
        }
    }

    func load() {
        let repository = self.repository
        Task {
            struct Entries: Decodable { let entries: [ConfigEntry] }
            struct Remotes: Decodable { let remotes: [RemoteEntry] }
            struct Ignore: Decodable { let gitignore: String; let exclude: String }
            struct Hooks: Decodable { let hooksPath: String?; let hooks: [HookEntry]; let lfs: Bool; let lfsPatterns: [String] }
            do {
                let (e, r, i, h) = try await Task.detached(priority: .userInitiated) {
                    (
                        try repository.info(["op": "configList"], as: Entries.self),
                        try repository.info(["op": "remotes"], as: Remotes.self),
                        try repository.info(["op": "ignoreFiles"], as: Ignore.self),
                        try repository.info(["op": "hooksAndLfs"], as: Hooks.self)
                    )
                }.value
                hooksPath = h.hooksPath
                hooks = h.hooks
                lfs = h.lfs
                lfsPatterns = h.lfsPatterns
                entries = e.entries
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

    /// 今効いている値（後に出るものほど優先）と、その段。
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

    /// local の値を変える。nil か空なら消す。
    func set(_ key: String, _ value: String?) {
        let value = value?.trimmingCharacters(in: .whitespaces)
        run(["op": "configSet", "key": key, "value": (value?.isEmpty ?? true) ? NSNull() : value!])
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
        }
    }
}

struct HookEntry: Decodable, Hashable, Identifiable {
    let name: String
    let enabled: Bool
    var id: String { name }
}

struct IgnoreMatch: Decodable, Hashable, Identifiable {
    let line: Int
    let pattern: String
    let examples: [String]
    let count: Int
    var id: Int { line }
}

/// 表示名・色ラベル・自動 fetch（このアプリの中だけの設定。ブックマークに置く）。
private struct GeneralTab: View {
    @Bindable var model: RepositorySettingsModel
    var body: some View {
        Form {
            Section {
                TextField("表示名", text: $model.displayName, prompt: Text((RepositoryLocation.path(model.location) as NSString).lastPathComponent))
                Picker("色ラベル", selection: $model.color) {
                    Text("なし").tag(String?.none)
                    ForEach(Bookmarks.colors, id: \.name) { entry in
                        Label { Text(entry.title) } icon: { Image(nsImage: RepositoryCell.dot(entry.color) ?? NSImage()) }
                            .tag(Optional(entry.name))
                    }
                }
                Picker("自動で fetch", selection: $model.autoFetch) {
                    Text("しない").tag(0)
                    ForEach([5, 15, 30, 60], id: \.self) { Text("\($0) 分ごと").tag($0) }
                }
            } footer: {
                Text("このアプリの中だけの設定（git の設定には書かない）。表示名はウィンドウ・タブ・リポジトリの一覧に出る。自動の fetch は、このリポジトリを開いている間だけ。")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .onChange(of: model.displayName) { model.saveGeneral() }
        .onChange(of: model.color) { model.saveGeneral() }
        .onChange(of: model.autoFetch) { model.saveGeneral() }
    }
}

/// hooks（git が呼ぶスクリプト）と Git LFS の様子。
private struct HooksTab: View {
    let model: RepositorySettingsModel
    var body: some View {
        Form {
            Section {
                if model.hooks.isEmpty {
                    Text(model.hooksPath.map { $0.hasPrefix("/") || $0.hasPrefix("~") ? "core.hooksPath（\($0)）は作業ツリーの外なので、中を見られない" : "なし" } ?? "なし")
                        .foregroundStyle(.secondary)
                }
                ForEach(model.hooks) { hook in
                    HStack {
                        Text(hook.name).font(.system(.body, design: .monospaced))
                        Spacer()
                        Text(hook.enabled ? "有効" : "実行できない（chmod +x がない）")
                            .foregroundStyle(hook.enabled ? Color.green : Color.secondary)
                    }
                }
            } header: {
                Text("Hooks")
            } footer: {
                Text("場所: " + (model.hooksPath.map { "core.hooksPath = \($0)" } ?? ".git/hooks") + "。.sample は数えない。commit や push のときに git が実行する。")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section {
                LabeledContent("Git LFS", value: model.lfs ? "入っている（filter.lfs が設定されている）" : "入っていない")
                if model.lfsPatterns.isEmpty {
                    Text(".gitattributes に LFS で扱うパターンはない").foregroundStyle(.secondary)
                }
                ForEach(model.lfsPatterns, id: \.self) { Text($0).font(.system(.body, design: .monospaced)) }
            } header: {
                Text("LFS")
            } footer: {
                if !model.lfs && !model.lfsPatterns.isEmpty {
                    Text("LFS のパターンがあるのに git lfs が入っていない。brew install git-lfs と git lfs install で入れる。")
                        .font(.caption).foregroundStyle(.orange)
                }
            }
        }
        .formStyle(.grouped)
    }
}

/// Ignore の各行に合うファイルの一覧。
private struct IgnoreMatchList: View {
    let matches: [IgnoreMatch]
    var body: some View {
        ForEach(matches) { m in
            VStack(alignment: .leading, spacing: 1) {
                Text("\(m.line): \(m.pattern)　— \(m.count == 0 ? "合うファイルなし" : "\(m.count) 個")")
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(m.count == 0 ? Color.orange : Color.primary)
                if !m.examples.isEmpty {
                    Text(m.examples.joined(separator: "　") + (m.count > m.examples.count ? "　…" : ""))
                        .font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                }
            }
        }
    }
}

/// 段の名前と、ファイルの場所（`file:` を取り、ホームは ~ にする）。
private func describe(_ entry: ConfigEntry) -> String {
    var origin = entry.origin
    if origin.hasPrefix("file:") { origin = String(origin.dropFirst(5)) }
    return "\(entry.scope)（\((origin as NSString).abbreviatingWithTildeInPath)）"
}

/// 文字の設定: このリポジトリでの値を入れる（空なら、上の段の値をそのまま使う）。
private struct TextSetting: View {
    let model: RepositorySettingsModel
    let title: String
    let key: String
    @State private var text = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            TextField(title, text: $text, prompt: Text(model.inherited(key)?.value ?? "未設定"))
                .onSubmit { model.set(key, text) }
                .onAppear { text = model.local(key) ?? "" }
                .onChange(of: model.local(key)) { _, new in text = new ?? "" }
            Origin(model: model, key: key)
        }
    }
}

/// 選ぶ設定: 「上の段のまま」か、このリポジトリでの値。
private struct ChoiceSetting: View {
    let model: RepositorySettingsModel
    let title: String
    let key: String
    let choices: [String]

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Picker(title, selection: Binding(
                get: { model.local(key) ?? "" },
                set: { model.set(key, $0) }
            )) {
                Text("上の段のまま（\(model.inherited(key)?.value ?? "未設定")）").tag("")
                ForEach(choices, id: \.self) { Text($0).tag($0) }
            }
            Origin(model: model, key: key)
        }
    }
}

/// 今効いている値がどこで決まっているか。
private struct Origin: View {
    let model: RepositorySettingsModel
    let key: String

    var body: some View {
        let text = model.effective(key).map { "\(key) = \($0.value)　← \(describe($0))" } ?? "\(key) は未設定"
        Text(text).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
    }
}

private struct IdentityTab: View {
    let model: RepositorySettingsModel
    var body: some View {
        Form {
            Section {
                TextSetting(model: model, title: "名前", key: "user.name")
                TextSetting(model: model, title: "メール", key: "user.email")
            } footer: {
                Text("このリポジトリのコミットにだけ使う（仕事用と個人用を分けるなど）。空にすると上の段（global など）の値を使う。")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section("署名") {
                TextSetting(model: model, title: "署名の鍵", key: "user.signingkey")
                ChoiceSetting(model: model, title: "コミットに署名する", key: "commit.gpgsign", choices: ["true", "false"])
                ChoiceSetting(model: model, title: "署名の形式", key: "gpg.format", choices: ["openpgp", "ssh", "x509"])
            }
        }
        .formStyle(.grouped)
    }
}

private struct BranchesTab: View {
    let model: RepositorySettingsModel
    var body: some View {
        Form {
            Section("Pull") {
                ChoiceSetting(model: model, title: "pull で rebase する", key: "pull.rebase", choices: ["false", "true", "merges"])
                ChoiceSetting(model: model, title: "fast-forward だけ", key: "pull.ff", choices: ["true", "false", "only"])
                ChoiceSetting(model: model, title: "rebase の前に退避する", key: "rebase.autoStash", choices: ["true", "false"])
            }
            Section("Push・Fetch") {
                ChoiceSetting(model: model, title: "上流がなければ作る", key: "push.autoSetupRemote", choices: ["true", "false"])
                ChoiceSetting(model: model, title: "push の既定", key: "push.default", choices: ["simple", "current", "upstream", "matching", "nothing"])
                ChoiceSetting(model: model, title: "fetch で消えたブランチを片づける", key: "fetch.prune", choices: ["true", "false"])
            }
            Section("Merge") {
                ChoiceSetting(model: model, title: "コンフリクトの書き方", key: "merge.conflictStyle", choices: ["merge", "diff3", "zdiff3"])
            }
        }
        .formStyle(.grouped)
    }
}

private struct RemotesTab: View {
    let model: RepositorySettingsModel
    @State private var selection: String?

    var body: some View {
        VStack(alignment: .leading) {
            Table(model.remotes, selection: $selection) {
                TableColumn("名前", value: \.name).width(min: 80, ideal: 100)
                TableColumn("URL", value: \.fetchUrl)
                TableColumn("push の URL") { r in
                    Text(r.pushUrl == r.fetchUrl ? "（同じ）" : r.pushUrl).foregroundStyle(r.pushUrl == r.fetchUrl ? .secondary : .primary)
                }
            }
            HStack {
                Button("Add…") { add() }
                Button("Rename…") { rename() }.disabled(selection == nil)
                Button("Edit URL…") { editURL() }.disabled(selection == nil)
                Button("Prune") { selection.map { model.changeRemote(["kind": "prune", "name": $0]) } }
                    .disabled(selection == nil)
                    .help("リモートで消えたブランチの追跡を消す")
                Spacer()
                Button("Remove", role: .destructive) { remove() }.disabled(selection == nil)
            }
        }
        .padding()
    }

    private func add() {
        guard let (name, url) = askPair("Add Remote", first: ("名前", "origin"), second: ("URL", "")) else { return }
        model.changeRemote(["kind": "add", "name": name, "url": url])
    }

    private func rename() {
        guard let from = selection, let to = ask("Rename Remote", label: "新しい名前", initial: from) else { return }
        model.changeRemote(["kind": "rename", "from": from, "to": to])
    }

    private func editURL() {
        guard let name = selection, let remote = model.remotes.first(where: { $0.name == name }),
              let url = ask("Edit URL", label: "\(name) の URL", initial: remote.fetchUrl) else { return }
        model.changeRemote(["kind": "setUrl", "name": name, "url": url])
    }

    private func remove() {
        guard let name = selection else { return }
        let alert = NSAlert()
        alert.messageText = "\(name) を消しますか？"
        alert.informativeText = "リモートの設定と、追跡しているリモートブランチ（\(name)/…）が消えます。リモートのリポジトリには触れません。"
        alert.addButton(withTitle: "消す")
        alert.addButton(withTitle: "キャンセル")
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
        alert.addButton(withTitle: "キャンセル")
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

private struct IgnoreTab: View {
    @Bindable var model: RepositorySettingsModel
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(".gitignore（作業ツリーの直下。コミットして共有する）").font(.headline)
            TextEditor(text: $model.gitignore).font(.system(.body, design: .monospaced)).frame(minHeight: 140)
            Text(".git/info/exclude（このリポジトリだけ。共有しない）").font(.headline)
            TextEditor(text: $model.exclude).font(.system(.body, design: .monospaced)).frame(minHeight: 80)
            if model.ignoreMatches != nil || model.excludeMatches != nil {
                ScrollView {
                    VStack(alignment: .leading, spacing: 4) {
                        IgnoreMatchList(matches: model.ignoreMatches ?? [])
                        if let exclude = model.excludeMatches, !exclude.isEmpty {
                            Text("exclude").font(.caption.bold())
                            IgnoreMatchList(matches: exclude)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 120)
            }
            HStack {
                Text("1 行に 1 つのパターン（`*.log`、`build/` など）。# から後は説明。").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button(model.matching ? "探しています…" : "合うファイルを見る") { model.findIgnoreMatches() }
                    .disabled(model.matching)
                    .help("保存する前の中身で、各行に合うファイル（追跡しているものも含む）の例と数を出す")
                Button("Revert") { model.load() }.disabled(!model.ignoreChanged)
                Button("Save") { model.saveIgnore() }.disabled(!model.ignoreChanged).keyboardShortcut("s")
            }
        }
        .padding()
    }
}

private struct AllSettingsTab: View {
    let model: RepositorySettingsModel
    @State private var filter = ""
    @State private var key = ""
    @State private var value = ""

    var body: some View {
        VStack(alignment: .leading) {
            TextField("キーで絞り込む", text: $filter)
            Table(model.entries.filter { filter.isEmpty || $0.key.localizedCaseInsensitiveContains(filter) }) {
                TableColumn("キー", value: \.key).width(min: 140, ideal: 200)
                TableColumn("値", value: \.value)
                TableColumn("段", value: \.scope).width(min: 50, ideal: 60)
                TableColumn("場所") { e in Text(describe(e)).foregroundStyle(.secondary) }
            }
            HStack {
                TextField("キー（core.autocrlf など）", text: $key).frame(width: 200)
                TextField("値", text: $value)
                Button("Set Local") { model.set(key, value) }.disabled(key.isEmpty || value.isEmpty)
                Button("Unset Local") { model.set(key, nil) }.disabled(key.isEmpty)
                    .help("このリポジトリ（local）の値を消し、上の段の値に戻す")
            }
            Text("後の行ほど優先される（system → global → local → worktree）。変えられるのはこのリポジトリ（local）の値だけ。")
                .font(.caption).foregroundStyle(.secondary)
        }
        .padding()
    }
}

private struct RepositorySettingsView: View {
    let model: RepositorySettingsModel
    let onClose: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            TabView {
                GeneralTab(model: model).tabItem { Text("General") }
                IdentityTab(model: model).tabItem { Text("Identity") }
                BranchesTab(model: model).tabItem { Text("Branches") }
                RemotesTab(model: model).tabItem { Text("Remotes") }
                IgnoreTab(model: model).tabItem { Text("Ignore") }
                HooksTab(model: model).tabItem { Text("Hooks・LFS") }
                AllSettingsTab(model: model).tabItem { Text("All Settings") }
            }
            .padding([.top, .horizontal])
            HStack {
                if let error = model.error {
                    Text(error).font(.caption).foregroundStyle(.red).lineLimit(2)
                }
                Spacer()
                Button("Done") { onClose() }.keyboardShortcut(.defaultAction)
            }
            .padding()
        }
        .frame(width: 720, height: 560)
    }
}

/// リポジトリの設定をシートで出す。
enum RepositorySettingsSheet {
    /// `onClose`: 閉じた（リモートなどが変わったかもしれないので、画面を読み直す）。
    static func show(
        for repository: Repository, location: String, title: String, in window: NSWindow, onClose: @escaping () -> Void
    ) {
        let model = RepositorySettingsModel(repository: repository, location: location)
        model.load()
        let sheet = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 720, height: 560), styleMask: [.titled], backing: .buffered, defer: false)
        sheet.title = "\(title) の設定"
        sheet.contentViewController = NSHostingController(
            rootView: RepositorySettingsView(model: model) { [weak window, weak sheet] in
                if let sheet { window?.endSheet(sheet) }
                onClose()
            })
        window.beginSheet(sheet)
    }
}
