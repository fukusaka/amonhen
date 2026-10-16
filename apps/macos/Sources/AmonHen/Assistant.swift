import AmonhenKit
import AppKit
import CAmonhen
import CryptoKit

/// AI に頼むこと。用途ごとにプロンプトを持ち、利用者が書き換えられる。
enum AssistPurpose: String, CaseIterable, Identifiable {
    case translate, explainCommit, explainHunk, suggestMessage, resolveConflict, explainConflict
    var id: String { rawValue }

    var title: String {
        switch self {
        case .translate: "コミットメッセージの翻訳"
        case .explainCommit: "コミットの解説"
        case .explainHunk: "hunk の解説"
        case .suggestMessage: "コミットメッセージの提案"
        case .resolveConflict: "コンフリクトの解消"
        case .explainConflict: "コンフリクトの解説"
        }
    }

    /// プロンプトで使える変数。
    var variables: [String] {
        switch self {
        case .translate: ["message", "language"]
        case .explainCommit: ["message", "diff", "files", "language"]
        case .explainHunk: ["path", "hunk", "message", "language"]
        case .suggestMessage: ["diff", "files", "recent_messages", "language"]
        case .resolveConflict, .explainConflict: ["path", "ours", "theirs", "base", "language"]
        }
    }

    /// Settings の「試す」で差し込む見本の値（プロンプトの書き換えを、実際のリポジトリなしで確かめる）。
    var sampleVariables: [String: String] {
        let diff = """
            --- src/cache.rs
            @@ -10,7 +10,9 @@ impl Cache {
                 pub fn get(&self, key: &str) -> Option<&Value> {
            -        self.map.get(key)
            +        let value = self.map.get(key)?;
            +        self.hits.fetch_add(1, Ordering::Relaxed);
            +        Some(value)
                 }
            """
        let message = "Count cache hits\n\nThe hit rate shows whether the cache size is right."
        return switch self {
        case .translate: ["message": message]
        case .explainCommit: ["message": message, "diff": diff, "files": "src/cache.rs"]
        case .explainHunk: ["path": "src/cache.rs", "hunk": diff, "message": message]
        case .suggestMessage: ["diff": diff, "files": "src/cache.rs", "recent_messages": "Add a size limit to the cache\n---\nFix eviction order"]
        case .resolveConflict, .explainConflict:
            ["path": "src/cache.rs", "ours": "let limit = 100;", "theirs": "let limit = config.limit;", "base": "let limit = 64;"]
        }
    }

    /// 組み込みのプロンプト。
    var defaultPrompt: String {
        switch self {
        case .translate:
            """
            次の git のコミットメッセージを{{language}}に翻訳してください。件名が短くても、件名を含めて文章はすべて訳してください。\
            コード・識別子・パス・コミット ID・URL は訳さずそのまま残し、構成（空行、箇条書き、トレーラー）も保ってください。\
            訳文だけを出力してください。

            {{message}}
            """
        case .explainCommit:
            """
            次の git のコミットを{{language}}で解説してください。
            1. 何を変えたか（要点を箇条書きで）
            2. なぜ変えたと考えられるか
            3. 影響しそうな箇所・注意点
            推測は推測と書いてください。

            # メッセージ
            {{message}}

            # 変更ファイル
            {{files}}

            # 差分
            {{diff}}
            """
        case .explainHunk:
            """
            次の差分の一部（hunk）が何をしているかを{{language}}で短く解説してください。変更の意図として考えられること、\
            気をつけるべき点があれば添えてください。推測は推測と書いてください。

            # ファイル
            {{path}}

            # コミットのメッセージ（あれば）
            {{message}}

            # hunk
            {{hunk}}
            """
        case .suggestMessage:
            """
            次の stage 済みの変更に合う git のコミットメッセージを書いてください。1 行目は 50 文字前後の件名（命令形）、\
            空行のあと必要なら本文（何をなぜ変えたか、72 桁で改行）。このリポジトリの最近のメッセージの書き方（言語・形式）に\
            合わせてください。メッセージだけを出力し、説明や囲みは付けないでください。

            # 最近のメッセージ
            {{recent_messages}}

            # 変更ファイル
            {{files}}

            # 差分
            {{diff}}
            """
        case .resolveConflict:
            """
            {{path}} のマージのコンフリクトを解消してください。Current（今のブランチ）と Incoming（取り込む側）の両方の\
            意図を活かした内容にしてください。どちらかを捨てるべきならそうしてください。

            # 共通の祖先（なければ空）
            {{base}}

            # Current
            {{ours}}

            # Incoming
            {{theirs}}
            """
        case .explainConflict:
            """
            {{path}} のマージのコンフリクトについて、Current と Incoming がそれぞれ何をしようとしているか、\
            どう解消するのがよさそうかを{{language}}で簡潔に説明してください。

            # 共通の祖先（なければ空）
            {{base}}

            # Current
            {{ours}}

            # Incoming
            {{theirs}}
            """
        }
    }

