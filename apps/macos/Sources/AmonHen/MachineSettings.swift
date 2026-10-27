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
/// git は git を動かす機械（この Mac か SSH 先）の Amon Hen の設定（`settings.json`。そこの CLI・daemon・VSCode 拡張も
/// 従う）、ほかは git の設定（`gpg.program` など）。SSH 先（Settings > SSH Hosts）なら候補も設定もコアが SSH 先で
/// 読み書きする（`programs`・`machineGit`）。
struct ProgramsPage: View {
    let model: RepositorySettingsModel
    @State private var found: [String: [Programs.Candidate]] = [:]
    /// 使う git と、それをどこで決めたか（読めるまで nil）。
    @State private var git: MachineGit?
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
            await readGit()
            let names = ["git", "gpg", "ssh-keygen", "gpgsm"]
            if model.host == nil {
                found = await Task.detached(priority: .userInitiated) {
                    Dictionary(uniqueKeysWithValues: names.map { ($0, Programs.candidates($0)) })
                }.value
                // --file-picker [program | repository | folder] で、この Mac のものを選ぶシートを開く（見た目の確認用）:
                // git の「その他…」、Repositories の「手元のリポジトリ…」「スキャンするフォルダ…」。
                if let option = AppDelegate.option("--file-picker") {
                    let window = SettingsWindowController.shared.window
                    switch option {
                    case "repository": RepositoryBrowser.chooseRepositories(in: window) { _ in }
                    case "folder": RepositoryBrowser.chooseScanFolder(in: window) { _ in }
                    default: ProgramPicker.chooseLocal(value: git?.chosen ?? found["git"]?.first?.path ?? "", in: window) { _ in }
                    }
                }
            } else {
                let repository = model.repository
                found = await Task.detached(priority: .userInitiated) {
                    struct Found: Decodable { struct Program: Decodable { let path: String; let version: String? }; let programs: [Program] }
                    return Dictionary(uniqueKeysWithValues: names.map { name in
                        let programs = (try? repository.info(["op": "programs", "name": name], as: Found.self))?.programs ?? []
                        return (name, programs.map { Programs.Candidate(path: $0.path, version: $0.version, onHost: true) })
                    })
                }.value
                // --host-file-picker [folder | connect] で、git の「その他…」のシートを開く（見た目の確認用）。folder なら
                // フォルダを選ぶ形、connect ならそれを SSH 先を選べる形で、つなぎ直して開く。
                if CommandLine.arguments.contains("--host-file-picker"), let onHost {
                    let window = SettingsWindowController.shared.window
                    let option = AppDelegate.option("--host-file-picker")
                    if option == "folder" || option == "connect" {
                        let connect = option == "connect"
                        FilePicker.present(title: L("Add a Folder to Scan over SSH"),
                                                 message: L("The engine on the host searches up to 3 levels below the folder."),
                                                 kind: .folder, host: onHost.name, repository: connect ? nil : onHost.repository,
                                                 hosts: connect ? SSHHosts.candidates() : nil, initial: "~/prj", in: window) { _ in }
                    } else {
                        ProgramPicker.chooseOnHost(onHost, value: git?.chosen ?? found["git"]?.first?.path ?? "", in: window) { _ in }
                    }
                }
            }
        }
    }

    /// SSH 先なら、その名前と、SSH 先のエンジンにつないだリポジトリ（「その他…」で SSH 先の中を見て選ぶ）。
    private var onHost: ProgramPicker.Host? {
        model.host.map { ProgramPicker.Host(name: $0, repository: model.repository) }
    }

    /// git の行: 値はその機械の Amon Hen の設定。行の形はほかと同じ。選んだ git がなければ、その下に理由
    /// （git を動かす操作はエラーになる）。
    private var gitRow: some View {
        let chosen = git?.chosen ?? ""
        return VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                Text("git").frame(minWidth: 72, alignment: .leading)
                Spacer()
                ProgramPicker(candidates: found["git"] ?? [], value: chosen, onHost: onHost) { value in
                    setGit(value)
                }
                if !chosen.isEmpty {
                    Button { setGit("") } label: { Image(systemName: "arrow.uturn.backward") }
                        .buttonStyle(.borderless)
                        .frame(width: 20)
                        .help(L("Stop overriding here. The value above takes over."))
                } else {
                    Color.clear.frame(width: 20, height: 1)
                }
                Text(source)
                    .font(.caption).foregroundStyle(.secondary).frame(width: 118, alignment: .trailing)
                Button { showingGit.toggle() } label: { Image(systemName: "info.circle") }
                    .buttonStyle(.borderless)
                    .popover(isPresented: $showingGit, arrowEdge: .trailing) { gitDetail }
            }
            if let problem = git?.problem {
                Label(problem, systemImage: "exclamationmark.triangle.fill")
                    .font(.caption).foregroundStyle(.orange).textSelection(.enabled)
            }
        }
        .padding(.vertical, 6)
    }

    /// 設定元の欄: 決めたところ。
    private var source: String {
        switch git?.source {
        case "settings": "Amon Hen"
        case "environment": "AMONHEN_GIT"
        case "fallback": "AMONHEN_FALLBACK_GIT"
        default: "PATH"
        }
    }

    /// ⓘ: 置き場所と値、今使う git。環境変数で上書きしていれば、そのこと。
    private var gitDetail: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let file = git?.file {
                Text(model.host == nil ? (file as NSString).abbreviatingWithTildeInPath : file)
                    .font(.system(.body, design: .monospaced))
            }
            Text(git?.chosen.map { "\"git\": \"\($0)\"" } ?? L("No git chosen: the first git on the PATH"))
                .font(.system(.body, design: .monospaced))
            Text(model.host.map { L("The engine, the CLI and the VSCode extension on %@ use it too.", $0) }
                ?? L("The CLI, the shared daemon and the VSCode extension on this machine use it too."))
                .font(.caption).foregroundStyle(.secondary)
            if git?.source == "environment", let program = git?.program {
                Text(model.host == nil ? L("AMONHEN_GIT in the app’s environment overrides it: %@", program)
                    : L("AMONHEN_GIT in the engine’s environment overrides it: %@", program))
                    .font(.caption).foregroundStyle(.orange)
            }
        }
        .textSelection(.enabled).padding(12).frame(width: 380, alignment: .leading)
    }

    /// 使う git を、その機械の Amon Hen の設定から読み直す。
    private func readGit() async {
        let repository = model.repository
        git = await Task.detached(priority: .userInitiated) {
            try? repository.info(["op": "machineGit"], as: MachineGit.self)
        }.value
        model.gitProblem = git?.problem
    }

    /// 使う git を変える（Undo に前の値を積む。⇧⌘Z でやり直せる）。SSH 先なら、SSH 先の設定に書き、つながりは
    /// そのままで次の操作から効く。
    private func setGit(_ value: String) {
        let old = git?.chosen ?? ""
        guard value != old else { return }
        model.undo?.registerUndo(withTarget: model) { _ in setGit(old) }
        model.undo?.setActionName(L("Change %@", "git"))
        git?.chosen = value.isEmpty ? nil : value
        let repository = model.repository
        Task {
            let failed = await Task.detached(priority: .userInitiated) { () -> String? in
                var command: [String: Any] = ["op": "setMachineGit"]
                if !value.isEmpty { command["git"] = value }
                do { try repository.run(command); return nil } catch { return "\(error)" }
            }.value
            if let failed { model.error = failed }
            await readGit()
        }
    }

    /// git の設定に書くプログラムの行（設定元・↩︎・ⓘ はリポジトリの設定の行と同じ）。
    private func configRow(_ name: String, keys: [String]) -> some View {
        // 決めてあるキーを先に（設定元はその値のもの）。どれもなければ最後のキーに書く。
        let key = keys.first { model.effective($0) != nil } ?? keys[keys.count - 1]
        return SettingRow(model: model, question: name, help: "", keys: [key]) {
            ProgramPicker(candidates: found[name] ?? [], value: model.local(key) ?? "", onHost: onHost) { value in
                model.set(key, value.isEmpty ? nil : value)
            }
        }
    }
}

