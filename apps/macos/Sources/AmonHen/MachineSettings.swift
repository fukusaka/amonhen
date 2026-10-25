import AmonhenKit
import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// この Mac の git の設定（Settings > Git）: 使う git、すべてのリポジトリの作成者と署名、global excludes、gpg などの
/// プログラム。`git config --global` を書く。端末の git もこれに従う。
@Observable @MainActor
final class MachineGit {
    struct Candidate: Identifiable, Hashable {
        let path: String
        let version: String?
        var id: String { path }
        /// 短い名前と版（Homebrew 2.56.0、Apple 2.54.0 (Apple Git-157)、/usr/local 2.55.0）。
        var label: String { "\(Candidate.name(of: path)) \(version ?? "?")" }
        static func name(of path: String) -> String {
            if path.hasPrefix("/opt/homebrew/") || path.hasPrefix("/usr/local/Cellar/") || path.hasPrefix("/home/linuxbrew/") { return "Homebrew" }
            if path == "/usr/bin/git" || path.hasPrefix("/Applications/Xcode") || path.hasPrefix("/Library/Developer/") { return "Apple" }
            if path.hasPrefix("/opt/local/") { return "MacPorts" }
            return (path as NSString).deletingLastPathComponent
        }
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


/// Settings > Git: 機械ごとの git の設定。左に機械（この Mac。SSH 先は 4.8 の段階 7 で）、右にその機械の行を
/// リポジトリの設定と同じ形で（値、設定元、↩︎、一文の help、ⓘ）。
struct GitSettings: View {
    @Bindable var model: SettingsModel
    @State private var machine = MachineGit()
    @State private var selection: String? = "local"

    var body: some View {
        HStack(spacing: 0) {
            List(selection: $selection) {
                Label(L("This Mac"), systemImage: "laptopcomputer").tag("local")
            }
            .listStyle(.sidebar)
            .frame(width: 180)
            Divider()
            Form {
                MachineRows(model: machine)
            }
            .formStyle(.grouped)
        }
        .frame(minHeight: 480)
        .onAppear { if !machine.loaded { machine.load() } }
    }
}

/// 1 つの機械の行。
private struct MachineRows: View {
    @Bindable var model: MachineGit
    @State private var editingExcludes = false
    @State private var details = false
    private static let other = "\u{1}other"

