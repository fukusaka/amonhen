import AmonhenKit
import CAmonhen
import CoreFoundation
import Foundation
import Testing

private func git(_ dir: URL, _ args: String...) throws {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/bin/env")
    p.arguments = ["git"] + args
    p.currentDirectoryURL = dir
    p.environment = [
        "PATH": "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin",
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_AUTHOR_NAME": "Alice",
        "GIT_AUTHOR_EMAIL": "a@example.com",
        "GIT_COMMITTER_NAME": "Alice",
        "GIT_COMMITTER_EMAIL": "a@example.com",
    ]
    try p.run()
    p.waitUntilExit()
    #expect(p.terminationStatus == 0, "git \(args)")
}

private func makeRepo() throws -> URL {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    try git(dir, "init", "-q")
    try git(dir, "symbolic-ref", "HEAD", "refs/heads/main")
    try git(dir, "commit", "--allow-empty", "-q", "-m", "first")
    try git(dir, "commit", "--allow-empty", "-q", "-m", "日本語の件名")
    return dir
}

@Test func readsHistoryAndTexts() throws {
    let dir = try makeRepo()
    defer { try? FileManager.default.removeItem(at: dir) }

    let repo = try Repository(path: dir.path)
    #expect(repo.latestHistory() == nil)
    let history = try repo.loadHistory()
    #expect(history.generation == 1)
    #expect(history.rowCount == 2)
    #expect(history.withNodes { $0.map(\.lane) } == [0, 0])
    #expect(history.edgeIndices(in: 0..<2) == [0])

    let chunk = try history.chunk(rows: 0..<2)
    #expect(chunk.subject(row: 0) == "日本語の件名")
    #expect(chunk.subject(row: 1) == "first")
    #expect(chunk.authorName(row: 1) == "Alice")
    #expect(chunk.authorTime(row: 1).timeIntervalSinceNow < 60)
}

/// NoCopy の CFString が、実際にコアのメモリを指したままであること。
@Test func stringsPointIntoCoreMemory() throws {
    let dir = try makeRepo()
    defer { try? FileManager.default.removeItem(at: dir) }

    let history = try Repository(path: dir.path).loadHistory()
    let chunk = try history.chunk(rows: 0..<2)
    #expect(chunk.subject(row: 1) == "first")
    chunk.withSubject(row: 0) { s in
        // UTF-16 は元の配列をそのまま返す。
        #expect(CFStringGetCharactersPtr(s) != nil)
    }
}

/// NUL で終わらない ASCII のバイト列でも、CFString がコピーせずに参照し続けること
/// （元のバッファを書き換えると CFString の内容も変わる）。
/// 短い文字列は tagged pointer になり、ポインタの値に埋め込まれる（ヒープ確保もない）ので対象外。
@Test func asciiNoCopyStringsKeepReferencingTheBuffer() {
    let text = "Fix the history graph when a branch is merged twice; trailing"
    var bytes: [UInt8] = Array(text.utf8)
    let length = text.count - ";trailing".count
    bytes.withUnsafeMutableBufferPointer { buf in
        let s = CFStringCreateWithBytesNoCopy(
            nil, buf.baseAddress, length, CFStringBuiltInEncodings.ASCII.rawValue, false, kCFAllocatorNull)!
        #expect(CFStringGetLength(s) == length)
        buf[0] = UInt8(ascii: "f")
        #expect(CFStringGetCharacterAtIndex(s, 0) == UniChar(UInt8(ascii: "f")))
    }
}

@Test func showsTheCachedHistoryOnTheNextOpen() throws {
    let dir = try makeRepo()
    let cache = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer {
        try? FileManager.default.removeItem(at: dir)
        try? FileManager.default.removeItem(at: cache)
    }
    let first = try Repository(path: dir.path, cacheDirectory: cache)
    #expect(first.loadCachedHistory() == nil)
    _ = try first.loadHistory()

    let second = try Repository(path: dir.path, cacheDirectory: cache)
    let cached = try #require(second.loadCachedHistory())
    #expect(cached.rowCount == 2)
    #expect(try cached.chunk(rows: 0..<1).subject(row: 0) == "日本語の件名")

    // 作り直すと、前回のものを引き継がずに新しい History を出す。
    let rebuilt = try second.rebuildHistory()
    #expect(rebuilt.generation > cached.generation)
    #expect(rebuilt.rowCount == 2)
    #expect(try rebuilt.chunk(rows: 0..<1).subject(row: 0) == "日本語の件名")
}