/// プログラムを選ぶ: 自動（PATH の先頭）、見つかったもの（出どころと版）、その他…。
struct ProgramPicker: View {
    /// SSH 先: その名前と、SSH 先のエンジンにつないだリポジトリ。
    struct Host {
        let name: String
        let repository: Repository
    }

    let candidates: [Programs.Candidate]
    /// 今の値（空なら自動）。
    let value: String
    /// SSH 先のプログラムなら、その SSH 先（「その他…」はこの Mac ではなく、SSH 先の中を見て選ぶ）。
    var onHost: Host? = nil
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
        if let onHost {
            let value = value
            Self.chooseOnHost(onHost, value: value.isEmpty ? candidates.first?.path ?? "" : value, in: nil) { path in
                if let path, path != value { choose(path) }
            }
            return
        }
        let value = value
        Self.chooseLocal(value: value.isEmpty ? candidates.first?.path ?? "/usr/local/bin" : value, in: nil) { path in
            if let path, path != value { choose(path) }
        }
    }

    /// この Mac の実行ファイルを選ぶシート。最初は `value` を選んだところ。
    static func chooseLocal(value: String, in window: NSWindow?, done: @escaping (String?) -> Void) {
        let places: [String] = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "~/.local/bin"].filter {
            FileManager.default.fileExists(atPath: ($0 as NSString).expandingTildeInPath)
        }
        FilePicker.presentLocal(title: L("Program"), message: L("Choose the program on this Mac."), kind: .executable,
                                initial: value, places: places, in: window) { done($0?.first) }
    }

    /// SSH 先の実行ファイルを選ぶシート。最初は `value` を選んだところ。
    static func chooseOnHost(_ host: Host, value: String, in window: NSWindow?, done: @escaping (String?) -> Void) {
        FilePicker.present(title: L("Program on %@", host.name), message: L("Choose the program on the host."),
                           kind: .executable, host: host.name, repository: host.repository, initial: value,
                           places: ["/usr/local/bin", "/usr/bin", "~/.local/bin", "~/bin"], in: window) {
            done($0?.path)
        }
    }
}

