import AmonhenKit
import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// この Mac の git の設定（Settings > Git の上の節）: 使う git、ユーザー全体の作成者と署名、gpg などのプログラム、
/// global excludes。`git config --global` を書くのはここだけで、端末の git もこれに従う。
@Observable @MainActor
final class MachineGit {
    struct Candidate: Identifiable, Hashable {
        let path: String
        let version: String?
        var id: String { path }
        var label: String { "\(version ?? "?") — \(path)" }
    }

    /// 見つかった git（よく置かれる場所と PATH）。
    private(set) var candidates: [Candidate] = []
    /// PATH で先に見つかる git（自動のときに使われるもの）。
    private(set) var onPath: Candidate?
    /// 読み込んだ global の値（キーは小文字）。
    private(set) var global: [String: String] = [:]
    /// 種類ごとの鍵。
    private(set) var keys: [String: [SigningKey]] = [:]
    /// PATH で見つかったプログラム。
    private(set) var found: [String: String] = [:]
    /// global excludes の中身（ファイルがなければ空）。
    var excludes = ""
    private(set) var excludesSaved = ""
    private(set) var loaded = false
    var error: String?

    /// 選んだ git（空なら自動）。
    var gitExecutable: String = Settings.gitExecutable ?? "" {
        didSet {
            Settings.gitExecutable = gitExecutable.isEmpty ? nil : gitExecutable
            Settings.applyGitExecutable()
        }
    }

    nonisolated static let gpgKeys = ["gpg.openpgp.program", "gpg.program"]
    nonisolated static let sshKeys = ["gpg.ssh.program"]
    nonisolated static let x509Keys = ["gpg.x509.program"]
    nonisolated static let excludesKey = "core.excludesfile"
    nonisolated static let globalKeys = [
        "user.name", "user.email", "commit.gpgsign", "gpg.format", "user.signingkey",
        "gpg.program", "gpg.openpgp.program", "gpg.ssh.program", "gpg.x509.program", excludesKey,
    ]

    /// global excludes の場所: `core.excludesFile`、なければ git の既定（`$XDG_CONFIG_HOME/git/ignore`、
    /// `~/.config/git/ignore`）。
    var excludesPath: String {
        if let p = global[Self.excludesKey], !p.isEmpty { return p }
        let xdg = ProcessInfo.processInfo.environment["XDG_CONFIG_HOME"].flatMap { $0.isEmpty ? nil : $0 }
        return ((xdg ?? "~/.config") as NSString).appendingPathComponent("git/ignore")
    }

    var excludesChanged: Bool { excludes != excludesSaved }

    func value(_ key: String) -> String { global[key.lowercased()] ?? "" }

    /// `keys` の順で最初に設定されているプログラム、なければ PATH で見つかった `fallback`。
    func program(_ keys: [String], _ fallback: String) -> (value: String, key: String, found: String?) {
        for k in keys {
            let v = value(k)
            if !v.isEmpty { return (v, k, found[fallback]) }
        }
        return ("", keys[0], found[fallback])
    }

    func load() {
        Task {
            let result = await Task.detached(priority: .userInitiated) { () -> ([Candidate], Candidate?, [String: String], [String: String]) in
                var global: [String: String] = [:]
                for key in MachineGit.globalKeys {
                    if let v = LocalGit.globalConfig(key) { global[key.lowercased()] = v }
                }
                var found: [String: String] = [:]
                for name in ["git", "gpg", "ssh-keygen", "gpgsm"] {
                    if let p = MachineGit.find(name) { found[name] = p }
                }
                let candidates = MachineGit.findGits().map { Candidate(path: $0, version: MachineGit.version(of: $0)) }
                let onPath = found["git"].map { p in candidates.first { $0.path == p } ?? Candidate(path: p, version: MachineGit.version(of: p)) }
                return (candidates, onPath, global, found)
            }.value
            candidates = result.0
            onPath = result.1
            global = result.2
            found = result.3
            loaded = true
            loadExcludes()
            for format in SigningRow.formats { loadKeys(format) }
        }
    }

