import AmonhenKit
import AppKit
import SwiftUI

/// Settings > SSH Hosts: SSH 先ごとの設定（docs/ux.md の 3 章「SSH Hosts」）。3 列: 左に SSH 先の一覧（＋／−）、
/// 真ん中に選んだ SSH 先のページ（接続と、そこにあるすべてのリポジトリに効く Settings > Git と同じページ）、右にその中身。
/// git のページは、SSH 先でリポジトリなしに動かしたエンジン（`ssh://host`）を通して SSH 先の `--global` を読み書きする。
struct SSHHostsSettings: View {
    @Bindable var model: SettingsModel
    @State private var hosts: [String] = []
    @State private var selected: String?
    @State private var page: HostPage = .connection
    @State private var sessions: [String: HostSession] = [:]
    @State private var appliedOption = false
    /// Places やリポジトリの設定から開くときに選ぶ SSH 先と、あればページ。
    static var pendingHost: (host: String, page: SettingsPage?)?
    static let showHost = Notification.Name("SSHHostsSettings.showHost")

    enum HostPage: Hashable {
        case connection
        case git(SettingsPage)
    }

    var body: some View {
        HStack(spacing: 0) {
            hostList.frame(width: 150)
            Divider()
            pageList.frame(width: 170)
            Divider()
            content.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear(perform: reload)
        .onReceive(NotificationCenter.default.publisher(for: Self.showHost)) { _ in reload() }
        .onChange(of: selected) { _, host in
            if let host { session(for: host) }
        }
    }

    /// Amon Hen が知っている SSH 先: Places にあるもの（リポジトリかスキャンするフォルダ）と、＋ で加えたもの。
    static func knownHosts() -> [String] {
        let fromPlaces = Bookmarks.shared.allLocations.compactMap(Settings.sshHost(of:))
            + Bookmarks.shared.data.scanRoots.compactMap { Settings.sshHost(of: $0.path) }
        return Set(Settings.sshHosts + fromPlaces).sorted { $0.lowercased() < $1.lowercased() }
    }

    private func reload() {
        hosts = Self.knownHosts()
        // --settings-page NAME で、最初に出すページ（見た目の確認用。Settings > Git と同じ名前）。
        if !appliedOption, let p = AppDelegate.option("--settings-page").flatMap(SettingsPage.init) {
            appliedOption = true
            page = .git(p)
        }
        if let pending = Self.pendingHost {
            Self.pendingHost = nil
            if !hosts.contains(pending.host) {
                Settings.sshHosts.append(pending.host)
                hosts = Self.knownHosts()
            }
            selected = pending.host
            page = pending.page.map { .git($0) } ?? .connection
        } else if selected == nil || !hosts.contains(selected!) {
            selected = hosts.first
        }
        if let selected { session(for: selected) }
    }

    /// その SSH 先につなぐ（初めて選んだとき）。
    private func session(for host: String) {
        guard sessions[host] == nil else { return }
        let session = HostSession(host: host)
        sessions[host] = session
        session.connect()
    }

    // MARK: - 左: SSH 先

    private var hostList: some View {
        VStack(spacing: 0) {
            List(selection: $selected) {
                ForEach(hosts, id: \.self) { host in
                    let unreachable = if case .failed = sessions[host]?.state { true } else { false }
                    // 幅が狭いので、つながらないことは名前の下に小さく。
                    VStack(alignment: .leading, spacing: 1) {
                        Text(host).lineLimit(1).truncationMode(.middle)
                        if unreachable {
                            Text(L("unreachable")).font(.caption)
                        }
                    }
                    .foregroundStyle(unreachable ? .secondary : .primary)
                    .tag(host)
                }
            }
            .listStyle(.sidebar)
            HStack(spacing: 2) {
                Button { addHost() } label: { Image(systemName: "plus") }
                    .help(L("Add an SSH host"))
                Button { removeSelected() } label: { Image(systemName: "minus") }
                    .disabled(selected == nil)
                    .help(L("Forget this host"))
                Spacer()
            }
            .buttonStyle(.borderless)
            .padding(.horizontal, 10).padding(.vertical, 6)
        }
    }

    /// 名前を聞いて SSH 先を加える（候補は ~/.ssh/config と known_hosts のもの）。
    private func addHost() {
        let alert = NSAlert()
        alert.messageText = L("Add an SSH host")
        alert.informativeText = L("Write it as ssh knows it: user@host:port, or a name in ~/.ssh/config.")
        let box = NSComboBox(frame: NSRect(x: 0, y: 0, width: 300, height: 24))
        box.addItems(withObjectValues: SSHHosts.candidates().filter { !hosts.contains($0) })
        box.numberOfVisibleItems = 12
        box.completes = true
        alert.accessoryView = box
        alert.window.initialFirstResponder = box
        alert.addButton(withTitle: L("Add"))
        alert.addButton(withTitle: L("Cancel"))
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let host = box.stringValue.trimmingCharacters(in: .whitespaces)
        guard !host.isEmpty, !host.contains(where: { $0 == "/" || $0.isWhitespace }) else { return }
        if !Settings.sshHosts.contains(host) { Settings.sshHosts.append(host) }
        hosts = Self.knownHosts()
        selected = host
        page = .connection
    }

    /// 選んだ SSH 先を忘れる。リポジトリやスキャンするフォルダがあれば、Places から外すのと同じ確認をして同じように外す。
    private func removeSelected() {
        guard let host = selected else { return }
        let paths = Bookmarks.shared.allLocations.filter { Settings.sshHost(of: $0) == host }
        let roots = Bookmarks.shared.data.scanRoots.map(\.path).filter { Settings.sshHost(of: $0) == host }
        if !paths.isEmpty || !roots.isEmpty {
            guard SSHConnectionSheet.confirmRemove(host: host, repositories: paths.count, folders: roots.count) else { return }
            for path in paths {
                RepositoryList.remove(path)
                Bookmarks.shared.remove(path)
            }
            for root in roots { Bookmarks.shared.removeScanRoot(root) }
        }
        Settings.sshHosts.removeAll { $0 == host }
        sessions[host] = nil
        selected = nil
        reload()
    }

    // MARK: - 真ん中: ページ

    private var pageList: some View {
        VStack(spacing: 0) {
            List(selection: Binding(get: { Optional(page) }, set: { if let p = $0 { page = p } })) {
                Text(L("Connection")).tag(HostPage.connection)
                ForEach(SettingsPage.cases(for: .allRepositories)) { Text($0.title).tag(HostPage.git($0)) }
            }
            .listStyle(.sidebar)
            .disabled(selected == nil)
            if let selected {
                Text("git \(sessions[selected]?.model?.gitVersion ?? "?") · \(selected)")
                    .font(.caption).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 14).padding(.vertical, 8)
            }
        }
    }