/// この Mac にあるプログラムを探す。
nonisolated enum Programs {
    struct Candidate: Identifiable, Hashable, Sendable {
        let path: String
        let version: String?
        /// SSH 先のもの（出どころはフォルダで言う。この Mac の Homebrew・Apple の見分けは当てはまらない）。
        var onHost = false
        var id: String { path }
        /// 出どころと版（Homebrew 2.56.0、Apple 2.54.0、MacGPG2 2.2.41。SSH 先では /usr/bin 2.43.0）。
        var label: String {
            // 出どころはリンクの先で見る（/usr/local/bin/gpg が MacGPG2 のものなど）。Apple の git は
            // 「2.54.0 (Apple Git-157)」と出すので、版は最初の語だけ。
            let source = onHost
                ? (path as NSString).deletingLastPathComponent
                : Candidate.source(of: URL(fileURLWithPath: path).resolvingSymlinksInPath().path)
            return [source, version.flatMap { $0.split(separator: " ").first.map(String.init) }]
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
            guard FileManager.default.isExecutableFile(atPath: p), !isUninstalledAppleTool(p) else { continue }
            let real = URL(fileURLWithPath: p).resolvingSymlinksInPath().path
            if seen.insert(real).inserted { paths.append(p) }
        }
        return paths.map { Candidate(path: $0, version: version(name, at: $0)) }
    }

    /// `/usr/bin` の git などは、Command Line Tools（か Xcode）の中のものを呼ぶ入り口。入っていなければ、動かすと
    /// インストールを勧めるダイアログが出るので、候補にしない（版を調べるだけでも出てしまう）。
    private static func isUninstalledAppleTool(_ path: String) -> Bool {
        guard ["/usr/bin/git"].contains(path) else { return false }
        guard let developer = run("/usr/bin/xcode-select", ["-p"]), !developer.isEmpty else { return true }
        return !FileManager.default.isExecutableFile(atPath: (developer as NSString).appendingPathComponent("usr/bin/git"))
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

// MARK: - 環境変数

/// Settings > Git > 環境変数: この Mac で git と ssh に渡す環境変数。ログインシェルのものを使うかと、表で見て足すものを
/// 選ぶ。値はアプリの設定（`Settings`）に置き、変えたらすぐ効かせる（`ShellEnvironment.refresh`。共有の daemon への
/// 今のつながりにも）。ほかのページと同じく ⌘Z で戻せる。
struct EnvironmentPage: View {
    let model: RepositorySettingsModel
    @State private var useShell = Settings.importShellEnvironment
    @State private var added = Settings.addedEnvironmentNames

    var body: some View {
        GroupedPage {
            VStack(alignment: .leading, spacing: 10) {
                Toggle(L("Use the login shell’s environment variables"), isOn: Binding(get: { useShell }, set: { setUseShell($0) }))
                Note(L("Reads the PATH, SSH agent and GIT_* that an app opened from the Finder lacks, for git and ssh."))
                ShellEnvironmentView()
                Divider()
                EnvironmentTable(useShell: useShell, addedNames: Binding(get: { added }, set: { setAdded($0) }),
                                 tableHeight: nil, detailHeight: 120)
            }
        }
    }

    private func setUseShell(_ value: Bool) {
        let old = useShell
        guard value != old else { return }
        model.undo?.registerUndo(withTarget: model) { _ in setUseShell(old) }
        model.undo?.setActionName(L("Change %@", L("Environment Variables")))
        useShell = value
        Settings.importShellEnvironment = value
        ShellEnvironment.refresh()
    }

    private func setAdded(_ value: String) {
        let old = added
        guard value != old else { return }
        model.undo?.registerUndo(withTarget: model) { _ in setAdded(old) }
        model.undo?.setActionName(L("Change %@", L("Environment Variables")))
        added = value
        Settings.addedEnvironmentNames = value
        ShellEnvironment.refresh()
    }
}

/// 取り込んだ変数とその値。読み直すボタン付き（シェルの設定を変えたあとに）。
private struct ShellEnvironmentView: View {
    @State private var values = ShellEnvironment.values
    @State private var shell = ShellEnvironment.shell
    @State private var lastRead = ShellEnvironment.lastRead
    @State private var reloading = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(status).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button(L("Read Again")) {
                    reloading = true
                    ShellEnvironment.reload()
                }
                .disabled(reloading || !ShellEnvironment.isEnabled)
            }
            if let socket = values["SSH_AUTH_SOCK"] { agentStatus(socket) }
        }
        .onReceive(NotificationCenter.default.publisher(for: ShellEnvironment.didChange)) { _ in
            values = ShellEnvironment.values
            shell = ShellEnvironment.shell
            lastRead = ShellEnvironment.lastRead
            reloading = false
        }
    }

    /// SSH の agent のソケットがあるか。なければ、ssh・git に代わりに渡すもの（起動したときの agent）。
    @ViewBuilder private func agentStatus(_ path: String) -> some View {
        if ShellEnvironment.isSocket(path) {
            Text(L("SSH agent found")).font(.caption).foregroundStyle(.secondary)
        } else if let launch = ShellEnvironment.launchAgent, launch != path, ShellEnvironment.isSocket(launch) {
            Text(L("No SSH agent here; ssh and git use %@", launch)).font(.caption).foregroundStyle(.orange)
        } else {
            Text(L("No SSH agent here, so keys that need a passphrase can’t be used")).font(.caption).foregroundStyle(.orange)
        }
    }

    private var status: String {
        if !ShellEnvironment.isEnabled { return L("Off") }
        guard let lastRead else { return shell.isEmpty ? L("Not read yet") : L("Couldn’t read %@", shell) }
        return L("Read from %@ at %@", shell, lastRead.formatted(date: .omitted, time: .shortened))
    }
}