    func loadKeys(_ format: String) {
        Task {
            let json = await Task.detached(priority: .utility) { LocalGit.signingKeysJSON(format: format) }.value
            guard let json, let data = json.data(using: .utf8),
                  let keys = try? JSONDecoder().decode([SigningKey].self, from: data) else { return }
            let email = value("user.email").lowercased()
            self.keys[format] = keys.sorted { a, b in
                let am = !email.isEmpty && a.emails.contains { $0.lowercased() == email }
                let bm = !email.isEmpty && b.emails.contains { $0.lowercased() == email }
                return am != bm ? am : false
            }
        }
    }

    func loadExcludes() {
        let path = (excludesPath as NSString).expandingTildeInPath
        excludesSaved = (try? String(contentsOfFile: path, encoding: .utf8)) ?? ""
        excludes = excludesSaved
    }

    func saveExcludes() {
        let path = (excludesPath as NSString).expandingTildeInPath
        do {
            try FileManager.default.createDirectory(atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
            try excludes.write(toFile: path, atomically: true, encoding: .utf8)
            excludesSaved = excludes
            error = nil
        } catch {
            self.error = "\(error)"
        }
    }

    /// `git config --global key value`（空なら消す）。書けたら読み直す。
    func set(_ key: String, _ value: String?) {
        let v = value.flatMap { $0.isEmpty ? nil : $0 }
        let lower = key.lowercased()
        if v == nil { global.removeValue(forKey: lower) } else { global[lower] = v }
        Task {
            do {
                try await Task.detached(priority: .userInitiated) { try LocalGit.setGlobalConfig(key, v) }.value
                error = nil
                if lower == Self.excludesKey { loadExcludes() }
            } catch {
                self.error = "\(error)"
            }
        }
    }

    // MARK: 探す

    nonisolated private static var pathDirectories: [String] {
        (ProcessInfo.processInfo.environment["PATH"] ?? "").split(separator: ":").map(String.init)
    }

    /// PATH で最初に見つかる `name`。
    nonisolated static func find(_ name: String) -> String? {
        for dir in pathDirectories {
            let p = (dir as NSString).appendingPathComponent(name)
            if FileManager.default.isExecutableFile(atPath: p) { return p }
        }
        return nil
    }

    /// よく置かれる場所と PATH の git（同じ実体は 1 つ）。
    nonisolated static func findGits() -> [String] {
        let usual = ["/usr/bin/git", "/opt/homebrew/bin/git", "/usr/local/bin/git", "/opt/local/bin/git"]
        var seen: Set<String> = []
        var result: [String] = []
        for p in usual + pathDirectories.map({ ($0 as NSString).appendingPathComponent("git") }) {
            guard FileManager.default.isExecutableFile(atPath: p) else { continue }
            let real = (try? FileManager.default.destinationOfSymbolicLink(atPath: p)).map { ($0 as NSString).isAbsolutePath ? $0 : (((p as NSString).deletingLastPathComponent as NSString).appendingPathComponent($0) as NSString).standardizingPath } ?? p
            if seen.insert(real).inserted { result.append(p) }
        }
        return result
    }

    /// `git --version` の版（`2.56.0`、Apple のものは `2.54.0 (Apple Git-157)`）。
    nonisolated static func version(of git: String) -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: git)
        process.arguments = ["--version"]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        guard (try? process.run()) != nil else { return nil }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard let out = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
              out.hasPrefix("git version ") else { return nil }
        return String(out.dropFirst("git version ".count))
    }
}

/// Settings > Git の上の節。
struct MachineGitSettings: View {
    @Bindable var model: MachineGit
    private static let other = "\u{1}other"