    // MARK: - 右: 中身

    @ViewBuilder private var content: some View {
        if let selected, let session = sessions[selected] {
            switch page {
            case .connection:
                ConnectionPage(session: session)
            case let .git(gitPage):
                switch session.state {
                case let .connected(model):
                    VStack(spacing: 0) {
                        Text(L("Applies to every repository on %@ (its ~/.gitconfig). git in a terminal there follows these too.", selected))
                            .font(.caption).foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 20).padding(.top, 10)
                        // 環境変数は、この Mac のものではなく SSH 先のエンジンのもの。
                        if gitPage == .environment {
                            HostEnvironmentPage(host: selected, model: model)
                        } else {
                            SettingsPageContent(model: model, page: gitPage)
                        }
                        if let error = model.error {
                            HStack {
                                Text(error).foregroundStyle(.red).lineLimit(2)
                                Spacer()
                            }
                            .padding([.horizontal, .bottom])
                        }
                    }
                case .connecting:
                    ProgressView(L("Connecting to %@…", selected)).padding()
                case let .failed(reason):
                    VStack(alignment: .leading, spacing: 8) {
                        Text(L("Couldn’t connect to %@", selected))
                        Text(reason).foregroundStyle(.red).textSelection(.enabled)
                        ChosenGitWarning(host: selected)
                        Button(L("Check Connection")) { session.connect() }
                    }
                    .padding()
                }
            }
        } else {
            Text(L("Add an SSH host with +, or open one from Places."))
                .foregroundStyle(.secondary).padding()
        }
    }
}

