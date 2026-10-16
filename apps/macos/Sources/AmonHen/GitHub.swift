import AmonhenKit
import AppKit

/// GitHub のリポジトリ（リモートの URL から判定する）。
struct GitHubRepository: Equatable, Sendable {
    let host: String
    let owner: String
    let name: String

    /// `gh` の `-R` に渡す形。
    var slug: String { host == "github.com" ? "\(owner)/\(name)" : "\(host)/\(owner)/\(name)" }
    var webURL: URL { URL(string: "https://\(host)/\(owner)/\(name)")! }

    /// `https://github.com/o/r.git`・`git@github.com:o/r.git`・`ssh://git@github.com/o/r` を読む。ホストの名前に
    /// "github" を含むものだけ（GitHub Enterprise を含む）。
    init?(remoteURL url: String) {
        var rest = url.trimmingCharacters(in: .whitespaces)
        var host: String
        if let r = rest.range(of: "://") {
            rest = String(rest[r.upperBound...])
            if let at = rest.firstIndex(of: "@"), rest[..<at].firstIndex(of: "/") == nil { rest = String(rest[rest.index(after: at)...]) }
            guard let slash = rest.firstIndex(of: "/") else { return nil }
            host = String(rest[..<slash])
            rest = String(rest[rest.index(after: slash)...])
        } else if let colon = rest.firstIndex(of: ":") {
            // scp 形式: user@host:owner/repo
            host = String(rest[..<colon])
            if let at = host.firstIndex(of: "@") { host = String(host[host.index(after: at)...]) }
            rest = String(rest[rest.index(after: colon)...])
        } else {
            return nil
        }
        if let colon = host.firstIndex(of: ":") { host = String(host[..<colon]) }
        guard host.lowercased().contains("github") else { return nil }
        if rest.hasSuffix(".git") { rest.removeLast(4) }
        let parts = rest.split(separator: "/").map(String.init)
        guard parts.count >= 2 else { return nil }
        self.host = host
        owner = parts[parts.count - 2]
        name = parts[parts.count - 1]
    }
}

/// プルリクエスト（`gh pr list --json`）。
struct PullRequest: Decodable, Sendable, Hashable {
    struct Author: Decodable, Sendable, Hashable { let login: String }
    struct Check: Decodable, Sendable, Hashable {
        /// CheckRun なら status・conclusion、StatusContext なら state。
        let status: String?
        let conclusion: String?
        let state: String?
    }

    let number: Int
    let title: String
    let url: String
    let headRefName: String
    let baseRefName: String
    let isDraft: Bool
    let author: Author
    let reviewDecision: String?
    let statusCheckRollup: [Check]?

    enum CI { case passing, failing, pending, none }