    var body: some View {
        Section {
            Picker(L("Git"), selection: $model.gitExecutable) {
                Text(model.onPath.map { L("Automatic (%@)", $0.label) } ?? L("Automatic")).tag("")
                Divider()
                ForEach(model.candidates) { Text($0.label).tag($0.path) }
                if !model.gitExecutable.isEmpty, !model.candidates.contains(where: { $0.path == model.gitExecutable }) {
                    Text(model.gitExecutable).tag(model.gitExecutable)
                }
                Divider()
                Text(L("Other…")).tag(Self.other)
            }
            .onChange(of: model.gitExecutable) { previous, value in
                if value == Self.other { chooseGit(previous: previous) }
            }
            Note(L("The git Amon Hen runs for repositories on this Mac. Automatic takes the first git on the PATH. Repositories opened from now on use the choice; the shared daemon uses it once restarted (Window > Activity). git in a terminal isn’t affected. Needs git 2.23 or later."))
        } header: {
            Text(L("This Mac"))
        }
        Section {
            GlobalTextField(model: model, title: L("Name"), key: "user.name", prompt: "")
            GlobalTextField(model: model, title: L("Email"), key: "user.email", prompt: "")
            Picker(L("Signing"), selection: Binding(get: { signing }, set: { chooseSigning($0) })) {
                Text(L("No signing")).tag("")
                ForEach(SigningRow.formats, id: \.self) { Text(SigningRow.name($0)).tag($0) }
            }
            if !signing.isEmpty {
                GlobalKeyPicker(model: model, format: signing)
            }
            Note(L("For every repository on this Mac (git config --global user.name, user.email, commit.gpgsign, gpg.format and user.signingkey; git in a terminal follows them too). A repository can choose differently in its own Settings."))
        } header: {
            Text(L("Author and Signing"))
        }
        Section {
            ProgramRow(model: model, title: "gpg", keys: MachineGit.gpgKeys, fallback: "gpg")
            ProgramRow(model: model, title: "ssh-keygen", keys: MachineGit.sshKeys, fallback: "ssh-keygen")
            ProgramRow(model: model, title: "gpgsm", keys: MachineGit.x509Keys, fallback: "gpgsm")
            Note(L("The programs git signs and verifies with (gpg.program, gpg.ssh.program and gpg.x509.program). Empty uses the one found on the PATH."))
        } header: {
            Text(L("Signing Programs"))
        }
        Section {
            HStack {
                TextField(L("File"), text: Binding(get: { model.value(MachineGit.excludesKey) }, set: { _ in }),
                          prompt: Text(model.excludesPath))
                    .disabled(true)
                Button(L("Choose…")) { chooseExcludes() }
                if !model.value(MachineGit.excludesKey).isEmpty {
                    Button(L("Use git’s Default")) { model.set("core.excludesFile", nil) }
                }
            }
            VStack(alignment: .leading, spacing: 6) {
                TextEditor(text: $model.excludes)
                    .font(.system(.body, design: .monospaced))
                    .scrollContentBackground(.hidden)
                    .frame(height: 140)
                HStack {
                    Spacer()
                    Button(L("Revert")) { model.loadExcludes() }.disabled(!model.excludesChanged)
                    Button(L("Save")) { model.saveExcludes() }.disabled(!model.excludesChanged)
                }
            }
            Note(L("Patterns ignored in every repository on this Mac (core.excludesFile; git’s default is ~/.config/git/ignore). One pattern per line, like .gitignore."))
        } header: {
            Text(L("Global Excludes"))
        }
        if let error = model.error {
            Section { Text(error).foregroundStyle(.red).font(.caption) }
        }
    }

    private var signing: String {
        guard normalizedBool(model.value("commit.gpgsign")) == "true" else { return "" }
        let f = model.value("gpg.format").lowercased()
        return SigningRow.formats.contains(f) ? f : "openpgp"
    }

    private func chooseSigning(_ value: String) {
        if value.isEmpty {
            model.set("commit.gpgsign", "false")
        } else {
            model.set("commit.gpgsign", "true")
            if model.value("gpg.format").lowercased() != value { model.set("gpg.format", value) }
        }
    }

    private func chooseGit(previous: String) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.showsHiddenFiles = true
        panel.directoryURL = URL(fileURLWithPath: "/usr/local/bin")
        panel.prompt = L("Choose")
        panel.message = L("Choose a git executable")
        if panel.runModal() == .OK, let url = panel.url, FileManager.default.isExecutableFile(atPath: url.path) {
            model.gitExecutable = url.path
        } else {
            model.gitExecutable = previous == Self.other ? "" : previous
        }
    }

    private func chooseExcludes() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.canCreateDirectories = true
        panel.showsHiddenFiles = true
        panel.prompt = L("Choose")
        panel.message = L("Choose the global excludes file")
        if panel.runModal() == .OK, let url = panel.url {
            model.set("core.excludesFile", url.path)
        }
    }
}