    var body: some View {
        Section {
            MachineRow(question: "Git", help: L("The git Amon Hen runs for repositories on this Mac. Repositories opened from now on use it; git in a terminal isn’t affected."),
                       set: !model.gitExecutable.isEmpty, revert: { model.gitExecutable = "" },
                       details: [("AMONHEN_GIT", model.gitExecutable.isEmpty ? nil : model.gitExecutable), ("PATH", model.onPath?.path)],
                       unsetLabel: "PATH") {
                Picker("", selection: $model.gitExecutable) {
                    Text(model.onPath.map { L("Automatic (%@)", $0.label) } ?? L("Automatic")).tag("")
                    Divider()
                    ForEach(model.candidates) { Text($0.label).tag($0.path) }
                    if !model.gitExecutable.isEmpty, !model.candidates.contains(where: { $0.path == model.gitExecutable }) {
                        Text(model.gitExecutable).tag(model.gitExecutable)
                    }
                    Divider()
                    Text(L("Other…")).tag(Self.other)
                }
                .labelsHidden().frame(maxWidth: 300)
                .onChange(of: model.gitExecutable) { previous, value in
                    if value == Self.other { chooseGit(previous: previous) }
                }
            }
        }
        Section(L("Author and Signing")) {
            MachineRow(question: L("Name"), help: "", set: !model.value("user.name").isEmpty,
                       revert: { model.set("user.name", nil) }, details: [("user.name", model.value("user.name"))]) {
                GlobalTextField(model: model, key: "user.name").frame(width: 260)
            }
            MachineRow(question: L("Email"), help: "", set: !model.value("user.email").isEmpty,
                       revert: { model.set("user.email", nil) }, details: [("user.email", model.value("user.email"))]) {
                GlobalTextField(model: model, key: "user.email").frame(width: 260)
            }
            MachineRow(question: L("Signing"), help: L("Signs every commit on this Mac unless a repository chooses otherwise."),
                       set: !model.value("commit.gpgsign").isEmpty || !model.value("gpg.format").isEmpty,
                       revert: { model.set("commit.gpgsign", nil); model.set("gpg.format", nil) },
                       details: [("commit.gpgsign", model.value("commit.gpgsign")), ("gpg.format", model.value("gpg.format"))]) {
                Picker("", selection: Binding(get: { signing }, set: { chooseSigning($0) })) {
                    Text(L("No signing")).tag("")
                    ForEach(SigningRow.formats, id: \.self) { Text(SigningRow.name($0)).tag($0) }
                }
                .labelsHidden().fixedSize()
            }
            if !signing.isEmpty {
                MachineRow(question: L("Key"), help: L("The keys gpg, ~/.ssh, ssh-agent and gpgsm know, the ones for your email first. Choosing a key chooses its format too. Empty picks gpg’s key for your email."),
                           set: !model.value("user.signingkey").isEmpty, revert: { model.set("user.signingkey", nil) },
                           details: [("user.signingkey", model.value("user.signingkey"))]) {
                    GlobalKeyPicker(model: model, format: signing).frame(maxWidth: 320)
                }
            }
        }
        Section {
            MachineRow(question: L("Ignore everywhere"), help: L("Patterns ignored in every repository on this Mac, like .gitignore."),
                       set: !model.value(MachineGit.excludesKey).isEmpty, revert: { model.set("core.excludesFile", nil) },
                       details: [("core.excludesFile", model.value(MachineGit.excludesKey).isEmpty ? nil : model.value(MachineGit.excludesKey))]) {
                HStack(spacing: 8) {
                    Text(model.excludesPath).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                    Button(L("Edit…")) { editingExcludes = true }
                }
            }
            DisclosureGroup(L("Details"), isExpanded: $details) {
                ProgramRow(model: model, title: "gpg", keys: MachineGit.gpgKeys, fallback: "gpg")
                ProgramRow(model: model, title: "ssh-keygen", keys: MachineGit.sshKeys, fallback: "ssh-keygen")
                ProgramRow(model: model, title: "gpgsm", keys: MachineGit.x509Keys, fallback: "gpgsm")
                Text(L("The programs git signs and verifies with. Empty uses the one found on the PATH."))
                    .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.horizontal, 8)
            }
            Text(L("git in a terminal follows these settings too.")).foregroundStyle(.secondary)
            if let error = model.error {
                Text(error).foregroundStyle(.red).font(.caption)
            }
        }
        .sheet(isPresented: $editingExcludes) { ExcludesSheet(model: model) }
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
}

/// 行: 問い、欄、↩︎（この Mac で決めているとき）、設定元、ⓘ。下に一文の help。リポジトリの設定の行と同じ形。
private struct MachineRow<Control: View>: View {
    let question: String
    let help: String
    /// この Mac で決めているか（でなければ git の既定）。
    let set: Bool
    let revert: () -> Void
    /// ⓘ に出すキーと値（nil は未設定）。
    let details: [(key: String, value: String?)]
    /// 決めていないときの設定元の言葉（git の設定でない行は変える）。
    var unsetLabel: String = L("git’s default")
    @ViewBuilder let control: () -> Control
    @State private var showingDetails = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(question).fixedSize()
                Spacer()
                control()
                if set {
                    Button { revert() } label: { Image(systemName: "arrow.uturn.backward") }
                        .buttonStyle(.borderless)
                        .help(L("Back to git’s default."))
                } else {
                    Color.clear.frame(width: 20, height: 1)
                }
                Text(set ? L("This Mac") : unsetLabel).font(.callout).foregroundStyle(.secondary).frame(width: 110, alignment: .trailing)
                Button { showingDetails.toggle() } label: { Image(systemName: "info.circle") }
                    .buttonStyle(.borderless)
                    .popover(isPresented: $showingDetails, arrowEdge: .trailing) {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(details, id: \.key) { d in
                                if let v = d.value, !v.isEmpty {
                                    Text("\(d.key) = \(v)").font(.system(.body, design: .monospaced))
                                } else {
                                    Text(L("%@ is not set", d.key)).font(.system(.body, design: .monospaced))
                                }
                            }
                            Text("~/.gitconfig").foregroundStyle(.secondary)
                        }
                        .font(.callout).textSelection(.enabled).padding(12).frame(minWidth: 240, alignment: .leading)
                    }
            }
            if !help.isEmpty {
                Text(help).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.horizontal, 8)
            }
        }
        .padding(.vertical, 4)
    }
}

/// global の文字の設定: 離れたとき・⏎ で書く。
private struct GlobalTextField: View {
    let model: MachineGit
    let key: String
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        TextField("", text: $text, prompt: Text(L("Not set")))
            .labelsHidden()
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
        VStack(alignment: .trailing, spacing: 4) {
            Picker("", selection: Binding(get: { typing ? Self.other : current }, set: { choose($0) })) {
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
            .labelsHidden()
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
        MachineRow(question: title, help: "", set: !program.value.isEmpty, revert: { model.set(program.key, nil) },
                   details: [(program.key, program.value.isEmpty ? nil : program.value)]) {
            HStack(spacing: 8) {
                TextField("", text: $text, prompt: Text(program.found.map { L("Found: %@", $0) } ?? L("Not found")))
                    .labelsHidden()
                    .frame(width: 260)
                    .focused($focused)
                    .onSubmit { commit() }
                    .onChange(of: focused) { _, now in if !now { commit() } }
                    .onChange(of: program.value, initial: true) { _, value in if !focused { text = value } }
                Button(L("Choose…")) { choose() }
            }
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

/// global excludes のシート: ファイルの場所と中身。
private struct ExcludesSheet: View {
    @Bindable var model: MachineGit
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(L("File")).foregroundStyle(.secondary)
                Text(model.excludesPath).lineLimit(1).truncationMode(.middle)
                Spacer()
                Button(L("Choose…")) { chooseFile() }
                if !model.value(MachineGit.excludesKey).isEmpty {
                    Button(L("Use git’s Default")) { model.set("core.excludesFile", nil) }
                }
            }
            TextEditor(text: $model.excludes)
                .font(.system(.body, design: .monospaced))
                .frame(width: 520, height: 280)
            HStack {
                Text(L("One pattern per line, like .gitignore.")).foregroundStyle(.secondary)
                Spacer()
                Button(L("Cancel")) { model.loadExcludes(); dismiss() }.keyboardShortcut(.cancelAction)
                Button(L("Save")) { model.saveExcludes(); dismiss() }.keyboardShortcut(.defaultAction).disabled(!model.excludesChanged)
            }
        }
        .padding()
        .onAppear { model.loadExcludes() }
    }

    private func chooseFile() {
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
