import AmonhenKit
import Foundation

/// Finder や Dock から起動したアプリは、ターミナルの環境を引き継がない（launchd の素の環境: PATH は
/// `/usr/bin:/bin:/usr/sbin:/sbin`、`SSH_AUTH_SOCK` は macOS の agent、`GIT_*` はなし）。そのため、Homebrew の git や
/// credential helper が見つからない、1Password などの SSH agent が見えない、といった形で、ターミナルでは通る
/// fetch・pull・push がアプリからは通らない。起動時にログインシェルを一度動かして環境を読み、git・daemon・gh に
/// 渡す（VSCode などと同じやり方）。
enum ShellEnvironment {
    /// 取り込む変数は、git と ssh がいつも使うもの（コアの決まり。PATH・SSH の agent・`GIT_*` など）と、設定で
    /// 足したもの。PATH はシェルのものを前に足し、残りはシェルのものがあれば置き換える。
    /// 設定で足した名前（`Settings.addedEnvironmentNames`。渡せないものは除く）。
    nonisolated(unsafe) private static var added: Set<String> = []
    /// シェルにないので、このプロセスの環境から外した「いつも渡す」変数（起動したときに親から引き継いだもの）。
    /// ログインシェルを使わなくなったら、起動したときの値に戻す。
    nonisolated(unsafe) private static var removed: [String] = []
    /// ログインの間だけ使える変数（agent のソケットはログインのたびに変わる）。前回の結果からは、ソケットが今も
    /// あるときだけ使う。
    nonisolated private static let session = ["SSH_AUTH_SOCK", "SSH_AGENT_PID"]

    /// 起動したときの環境（取り込む前）。シェルはこれで動かす。取り込んだあとの環境で動かすと、シェルが設定しない
    /// 変数（launchd の `SSH_AUTH_SOCK` など）は取り込んだ値をそのまま返すので、前回の値が古くても読み直すたびに
    /// 書き戻され、再起動しても直らなかった。
    nonisolated(unsafe) private static var launch: [String: String] = [:]
    /// このログインの SSH の agent（起動したときの `SSH_AUTH_SOCK`）。コアが起動する ssh・git の `SSH_AUTH_SOCK` が
    /// 死んでいたときに、代わりに渡す。
    static var launchAgent: String? { launch["SSH_AUTH_SOCK"] }
    /// 起動したときの環境（設定画面の表に出す。ログインシェルを使わないとき）。
    static var launchEnvironment: [String: String] { launch }
    /// ログインシェルから最後に読んだ変数すべて（設定画面の表で、渡すものを選ぶ）。保存はしない。
    nonisolated(unsafe) private(set) static var shellValues: [String: String] = [:]

    /// 取り込んだ変数の名前（`--print-environment` に出す）。取り込まなかったときは空。
    nonisolated(unsafe) private(set) static var imported: [String] = []
    /// 取り込んだ変数とその値（設定画面に出す）。
    nonisolated(unsafe) private(set) static var values: [String: String] = [:]
    /// 使ったシェルと、最後に読んだ時刻（設定画面に出す）。読めなかったときは nil。
    nonisolated(unsafe) private(set) static var shell = ""
    nonisolated(unsafe) private(set) static var lastRead: Date?
    /// 取り込んだあとに知らせる（設定画面が開いていれば出し直す）。
    nonisolated static let didChange = Notification.Name("shellEnvironmentDidChange")

    nonisolated private static let cacheKey = "shellEnvironment"

    /// ログインシェルの環境を取り込む。シェルを動かすのに 0.2〜0.3 秒かかるので、前回の結果を覚えておいてまず
    /// それを使い、今回の分は裏で読んで次の git から使う（初回だけ待つ）。設定で切っていれば（`AMONHEN_NO_SHELL_ENV`
    /// でも）何もしない。
    ///
    /// 裏で読む側は `Task.detached`（メインアクターから外れる）で呼ぶ。このパッケージは既定でメインアクターに
    /// 隔離されるので、`read`・`apply` は `nonisolated` にしてある。`DispatchQueue.async` のクロージャから呼ぶと、
    /// 隔離の確認（`dispatch_assert_queue`）に落ちて、キャッシュのある 2 回目以降の起動で必ずクラッシュしていた。
    static func `import`() {
        launch = ProcessInfo.processInfo.environment
        useSettings()
        guard isEnabled else { return }
        if let cached = remembered() {
            apply(cached)
            Task.detached(priority: .utility) { if let fresh = read() { apply(fresh) } }
        } else if let fresh = read() {
            apply(fresh)
        }
    }

