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
            do {
                let (e, r, i, h, v) = try await Task.detached(priority: .userInitiated) {
                    (
                        try repository.info(["op": "configList"], as: Entries.self),
                        try repository.info(["op": "remotes"], as: Remotes.self),
                        try repository.info(["op": "ignoreFiles"], as: Ignore.self),
                        try repository.info(["op": "hooksAndLfs"], as: Hooks.self),
                        try repository.info(["op": "gitVersion"], as: Version.self)
                    )
                }.value
                gitVersion = v.version
                updateRefsSupported = v.updateRefs
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

/// 段の名前と、ファイルの場所（`file:` を取り、ホームは ~ にする）。
private func describe(_ entry: ConfigEntry) -> String {
    var origin = entry.origin
    if origin.hasPrefix("file:") { origin = String(origin.dropFirst(5)) }
    return L("%@ (%@)", entry.scope, (origin as NSString).abbreviatingWithTildeInPath)
}

private struct RemotesTab: View {
    let model: RepositorySettingsModel
    @State private var selection: String?

    var body: some View {
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
        .padding()
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

/// このリポジトリだけで無視するファイル（`.git/info/exclude`）のエディタ。`.gitignore` はリポジトリのファイルなので
/// ここでは扱わない（Working Copy で開く）。
private struct ExcludeEditor: View {
    @Bindable var model: RepositorySettingsModel
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(L("Ignore in this repository only")).font(.headline)
            Text(L("Patterns in .git/info/exclude, one per line (`*.log`, `build/` and so on; text after # is a comment). Not shared with anyone. Patterns everyone should share go in .gitignore, which is a file of the repository."))
                .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            TextEditor(text: $model.exclude).font(.system(.body, design: .monospaced)).frame(minHeight: 120, maxHeight: 240)
            HStack {
                Spacer()
                Button("Revert") { model.load() }.disabled(!model.ignoreChanged)
                Button(L("Save")) { model.saveIgnore() }.disabled(!model.ignoreChanged).keyboardShortcut("s")
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
            TextField(L("Filter by key"), text: $filter)
            Table(model.entries.filter { filter.isEmpty || $0.key.localizedCaseInsensitiveContains(filter) }) {
                TableColumn(L("Key"), value: \.key).width(min: 140, ideal: 200)
                TableColumn(L("Value"), value: \.value)
                TableColumn(L("Scope"), value: \.scope).width(min: 50, ideal: 60)
                TableColumn(L("Location")) { e in Text(describe(e)).foregroundStyle(.secondary) }
            }
            HStack {
                TextField(L("Key (e.g. core.autocrlf)"), text: $key).frame(width: 200)
                TextField(L("Value"), text: $value)
                Button(L("Set Local")) { model.set(key, value) }.disabled(key.isEmpty || value.isEmpty)
                Button(L("Unset Local")) { model.set(key, nil) }.disabled(key.isEmpty)
                    .help(L("Remove this repository’s (local) value and go back to the inherited one."))
            }
            Text(L("Later rows take precedence (system → global → local → worktree). Only this repository’s (local) values can be changed."))
                .font(.caption).foregroundStyle(.secondary)
        }
        .padding()
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
    /// `key` の今効いている値の出どころ（札の言葉）と、そのファイル。どこにも書かれていなければ「git の既定」。
    func origin(of key: String) -> (label: String, file: String?) {
        guard let e = effective(key) else { return (L("git’s default"), nil) }
        var file = e.origin
        if file.hasPrefix("file:") { file = String(file.dropFirst(5)) }
        file = (file as NSString).abbreviatingWithTildeInPath
        let label = switch e.scope {
        case "local": L("this repository")
        case "global": host.map { L("your settings on %@", $0) } ?? L("your settings")
        case "worktree": L("this worktree")
        case "system": L("system")
        default: e.scope
        }
        return (label, file)
    }

    /// local の値を消す（上の値に戻る）。
    func revert(_ keys: [String]) {
        for key in keys where local(key) != nil { set(key, nil) }
    }
}

/// 値の出どころの札。
private struct OriginBadge: View {
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
/// 最後に札。札の横の ⓘ で、キーと値とファイル、上の値が吹き出しに出る。
private struct SettingRow<Control: View>: View {
    let model: RepositorySettingsModel
    let question: String
    let help: String
    /// この行が読み書きするキー。札は先頭のキーのもの。↩︎ はすべての local を消す。
    let keys: [String]
    /// 生の値を人の言葉にする（吹き出しの表示に使う）。
    var describe: (String) -> String = { $0 }
    /// 使えないときの理由（古い git など）。あれば欄を灰色にして、下に出す。
    var disabledReason: String?
    @ViewBuilder let control: () -> Control
    @State private var showingDetails = false

    private var overriding: Bool { keys.contains { model.local($0) != nil } }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(question)
                    Text(help).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.leading, 8)
                }
                .frame(maxWidth: 520, alignment: .leading)
                Spacer()
                control().disabled(disabledReason != nil)
                if overriding {
                    Button { model.revert(keys) } label: { Image(systemName: "arrow.uturn.backward") }
                        .buttonStyle(.borderless)
                        .help(L("Stop overriding here. The value above takes over."))
                } else {
                    Color.clear.frame(width: 20, height: 1)
                }
                OriginBadge(model: model, key: keys[0])
                Button { showingDetails.toggle() } label: { Image(systemName: "info.circle") }
                    .buttonStyle(.borderless)
                    .help(L("Where this is set"))
                    .popover(isPresented: $showingDetails, arrowEdge: .trailing) { details }
            }
            if let disabledReason {
                Text("⚠ " + disabledReason).font(.caption).foregroundStyle(.orange)
            }
        }
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
                        Text(L("Above: %@", describe(above.value))).foregroundStyle(.secondary)
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
    /// 札に出すのは、取り込み方を決めているほうのキー。
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
            help: L("Merge records a merge commit when both sides have new commits. Rebase replays your commits on top of the fetched ones, keeping History in one line. Only fast-forward refuses when the branches have diverged. Amon Hen’s Pull follows this when Settings > Git’s Default Pull is “Follow git’s settings”."),
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

/// 左の一覧の項目。
enum SettingsPage: String, CaseIterable, Identifiable {
    case author, commits, operations, workingCopy, remotes
    case about, all
    var id: String { rawValue }

    var title: String {
        switch self {
        case .author: L("Author and Signing")
        case .commits: L("Commits")
        case .operations: L("Operations")
        case .workingCopy: "Working Copy"
        case .remotes: L("Remotes")
        case .about: L("About This Repository")
        case .all: L("All Settings")
        }
    }

    static let settings: [SettingsPage] = [.author, .commits, .operations, .workingCopy, .remotes]
    static let information: [SettingsPage] = [.about, .all]
}

private struct AuthorSigningPage: View {
    let model: RepositorySettingsModel
    var body: some View {
        let format = model.effective("gpg.format")?.value.lowercased() ?? "openpgp"
        Form {
            Section {
                TextRow(model: model, question: L("Name"), help: L("Recorded as the author and committer of commits made here."), key: "user.name")
                TextRow(model: model, question: L("Email"), help: L("Recorded with the name. Set these here to keep work and personal commits apart."), key: "user.email")
            } header: {
                Text(L("Author"))
            }
            Section {
                SwitchRow(model: model, question: L("Sign commits"), help: L("Every commit made here is signed with the key below."), key: "commit.gpgsign", defaultOn: false)
                ChoiceRow(model: model, question: L("Signature format"), help: L("OpenPGP uses gpg. SSH signs with an SSH key, and X.509 with gpgsm."), key: "gpg.format",
                          options: [("openpgp", "OpenPGP"), ("ssh", "SSH"), ("x509", "X.509")], defaultValue: "openpgp")
                TextRow(model: model, question: L("Key"),
                        help: format == "ssh" ? L("The path of the SSH public key, or the key itself (“ssh-ed25519 AAAA…”).") : L("The fingerprint or ID of the key; empty uses the key for your email."),
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
                TextRow(model: model, question: L("Message template"), help: L("A file whose contents start every new commit message (the commit box in Working Copy starts from it too). Empty: messages start empty."), key: "commit.template", placeholder: L("None"))
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
            Section {
                SwitchRow(model: model, question: L("Remove remote-tracking branches deleted on the remote"), help: L("When nothing is set, Amon Hen’s Fetch removes them and git fetch in a terminal keeps them. Off: neither removes them. On: both do."), key: "fetch.prune", defaultOn: false)
            } header: {
                Text("Fetch")
            } footer: {
                Text(L("Push has no settings here: Amon Hen’s Push sends the branch to the target chosen in its sheet, so push.default and push.autoSetupRemote only affect git push in a terminal (see All Settings)."))
                    .foregroundStyle(.secondary)
            }
            Section("Rebase") {
                SwitchRow(model: model, question: L("Stash changes before rebasing"), help: L("Uncommitted changes are stashed before a rebase or a pull with rebase and restored after it. History edits (reword, squash and so on) always stash, whatever this says."), key: "rebase.autoStash", defaultOn: false)
                ChoiceRow(model: model, question: L("Move stacked branches along when rebasing or editing History"),
                          help: L("Branches whose tips are among the rewritten commits (stacked under the current one) move to the rewritten commits, or stay on the old ones. Branches checked out here or in another worktree always stay. Undo puts back whatever moved."),
                          key: "rebase.updateRefs", options: [("false", L("Leave them")), ("true", L("Move them along"))], defaultValue: "false", isBool: true, disabledReason: tooOld)
            }
            Section("Merge") {
                ChoiceRow(model: model, question: L("How conflicts are written in files"), help: L("Amon Hen’s merge editor works with all three. With the base (diff3, zdiff3) it resolves conflicts where only one side changed, or the sides changed different words, by itself; zdiff3 also leaves out lines both sides changed the same way. Amon Hen recommends zdiff3."), key: "merge.conflictStyle",
                          options: [("merge", L("Both sides")), ("diff3", L("Both sides and the common base")), ("zdiff3", L("Both sides and the base, shortened"))], defaultValue: "merge")
            }
        }
        .formStyle(.grouped)
    }
}

private struct WorkingCopyPage: View {
    @Bindable var model: RepositorySettingsModel
    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section {
                    ChoiceRow(model: model, question: L("Line endings"), help: L("What git does with CRLF and LF when checking out and committing text files. Amon Hen’s diffs follow it."), key: "core.autocrlf",
                              options: [("false", L("Keep as committed")), ("true", L("CRLF in the working tree, LF in commits")), ("input", L("LF in commits, no change on checkout"))],
                              defaultValue: "false", isBool: false)
                    SwitchRow(model: model, question: L("Track changes to the executable bit"), help: L("Off ignores file mode changes, as on file systems that don’t keep them."), key: "core.fileMode", defaultOn: true)
                }
            }
            .formStyle(.grouped)
            .frame(height: 220)
            // Form の中身と同じ幅にそろえる。
            ExcludeEditor(model: model).frame(maxWidth: 740)
        }
    }
}

