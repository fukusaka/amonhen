import AmonhenKit
import AppKit

/// コアの `worktrees` の 1 つ（`crates/core/src/worktree.rs`）。
nonisolated struct WorktreeInfo: Decodable, Sendable, Equatable {
    let path: String
    let head: String?
    let branch: String?
    let detached: Bool
    let bare: Bool
    let locked: String?
    let prunable: String?
    let main: Bool
    let current: Bool

    /// 見せる名前: フォルダの名前。同じ名前のものがほかにあれば、見分けがつくところまで親のフォルダを足す
    /// （`build/aslr/src` と `build/ssp/src` なら `aslr/src`・`ssp/src`）。一覧を読んだときに `WorktreeList` が決める。
    var name: String

    private enum CodingKeys: String, CodingKey {
        case path, head, branch, detached, bare, locked, prunable, main, current
    }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        path = try c.decode(String.self, forKey: .path)
        head = try c.decodeIfPresent(String.self, forKey: .head)
        branch = try c.decodeIfPresent(String.self, forKey: .branch)
        detached = try c.decode(Bool.self, forKey: .detached)
        bare = try c.decode(Bool.self, forKey: .bare)
        locked = try c.decodeIfPresent(String.self, forKey: .locked)
        prunable = try c.decodeIfPresent(String.self, forKey: .prunable)
        main = try c.decode(Bool.self, forKey: .main)
        current = try c.decode(Bool.self, forKey: .current)
        name = (path as NSString).lastPathComponent
    }

    /// `list` の名前を、重ならないところまで親のフォルダを足して決める。
    static func disambiguate(_ list: [WorktreeInfo]) -> [WorktreeInfo] {
        let parts = list.map { $0.path.split(separator: "/").map(String.init) }
        var depth = Array(repeating: 1, count: list.count)
        func label(_ i: Int) -> String { parts[i].suffix(depth[i]).joined(separator: "/") }
        while true {
            let groups = Dictionary(grouping: list.indices, by: label)
            var grew = false
            for (_, members) in groups where members.count > 1 {
                for i in members where depth[i] < parts[i].count {
                    depth[i] += 1
                    grew = true
                }
            }
            if !grew { break }
        }
        return list.indices.map { i in
            var w = list[i]
            if !parts[i].isEmpty { w.name = label(i) }
            return w
        }
    }

    /// 同じ worktree か（シンボリックリンクを解いて比べる）。
    func isSame(_ other: String) -> Bool { Self.canonical(path) == Self.canonical(other) }

    /// シンボリックリンクを解いたパス。ファイルシステムを引くので、一度解いたものは覚えておく（一覧を描くたびに
    /// 何千回も呼ばれる）。
    static func canonical(_ path: String) -> String {
        canonicalLock.lock()
        if let known = canonicalCache[path] {
            canonicalLock.unlock()
            return known
        }
        canonicalLock.unlock()
        // /home・/net は macOS では自動マウントの場所で、引くとマウントを探しに行って数秒止まる（SSH 先の worktree の
        // パスがここに来る）。手元のものではないので、そのまま比べる。
        let automounted = ["/home/", "/net/", "/Network/"].contains { path.hasPrefix($0) }
        let resolved = automounted ? path : URL(fileURLWithPath: path).resolvingSymlinksInPath().standardizedFileURL.path
        canonicalLock.lock()
        canonicalCache[path] = resolved
        canonicalLock.unlock()
        return resolved
    }

    nonisolated(unsafe) private static var canonicalCache: [String: String] = [:]
    private static let canonicalLock = NSLock()
}

nonisolated struct WorktreeList: Decodable, Sendable {
    let worktrees: [WorktreeInfo]

    private enum CodingKeys: String, CodingKey { case worktrees }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        worktrees = WorktreeInfo.disambiguate(try c.decode([WorktreeInfo].self, forKey: .worktrees))
    }
}

/// コアの `worktreeChanges` の答え（ほかの worktree の変更の数）。
nonisolated struct WorktreeChangesList: Decodable, Sendable {
    struct Entry: Decodable, Sendable {
        let path: String
        let count: Int?
        let stamp: UInt64
    }
    let worktrees: [Entry]
}

/// 書き込む先にできる worktree の組: そのリポジトリ、操作、Working Copy、変化の見張り。History・ref・詳細は
/// リポジトリで共通なので、ウィンドウが開いたリポジトリのものを使う（docs/ux.md の「Worktrees」）。
final class WorktreeSlot {
    /// worktree の作業ツリーのパス（SSH 先なら SSH 先の上のパス）。
    let path: String
    let repository: Repository
    let actions: Actions
    let workingCopy: WorkingCopyViewController
    var watch: RepositoryWatch?

    init(path: String, repository: Repository, renderer: TextRenderer) {
        self.path = path
        self.repository = repository
        actions = Actions(repository: repository)
        workingCopy = WorkingCopyViewController(repository: repository, renderer: renderer)
        workingCopy.actions = actions
    }
}
