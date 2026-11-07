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

    init(host: String, owner: String, name: String) {
        self.host = host
        self.owner = owner
        self.name = name
    }

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

/// プルリクエストを出す先。origin が GitHub の fork なら、その元のリポジトリ（`forkOwner` は fork の持ち主。
/// head は `持ち主:ブランチ` の形で渡し、一覧はその fork から出したものだけにする）。fork でなければ origin。
struct PullRequestTarget: Equatable, Sendable {
    let repo: GitHubRepository
    let forkOwner: String?
    /// fork の持ち主が組織（プルリクエストの作者は人なので、作者では絞れない）。
    var forkInOrganization = false

    /// fork から元のリポジトリへ出す。既定のブランチ（fork の main など）からも出せる。
    var isFork: Bool { forkOwner != nil }

    /// `gh pr create --head` に渡す形。
    func head(_ branch: String) -> String { forkOwner.map { "\($0):\(branch)" } ?? branch }

    /// 一覧のうち、この先に出したもの（fork なら、その fork から出したものだけ）。
    func mine<T>(_ list: [T], owner: (T) -> String?) -> [T] {
        guard let forkOwner else { return list }
        return list.filter { owner($0)?.lowercased() == forkOwner.lowercased() }
    }
}

/// head のリポジトリの持ち主（`gh pr list --json headRepositoryOwner`）。
struct RepositoryOwner: Decodable, Sendable, Hashable { let login: String }

/// マージされたプルリクエスト（`gh pr list --state merged --json`）。`headRefOid` はマージしたときの head。
struct MergedPullRequest: Decodable, Sendable, Equatable {
    let number: Int
    let headRefName: String
    let headRefOid: String
    let headRepositoryOwner: RepositoryOwner?
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
    /// head のコミット ID（CI の吹き出しで、そのコミットのチェックを読む）。
    let headRefOid: String?
    /// head のリポジトリの持ち主（fork から出したものを見分ける。fork のときだけ読む）。
    let headRepositoryOwner: RepositoryOwner?

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

/// 1 つのコミットのチェック 1 つ（GitHub Actions のジョブ、または外部の status）。
struct CommitCheck: Sendable, Hashable {
    let name: String
    /// CheckRun なら conclusion（success・failure・cancelled・skipped・timed_out・action_required・neutral…。走っている
    /// 途中は status: queued・in_progress）、status context なら state（success・failure・error・pending）。小文字。
    let result: String
    let url: String?
    /// かかった時間（秒。終わっていなければ nil）。
    let seconds: Int?

    /// 吹き出しの行の見た目に使う状態。取り消しは失敗ではない（やり直す前の run や、上書きされた run）。
    var ci: PullRequest.CI {
        switch result {
        case "success": .passing
        case "failure", "error", "timed_out", "action_required", "startup_failure", "stale": .failing
        case "queued", "in_progress", "pending", "waiting", "requested", "expected": .pending
        default: .none
        }
    }

    var isCancelled: Bool { result == "cancelled" }

    /// 同じ名前のチェック（やり直しや上書きで run が増えたもの）を 1 つにまとめる。成功があれば成功 1 つ（取り消しや
    /// ほかの成功は出さない）、なければ失敗、走っている途中、取り消し、その他の順で代表を選ぶ。順は読んだ順
    /// （新しい run が先）。
    static func collapsed(_ checks: [CommitCheck]) -> [CommitCheck] {
        var names: [String] = []
        var runs: [String: [CommitCheck]] = [:]
        for check in checks {
            if runs[check.name] == nil { names.append(check.name) }
            runs[check.name, default: []].append(check)
        }
        let rank: (CommitCheck) -> Int = { check in
            switch check.ci {
            case .passing: 0
            case .failing: 1
            case .pending: 2
            case .none: check.isCancelled ? 3 : 4
            }
        }
        // min は同じ順位なら先のもの（新しい run）を残す。
        return names.compactMap { runs[$0]?.min { rank($0) < rank($1) } }
    }