    /// 前回読んだシェルの環境のうち、今の設定で取り込むもの（agent は、ソケットが今もあるときだけ）。
    private static func remembered() -> [String: String]? {
        guard let cached = UserDefaults.standard.dictionary(forKey: cacheKey) as? [String: String], !cached.isEmpty else {
            return nil
        }
        return cached.filter { takes($0.key) && (!session.contains($0.key) || isSocket(cached["SSH_AUTH_SOCK"])) }
    }

    /// 設定で足した名前を `AMONHEN_PASS_ENV` に置く（コアが渡す名前を決め、共有の daemon にも伝わる）。
    private static func useSettings() {
        added = Set(Settings.addedEnvironmentNameList.filter { PassedEnvironment.refusal($0) == nil })
        if added.isEmpty {
            unsetenv("AMONHEN_PASS_ENV")
        } else {
            setenv("AMONHEN_PASS_ENV", added.sorted().joined(separator: ","), 1)
        }
    }

    /// 設定（ログインシェルを使うか・足した名前）を変えたときに、再起動せずに効かせる。取り込んだ変数を起動した
    /// ときの値に戻し、設定に従って取り込み直す（前回の結果をすぐ使い、シェルは裏で読み直す）。共有の daemon への
    /// 今のつながりにも送る。
    static func refresh() {
        for name in imported + removed where !name.hasPrefix("AMONHEN_") {
            if let value = launch[name] { setenv(name, value, 1) } else { unsetenv(name) }
        }
        removed = []
        useSettings()
        guard isEnabled else {
            imported = []
            values = [:]
            DispatchQueue.main.async { NotificationCenter.default.post(name: didChange, object: nil) }
            DispatchQueue.global(qos: .utility).async { SharedDaemon.updateEnvironment() }
            return
        }
        if let cached = remembered() { apply(cached) }
        reload()
    }

    /// シェルの環境から取り込むか: git と ssh がいつも使うものと、設定で足したもの。`AMONHEN_*` はこのアプリが
    /// 決めるので取り込まない。
    nonisolated private static func takes(_ name: String) -> Bool {
        !name.hasPrefix("AMONHEN_") && (PassedEnvironment.always(name) || added.contains(name))
    }

    /// 設定で切れる（既定は取り込む）。
    static var isEnabled: Bool {
        ProcessInfo.processInfo.environment["AMONHEN_NO_SHELL_ENV"] == nil && Settings.importShellEnvironment
    }

    /// 設定画面の「読み直す」: シェルをもう一度動かして取り込む（裏で。終わったら `didChange`）。
    static func reload() {
        Task.detached(priority: .userInitiated) {
            if let fresh = read() { apply(fresh) }
        }
    }

    /// ログインシェル（`$SHELL`、なければ zsh）を対話的に動かして `env -0` を読む（`.zshrc` で PATH を足す人が多い
    /// ので対話的に）。3 秒で諦める（遅い rc で起動を待たせない）。取り込む変数だけを返す。
    nonisolated private static func read() -> [String: String]? {
        let shell = ProcessInfo.processInfo.environment["SHELL"].flatMap { $0.isEmpty ? nil : $0 } ?? "/bin/zsh"
        Self.shell = shell
        guard let output = run(shell, ["-ilc", "env -0"]) ?? run(shell, ["-lc", "env -0"]) else {
            lastRead = nil
            DispatchQueue.main.async { NotificationCenter.default.post(name: didChange, object: nil) }
            return nil
        }
        lastRead = Date()
        var values: [String: String] = [:]
        var all: [String: String] = [:]
        for chunk in output.split(separator: "\0", omittingEmptySubsequences: true) {
            guard let eq = chunk.firstIndex(of: "=") else { continue }
            // rc が何か印字していれば、最初の項目の頭に混ざる。最後の行だけを名前にする。
            let key = String(chunk[..<eq]).split(separator: "\n").last.map(String.init) ?? ""
            let value = String(chunk[chunk.index(after: eq)...])
            all[key] = value
            guard takes(key), !value.isEmpty else { continue }
            values[key] = value
        }
        shellValues = all
        UserDefaults.standard.set(values, forKey: cacheKey)
        return values
    }