/// 1 つの SSH 先へのつながり: リポジトリなしで動かしたエンジン（`ssh://host`、共有の daemon 越しなら `daemon:ssh://host`）と、
/// それを通してその SSH 先の `--global` を読み書きする設定のモデル。
@Observable @MainActor
final class HostSession {
    enum State {
        case connecting
        case connected(RepositorySettingsModel)
        case failed(String)
    }

    let host: String
    private(set) var state: State = .connecting
    /// SSH 先で使っているエンジンと、その用意のしかた（つながったとき。共有の daemon 越しなら nil）。
    private(set) var engine: String?
    /// 共有の daemon 越しにつないだか（エンジンは daemon が起動し、ほかの相手と共有する）。
    private(set) var throughDaemon = false
    /// 何回目のつなぎか（古いつなぎの結果は捨てる）。
    private var attempt = 0

    init(host: String) {
        self.host = host
        // エンジンや git の選択を変えたら、つなぎ直す（新しい選択で SSH 先のエンジンを起動する）。
        observer = NotificationCenter.default.addObserver(forName: .hostOptionsChanged, object: nil, queue: .main) { [weak self] note in
            guard let host = note.object as? String else { return }
            MainActor.assumeIsolated {
                guard let self, host == self.host else { return }
                self.connect()
            }
        }
    }

    @ObservationIgnored nonisolated(unsafe) private var observer: NSObjectProtocol?

    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
    }

    var model: RepositorySettingsModel? {
        if case let .connected(model) = state { model } else { nil }
    }

    /// つなぐ（つなぎ直す）。SSH でつないでエンジンを用意するので数秒かかる。
    func connect() {
        attempt += 1
        let attempt = attempt
        state = .connecting
        engine = nil
        throughDaemon = Settings.openThroughDaemon
        let location = (throughDaemon ? "daemon:" : "") + "ssh://\(host)"
        let daemon = Daemon.path, daemons = Daemon.bundledDirectory
        Task {
            let result: Result<Repository, Error> = await Task.detached(priority: .userInitiated) {
                Result { try RepositoryWindow.openRepository(location: location, daemon: daemon, daemons: daemons) }
            }.value
            guard attempt == self.attempt else { return }
            switch result {
            case let .success(repository):
                engine = repository.remoteEngine
                let model = RepositorySettingsModel(repository: repository, location: "ssh://\(host)", level: .allRepositories)
                state = .connected(model)
                model.load()
            case let .failure(error):
                state = .failed("\(error)")
            }
        }
    }
}

/// 「接続」のページ: SSH 先と状態、使っているエンジン、署名の検証に使う鍵、探すフォルダ（Places と 1 つの設定）。
private struct ConnectionPage: View {
    let session: HostSession
    @State private var roots: [ScanRoot] = []