    /// CI の状態: 1 つでも失敗なら失敗、走っているものがあれば途中、すべて成功なら成功。
    var ci: CI {
        guard let checks = statusCheckRollup, !checks.isEmpty else { return .none }
        let results = checks.map { ($0.conclusion ?? $0.state ?? "").uppercased() }
        if results.contains(where: { ["FAILURE", "ERROR", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED"].contains($0) }) {
            return .failing
        }
        if checks.contains(where: { ($0.status ?? "").uppercased() != "COMPLETED" && $0.state == nil })
            || results.contains(where: { ["PENDING", "EXPECTED", ""].contains($0) }) {
            return .pending
        }
        return .passing
    }

    var ciMark: String {
        switch ci {
        case .passing: "✓"
        case .failing: "✗"
        case .pending: "●"
        case .none: ""
        }
    }
}

/// GitHub との連携。手元の `gh`（GitHub CLI）を起動して読み書きする。ログインとトークンは gh に任せ、
/// アプリはトークンに触れない。gh がなければ連携の機能を出さない。
enum GitHub {
    /// gh の場所。Finder から起動したアプリには端末の PATH がないので、よくある場所も探す。
    static let ghPath: String? = {
        let candidates = ["/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"]
            + (ProcessInfo.processInfo.environment["PATH"] ?? "").split(separator: ":").map { "\($0)/gh" }
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }()

    static var isAvailable: Bool { ghPath != nil }

    struct Failure: Error, CustomStringConvertible {
        let description: String
    }

    /// gh を `dir` で実行し、標準出力を返す。失敗したら標準エラーの文を投げる。裏で動く。
    static func run(_ args: [String], in dir: String? = nil) async throws -> Data {
        guard let gh = ghPath else { throw Failure(description: "gh（GitHub CLI）が見つかりません。brew install gh で入れ、gh auth login でログインしてください") }
        let started = CACurrentMediaTime()
        defer { PerformanceLog.record("GitHub: gh \(args.first ?? "")", ms: (CACurrentMediaTime() - started) * 1000) }
        return try await Task.detached(priority: .utility) {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: gh)
            process.arguments = args
            if let dir { process.currentDirectoryURL = URL(fileURLWithPath: dir) }
            var env = ProcessInfo.processInfo.environment
            env["GH_PROMPT_DISABLED"] = "1"
            env["NO_COLOR"] = "1"
            env["PATH"] = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:" + (env["PATH"] ?? "")
            process.environment = env
            let out = Pipe(), err = Pipe()
            process.standardOutput = out
            process.standardError = err
            try process.run()
            // 出力が大きくても詰まらないよう、終わるのを待つ前に読む。
            let data = out.fileHandleForReading.readDataToEndOfFile()
            let errData = err.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            guard process.terminationStatus == 0 else {
                let message = String(decoding: errData, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                throw Failure(description: message.isEmpty ? "gh \(args.joined(separator: " ")) が失敗しました" : message)
            }
            return data
        }.value
    }

    /// 開いているプルリクエスト（新しい順、最大 50 件）。
    static func pullRequests(_ repo: GitHubRepository) async throws -> [PullRequest] {
        let data = try await run([
            "pr", "list", "-R", repo.slug, "--state", "open", "--limit", "50",
            "--json", "number,title,url,headRefName,baseRefName,isDraft,author,reviewDecision,statusCheckRollup",
        ])
        return try JSONDecoder().decode([PullRequest].self, from: data)
    }

    /// 既定のブランチ（プルリクエストの base の初期値）。
    static func defaultBranch(_ repo: GitHubRepository) async -> String? {
        struct View: Decodable {
            struct Ref: Decodable { let name: String }
            let defaultBranchRef: Ref?
        }
        let data = try? await run(["repo", "view", repo.slug, "--json", "defaultBranchRef"])
        return data.flatMap { try? JSONDecoder().decode(View.self, from: $0) }?.defaultBranchRef?.name
    }

    /// プルリクエストを作り、その URL を返す。今のブランチを push してあること。
    static func createPullRequest(
        in dir: String, repo: GitHubRepository, title: String, body: String, base: String, draft: Bool
    ) async throws -> String {
        var args = ["pr", "create", "-R", repo.slug, "--title", title, "--body", body, "--base", base]
        if draft { args.append("--draft") }
        let data = try await run(args, in: dir)
        return String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// プルリクエストのブランチをチェックアウトする（なければ作る）。
    static func checkout(_ number: Int, in dir: String, repo: GitHubRepository) async throws {
        _ = try await run(["pr", "checkout", "\(number)", "-R", repo.slug], in: dir)
    }
}

/// Repository > Create Pull Request… のシート。
enum CreatePullRequestSheet {
    /// `title`・`body` を初期値に、base と draft を選んでもらう。キャンセルなら nil。
    static func ask(
        in window: NSWindow, branch: String, title: String, body: String, base: String,
        completion: @escaping ((title: String, body: String, base: String, draft: Bool)?) -> Void
    ) {
        let alert = NSAlert()
        alert.messageText = "Create Pull Request"
        alert.informativeText = "\(branch) から"
        alert.addButton(withTitle: "Create")
        alert.addButton(withTitle: "キャンセル")
        let titleField = NSTextField(string: title)
        titleField.placeholderString = "タイトル"
        let baseField = NSTextField(string: base)
        baseField.placeholderString = "main"
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 460, height: 160))
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        let bodyView = NSTextView(frame: scroll.bounds)
        bodyView.isRichText = false
        bodyView.font = .systemFont(ofSize: 12)
        bodyView.isAutomaticQuoteSubstitutionEnabled = false
        bodyView.autoresizingMask = [.width]
        bodyView.string = body
        scroll.documentView = bodyView
        let draft = NSButton(checkboxWithTitle: "Draft として作る", target: nil, action: nil)
        let grid = NSGridView(views: [
            [NSTextField(labelWithString: "タイトル:"), titleField],
            [NSTextField(labelWithString: "base:"), baseField],
            [NSTextField(labelWithString: "本文:"), scroll],
            [NSGridCell.emptyContentView, draft],
        ])
        grid.column(at: 0).xPlacement = .trailing
        grid.row(at: 2).yPlacement = .top
        grid.rowSpacing = 8
        grid.column(at: 1).width = 460
        grid.frame = NSRect(x: 0, y: 0, width: 540, height: 260)
        alert.accessoryView = grid
        alert.window.initialFirstResponder = titleField
        alert.beginSheetModal(for: window) { response in
            let t = titleField.stringValue.trimmingCharacters(in: .whitespaces)
            let b = baseField.stringValue.trimmingCharacters(in: .whitespaces)
            guard response == .alertFirstButtonReturn, !t.isEmpty, !b.isEmpty else { return completion(nil) }
            completion((t, bodyView.string, b, draft.state == .on))
        }
    }
}