    /// 返させる形（アプリが読むので、プロンプトの編集では変えられない。末尾に自動で足す）。
    var outputContract: String? {
        switch self {
        case .resolveConflict:
            """


            出力は次の形の JSON だけにしてください（前後に文章やコードの囲みを付けない）:
            {"text": "解消した後の内容（コンフリクトの範囲を置き換える行。マーカーは含めない）", "reason": "そうした理由（短く、{{language}}で）"}
            """
        default:
            nil
        }
    }
}

/// AI の CLI。設定で明示的に選ぶ（既定は使わない）。
enum AssistantCLI: String, CaseIterable, Identifiable {
    case none, claude, codex
    var id: String { rawValue }

    var title: String {
        switch self {
        case .none: "使わない"
        case .claude: "Claude Code"
        case .codex: "Codex"
        }
    }

    /// 実行ファイルの場所（Finder から起動したアプリには端末の PATH がないので、よくある場所を探す）。
    var path: String? {
        let home = NSHomeDirectory()
        let names: [String] = switch self {
        case .none: []
        case .claude: ["\(home)/.local/bin/claude", "\(home)/.claude/local/claude", "/opt/homebrew/bin/claude", "/usr/local/bin/claude"]
        case .codex: ["/opt/homebrew/bin/codex", "/usr/local/bin/codex", "\(home)/.local/bin/codex", "\(home)/.npm-global/bin/codex"]
        }
        return names.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    var defaultModel: String {
        switch self {
        case .none: ""
        case .claude: "claude-haiku-5-5"
        case .codex: ""
        }
    }
}

extension Settings {
    /// AI に使う CLI（既定は使わない）。
    static var assistant: AssistantCLI {
        get { UserDefaults.standard.string(forKey: "assistant").flatMap(AssistantCLI.init) ?? .none }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "assistant") }
    }

    /// CLI ごとのモデル（空なら CLI の既定）。
    static func assistantModel(_ cli: AssistantCLI) -> String {
        UserDefaults.standard.string(forKey: "assistantModel.\(cli.rawValue)") ?? cli.defaultModel
    }

    static func setAssistantModel(_ model: String, for cli: AssistantCLI) {
        UserDefaults.standard.set(model, forKey: "assistantModel.\(cli.rawValue)")
    }

    /// 翻訳と解説の言語。
    static var assistantLanguage: String {
        get { UserDefaults.standard.string(forKey: "assistantLanguage") ?? "日本語" }
        set { UserDefaults.standard.set(newValue, forKey: "assistantLanguage") }
    }
}

/// AI の呼び出し。選んだ CLI を、ツールを使わせない設定で起動し、プロンプトを標準入力で渡して、流れてくる文字を
/// 受け取る。結果は CLI・モデル・プロンプト・入力のハッシュでキャッシュする（2 回目は即表示）。
enum Assistant {
    struct Failure: Error, CustomStringConvertible { let description: String }

    static var isConfigured: Bool { Settings.assistant != .none && Settings.assistant.path != nil }

    // MARK: - プロンプト

