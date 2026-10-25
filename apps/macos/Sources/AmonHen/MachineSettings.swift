import AmonhenKit
import AppKit
import SwiftUI

/// Settings > Git: 機械ごとの git の設定。リポジトリの設定と同じ画面（同じページ、同じ行）を「すべてのリポジトリ」の段で
/// 開いたもので、`git config --global` に書く。段のために空のリポジトリ（Application Support の中）を握る。
/// 左の一覧の先頭は機械（今はこの Mac だけ。SSH 先は 4.8 の段階 7 で）。
struct GitSettings: View {
    @Bindable var model: SettingsModel
    @State private var settings: RepositorySettingsModel?
    @State private var error: String?

    var body: some View {
        VStack(spacing: 0) {
            if let settings {
                RepositorySettingsView(model: settings, page: .machine)
            } else if let error {
                Text(error).foregroundStyle(.red).padding()
            } else {
                ProgressView().padding()
            }
        }
        .frame(height: 560)
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
            process.executableURL = URL(fileURLWithPath: MachineGit.find("git") ?? "/usr/bin/git")
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

/// この Mac のページ: 使う git と、署名のプログラム（詳細）。機械にしかないもの。
struct MachinePage: View {
    let model: RepositorySettingsModel
    @State private var candidates: [MachineGit.Candidate] = []
    @State private var onPath: MachineGit.Candidate?
    @State private var found: [String: String] = [:]
    @State private var details = false
    @State private var showingDetails = false
    @State private var gitExecutable = Settings.gitExecutable ?? ""
    private static let other = "\u{1}other"

    var body: some View {
        Form {
            Section {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 8) {
                        Text("Git").frame(minWidth: 72, alignment: .leading)
                        Spacer()
                        Picker("", selection: $gitExecutable) {
                            Text(onPath.map { L("Automatic (%@)", $0.label) } ?? L("Automatic")).tag("")
                            Divider()
                            ForEach(candidates) { Text($0.label).tag($0.path) }
                            if !gitExecutable.isEmpty, !candidates.contains(where: { $0.path == gitExecutable }) {
                                Text(gitExecutable).tag(gitExecutable)
                            }
                            Divider()
                            Text(L("Other…")).tag(Self.other)
                        }
                        .labelsHidden().frame(maxWidth: 360)
                        .onChange(of: gitExecutable) { previous, value in
                            if value == Self.other { chooseGit(previous: previous); return }
                            if value != (Settings.gitExecutable ?? "") {
                                Settings.gitExecutable = value.isEmpty ? nil : value
                                Settings.applyGitExecutable()
                            }
                        }
                        if !gitExecutable.isEmpty {
                            Button { gitExecutable = "" } label: { Image(systemName: "arrow.uturn.backward") }
                                .buttonStyle(.borderless)
                                .help(L("Back to the git on the PATH."))
                        } else {
                            Color.clear.frame(width: 20, height: 1)
                        }
                        Text(gitExecutable.isEmpty ? "PATH" : L("This Mac")).font(.caption).foregroundStyle(.secondary).frame(width: 118, alignment: .trailing)
                        Button { showingDetails.toggle() } label: { Image(systemName: "info.circle") }
                            .buttonStyle(.borderless)
                            .popover(isPresented: $showingDetails, arrowEdge: .trailing) {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text("AMONHEN_GIT = \(gitExecutable.isEmpty ? L("(not set)") : gitExecutable)").font(.system(.body, design: .monospaced))
                                    Text(L("On the PATH: %@", onPath?.path ?? L("Not found"))).foregroundStyle(.secondary)
                                }
                                .font(.callout).textSelection(.enabled).padding(12).frame(minWidth: 240, alignment: .leading)
                            }
                    }
                    Text(L("The git Amon Hen runs for repositories on this Mac. Repositories opened from now on use it; git in a terminal isn’t affected."))
                        .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.horizontal, 8)
                }
                .padding(.vertical, 6)
            }
            Section {
                DisclosureGroup(L("Details"), isExpanded: $details) {
                    TextRow(model: model, question: "gpg", help: "", key: "gpg.program", placeholder: foundLabel("gpg"))
                    TextRow(model: model, question: "ssh-keygen", help: "", key: "gpg.ssh.program", placeholder: foundLabel("ssh-keygen"))
                    TextRow(model: model, question: "gpgsm", help: "", key: "gpg.x509.program", placeholder: foundLabel("gpgsm"))
                    Text(L("The programs git signs and verifies with. Empty uses the one found on the PATH."))
                        .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.horizontal, 8)
                }
                Text(L("git in a terminal follows these settings too.")).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .task {
            let result = await Task.detached(priority: .userInitiated) { () -> ([MachineGit.Candidate], MachineGit.Candidate?, [String: String]) in
                var found: [String: String] = [:]
                for name in ["git", "gpg", "ssh-keygen", "gpgsm"] {
                    if let p = MachineGit.find(name) { found[name] = p }
                }
                let candidates = MachineGit.findGits().map { MachineGit.Candidate(path: $0, version: MachineGit.version(of: $0)) }
                let onPath = found["git"].map { p in candidates.first { $0.path == p } ?? MachineGit.Candidate(path: p, version: MachineGit.version(of: p)) }
                return (candidates, onPath, found)
            }.value
            candidates = result.0
            onPath = result.1
            found = result.2
        }
    }

    private func foundLabel(_ name: String) -> String {
        found[name].map { L("Found: %@", $0) } ?? L("Not found")
    }

    private func chooseGit(previous: String) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.showsHiddenFiles = true
        panel.directoryURL = URL(fileURLWithPath: "/usr/local/bin")
        panel.prompt = L("Choose")
        panel.message = L("Choose a git executable")
        if panel.runModal() == .OK, let url = panel.url, FileManager.default.isExecutableFile(atPath: url.path) {
            gitExecutable = url.path
        } else {
            gitExecutable = previous == Self.other ? "" : previous
        }
    }
}

/// この Mac にある git とプログラムを探す。
nonisolated enum MachineGit {
    struct Candidate: Identifiable, Hashable, Sendable {
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

    /// よく置かれる場所と PATH の git（同じ実体は 1 つ）。
    static func findGits() -> [String] {
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
    static func version(of git: String) -> String? {
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