    var body: some View {
        Form {
            Section(L("Connection")) {
                LabeledContent(L("Host")) { Text(session.host).textSelection(.enabled) }
                LabeledContent(L("State")) {
                    HStack {
                        stateText
                        Spacer()
                        Button(L("Check Connection")) { session.connect() }
                            .disabled({ if case .connecting = session.state { true } else { false } }())
                    }
                }
            }
            Section(L("Engine")) {
                EngineChoice(host: session.host)
                LabeledContent(L("In use")) {
                    VStack(alignment: .trailing, spacing: 2) {
                        Text(session.engine ?? (session.model != nil ? L("Through the shared daemon") : L("Not connected")))
                            .foregroundStyle(.secondary).textSelection(.enabled)
                        // daemon 越しなら、エンジンは daemon が起動したもの（同じ SSH 先を開いたほかの相手と共有する）。
                        if session.engine != nil, session.throughDaemon {
                            Text(L("Started by the shared daemon")).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .trailing)
                }
                if let overridden = EngineChoice.overriddenBy {
                    Note(L("%@ is set in the environment, so it decides instead.", overridden))
                }
            }
            Section(L("Signature Verification")) {
                Picker(L("Verify with"), selection: Binding(
                    get: { Settings.checkSignaturesLocally(on: session.host) },
                    set: { Settings.setCheckSignaturesLocally($0, on: session.host) })) {
                    Text(L("This Mac’s keys")).tag(true)
                    Text(L("The host’s git and keys")).tag(false)
                }
                Note(L("Signatures of commits in repositories on this host are checked with these."))
            }
            Section(L("Folders to Scan")) {
                ForEach(roots, id: \.path) { root in
                    HStack {
                        Text(title(root.path)).font(.system(.body, design: .monospaced))
                        Spacer()
                        Picker("", selection: Binding(get: { root.depth }, set: { Bookmarks.shared.setDepth($0, of: root.path); refresh() })) {
                            ForEach(1...6, id: \.self) { Text(L("%ld levels", $0)).tag($0) }
                        }
                        .labelsHidden().fixedSize()
                        Button { Bookmarks.shared.removeScanRoot(root.path); refresh() } label: { Image(systemName: "minus") }
                            .buttonStyle(.borderless)
                    }
                }
                HStack {
                    Button { add() } label: { Image(systemName: "plus") }.buttonStyle(.borderless)
                    Note(L("The same list as under the host in Places."))
                }
            }
        }
        .formStyle(.grouped)
        .onAppear(perform: refresh)
    }

    @ViewBuilder private var stateText: some View {
        switch session.state {
        case .connecting:
            HStack(spacing: 6) {
                ProgressView().controlSize(.small)
                Text(L("Connecting…"))
            }
        case let .connected(model):
            VStack(alignment: .leading, spacing: 2) {
                Text("● " + L("Connected")).foregroundStyle(.green)
                Text(model.gitVersion.map { "git \($0)" } ?? L("git wasn’t found on the host"))
                    .font(.caption).foregroundStyle(.secondary)
                if model.gitVersion == nil { ChosenGitWarning(host: session.host) }
            }
        case let .failed(reason):
            VStack(alignment: .leading, spacing: 2) {
                Text("● " + L("unreachable")).foregroundStyle(.red)
                Text(reason).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                ChosenGitWarning(host: session.host)
            }
        }
    }

    private func refresh() {
        roots = Bookmarks.shared.data.scanRoots.filter { Settings.sshHost(of: $0.path) == session.host }
    }

    /// `/~/x` は `~/x`（Places と同じ見せ方）。
    private func title(_ location: String) -> String {
        let path = RepositoryBrowser.pathPart(location)
        return path.hasPrefix("/~") ? String(path.dropFirst()) : path
    }

    private func add() {
        guard let chosen = SSHFolderChooser.run(
            title: L("Add a Folder to Scan on %@", session.host),
            message: L("The engine on the host searches up to 3 levels below the folder. Double-click a folder to go into it."),
            button: L("Add"), fixedHost: session.host, hosts: [session.host], initialHost: session.host, initialPath: "~/prj"),
            let location = RepositoryLocation.ssh(host: chosen.host, path: chosen.path)
        else { return }
        Bookmarks.shared.addScanRoot(location)
        refresh()
    }
}

/// エンジンの選択: 自動（PATH のこのビルドのもの → 前に送ったもの → 送り込む）・SSH 先の PATH のもの・Amon Hen が
/// 送り込むもの・その他…（SSH 先のパス）と、合うものがなければ送り込むか。変えると、次につなぐときから効く。
private struct EngineChoice: View {
    let host: String
    @State private var options = HostOptions()
    private static let other = "\u{1}other"