    private static var promptsDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Amon Hen/prompts")
    }

    static func promptFile(_ purpose: AssistPurpose) -> URL {
        promptsDirectory.appendingPathComponent("\(purpose.rawValue).md")
    }

    /// 利用者のプロンプト（書き換えていなければ組み込みのもの）。
    static func userPrompt(_ purpose: AssistPurpose) -> String {
        (try? String(contentsOf: promptFile(purpose), encoding: .utf8)) ?? purpose.defaultPrompt
    }

    static func isCustomized(_ purpose: AssistPurpose) -> Bool {
        FileManager.default.fileExists(atPath: promptFile(purpose).path)
    }

    /// 書き換えるためにファイルを作り（なければ組み込みのものを書く）、その場所を返す。
    static func editablePromptFile(_ purpose: AssistPurpose) throws -> URL {
        let file = promptFile(purpose)
        if !FileManager.default.fileExists(atPath: file.path) {
            try FileManager.default.createDirectory(at: promptsDirectory, withIntermediateDirectories: true)
            try purpose.defaultPrompt.write(to: file, atomically: true, encoding: .utf8)
        }
        return file
    }

    static func resetPrompt(_ purpose: AssistPurpose) {
        try? FileManager.default.removeItem(at: promptFile(purpose))
    }

    /// リポジトリの `.amonhen/prompts/<用途>.md`（あれば利用者のものより優先する）。
    private static func repositoryPrompt(_ purpose: AssistPurpose, workdir: String?) -> (path: String, text: String)? {
        guard let workdir else { return nil }
        let path = (workdir as NSString).appendingPathComponent(".amonhen/prompts/\(purpose.rawValue).md")
        return (try? String(contentsOfFile: path, encoding: .utf8)).map { (path, $0) }
    }

    private static func sha256(_ text: String) -> String {
        SHA256.hash(data: Data(text.utf8)).map { String(format: "%02x", $0) }.joined()
    }

    /// 使うプロンプト。リポジトリのものは clone したものにも入っているので、初めて使うとき・中身が変わったときに
    /// 見せて確かめる（許可はパスと中身のハッシュで覚える）。断られたら利用者のものを使う。
    private static func prompt(_ purpose: AssistPurpose, workdir: String?) -> String {
        guard let (path, text) = repositoryPrompt(purpose, workdir: workdir) else { return userPrompt(purpose) }
        var trusted = UserDefaults.standard.dictionary(forKey: "trustedPrompts") as? [String: String] ?? [:]
        let hash = sha256(text)
        if trusted[path] == hash { return text }
        let alert = NSAlert()
        alert.messageText = "リポジトリのプロンプトを使いますか？"
        alert.informativeText = "\(path)\n\n" + String(text.prefix(1500))
        alert.addButton(withTitle: "使う")
        alert.addButton(withTitle: "使わない（自分のプロンプトを使う）")
        guard alert.runModal() == .alertFirstButtonReturn else { return userPrompt(purpose) }
        trusted[path] = hash
        UserDefaults.standard.set(trusted, forKey: "trustedPrompts")
        return text
    }

    /// `{{name}}` を差し込む。知らない変数があれば失敗にする。
    static func fill(_ template: String, _ variables: [String: String], purpose: AssistPurpose) throws -> String {
        var out = template
        let regex = try NSRegularExpression(pattern: #"\{\{\s*([a-z_]+)\s*\}\}"#)
        let names = regex.matches(in: template, range: NSRange(template.startIndex..., in: template)).compactMap {
            Range($0.range(at: 1), in: template).map { String(template[$0]) }
        }
        if let unknown = names.first(where: { !purpose.variables.contains($0) }) {
            throw Failure(description: "プロンプトの変数 {{\(unknown)}} は「\(purpose.title)」では使えません（使えるもの: \(purpose.variables.joined(separator: ", "))）")
        }
        for name in Set(names) {
            out = out.replacingOccurrences(
                of: #"\{\{\s*"# + name + #"\s*\}\}"#, with: variables[name] ?? "", options: .regularExpression)
        }
        return out
    }

    // MARK: - 送る前の確認

    /// このリポジトリで AI を使ってよいか。初めて使うときに、送る内容の大きさを見せて確かめる。
    @discardableResult
    static func confirm(repository key: String, summary: String) -> Bool {
        var allowed = UserDefaults.standard.dictionary(forKey: "assistantRepositories") as? [String: Bool] ?? [:]
        if let decided = allowed[key] { return decided }
        let alert = NSAlert()
        alert.messageText = "このリポジトリで AI を使いますか？"
        alert.informativeText = """
            \(Settings.assistant.title) に送ります: \(summary)
            このリポジトリの内容（メッセージ・差分）が、CLI を通じて外部のサービスに送られます。
            あとで Settings > AI の「このリポジトリの許可を消す」で選び直せます。
            """
        alert.addButton(withTitle: "使う")
        alert.addButton(withTitle: "このリポジトリでは使わない")
        alert.addButton(withTitle: "キャンセル")
        switch alert.runModal() {
        case .alertFirstButtonReturn: allowed[key] = true
        case .alertSecondButtonReturn: allowed[key] = false
        default: return false
        }
        UserDefaults.standard.set(allowed, forKey: "assistantRepositories")
        return allowed[key] ?? false
    }

    static func forgetRepositoryDecisions() {
        UserDefaults.standard.removeObject(forKey: "assistantRepositories")
    }

    // MARK: - キャッシュ

    private static var cacheDirectory: URL {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent(Bundle.main.bundleIdentifier ?? "io.github.fukusaka.amonhen")
            .appendingPathComponent("assist")
    }

    // MARK: - 実行

    /// `purpose` のプロンプトに `variables` を差し込んで送り、返事を流しながら `onText` に渡す（それまでの全文）。
    /// `useCache` なら、同じ問い合わせの前の結果をすぐ返す。
    static func run(
        _ purpose: AssistPurpose, variables: [String: String], workdir: String?, useCache: Bool = true,
        onText: @escaping @MainActor (String) -> Void
    ) async throws -> String {
        let cli = Settings.assistant
        guard cli != .none, let executable = cli.path else {
            throw Failure(description: "AI の CLI が選ばれていないか、見つかりません（Settings > AI）")
        }
        var vars = variables
        vars["language"] = vars["language"] ?? Settings.assistantLanguage
        var prompt = try fill(prompt(purpose, workdir: workdir), vars, purpose: purpose)
        if let contract = purpose.outputContract { prompt += try fill(contract, vars, purpose: purpose) }
        let model = Settings.assistantModel(cli)
        let key = sha256([cli.rawValue, model, purpose.rawValue, prompt].joined(separator: "\u{0}"))
        let cacheFile = cacheDirectory.appendingPathComponent(key + ".txt")
        if useCache, let cached = try? String(contentsOf: cacheFile, encoding: .utf8) {
            onText(cached)
            return cached
        }
        let started = CACurrentMediaTime()
        let result = try await stream(cli: cli, executable: executable, model: model, prompt: prompt, onText: onText)
        PerformanceLog.record("AI: \(purpose.title)", ms: (CACurrentMediaTime() - started) * 1000)
        try? FileManager.default.createDirectory(at: cacheDirectory, withIntermediateDirectories: true)
        try? result.write(to: cacheFile, atomically: true, encoding: .utf8)
        return result
    }

    /// CLI を起動して、標準出力の JSON の行から文字を拾う。
    private static func stream(
        cli: AssistantCLI, executable: String, model: String, prompt: String,
        onText: @escaping @MainActor (String) -> Void
    ) async throws -> String {
        var args: [String]
        switch cli {
        case .claude:
            // ツールは使わせない（解説・翻訳でファイルを書いたりコマンドを実行したりする必要はない）。
            args = ["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages",
                    "--tools", "", "--no-session-persistence"]
            if !model.isEmpty { args += ["--model", model] }
        case .codex:
            // sandbox は読み取りだけ。プロンプトは標準入力（-）。
            args = ["exec", "--json", "--sandbox", "read-only", "--skip-git-repo-check"]
            if !model.isEmpty { args += ["-m", model] }
            args.append("-")
        case .none:
            throw Failure(description: "AI の CLI が選ばれていません")
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = args
        // リポジトリの CLAUDE.md などを読ませないよう、空の場所で動かす。
        process.currentDirectoryURL = FileManager.default.temporaryDirectory
        var env = ProcessInfo.processInfo.environment
        env["PATH"] = "\(NSHomeDirectory())/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:" + (env["PATH"] ?? "")
        process.environment = env
        let input = Pipe(), output = Pipe(), errors = Pipe()
        process.standardInput = input
        process.standardOutput = output
        process.standardError = errors
        try process.run()
        input.fileHandleForWriting.write(Data(prompt.utf8))
        try? input.fileHandleForWriting.close()

        let reader = output.fileHandleForReading
        let collected = try await Task.detached(priority: .userInitiated) { () -> (String, String?) in
            var text = "", final: String?, buffer = Data()
            func handle(_ line: Data) {
                guard let json = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else { return }
                if let delta = Self.delta(json, cli: cli) {
                    text += delta
                    let snapshot = text
                    Task { @MainActor in onText(snapshot) }
                }
                if let done = Self.finalText(json, cli: cli) { final = done }
            }
            while true {
                let chunk = reader.availableData
                if chunk.isEmpty { break }
                buffer.append(chunk)
                while let newline = buffer.firstIndex(of: 0x0A) {
                    handle(buffer[buffer.startIndex..<newline])
                    buffer.removeSubrange(buffer.startIndex...newline)
                }
            }
            if !buffer.isEmpty { handle(buffer) }
            return (text, final)
        }.value
        process.waitUntilExit()
        let errorText = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        let result = (collected.1 ?? collected.0).trimmingCharacters(in: .whitespacesAndNewlines)
        guard process.terminationStatus == 0, !result.isEmpty else {
            throw Failure(description: errorText.isEmpty ? "\(cli.title) が失敗しました（終了コード \(process.terminationStatus)）" : errorText)
        }
        onText(result)
        return result
    }

    /// 流れてくる途中の文字。
    nonisolated private static func delta(_ json: [String: Any], cli: AssistantCLI) -> String? {
        switch cli {
        case .claude:
            guard json["type"] as? String == "stream_event", let event = json["event"] as? [String: Any],
                  event["type"] as? String == "content_block_delta", let delta = event["delta"] as? [String: Any],
                  delta["type"] as? String == "text_delta" else { return nil }
            return delta["text"] as? String
        default:
            return nil
        }
    }

    /// 最後の全文。Claude Code は `result`、Codex は `item.completed` の `agent_message`（確かめていない形）。
    nonisolated private static func finalText(_ json: [String: Any], cli: AssistantCLI) -> String? {
        switch cli {
        case .claude:
            guard json["type"] as? String == "result" else { return nil }
            return json["result"] as? String
        case .codex:
            if let item = json["item"] as? [String: Any], (item["type"] as? String)?.contains("message") == true {
                return item["text"] as? String
            }
            if let msg = json["msg"] as? [String: Any], msg["type"] as? String == "agent_message" {
                return msg["message"] as? String
            }
            return nil
        case .none:
            return nil
        }
    }

    /// CLI の版（Settings に出す）。見つからなければ nil。
    static func version(_ cli: AssistantCLI) async -> String? {
        guard let path = cli.path else { return nil }
        return await Task.detached {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: path)
            p.arguments = ["--version"]
            let out = Pipe()
            p.standardOutput = out
            p.standardError = Pipe()
            guard (try? p.run()) != nil else { return nil }
            let data = out.fileHandleForReading.readDataToEndOfFile()
            p.waitUntilExit()
            return String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        }.value
    }
}

/// 差分を、AI に渡す unified diff 風の文字にする（大きければ切る）。
nonisolated enum DiffText {
    static let limit = 60_000

    /// 全ファイルを並べた差分（見出しの行がある）か、1 ファイルの差分。
    static func make(_ diff: FileDiff, path: String? = nil) -> String {
        var out = path.map { "--- \($0)\n" } ?? ""
        diff.withRows { rows in
            for (i, row) in rows.enumerated() {
                let text = diff.text(row: i)
                switch row.kind {
                case UInt8(AmonhenDiffRow_FILE): out += "\n--- \(text)\n"
                case UInt8(AmonhenDiffRow_HUNK): out += text + "\n"
                case UInt8(AmonhenDiffRow_ADDED): out += "+" + text + "\n"
                case UInt8(AmonhenDiffRow_REMOVED): out += "-" + text + "\n"
                default: out += " " + text + "\n"
                }
                if out.utf8.count > limit {
                    out += "\n…（長いので以下は省略）\n"
                    return
                }
            }
        }
        return out
    }
}

/// AI の返事を流しながら見せるパネル。コピー・作り直し（キャッシュを使わない）と、用途ごとの「使う」ボタン。
final class AssistantPanel: NSObject, NSWindowDelegate {
    private static var open: [AssistantPanel] = []

    private let panel: NSPanel
    private let text = NSTextView()
    private let status = NSTextField(labelWithString: "")
    private let spinner = NSProgressIndicator()
    private let regenerate = NSButton(title: "作り直す", target: nil, action: nil)
    private let useButton = NSButton(title: "使う", target: nil, action: nil)
    private let work: (_ useCache: Bool, _ onText: @escaping @MainActor (String) -> Void) async throws -> String
    private let onUse: ((String) -> Void)?
    private var result = ""
    private var task: Task<Void, Never>?

    /// `work` を実行して返事を出す。`useTitle` と `onUse` を渡すと「使う」ボタンを出す（結果を画面に入れる）。
    @discardableResult
    static func show(
        title: String, over window: NSWindow?, useTitle: String? = nil, onUse: ((String) -> Void)? = nil,
        work: @escaping (_ useCache: Bool, _ onText: @escaping @MainActor (String) -> Void) async throws -> String
    ) -> AssistantPanel {
        let p = AssistantPanel(title: title, useTitle: useTitle, onUse: onUse, work: work)
        open.append(p)
        if let window {
            let frame = window.frame
            p.panel.setFrameOrigin(NSPoint(x: frame.midX - p.panel.frame.width / 2, y: frame.midY - p.panel.frame.height / 2))
        } else {
            p.panel.center()
        }
        p.panel.makeKeyAndOrderFront(nil)
        p.start(useCache: true)
        return p
    }

    private init(
        title: String, useTitle: String?, onUse: ((String) -> Void)?,
        work: @escaping (_ useCache: Bool, _ onText: @escaping @MainActor (String) -> Void) async throws -> String
    ) {
        self.work = work
        self.onUse = onUse
        panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 620, height: 460),
            styleMask: [.titled, .closable, .resizable, .utilityWindow], backing: .buffered, defer: false)
        super.init()
        panel.title = "\(title) — \(Settings.assistant.title)"
        panel.isReleasedWhenClosed = false
        panel.delegate = self
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        text.isEditable = false
        text.isSelectable = true
        text.isRichText = false
        text.font = .systemFont(ofSize: 13)
        text.textContainerInset = NSSize(width: 8, height: 8)
        text.autoresizingMask = [.width]
        scroll.documentView = text
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        status.textColor = .secondaryLabelColor
        status.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        status.lineBreakMode = .byTruncatingTail
        let copy = NSButton(title: "コピー", target: self, action: #selector(copyResult))
        regenerate.target = self
        regenerate.action = #selector(regenerateResult)
        regenerate.toolTip = "前の結果を使わずに、もう一度頼む"
        useButton.title = useTitle ?? ""
        useButton.isHidden = onUse == nil
        useButton.target = self
        useButton.action = #selector(useResult)
        useButton.keyEquivalent = "\r"
        let bar = NSStackView(views: [spinner, status, NSView(), copy, regenerate, useButton])
        bar.spacing = 8
        for v in [scroll, bar] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            panel.contentView?.addSubview(v)
        }
        guard let content = panel.contentView else { return }
        NSLayoutConstraint.activate([
            scroll.topAnchor.constraint(equalTo: content.topAnchor),
            scroll.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            bar.topAnchor.constraint(equalTo: scroll.bottomAnchor, constant: 8),
            bar.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            bar.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            bar.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -10),
        ])
    }

    private func start(useCache: Bool) {
        task?.cancel()
        result = ""
        text.string = ""
        status.stringValue = "待っています…"
        spinner.startAnimation(nil)
        regenerate.isEnabled = false
        useButton.isEnabled = false
        RunningOperations.begin()
        task = Task { [weak self] in
            defer { RunningOperations.end() }
            guard let self else { return }
            do {
                let final = try await work(useCache) { [weak self] partial in
                    self?.text.string = partial
                    self?.text.scrollToEndOfDocument(nil)
                    self?.status.stringValue = "受け取っています…"
                }
                result = final
                text.string = final
                status.stringValue = "できました（\(Settings.assistantModel(Settings.assistant).isEmpty ? Settings.assistant.title : Settings.assistantModel(Settings.assistant))）"
                useButton.isEnabled = true
            } catch {
                status.stringValue = "失敗しました"
                text.string = "\(error)"
            }
            spinner.stopAnimation(nil)
            regenerate.isEnabled = true
        }
    }

    @objc private func copyResult() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(result.isEmpty ? text.string : result, forType: .string)
    }

    @objc private func regenerateResult() { start(useCache: false) }

    @objc private func useResult() {
        onUse?(result)
        panel.close()
    }

    func windowWillClose(_ notification: Notification) {
        task?.cancel()
        Self.open.removeAll { $0 === self }
    }
}