@Test func listsRefsAndFindsTheirRows() throws {
    let dir = try makeRepo()
    defer { try? FileManager.default.removeItem(at: dir) }
    try git(dir, "branch", "topic", "HEAD~1")
    let repo = try Repository(path: dir.path)
    let history = try repo.loadHistory()
    let refs = try repo.loadRefs()
    #expect(refs.headBranch == "main")
    #expect(refs.items.map(\.name) == ["main", "topic"])
    #expect(history.row(of: refs.items[1].target!) == 1)
}

@Test(.timeLimit(.minutes(1))) func reportsChangesToRefs() async throws {
    let dir = try makeRepo()
    defer { try? FileManager.default.removeItem(at: dir) }
    let repo = try Repository(path: dir.path)
    let changes = AsyncStream<Changes>.makeStream()
    let watch = try RepositoryWatch(repository: repo) { changes.continuation.yield($0) }
    try await Task.sleep(for: .milliseconds(300))
    try git(dir, "branch", "new-branch")
    for await c in changes.stream where c.contains(.refs) { break }
    withExtendedLifetime(watch) {}
}

@Test func showsTheDiffOfASelectedCommit() throws {
    let dir = try makeRepo()
    defer { try? FileManager.default.removeItem(at: dir) }
    try "one\ntwo\n".write(to: dir.appendingPathComponent("a.txt"), atomically: true, encoding: .utf8)
    try git(dir, "add", ".")
    try git(dir, "commit", "-q", "-m", "add a")
    try "one\n二\n".write(to: dir.appendingPathComponent("a.txt"), atomically: true, encoding: .utf8)
    try git(dir, "commit", "-q", "-am", "change a")

    let repo = try Repository(path: dir.path)
    let history = try repo.loadHistory()
    let changes = try repo.changes(of: try #require(history.commitID(row: 0)))
    #expect(changes.items.map(\.path) == ["a.txt"])
    let diff = try changes.diff(at: 0)
    #expect((diff.rowCount, diff.added, diff.removed) == (4, 1, 1))
    #expect((0..<4).map { diff.text(row: $0) } == ["@@ -1,2 +1,2 @@", "one", "two", "二"])
    #expect(diff.withRows { $0.map(\.kind) } == [3, 0, 2, 1])
}

@Test func stagesLinesAndCommits() throws {
    let dir = try makeRepo()
    defer { try? FileManager.default.removeItem(at: dir) }
    let file = dir.appendingPathComponent("a.txt")
    try "1\n2\n3\n".write(to: file, atomically: true, encoding: .utf8)
    try git(dir, "add", ".")
    try git(dir, "commit", "-q", "-m", "add a")
    try "one\n2\nthree\n".write(to: file, atomically: true, encoding: .utf8)
    try "new\n".write(to: dir.appendingPathComponent("b.txt"), atomically: true, encoding: .utf8)

    let repo = try Repository(path: dir.path)
    var status = try repo.loadStatus()
    #expect(status.branch == "main")
    #expect(status.items.map(\.path) == ["a.txt", "b.txt"])
    #expect(status.items[1].kind == .untracked)

    let diff = try status.diff(of: status.items[0], staged: false)
    #expect(diff.rowCount == 6)
    try diff.stage(rows: [1, 2])
    try repo.stage(paths: ["b.txt"])

    status = try repo.loadStatus()
    #expect(status.items.map { "\($0.indexState)\($0.worktreeState)" } == ["MM", "A."])
    try repo.unstage(paths: ["b.txt"])
    try repo.commit(message: "first line only", amend: false)
    status = try repo.loadStatus()
    #expect(status.items.map { "\($0.indexState)\($0.worktreeState)" } == [".M", "??"])
}

@Test func reportsLineEndingsOfWorkingCopyFiles() throws {
    let dir = try makeRepo()
    defer { try? FileManager.default.removeItem(at: dir) }
    let file = dir.appendingPathComponent("a.txt")
    try "1\n2\n".write(to: file, atomically: true, encoding: .utf8)
    try git(dir, "add", ".")
    try git(dir, "commit", "-q", "-m", "add a")
    // Windows の既定（core.autocrlf=true）: 作業ツリーは CRLF、git には LF。
    try git(dir, "config", "core.autocrlf", "true")
    try "1\r\nTWO\r\n".write(to: file, atomically: true, encoding: .utf8)

    let status = try Repository(path: dir.path).loadStatus()
    let le = try #require(try status.diff(of: status.items[0], staged: false).lineEndings)
    #expect([le.index, le.worktree, le.checkout] == [.lf, .crlf, .crlf])
    #expect(le.rule == "core.autocrlf=true" && le.notable && !le.warning)
    #expect(try status.combinedDiff(of: status.items[0])?.lineEndings == le)
    #expect(try status.diff(of: status.items[0], staged: true).lineEndings == nil)
}

@Test func runsCommandsAndUndoes() throws {
    let dir = try makeRepo()
    defer { try? FileManager.default.removeItem(at: dir) }
    let repo = try Repository(path: dir.path)
    try repo.run(["op": "createBranch", "name": "topic", "start": "main", "checkout": true])
    #expect(try repo.loadRefs().headBranch == "topic")
    #expect(try repo.run(["op": "undoPeek"]).label == "Create branch topic")
    #expect(throws: AmonhenError.self) { try repo.run(["op": "merge", "rev": "no-such-branch"]) }
    // 作って切り替えたブランチの Undo は、元のブランチに戻ってからブランチを消す。
    #expect(try repo.run(["op": "undo"]).label == "Create branch topic")
    let refs = try repo.loadRefs()
    #expect(refs.headBranch == "main")
    #expect(refs.items.map(\.name) == ["main"])
}

@Test func readsTheReflog() throws {
    let dir = try makeRepo()
    defer { try? FileManager.default.removeItem(at: dir) }
    try git(dir, "reset", "-q", "--hard", "HEAD~1")
    let repo = try Repository(path: dir.path)
    let entries = try repo.reflog()
    #expect(entries.map(\.selector) == ["HEAD@{0}", "HEAD@{1}", "HEAD@{2}"])
    #expect(entries[0].message.hasPrefix("reset: moving to"))
    // reset で外れたコミットは、どのブランチからも辿れない。
    #expect(entries[1].subject == "日本語の件名")
    #expect(!entries[1].reachable)
    #expect(entries[2].old == nil && entries[2].reachable)
    #expect(try repo.reflog(limit: 1).count == 1)
}

@Test func reportsOpenErrors() {
    #expect(throws: AmonhenError.self) {
        try Repository(path: "/nonexistent/amonhen")
    }
}

