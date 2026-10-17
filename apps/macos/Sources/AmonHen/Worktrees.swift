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

    /// 見せる名前（フォルダの名前）。
    var name: String { (path as NSString).lastPathComponent }

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
        let resolved = URL(fileURLWithPath: path).resolvingSymlinksInPath().standardizedFileURL.path
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