    var resultLabel: String {
        switch result {
        case "success": L("passed")
        case "failure", "error", "startup_failure": L("failed")
        case "cancelled": L("cancelled")
        case "timed_out": L("timed out")
        case "skipped": L("skipped")
        case "neutral": L("neutral")
        case "action_required": L("action required")
        case "stale": L("stale")
        case "queued", "waiting", "requested", "expected": L("queued")
        case "in_progress", "pending": L("running")
        default: result
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
        guard let gh = ghPath else { throw Failure(description: L("gh (GitHub CLI) wasn’t found. Install it with brew install gh, then sign in with gh auth login.")) }
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
                throw Failure(description: message.isEmpty ? L("gh %@ failed.", args.joined(separator: " ")) : message)
            }
            return data
        }.value
    }

    /// `repo` が fork なら、その元のリポジトリ（fork でなければ nil）。読めなければ投げる。
    static func parent(_ repo: GitHubRepository) async throws -> (repo: GitHubRepository, organization: Bool)? {
        struct View: Decodable { let isInOrganization: Bool? }
        let data = try await run(["repo", "view", repo.slug, "--json", "parent,isInOrganization"])
        guard let parent = parent(from: data, host: repo.host) else { return nil }
        return (parent, (try? JSONDecoder().decode(View.self, from: data))?.isInOrganization == true)
    }

    /// `gh repo view --json parent` の答えから、元のリポジトリ（同じホスト）。
    static func parent(from data: Data, host: String) -> GitHubRepository? {
        struct View: Decodable {
            struct Parent: Decodable { let name: String; let owner: RepositoryOwner }
            let parent: Parent?
        }
        guard let parent = (try? JSONDecoder().decode(View.self, from: data))?.parent else { return nil }
        return GitHubRepository(host: host, owner: parent.owner.login, name: parent.name)
    }

    /// 読めた fork の元（slug → 元と、持ち主が組織か。fork でなければ nil）。読めなかったものは覚えない（次にまた読む）。
    @MainActor private static var parents: [String: (repo: GitHubRepository, organization: Bool)?] = [:]

    /// `origin` のリポジトリからプルリクエストを出す先（fork なら元のリポジトリ）。
    @MainActor static func pullRequestTarget(_ origin: GitHubRepository) async -> PullRequestTarget {
        let parent: (repo: GitHubRepository, organization: Bool)?
        if let known = parents[origin.slug] {
            parent = known
        } else if let read = try? await Self.parent(origin) {
            parents[origin.slug] = .some(read)
            parent = read
        } else {
            parent = nil
        }
        guard let parent else { return PullRequestTarget(repo: origin, forkOwner: nil) }
        return PullRequestTarget(repo: parent.repo, forkOwner: origin.owner, forkInOrganization: parent.organization)
    }

    /// `pr list` の、fork のときの引数（その fork の持ち主が出したものに絞り、head の持ち主も読む）。元のリポジトリの
    /// 新しいものから数えると、ほかの人のプルリクエストで埋まるので、作者で絞ってから数える。組織の fork は作者で
    /// 絞れないので、多めに読んで `mine` で絞る（それより古いものは見つからない）。
    private static func forkArguments(_ target: PullRequestTarget) -> (args: [String], field: String) {
        guard let owner = target.forkOwner else { return ([], "") }
        return (target.forkInOrganization ? [] : ["--author", owner], ",headRepositoryOwner")
    }

    /// `pr list` の `--limit`。
    private static func limit(_ count: Int, _ target: PullRequestTarget) -> String {
        String(target.forkInOrganization ? 300 : count)
    }

    /// 開いているプルリクエスト（新しい順、最大 50 件。fork なら、その fork から出したもの）。
    static func pullRequests(_ target: PullRequestTarget) async throws -> [PullRequest] {
        let fork = forkArguments(target)
        let data = try await run([
            "pr", "list", "-R", target.repo.slug, "--state", "open", "--limit", limit(50, target),
            "--json", "number,title,url,headRefName,baseRefName,isDraft,author,reviewDecision,statusCheckRollup,headRefOid" + fork.field,
        ] + fork.args)
        return target.mine(try JSONDecoder().decode([PullRequest].self, from: data)) { $0.headRepositoryOwner?.login }
    }