/// このリポジトリについて（動かしている git、hooks、LFS）。設定ではなく情報。
private struct AboutPage: View {
    let model: RepositorySettingsModel
    var body: some View {
        Form {
            Section {
                LabeledContent(L("Version"), value: model.gitVersion ?? L("Unknown"))
                LabeledContent(L("Runs on"), value: model.host ?? L("This Mac"))
            } header: {
                Text("git")
            } footer: {
                Text(L("The git that reads and writes this repository. Settings marked “your settings” are that git’s user settings (git config --global)."))
                    .foregroundStyle(.secondary)
            }
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
            List(selection: Binding(get: { Optional(page) }, set: { if let p = $0 { page = p } })) {
                Section(L("Settings")) {
                    ForEach(SettingsPage.settings) { Text($0.title).tag($0) }
                }
                Section(L("Information")) {
                    ForEach(SettingsPage.information) { Text($0.title).tag($0) }
                }
            }
            .listStyle(.sidebar)
            .frame(width: 200)
            Divider()
            VStack(spacing: 0) {
                content
                if let error = model.error {
                    HStack {
                        Text(error).font(.caption).foregroundStyle(.red).lineLimit(2)
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
        case .workingCopy: WorkingCopyPage(model: model)
        case .remotes: RemotesTab(model: model)
        case .about: AboutPage(model: model)
        case .all: AllSettingsTab(model: model)
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