/// 環境変数の表: シェル（使わないときは起動したときの環境）の変数を値と一緒に並べ、渡すものをチェックで選ぶ。
/// いつも渡すもの（git と ssh が使う）は外せず、渡せないものは選べない（理由はヘルプ）。案: 決まりはコアのものにする。
struct EnvironmentTable: View {
    /// ログインシェルの環境変数を使うか（使わないときは、起動したときの環境を並べる）。
    let useShell: Bool
    /// 足した名前（コンマ区切り。`Settings.addedEnvironmentNames`）。
    @Binding var addedNames: String
    /// 表の高さ（nil なら置き場所に合わせて伸びる）と、詳しい欄の高さ。
    var tableHeight: CGFloat? = 150
    var detailHeight: CGFloat = 96
    @State private var source = ShellEnvironment.shellValues
    @State private var showAll = true
    @State private var filter = ""
    /// 選んだ行（表の下に値の全体を出す）。前に選んだものを覚えておく。
    @AppStorage("environmentTableSelection") private var selection: String = ""
    @State private var revealed = false

    enum Kind { case always, chosen, other, refused(String) }

    struct Row: Identifiable {
        let name: String
        let value: String
        let kind: Kind
        var id: String { name }
        var passed: Bool { if case .always = kind { true } else if case .chosen = kind { true } else { false } }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
        HStack {
            Picker("", selection: $showAll) {
                Text(L("Passed")).tag(false)
                Text(L("All")).tag(true)
            }
            .pickerStyle(.segmented).labelsHidden().fixedSize()
            Spacer()
            TextField("", text: $filter, prompt: Text(L("Search by name or value")))
                .textFieldStyle(.roundedBorder)
                .frame(minWidth: 60, maxWidth: 240)
        }
        Table(rows, selection: Binding(get: { selection.isEmpty ? nil : selection }, set: { selection = $0 ?? ""; revealed = false })) {
            TableColumn(L("Pass")) { row in
                Toggle("", isOn: Binding(get: { row.passed }, set: { set(row.name, $0) }))
                    .labelsHidden()
                    .disabled({ if case .other = row.kind { false } else if case .chosen = row.kind { false } else { true } }())
                    .help(help(row))
            }
            .width(40)
            TableColumn(L("Name")) { row in
                HStack(spacing: 6) {
                    Text(row.name).font(.body.monospaced())
                        .foregroundStyle({ if case .refused = row.kind { Color.secondary } else { Color.primary } }())
                    // いつも渡すもの（外せない）は、名前の横に印を付ける。
                    if case .always = row.kind {
                        Text(verbatim: "Always").font(.caption2).foregroundStyle(.secondary).fixedSize()
                            .padding(.horizontal, 5).padding(.vertical, 1)
                            .overlay(RoundedRectangle(cornerRadius: 4).stroke(.secondary.opacity(0.5)))
                    }
                }
                .help(help(row))
            }
            .width(min: 120, ideal: 180)
            TableColumn(L("Value")) { row in
                Text(display(row)).font(.callout.monospaced()).foregroundStyle(.secondary)
                    .lineLimit(1).truncationMode(.middle).help(display(row))
            }
        }
        .frame(minHeight: 100, idealHeight: tableHeight, maxHeight: tableHeight ?? .infinity)
        EnvironmentValueView(name: selection, value: values[selection], masked: isSecret(selection) && !revealed,
                             reveal: { revealed = true }, height: detailHeight)
        Note(L("Checked variables go to the git and ssh this app runs, through the shared daemon too, but not to SSH hosts. Always passed, when the shell sets them: %@.", PassedEnvironment.alwaysNames.joined(separator: ", ")))
        }
        .onReceive(NotificationCenter.default.publisher(for: ShellEnvironment.didChange)) { _ in
            source = ShellEnvironment.shellValues
        }
    }

    private var values: [String: String] {
        useShell && !source.isEmpty ? source : ShellEnvironment.launchEnvironment
    }

    private var added: Set<String> {
        Set(addedNames.split(whereSeparator: { $0 == "," || $0.isWhitespace }).map(String.init))
    }

    private var rows: [Row] {
        let f = filter.trimmingCharacters(in: .whitespaces)
        // 渡すものを先に、そのあとにほかの変数（どちらも名前の順）。
        let names = values.keys.sorted()
        let ordered = names.filter { Row(name: $0, value: "", kind: kind(of: $0)).passed }
            + names.filter { !Row(name: $0, value: "", kind: kind(of: $0)).passed }
        return ordered.compactMap { name in
            let value = values[name, default: ""]
            let row = Row(name: name, value: value, kind: kind(of: name))
            guard showAll || row.passed else { return nil }
            guard f.isEmpty || Self.matches(name: name, value: value, filter: f) else { return nil }
            return row
        }
    }

    /// 行の扱い（決まりはコアのもの）。
    private func kind(of name: String) -> Kind {
        if let reason = PassedEnvironment.refusal(name) { return .refused(reason) }
        if PassedEnvironment.always(name) { return .always }
        return added.contains(name) ? .chosen : .other
    }

    private func help(_ row: Row) -> String {
        switch row.kind {
        case .always: L("git and ssh always get this")
        case .refused(let reason): reason
        case .chosen, .other: ""
        }
    }

    /// 値。トークン・パスワード・鍵らしい名前のものは伏せる（画面に出したまま共有しないように）。
    private func display(_ row: Row) -> String {
        isSecret(row.name) ? "••••••••" : row.value
    }

    private func isSecret(_ name: String) -> Bool { Self.isSecret(name) }

    /// トークン・パスワード・鍵らしい名前か（値を伏せる）。
    /// 検索に合うか。伏せた値は探さない（一部を入れて当たるかで、値を当てられてしまう）。名前では探せる。
    static func matches(name: String, value: String, filter: String) -> Bool {
        name.localizedCaseInsensitiveContains(filter)
            || (!isSecret(name) && value.localizedCaseInsensitiveContains(filter))
    }

    /// 値を伏せるか（決まりは `SecretNames`）。
    static func isSecret(_ name: String) -> Bool { SecretNames.isSecret(name) }

    private func set(_ name: String, _ on: Bool) {
        var names = added
        if on { names.insert(name) } else { names.remove(name) }
        addedNames = names.sorted().joined(separator: ", ")
    }
}

