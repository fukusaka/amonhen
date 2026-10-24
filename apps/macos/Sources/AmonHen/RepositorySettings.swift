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
    /// このリポジトリを扱う git の版（SSH 先なら SSH 先の git）。読めなければ nil。
    private(set) var gitVersion: String?
    /// rebase で積み重ねたブランチを一緒に動かせるか（git 2.38 から）。
    private(set) var updateRefsSupported = true
    /// 表示名・色ラベル・自動 fetch を変えた（ウィンドウの題名などを直す）。
    var onGeneralChange: (() -> Void)?
    /// git の設定・リモートなどを変えた（画面を読み直す）。
    var onRepositoryChange: (() -> Void)?

    /// SSH 先のリポジトリなら、その SSH 先の名前。
    var host: String? { RepositoryLocation.isRemote(location) ? RepositoryLocation.host(location) : nil }

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
        onGeneralChange?()
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
                TextField(L("Display name"), text: $model.displayName, prompt: Text((RepositoryLocation.path(model.location) as NSString).lastPathComponent))
                Picker(L("Color label"), selection: $model.color) {
                    Text(L("None")).tag(String?.none)
                    ForEach(Bookmarks.colors, id: \.name) { entry in
                        Label { Text(entry.title) } icon: { Image(nsImage: RepositoryCell.dot(entry.color) ?? NSImage()) }
                            .tag(Optional(entry.name))
                    }
                }
                Picker(L("Fetch automatically"), selection: $model.autoFetch) {
                    Text(L("Never")).tag(0)
                    ForEach([5, 15, 30, 60], id: \.self) { Text(L("Every %ld minutes", $0)).tag($0) }
                }
            } footer: {
                Text(L("These settings are kept in this app only (not written to the git config). The display name appears in windows, tabs and the repository list. Automatic fetch runs only while this repository is open."))
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section {
                LabeledContent(L("Version"), value: model.gitVersion ?? L("Unknown"))
                LabeledContent(L("Runs on"), value: model.host ?? L("This Mac"))
            } header: {
                Text("git")
            } footer: {
                Text(L("The git that reads and writes this repository. Settings marked “all repositories” are that git’s user settings (git config --global)."))
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .onChange(of: model.displayName) { model.saveGeneral() }
        .onChange(of: model.color) { model.saveGeneral() }
        .onChange(of: model.autoFetch) { model.saveGeneral() }
    }
}

/// hooks（git が呼ぶスクリプト）。
private struct HooksTab: View {
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
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

/// Git LFS の様子（入っているか、.gitattributes の LFS のパターン）。
private struct LFSSection: View {
    let model: RepositorySettingsModel
    var body: some View {
        Form {
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
                Text(m.count == 0 ? L("%ld: %@ — no matching files", m.line, m.pattern) : L("%ld: %@ — %ld files", m.line, m.pattern, m.count))
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
    return L("%@ (%@)", entry.scope, (origin as NSString).abbreviatingWithTildeInPath)
}

/// 文字の設定: このリポジトリでの値を入れる（空なら、上の段の値をそのまま使う）。
private struct TextSetting: View {
    let model: RepositorySettingsModel
    let title: String
    let key: String
    @State private var text = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            TextField(title, text: $text, prompt: Text(model.inherited(key)?.value ?? L("Not set")))
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
    /// 値の横に添える意味（`true` → 動かす など）。
    var meanings: [String: String] = [:]
    /// どこにも書かれていないときの意味（git の既定）。
    var unset: String?
    /// 欄の下に、今効いている値がどこで決まっているかを出す。
    var showsOrigin = true

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Picker(title, selection: Binding(
                get: { model.local(key) ?? "" },
                set: { model.set(key, $0) }
            )) {
                Text(L("Inherited (%@)", inheritedText)).tag("")
                ForEach(choices, id: \.self) { Text(label($0)).tag($0) }
            }
            if showsOrigin { Origin(model: model, key: key) }
        }
    }

    private var inheritedText: String {
        if let value = model.inherited(key)?.value { return label(value) }
        return unset ?? L("Not set")
    }

