import AmonhenKit
import AppKit
import SwiftUI

/// Settings > Git: 機械ごとの git の設定。リポジトリの設定と同じ画面（同じページ、同じ行）を「すべてのリポジトリ」の段で
/// 開いたもので、`git config --global` に書く。段のために空のリポジトリ（Application Support の中）を握る。
/// SSH 先は 4.8 の段階 7 で。
struct GitSettings: View {
    @Bindable var model: SettingsModel
    @State private var settings: RepositorySettingsModel?
    @State private var error: String?

    /// 次に Git タブを出すときに開くページ（リポジトリの設定から開くとき）。
    static var pendingPage: SettingsPage?
    /// 出ている Git タブのページを変える（`object` は `SettingsPage`）。
    static let showPage = Notification.Name("GitSettings.showPage")

    var body: some View {
        VStack(spacing: 0) {
            if let settings {
                RepositorySettingsView(model: settings, page: Self.pendingPage
                    ?? AppDelegate.option("--settings-page").flatMap(SettingsPage.init) ?? .author)
                    .onAppear { Self.pendingPage = nil }
            } else if let error {
                Text(error).foregroundStyle(.red).padding()
            } else {
                ProgressView().padding()
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onAppear(perform: open)
    }

    /// 空のリポジトリを用意して（なければ `git init`）、その設定のモデルをすべてのリポジトリの段で作る。
    private func open() {
        if let settings { settings.load(); return }
        Task {
            let result: Result<(Repository, String), Error> = await Task.detached(priority: .userInitiated) {
                do {
                    let dir = try Self.hiddenRepository()
                    return .success((try Repository(path: dir), dir))
                } catch {
                    return .failure(error)
                }
            }.value
            switch result {
            case .success(let (repository, dir)):
                let model = RepositorySettingsModel(repository: repository, location: dir, level: .allRepositories)
                settings = model
                model.load()
            case .failure(let e):
                error = "\(e)"
            }
        }
    }

    /// `~/Library/Application Support/Amon Hen/settings-repo`（空のリポジトリ）。
    nonisolated private static func hiddenRepository() throws -> String {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Amon Hen/settings-repo")
        if !FileManager.default.fileExists(atPath: dir.appendingPathComponent(".git").path) {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let process = Process()
            process.executableURL = URL(fileURLWithPath: Programs.find("git") ?? "/usr/bin/git")
            process.arguments = ["init", "-q"]
            process.currentDirectoryURL = dir
            process.standardOutput = FileHandle.nullDevice
            process.standardError = FileHandle.nullDevice
            try process.run()
            process.waitUntilExit()
            guard process.terminationStatus == 0 else { throw AmonhenError(description: L("Couldn’t prepare the settings repository")) }
        }
        return dir.path
    }
}

/// プログラムのページ: git・gpg・ssh-keygen・gpgsm を同じ形で選ぶ（自動＝PATH の先頭／見つかったもの／その他…）。
/// git は Amon Hen の設定（`AMONHEN_GIT`）、ほかは git の設定（`gpg.program` など）。
struct ProgramsPage: View {
    let model: RepositorySettingsModel
    @State private var found: [String: [Programs.Candidate]] = [:]
    @State private var gitExecutable = Settings.gitExecutable ?? ""
    @State private var showingGit = false

    var body: some View {
        Form {
            Section {
                gitRow
                configRow("gpg", keys: ["gpg.openpgp.program", "gpg.program"])
                configRow("ssh-keygen", keys: ["gpg.ssh.program"])
                configRow("gpgsm", keys: ["gpg.x509.program"])
            }
        }
        .formStyle(.grouped)
        .task {
            found = await Task.detached(priority: .userInitiated) {
                Dictionary(uniqueKeysWithValues: ["git", "gpg", "ssh-keygen", "gpgsm"].map { ($0, Programs.candidates($0)) })
            }.value
        }
    }

    /// git の行: 値は Amon Hen が持つ。行の形はほかと同じ。
    private var gitRow: some View {
        HStack(spacing: 8) {
            Text("git").frame(minWidth: 72, alignment: .leading)
            Spacer()
            ProgramPicker(candidates: found["git"] ?? [], value: gitExecutable) { value in
                setGit(value)
            }
            if !gitExecutable.isEmpty {
                Button { setGit("") } label: { Image(systemName: "arrow.uturn.backward") }
                    .buttonStyle(.borderless)
                    .frame(width: 20)
                    .help(L("Stop overriding here. The value above takes over."))
            } else {
                Color.clear.frame(width: 20, height: 1)
            }
            Text(gitExecutable.isEmpty ? "PATH" : "Amon Hen")
                .font(.caption).foregroundStyle(.secondary).frame(width: 118, alignment: .trailing)
            Button { showingGit.toggle() } label: { Image(systemName: "info.circle") }
                .buttonStyle(.borderless)
                .popover(isPresented: $showingGit, arrowEdge: .trailing) {
                    Text("AMONHEN_GIT = \(gitExecutable.isEmpty ? L("(not set)") : gitExecutable)")
                        .font(.system(.body, design: .monospaced)).textSelection(.enabled).padding(12)
                }
        }
        .padding(.vertical, 6)
    }

    /// 使う git を変える（Undo に前の値を積む。⇧⌘Z でやり直せる）。
    private func setGit(_ value: String) {
        let old = gitExecutable
        guard value != old else { return }
        model.undo?.registerUndo(withTarget: model) { _ in setGit(old) }
        model.undo?.setActionName(L("Change %@", "git"))
        gitExecutable = value
        Settings.gitExecutable = value.isEmpty ? nil : value
        Settings.applyGitExecutable()
    }

    /// git の設定に書くプログラムの行（設定元・↩︎・ⓘ はリポジトリの設定の行と同じ）。
    private func configRow(_ name: String, keys: [String]) -> some View {
        // 決めてあるキーを先に（設定元はその値のもの）。どれもなければ最後のキーに書く。
        let key = keys.first { model.effective($0) != nil } ?? keys[keys.count - 1]
        return SettingRow(model: model, question: name, help: "", keys: [key]) {
            ProgramPicker(candidates: found[name] ?? [], value: model.local(key) ?? "") { value in
                model.set(key, value.isEmpty ? nil : value)
            }
        }
    }
}

/// プログラムを選ぶ: 自動（PATH の先頭）、見つかったもの（出どころと版）、その他…。
private struct ProgramPicker: View {
    let candidates: [Programs.Candidate]
    /// 今の値（空なら自動）。
    let value: String
    let choose: (String) -> Void
    private static let other = "\u{1}other"

    var body: some View {
        Picker("", selection: Binding(get: { value }, set: { picked($0) })) {
            Text(candidates.first.map { L("Automatic (%@)", $0.label) } ?? L("Automatic")).tag("")
            Divider()
            ForEach(candidates) { Text($0.label).tag($0.path) }
            if !value.isEmpty, !candidates.contains(where: { $0.path == value }) {
                Text(value).tag(value)
            }
            Divider()
            Text(L("Other…")).tag(Self.other)
        }
        .labelsHidden()
        // 中身の幅で、右端を揃える（行ごとに矢印の位置が変わらないように）。
        .fixedSize()
        .frame(width: 340, alignment: .trailing)
        .help(value.isEmpty ? (candidates.first?.path ?? "") : value)
    }

    private func picked(_ new: String) {
        guard new == Self.other else {
            if new != value { choose(new) }
            return
        }
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.showsHiddenFiles = true
        panel.directoryURL = URL(fileURLWithPath: ((candidates.first?.path ?? "/usr/local/bin") as NSString).deletingLastPathComponent)
        panel.prompt = L("Choose")
        if panel.runModal() == .OK, let url = panel.url, FileManager.default.isExecutableFile(atPath: url.path) {
            choose(url.path)
        }
    }
}

/// この Mac にあるプログラムを探す。
nonisolated enum Programs {
    struct Candidate: Identifiable, Hashable, Sendable {
        let path: String
        let version: String?
        var id: String { path }
        /// 出どころと版（Homebrew 2.56.0、Apple 2.54.0、MacGPG2 2.2.41）。
        var label: String {
            // 出どころはリンクの先で見る（/usr/local/bin/gpg が MacGPG2 のものなど）。Apple の git は
            // 「2.54.0 (Apple Git-157)」と出すので、版は最初の語だけ。
            [Candidate.source(of: URL(fileURLWithPath: path).resolvingSymlinksInPath().path),
             version.flatMap { $0.split(separator: " ").first.map(String.init) }]
                .compactMap { $0 }.joined(separator: " ")
        }

        static func source(of path: String) -> String {
            if path.hasPrefix("/opt/homebrew/") || path.hasPrefix("/usr/local/Cellar/") || path.hasPrefix("/home/linuxbrew/") { return "Homebrew" }
            if path.hasPrefix("/usr/local/MacGPG2/") { return "MacGPG2" }
            if path.hasPrefix("/usr/bin/") || path.hasPrefix("/Applications/Xcode") || path.hasPrefix("/Library/Developer/") { return "Apple" }
            if path.hasPrefix("/opt/local/") { return "MacPorts" }
            return (path as NSString).deletingLastPathComponent
        }
    }

    private static var pathDirectories: [String] {
        (ProcessInfo.processInfo.environment["PATH"] ?? "").split(separator: ":").map(String.init)
    }

    /// PATH で最初に見つかる `name`。
    static func find(_ name: String) -> String? {
        for dir in pathDirectories {
            let p = (dir as NSString).appendingPathComponent(name)
            if FileManager.default.isExecutableFile(atPath: p) { return p }
        }
        return nil
    }

    /// `name` の候補。先頭は PATH で最初に見つかるもの（自動で使われるもの）、続いてよく置かれる場所と PATH の残り。
    /// 同じ実体は 1 つ。
    static func candidates(_ name: String) -> [Candidate] {
        let usual = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/local/MacGPG2/bin", "/opt/local/bin", "/usr/bin"]
        var seen: Set<String> = []
        var paths: [String] = []
        for p in [find(name)].compactMap({ $0 }) + (usual + pathDirectories).map({ ($0 as NSString).appendingPathComponent(name) }) {
            guard FileManager.default.isExecutableFile(atPath: p) else { continue }
            let real = URL(fileURLWithPath: p).resolvingSymlinksInPath().path
            if seen.insert(real).inserted { paths.append(p) }
        }
        return paths.map { Candidate(path: $0, version: version(name, at: $0)) }
    }

    /// 版。git・gpg・gpgsm は `--version` の 1 行目の最後の語。ssh-keygen は同じ場所の `ssh -V`（OpenSSH_9.9p2）。
    static func version(_ name: String, at path: String) -> String? {
        if name == "ssh-keygen" {
            let ssh = ((path as NSString).deletingLastPathComponent as NSString).appendingPathComponent("ssh")
            guard let out = run(ssh, ["-V"]), let r = out.range(of: "OpenSSH_") else { return nil }
            return String(out[r.upperBound...].prefix { !$0.isWhitespace && $0 != "," })
        }
        guard let line = run(path, ["--version"])?.split(separator: "\n").first else { return nil }
        if name == "git" { return line.hasPrefix("git version ") ? String(line.dropFirst("git version ".count)) : nil }
        return line.split(separator: " ").last.map(String.init)
    }

    private static func run(_ path: String, _ arguments: [String]) -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = arguments
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        guard (try? process.run()) != nil else { return nil }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