/// 表で選んだ変数の値の全体。PATH のように場所を並べたもの（名前が PATH・_DIRS で終わる）は 1 行に 1 か所に分け、
/// その場所があるか、前に同じ場所があるか、PATH ならどこの git が使われるかを添える。伏せた値は「表示」で見せる。
struct EnvironmentValueView: View {
    let name: String
    let value: String?
    let masked: Bool
    let reveal: () -> Void
    var height: CGFloat = 96
    /// SSH 先の変数なら、その git（プログラムで選んだもの、なければ SSH 先の PATH で見つかったものの場所）。SSH 先の
    /// 場所はこの Mac では調べられないので、「ない」は出さない。
    var onHost: (chosenGit: String?, gitDirectory: String?)? = nil

    var body: some View {
        Group {
            if let value, !name.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    HStack {
                        Text(Self.isList(name) ? L("%@ (%ld places)", name, Self.places(value).count) : name)
                            .font(.callout.monospaced().weight(.medium))
                        Spacer()
                        if masked { Button(L("Show"), action: reveal).controlSize(.small) }
                    }
                    ScrollView {
                        if masked {
                            Text("••••••••").font(.callout.monospaced()).foregroundStyle(.secondary)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        } else if Self.isList(name) {
                            placeList(value)
                        } else {
                            Text(value).font(.callout.monospaced()).textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    .frame(height: height)
                }
            } else {
                Text(L("Select a variable to see its whole value.")).font(.caption).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
            }
        }
    }