    /// 変数をこのプロセスの環境に入れる（これから動かす git・daemon・gh が引き継ぐ）。
    nonisolated private static func apply(_ values: [String: String]) {
        let current = ProcessInfo.processInfo.environment
        var names: [String] = []
        for (key, value) in values {
            if key == "PATH" {
                // シェルの PATH を前に。今の PATH にしかない場所は後ろに残す。同じ場所は 1 回だけ。
                var seen: Set<String> = []
                let merged = (value.split(separator: ":") + (current["PATH"] ?? "").split(separator: ":"))
                    .map(String.init).filter { seen.insert($0).inserted }
                setenv("PATH", merged.joined(separator: ":"), 1)
            } else {
                setenv(key, value, 1)
            }
            names.append(key)
        }
        // シェルにない「いつも渡す」変数は、起動したときに親（端末・ほかのアプリ）から引き継いだもの。渡すと、表に
        // 出ないものが git に渡る（Claude Code から起動すると `GIT_EDITOR=true` など）ので外す。PATH と agent は残す
        // （PATH はシェルのものを前に足してあり、agent はログインのもの）。`AMONHEN_*` はこのアプリが決める。
        for key in current.keys
        where values[key] == nil && key != "PATH" && key != "SSH_AUTH_SOCK" && !key.hasPrefix("AMONHEN_")
            && PassedEnvironment.always(key) {
            unsetenv(key)
            if !removed.contains(key) { removed.append(key) }
        }
        imported = names.sorted()
        Self.values = values
        DispatchQueue.main.async { NotificationCenter.default.post(name: didChange, object: nil) }
        // 共有の daemon への今のつながりにも送る（daemon が動かす git と ssh も、つなぎ直さずに新しい環境になる）。
        DispatchQueue.global(qos: .utility).async { SharedDaemon.updateEnvironment() }
    }

    /// `path` がソケットか（リンクの先を見る。1Password の agent は `~/.1password/agent.sock` のリンク）。
    nonisolated static func isSocket(_ path: String?) -> Bool {
        guard let path else { return false }
        var info = stat()
        return stat(path, &info) == 0 && (info.st_mode & S_IFMT) == S_IFSOCK
    }

    /// ログインシェルを動かす環境: 端末で新しいウィンドウを開くときと同じく、利用者とログインのもの（`HOME`・`USER`・
    /// 言語・`TMPDIR`・launchd の `SSH_AUTH_SOCK` など）だけを渡し、PATH は素にする。読みたいのは利用者のシェルが決める
    /// 変数で、アプリを何から起動したかで変わってはいけない。起動したときの環境をそのまま渡すと、端末や別のアプリから
    /// 起動したときに、その親の変数（Claude Code の `AI_AGENT` など）がシェルの変数として混ざり、`SHLVL` も引き継いで
    /// `SHLVL` が 1 のときだけ PATH を組み立てる rc では PATH が組み立てられず、端末と違う git（/usr/bin の Apple の git）
    /// になっていた。
    nonisolated static func shellEnvironment(_ launch: [String: String]) -> [String: String] {
        let kept = ["HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE", "__CF_USER_TEXT_ENCODING",
                    "SSH_AUTH_SOCK"]
        var environment = launch.filter { kept.contains($0.key) }
        environment["PATH"] = "/usr/bin:/bin:/usr/sbin:/sbin"
        return environment
    }

    nonisolated private static func run(_ shell: String, _ arguments: [String]) -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: shell)
        process.arguments = arguments
        if !launch.isEmpty { process.environment = shellEnvironment(launch) }
        process.standardInput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        let out = Pipe()
        process.standardOutput = out
        guard (try? process.run()) != nil else { return nil }
        // 読み切るのは裏で（パイプが詰まると終わらない）。3 秒で終わらなければ止める。
        let done = DispatchSemaphore(value: 0)
        nonisolated(unsafe) var data = Data()
        DispatchQueue.global().async {
            data = out.fileHandleForReading.readDataToEndOfFile()
            done.signal()
        }
        if done.wait(timeout: .now() + 3) == .timedOut {
            process.terminate()
            return nil
        }
        process.waitUntilExit()
        guard process.terminationStatus == 0, !data.isEmpty else { return nil }
        return String(decoding: data, as: UTF8.self)
    }
}