    /// マージされたプルリクエスト（新しい順、最大 200 件。fork なら、その fork から出したもの）。squash・rebase で
    /// 取り込んだブランチを見分ける（コアの `branchCleanup` の `pullRequests`）。
    static func mergedPullRequests(_ target: PullRequestTarget) async throws -> [MergedPullRequest] {
        let fork = forkArguments(target)
        let data = try await run([
            "pr", "list", "-R", target.repo.slug, "--state", "merged", "--limit", limit(200, target),
            "--json", "number,headRefName,headRefOid" + fork.field,
        ] + fork.args)
        return target.mine(try JSONDecoder().decode([MergedPullRequest].self, from: data)) { $0.headRepositoryOwner?.login }
    }

    /// 開いている Issue（新しい順、最大 30 件）。コミットのメッセージの #123 の候補。
    static func issues(_ repo: GitHubRepository) async throws -> [(number: Int, title: String)] {
        struct Issue: Decodable { let number: Int; let title: String }
        let data = try await run(["issue", "list", "-R", repo.slug, "--state", "open", "--limit", "30", "--json", "number,title"])
        return try JSONDecoder().decode([Issue].self, from: data).map { ($0.number, $0.title) }
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

    /// `head` のブランチからプルリクエストを作り、その URL を返す。そのブランチを push してあること。
    static func createPullRequest(
        in dir: String?, repo: GitHubRepository, head: String, title: String, body: String, base: String, draft: Bool
    ) async throws -> String {
        var args = ["pr", "create", "-R", repo.slug, "--head", head, "--title", title, "--body", body, "--base", base]
        if draft { args.append("--draft") }
        let data = try await run(args, in: dir)
        return String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// ブランチの直近のコミット（それぞれ `count` 個まで）の CI の状態（`ci` のとき）と、作者の GitHub のアバターの
    /// URL（`authors` のとき。メールアドレス → URL）。GraphQL で 1 回に読む。GitHub にないブランチは飛ばす。
    static func commitStatuses(
        _ repo: GitHubRepository, branches: [String], count: Int = 100, ci: Bool = true, authors: Bool = true
    ) async throws -> (ci: [String: PullRequest.CI], avatars: [String: URL]) {
        guard !branches.isEmpty, ci || authors else { return ([:], [:]) }
        // 要るものだけを頼む（量を減らす）。
        let wanted = ["oid", authors ? "author { email avatarUrl(size: 96) }" : nil, ci ? "statusCheckRollup { state }" : nil]
            .compactMap { $0 }.joined(separator: " ")
        // ブランチごとに別名を付けて並べる（b0・b1 …）。
        let fields = branches.indices.map { i in
            """
            b\(i): ref(qualifiedName: $r\(i)) { target { ... on Commit { history(first: \(count)) { nodes {
              \(wanted) } } } } }
            """
        }.joined(separator: "\n")
        let variables = branches.indices.map { "$r\($0): String!" }.joined(separator: ", ")
        let query = "query($owner: String!, $name: String!, \(variables)) { repository(owner: $owner, name: $name) { \(fields) } }"
        var args = ["api", "graphql", "-f", "query=\(query)", "-f", "owner=\(repo.owner)", "-f", "name=\(repo.name)"]
        if repo.host != "github.com" { args += ["--hostname", repo.host] }
        for (i, branch) in branches.enumerated() { args += ["-f", "r\(i)=refs/heads/\(branch)"] }
        let data = try await run(args)
        struct Node: Decodable {
            struct Author: Decodable { let email: String?; let avatarUrl: String? }
            struct Rollup: Decodable { let state: String }
            let oid: String
            let author: Author?
            let statusCheckRollup: Rollup?
        }
        struct Ref: Decodable {
            struct Target: Decodable {
                struct History: Decodable { let nodes: [Node] }
                let history: History?
            }
            let target: Target?
        }
        struct Response: Decodable {
            struct Body: Decodable { let repository: [String: Ref?]? }
            let data: Body?
        }
        let response = try JSONDecoder().decode(Response.self, from: data)
        var ci: [String: PullRequest.CI] = [:]
        var avatars: [String: URL] = [:]
        for ref in (response.data?.repository ?? [:]).values {
            for node in ref?.target?.history?.nodes ?? [] {
                if let state = node.statusCheckRollup?.state {
                    ci[node.oid] = switch state {
                    case "SUCCESS": .passing
                    case "FAILURE", "ERROR": .failing
                    default: .pending
                    }
                }
                if let email = node.author?.email?.lowercased(), !email.isEmpty,
                   let url = node.author?.avatarUrl.flatMap(URL.init(string:)) {
                    avatars[email] = url
                }
            }
        }
        return (ci, avatars)
    }

    /// `sha` のコミットのチェック（Actions の check run と、外部の status）。印を押したときに読む。
    static func commitChecks(_ repo: GitHubRepository, sha: String) async throws -> [CommitCheck] {
        var host: [String] = []
        if repo.host != "github.com" { host = ["--hostname", repo.host] }
        let base = "repos/\(repo.owner)/\(repo.name)/commits/\(sha)"
        async let runsData = run(["api", "\(base)/check-runs?per_page=100"] + host)
        async let statusData = run(["api", "\(base)/status"] + host)
        struct Runs: Decodable {
            struct Run: Decodable {
                let name: String
                let status: String?
                let conclusion: String?
                let html_url: String?
                let started_at: String?
                let completed_at: String?
            }
            let check_runs: [Run]
        }
        struct Status: Decodable {
            struct Context: Decodable {
                let context: String
                let state: String
                let target_url: String?
            }
            let statuses: [Context]
        }
        let iso = ISO8601DateFormatter()
        var out: [CommitCheck] = []
        for run in try JSONDecoder().decode(Runs.self, from: try await runsData).check_runs {
            let result = (run.conclusion ?? run.status ?? "").lowercased()
            let seconds: Int? = {
                guard let s = run.started_at.flatMap(iso.date(from:)), let e = run.completed_at.flatMap(iso.date(from:)) else { return nil }
                return Int(e.timeIntervalSince(s))
            }()
            out.append(CommitCheck(name: run.name, result: result, url: run.html_url, seconds: seconds))
        }
        for context in try JSONDecoder().decode(Status.self, from: try await statusData).statuses {
            out.append(CommitCheck(name: context.context, result: context.state.lowercased(), url: context.target_url, seconds: nil))
        }
        return out
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
        alert.messageText = L("Create Pull Request")
        alert.informativeText = L("From %@", branch)
        let create = alert.addButton(withTitle: L("Create"))
        alert.addButton(withTitle: L("Cancel"))
        let titleField = NSTextField(string: title)
        titleField.placeholderString = L("Title")
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
        let draft = NSButton(checkboxWithTitle: L("Create as Draft"), target: nil, action: nil)
        let grid = NSGridView(views: [
            [NSTextField(labelWithString: L("Title:")), titleField],
            [NSTextField(labelWithString: "base:"), baseField],
            [NSTextField(labelWithString: L("Body:")), scroll],
            [NSGridCell.emptyContentView, draft],
        ])
        grid.column(at: 0).xPlacement = .trailing
        grid.row(at: 2).yPlacement = .top
        grid.rowSpacing = 8
        grid.column(at: 1).width = 460
        grid.frame = NSRect(x: 0, y: 0, width: 540, height: 260)
        alert.accessoryView = grid
        alert.window.initialFirstResponder = titleField
        // 題と base が空の間は Create を押せない（押してもシートと打った文を黙って捨てないように）。
        let trimmed = { (field: NSTextField) in field.stringValue.trimmingCharacters(in: .whitespaces) }
        let update = { create.isEnabled = !trimmed(titleField).isEmpty && !trimmed(baseField).isEmpty }
        update()
        nonisolated(unsafe) let refresh = update
        let observers = [titleField, baseField].map {
            NotificationCenter.default.addObserver(forName: NSControl.textDidChangeNotification, object: $0, queue: .main) { _ in
                MainActor.assumeIsolated { refresh() }
            }
        }
        alert.beginSheetModal(for: window) { response in
            observers.forEach(NotificationCenter.default.removeObserver)
            let t = titleField.stringValue.trimmingCharacters(in: .whitespaces)
            let b = baseField.stringValue.trimmingCharacters(in: .whitespaces)
            guard response == .alertFirstButtonReturn, !t.isEmpty, !b.isEmpty else { return completion(nil) }
            completion((t, bodyView.string, b, draft.state == .on))
        }
    }
}