    /// 1 行に 1 か所。番号、場所、添え書き（ない・前と同じ・ここの git を使う）。設定 > Git で git を選んでいれば、
    /// PATH ではなくそれを使うと書く。
    private func placeList(_ value: String) -> some View {
        let places = Self.places(value)
        let chosenGit = onHost.map { $0.chosenGit } ?? MachineGit.thisMac.flatMap { $0.source == "path" ? nil : $0.program }
        let git: Int? = if name != "PATH" || chosenGit != nil {
            nil
        } else if let onHost {
            onHost.gitDirectory.flatMap { places.firstIndex(of: $0) }
        } else {
            places.firstIndex { FileManager.default.isExecutableFile(atPath: ($0 as NSString).appendingPathComponent("git")) }
        }
        return VStack(alignment: .leading, spacing: 2) {
            if name == "PATH", let chosenGit {
                Text(onHost == nil ? L("git is the one chosen in Settings > Git: %@", chosenGit) : L("git is the one chosen in Programs: %@", chosenGit))
                    .font(.caption).foregroundStyle(.tint)
            }
            ForEach(Array(places.enumerated()), id: \.offset) { i, place in
                let missing = onHost == nil && !FileManager.default.fileExists(atPath: place)
                let repeated = places[..<i].contains(place)
                HStack(spacing: 8) {
                    Text("\(i + 1)").font(.caption.monospacedDigit()).foregroundStyle(.tertiary).frame(width: 22, alignment: .trailing)
                    Text(place).font(.callout.monospaced())
                        .foregroundStyle(missing || repeated ? Color.secondary : Color.primary)
                        .strikethrough(repeated)
                        .textSelection(.enabled)
                    if i == git { Text(L("git used from here")).font(.caption).foregroundStyle(.tint) }
                    if missing { Text(L("doesn’t exist")).font(.caption).foregroundStyle(.secondary) }
                    if repeated { Text(L("already above")).font(.caption).foregroundStyle(.secondary) }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    static func isList(_ name: String) -> Bool {
        name.hasSuffix("PATH") || name.hasSuffix("_DIRS") || name.hasSuffix("PATHS")
    }

    static func places(_ value: String) -> [String] {
        value.split(separator: ":", omittingEmptySubsequences: true).map(String.init)
    }
}