    private func label(_ value: String) -> String {
        meanings[value].map { "\(value) — \($0)" } ?? value
    }
}

/// rebase と History の編集で、積み重ねたブランチも一緒に動かすか（git の `rebase.updateRefs`）。このリポジトリと、
/// ユーザー全体の 2 段で選ぶ。git が古ければ選べない。
private struct StackedBranchesSection: View {
    let model: RepositorySettingsModel
    private let key = "rebase.updateRefs"
    private var meanings: [String: String] { ["true": L("Move them along"), "false": L("Leave them")] }

    var body: some View {
        Section {
            ChoiceSetting(
                model: model, title: L("This repository"), key: key, choices: ["true", "false"],
                meanings: meanings, unset: L("git’s default: leave them"), showsOrigin: false)
            Picker(model.host.map { L("All repositories on %@", $0) } ?? L("All repositories"), selection: Binding(
                get: { model.global(key) ?? "" },
                set: { model.setGlobal(key, $0) }
            )) {
                Text(L("Not set (%@)", L("git’s default: leave them"))).tag("")
                ForEach(["true", "false"], id: \.self) { Text("\($0) — \(meanings[$0] ?? "")").tag($0) }
            }
            Origin(model: model, key: key)
        } header: {
            Text(L("Stacked Branches"))
        } footer: {
            VStack(alignment: .leading, spacing: 4) {
                Text(L("When a rebase or a History edit rewrites commits, the branches whose tips are among them (branches stacked under the current one) can move along to the rewritten commits, or stay on the old ones. Branches checked out here or in another worktree always stay. Undo puts back whatever moved."))
                Text(model.host.map { L("“All repositories on %@” is the git config --global of the user on that host.", $0) }
                    ?? L("“All repositories” is your git config --global, also used by git in a terminal."))
                if !model.updateRefsSupported {
                    Text(L("Needs git 2.38 or later (this git is %@).", model.gitVersion ?? "?"))
                        .foregroundStyle(.orange)
                }
            }
            .font(.caption).foregroundStyle(.secondary)
        }
        .disabled(!model.updateRefsSupported)
    }
}

/// 今効いている値がどこで決まっているか。
private struct Origin: View {
    let model: RepositorySettingsModel
    let key: String