    /// 環境変数で決めてあれば、その名前（そちらが優先される）。
    static var overriddenBy: String? {
        let env = ProcessInfo.processInfo.environment
        return ["AMONHEN_REMOTE_ENGINE", "AMONHEN_PUSH"].filter { !(env[$0] ?? "").isEmpty }.first
    }

    var body: some View {
        Group {
            Picker(L("Engine to use"), selection: Binding(get: { options.engine ?? "" }, set: { choose($0) })) {
                Text(L("Automatic")).tag("")
                Divider()
                Text(L("The amonhen on the host’s PATH")).tag("installed")
                Text(L("The one Amon Hen sends")).tag("sent")
                if let path = options.engine, path.hasPrefix("/") { Text(path).tag(path) }
                Divider()
                Text(L("Other…")).tag(Self.other)
            }
            Note(help)
            Toggle(L("Send one when none fits"), isOn: Binding(get: { options.push ?? true }, set: { setPush($0) }))
                .disabled(options.engine == "installed" || options.engine?.hasPrefix("/") == true)
        }
        .onAppear { options = Settings.hostOptions(for: host) }
        .onChange(of: host) { _, now in options = Settings.hostOptions(for: now) }
    }

    private var help: String {
        switch options.engine {
        case "installed": L("Uses the amonhen installed on the host. It must be this build.")
        case "sent": L("Uses the one Amon Hen sent to the host, sending it first when there is none.")
        case let path? where path.hasPrefix("/"): L("Uses this path on the host.")
        default: L("The amonhen on the host’s PATH if it is this build, else one sent earlier, else Amon Hen sends one.")
        }
    }

    private func choose(_ value: String) {
        var new = options
        if value == Self.other {
            let alert = NSAlert()
            alert.messageText = L("Engine on %@", host)
            alert.informativeText = L("The path of amonhen on the host. It must be this build.")
            let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 320, height: 24))
            field.placeholderString = "/usr/local/bin/amonhen"
            field.stringValue = options.engine?.hasPrefix("/") == true ? options.engine! : ""
            alert.accessoryView = field
            alert.window.initialFirstResponder = field
            alert.addButton(withTitle: L("Choose"))
            alert.addButton(withTitle: L("Cancel"))
            guard alert.runModal() == .alertFirstButtonReturn else { return }
            let chosen = field.stringValue.trimmingCharacters(in: .whitespaces)
            guard chosen.hasPrefix("/") else { return }
            new.engine = chosen
        } else {
            new.engine = value.isEmpty ? nil : value
        }
        save(new)
    }

    private func setPush(_ on: Bool) {
        var new = options
        new.push = on ? nil : false
        save(new)
    }

    private func save(_ new: HostOptions) {
        guard new != options else { return }
        options = new
        Settings.setHostOptions(new, for: host)
    }
}

/// 選んだ git（プログラムのページ）が SSH 先で動かないかもしれないときに、それを言い、PATH の git に戻せるようにする
/// （つながらなければプログラムのページを開けないので、ここで戻す）。git を選んでいなければ何も出さない。
private struct ChosenGitWarning: View {
    let host: String

    var body: some View {
        if let git = Settings.hostOptions(for: host).git {
            HStack(spacing: 8) {
                Text(L("The git chosen in Programs is %@.", git))
                    .font(.caption).foregroundStyle(.orange).textSelection(.enabled)
                Button(L("Use the git on the PATH")) {
                    var options = Settings.hostOptions(for: host)
                    options.git = nil
                    Settings.setHostOptions(options, for: host)
                }
                .controlSize(.small)
            }
        }
    }
}