@Test func buildsSSHLocations() {
    #expect(RepositoryLocation.ssh(host: "dev", path: "/srv/repo/") == "ssh://dev/srv/repo")
    #expect(RepositoryLocation.ssh(host: " me@dev:2222 ", path: "~/src/repo") == "ssh://me@dev:2222/~/src/repo")
    #expect(RepositoryLocation.ssh(host: "dev", path: "src/repo") == "ssh://dev/~/src/repo")
    #expect(RepositoryLocation.ssh(host: "", path: "/r") == nil)
    #expect(RepositoryLocation.ssh(host: "dev/x", path: "/r") == nil)
    #expect(RepositoryLocation.ssh(host: "dev", path: " ") == nil)
    let location = RepositoryLocation.ssh(host: "me@dev", path: "~/repo")!
    #expect(RepositoryLocation.host(location) == "me@dev")
    #expect(RepositoryLocation.path(location) == "/~/repo")
}

@Test func masksVariablesWhoseNamesLookSecret() {
    for name in ["GH_TOKEN", "CLIENT_SECRET", "DB_PASSWORD", "MYSQL_PASSWD", "AWS_SECRET_ACCESS_KEY", "SSH_KEY",
                 "OPENAI_APIKEY", "ACCESSKEY", "GOOGLE_APPLICATION_CREDENTIALS", "NPM_AUTH", "DB_PASS",
                 "SIGNING_PRIVATE", "api_key", "Npm-Auth"] {
        #expect(SecretNames.isSecret(name), "\(name)")
    }
    for name in ["PATH", "HOME", "SSH_AUTH_SOCK", "AMONHEN_PASS_ENV", "GIT_ASKPASS", "SSH_ASKPASS", "GIT_AUTHOR_NAME",
                 "GPG_KEYID", "XKB_KEYMAP", "KEYCHAIN_PATH", "PASSAGE_DIR", "LANG"] {
        #expect(!SecretNames.isSecret(name), "\(name)")
    }
}
