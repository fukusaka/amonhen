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
    private(set) var entries: [ConfigEntry] = []
    private(set) var remotes: [RemoteEntry] = []
    var gitignore = ""
    var exclude = ""
    private(set) var savedIgnore = ("", "")
    var error: String?

    init(repository: Repository) {
        self.repository = repository
    }

    func load() {
        let repository = self.repository
        Task {
            struct Entries: Decodable { let entries: [ConfigEntry] }
            struct Remotes: Decodable { let remotes: [RemoteEntry] }
            struct Ignore: Decodable { let gitignore: String; let exclude: String }
            do {
                let (e, r, i) = try await Task.detached(priority: .userInitiated) {
                    (
                        try repository.info(["op": "configList"], as: Entries.self),
                        try repository.info(["op": "remotes"], as: Remotes.self),
                        try repository.info(["op": "ignoreFiles"], as: Ignore.self)
                    )
                }.value
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
            HStack {
                Text("1 行に 1 つのパターン（`*.log`、`build/` など）。# から後は説明。").font(.caption).foregroundStyle(.secondary)
                Spacer()
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
                IdentityTab(model: model).tabItem { Text("Identity") }
                BranchesTab(model: model).tabItem { Text("Branches") }
                RemotesTab(model: model).tabItem { Text("Remotes") }
                IgnoreTab(model: model).tabItem { Text("Ignore") }
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
    static func show(for repository: Repository, title: String, in window: NSWindow, onClose: @escaping () -> Void) {
        let model = RepositorySettingsModel(repository: repository)
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