/// SSH 先の「環境変数」: エンジンを SSH 先のログインシェル経由で起動するか（SSH 先ごと）と、エンジンが実際に持っている
/// 環境（そのまま git に渡る）。この Mac の「環境変数」と同じ形で、渡すかどうかを選ぶ列はない（SSH 先では、エンジンの
/// 環境がすべて git に渡る）。
private struct HostEnvironmentPage: View {
    let host: String
    let model: RepositorySettingsModel
    @State private var vars: [Variable] = []
    @State private var loaded = false
    @State private var filter = ""
    @State private var selection: String?
    @State private var revealed = false
    @State private var gitDirectory: String?

    nonisolated struct Variable: Decodable, Identifiable, Sendable {
        let name: String
        let value: String
        var id: String { name }
    }

    var body: some View {
        GroupedPage {
            VStack(alignment: .leading, spacing: 10) {
                Toggle(L("Use the login shell’s environment variables"), isOn: Binding(
                    get: { Settings.hostOptions(for: host).loginShell == true },
                    set: { on in
                        var options = Settings.hostOptions(for: host)
                        options.loginShell = on ? true : nil
                        Settings.setHostOptions(options, for: host)
                    }))
                Note(L("Starts the engine on %@ through its login shell, so it and git get the PATH and GIT_* set in ~/.profile and the like, which a command run over ssh lacks. Changing it reconnects.", host))
                Divider()
                HStack {
                    Text(L("The engine’s environment, which git on %@ gets", host)).font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    TextField("", text: $filter, prompt: Text(L("Search by name or value")))
                        .textFieldStyle(.roundedBorder)
                        .frame(minWidth: 60, maxWidth: 240)
                }
                Table(rows, selection: Binding(get: { selection }, set: { selection = $0; revealed = false })) {
                    TableColumn(L("Name")) { v in Text(v.name).font(.body.monospaced()) }
                        .width(min: 120, ideal: 180)
                    TableColumn(L("Value")) { v in
                        let shown = EnvironmentTable.isSecret(v.name) ? "••••••••" : v.value
                        Text(shown).font(.callout.monospaced()).foregroundStyle(.secondary)
                            .lineLimit(1).truncationMode(.middle).help(shown)
                    }
                }
                .frame(minHeight: 100, maxHeight: .infinity)
                .overlay { if !loaded { ProgressView() } }
                EnvironmentValueView(
                    name: selection ?? "", value: selection.flatMap { name in vars.first { $0.name == name }?.value },
                    masked: selection.map(EnvironmentTable.isSecret) == true && !revealed,
                    reveal: { revealed = true }, height: 120,
                    onHost: (chosenGit: Settings.hostOptions(for: host).git, gitDirectory: gitDirectory))
            }
        }
        .task(id: ObjectIdentifier(model)) { await load() }
    }

    private var rows: [Variable] {
        let f = filter.trimmingCharacters(in: .whitespaces)
        guard !f.isEmpty else { return vars }
        return vars.filter { $0.name.localizedCaseInsensitiveContains(f) || $0.value.localizedCaseInsensitiveContains(f) }
    }

    /// エンジンの環境と、PATH で見つかる git の場所を SSH 先から読む。
    private func load() async {
        loaded = false
        let repository = model.repository
        let (v, g) = await Task.detached(priority: .userInitiated) {
            struct Vars: Decodable { let vars: [Variable] }
            struct Found: Decodable { struct Program: Decodable { let path: String }; let programs: [Program] }
            return ((try? repository.info(["op": "processEnvironment"], as: Vars.self))?.vars ?? [],
                    (try? repository.info(["op": "programs", "name": "git"], as: Found.self))?.programs.first?.path)
        }.value
        vars = v
        gitDirectory = g.map { ($0 as NSString).deletingLastPathComponent }
        loaded = true
    }
}