    var body: some View {
        let text = model.effective(key).map { "\(key) = \($0.value)　← \(describe($0))" } ?? L("%@ is not set", key)
        Text(text).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
    }
}

private struct IdentityTab: View {
    let model: RepositorySettingsModel
    var body: some View {
        Form {
            Section {
                TextSetting(model: model, title: L("Name"), key: "user.name")
                TextSetting(model: model, title: L("Email"), key: "user.email")
            } footer: {
                Text(L("Used only for commits in this repository (for example, to separate work and personal). Leave empty to use the inherited value (global and so on)."))
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section(L("Signing")) {
                TextSetting(model: model, title: L("Signing key"), key: "user.signingkey")
                ChoiceSetting(model: model, title: L("Sign commits"), key: "commit.gpgsign", choices: ["true", "false"])
                ChoiceSetting(model: model, title: L("Signature format"), key: "gpg.format", choices: ["openpgp", "ssh", "x509"])
            }
        }
        .formStyle(.grouped)
    }
}

/// リモートとのやり取り（pull・push・fetch）。
private struct SyncTab: View {
    let model: RepositorySettingsModel
    var body: some View {
        Form {
            Section("Pull") {
                ChoiceSetting(model: model, title: L("Rebase on pull"), key: "pull.rebase", choices: ["false", "true", "merges"])
                ChoiceSetting(model: model, title: L("Fast-forward only"), key: "pull.ff", choices: ["true", "false", "only"])
            }
            Section("Push") {
                ChoiceSetting(model: model, title: L("Default push behavior"), key: "push.default", choices: ["simple", "current", "upstream", "matching", "nothing"])
                ChoiceSetting(model: model, title: L("Set up the upstream if missing"), key: "push.autoSetupRemote", choices: ["true", "false"])
            }
            Section("Fetch") {
                ChoiceSetting(model: model, title: L("Prune deleted branches on fetch"), key: "fetch.prune", choices: ["true", "false"])
            }
        }
        .formStyle(.grouped)
    }
}

/// 履歴を書き換える・つなぐ操作（rebase と History の編集、merge）。
private struct RewritingTab: View {
    let model: RepositorySettingsModel
    var body: some View {
        Form {
            Section {
                ChoiceSetting(model: model, title: L("Stash before rebase"), key: "rebase.autoStash", choices: ["true", "false"])
            } header: {
                Text("Rebase")
            } footer: {
                Text(L("Also used by pull with rebase."))
                    .font(.caption).foregroundStyle(.secondary)
            }
            StackedBranchesSection(model: model)
            Section("Merge") {
                ChoiceSetting(model: model, title: L("Conflict style"), key: "merge.conflictStyle", choices: ["merge", "diff3", "zdiff3"])
            }
        }
        .formStyle(.grouped)
    }
}

/// 無視するファイルと LFS。
private struct FilesTab: View {
    @Bindable var model: RepositorySettingsModel
    var body: some View {
        VStack(spacing: 0) {
            IgnoreEditor(model: model)
            // 上の Ignore と同じ地の色にする。
            LFSSection(model: model).scrollContentBackground(.hidden).frame(height: 180)
        }
    }
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

private struct IgnoreEditor: View {
    @Bindable var model: RepositorySettingsModel
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(L(".gitignore (at the top of the working tree; committed and shared)")).font(.headline)
            TextEditor(text: $model.gitignore).font(.system(.body, design: .monospaced)).frame(minHeight: 140)
            Text(L(".git/info/exclude (this repository only; not shared)")).font(.headline)
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
                Text(LocalizedStringKey(L("One pattern per line (`*.log`, `build/` and so on). Text after # is a comment."))).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button(model.matching ? L("Searching…") : L("Show Matching Files")) { model.findIgnoreMatches() }
                    .disabled(model.matching)
                    .help(L("Show how many files (tracked ones included) match each line of the unsaved text, with examples."))
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

private struct RepositorySettingsView: View {
    let model: RepositorySettingsModel
    /// 開いているタブ（`general`・`branches` など）。
    @State var tab: String

    var body: some View {
        VStack(spacing: 0) {
            TabView(selection: $tab) {
                GeneralTab(model: model).tabItem { Text(L("General")) }.tag("general")
                IdentityTab(model: model).tabItem { Text(L("Identity")) }.tag("identity")
                SyncTab(model: model).tabItem { Text(L("Sync")) }.tag("sync")
                RewritingTab(model: model).tabItem { Text(L("Rewriting")) }.tag("rewriting")
                RemotesTab(model: model).tabItem { Text("Remotes") }.tag("remotes")
                FilesTab(model: model).tabItem { Text(L("Files")) }.tag("files")
                HooksTab(model: model).tabItem { Text("Hooks") }.tag("hooks")
                AllSettingsTab(model: model).tabItem { Text(L("All Settings")) }.tag("all")
            }
            .frame(maxWidth: 820)
            .padding([.top, .horizontal])
            if let error = model.error {
                HStack {
                    Text(error).font(.caption).foregroundStyle(.red).lineLimit(2)
                    Spacer()
                }
                .frame(maxWidth: 820)
                .padding([.horizontal, .bottom])
            }
        }
        .padding(.bottom)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}

/// リポジトリの設定の画面（サイドバーの Workspace の Settings）。
final class RepositorySettingsPage: NSHostingController<AnyView> {
    let model: RepositorySettingsModel

    init(repository: Repository, location: String) {
        model = RepositorySettingsModel(repository: repository, location: location)
        let tab = AppDelegate.option("--settings-tab")?.lowercased() ?? "general"
        super.init(rootView: AnyView(RepositorySettingsView(model: model, tab: tab)))
    }

    @MainActor required dynamic init?(coder: NSCoder) { fatalError() }

    /// 開くたびに読み直す（端末などで変えたかもしれない）。
    override func viewWillAppear() {
        super.viewWillAppear()
        model.load()
    }
}