/// global の文字の設定: 離れたとき・⏎ で書く。
private struct GlobalTextField: View {
    let model: MachineGit
    let title: String
    let key: String
    let prompt: String
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        TextField(title, text: $text, prompt: prompt.isEmpty ? nil : Text(prompt))
            .focused($focused)
            .onSubmit { commit() }
            .onChange(of: focused) { _, now in if !now { commit() } }
            .onChange(of: model.value(key), initial: true) { _, value in if !focused { text = value } }
    }

    private func commit() {
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        if trimmed != model.value(key) { model.set(key, trimmed) }
    }
}

/// global の `user.signingkey`: この Mac の鍵（種類ごと、自分のメールの鍵が先）から選ぶ。
private struct GlobalKeyPicker: View {
    let model: MachineGit
    let format: String
    private static let other = "\u{1}other"
    @State private var typing = false
    @State private var text = ""
    @FocusState private var focused: Bool

    private var current: String { model.value("user.signingkey") }

    private func kind(of id: String) -> String? {
        SigningRow.formats.first { f in (model.keys[f] ?? []).contains { $0.id == id } }
    }

    private func label(_ value: String) -> String {
        if value.isEmpty { return format == "openpgp" ? L("Key for your email") : L("None") }
        return kind(of: value).flatMap { f in model.keys[f]?.first { $0.id == value }?.label } ?? value
    }

    var body: some View {
        Picker(L("Key"), selection: Binding(get: { typing ? Self.other : current }, set: { choose($0) })) {
            Text(label("")).tag("")
            ForEach(SigningRow.formats, id: \.self) { f in
                if let keys = model.keys[f], !keys.isEmpty {
                    Section(SigningRow.name(f)) { ForEach(keys) { Text($0.label).tag($0.id) } }
                }
            }
            if !current.isEmpty, kind(of: current) == nil {
                Text(current).tag(current)
            }
            Divider()
            Text(L("Other…")).tag(Self.other)
        }
        if let k = kind(of: current), k != format {
            Text("⚠ " + L("This is an %1$@ key, but the format is %2$@.", SigningRow.name(k), SigningRow.name(format)))
                .foregroundStyle(.orange).font(.caption)
        }
        if typing {
            TextField("", text: $text, prompt: Text(format == "ssh" ? "~/.ssh/id_ed25519.pub" : L("Fingerprint or key ID")))
                .focused($focused)
                .onSubmit { commit() }
                .onExitCommand { typing = false }
        }
    }

    private func choose(_ value: String) {
        if value == Self.other {
            text = current
            typing = true
            DispatchQueue.main.async { focused = true }
        } else {
            typing = false
            model.set("user.signingkey", value)
            if let f = kind(of: value), f != format { model.set("gpg.format", f) }
        }
    }

    private func commit() {
        typing = false
        let v = text.trimmingCharacters(in: .whitespaces)
        if v != current { model.set("user.signingkey", v) }
    }
}

/// 署名のプログラム: global の値（空なら PATH で見つかったもの）と「選ぶ…」。
private struct ProgramRow: View {
    let model: MachineGit
    let title: String
    let keys: [String]
    let fallback: String
    @State private var text = ""
    @FocusState private var focused: Bool

    private var program: (value: String, key: String, found: String?) { model.program(keys, fallback) }

    var body: some View {
        HStack {
            TextField(title, text: $text, prompt: Text(program.found.map { L("Found: %@", $0) } ?? L("Not found")))
                .focused($focused)
                .onSubmit { commit() }
                .onChange(of: focused) { _, now in if !now { commit() } }
                .onChange(of: program.value, initial: true) { _, value in if !focused { text = value } }
            Button(L("Choose…")) { choose() }
        }
    }

    private func commit() {
        let v = text.trimmingCharacters(in: .whitespaces)
        if v != program.value { model.set(program.key, v) }
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.showsHiddenFiles = true
        panel.directoryURL = URL(fileURLWithPath: ((program.found ?? "/usr/local/bin") as NSString).deletingLastPathComponent)
        panel.prompt = L("Choose")
        if panel.runModal() == .OK, let url = panel.url {
            text = url.path
            commit()
        }
    }
}
