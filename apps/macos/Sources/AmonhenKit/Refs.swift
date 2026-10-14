import CAmonhen
import Foundation

public enum RefKind: UInt8, Sendable {
    case local, remote, tag, stash, submodule
}

public struct RefItem: Hashable, Sendable {
    public let kind: RefKind
    /// 表示名（`main`、`origin/main`、`v1.0`、`stash@{0}`、サブモジュールのパス）。
    public let name: String
    /// 指すコミット（20 バイト）。コミット以外を指すタグなどは nil。
    public let target: Data?
    /// ローカルブランチの上流、stash のメッセージ。
    public let detail: String?
    /// ローカルブランチの、上流より先・後ろにあるコミットの数。
    public var ahead = 0
    public var behind = 0
    /// 上流のリモートブランチがもうない。
    public var gone = false
}

/// ブランチ・リモートブランチ・タグ・stash・サブモジュールの一覧。数百〜数千件なので文字列はコピーする。
public struct Refs: Sendable {
    public let generation: UInt64
    public let headBranch: String?
    /// HEAD のコミット（20 バイト）。
    public var head: Data?
    public let items: [RefItem]
}

extension Repository {
    /// ref の一覧を読み直す。前回と同じなら同じ世代を返す。
    public func loadRefs() throws -> Refs {
        guard let raw = amonhen_refs_load(raw) else { throw AmonhenError.last() }
        defer { amonhen_refs_release(raw) }
        let v = amonhen_refs_view(raw)
        func text(_ off: UInt32, _ len: UInt32) -> String? {
            guard len != UInt32.max else { return nil }
            return String(decoding: UnsafeBufferPointer(start: v.strings.advanced(by: Int(off)), count: Int(len)), as: UTF8.self)
        }
        let items = UnsafeBufferPointer(start: v.entries, count: Int(v.count)).map { e in
            var target = e.target
            return RefItem(
                kind: RefKind(rawValue: e.kind) ?? .local,
                name: text(e.name_off, e.name_len) ?? "",
                target: e.has_target != 0 ? withUnsafeBytes(of: &target) { Data($0) } : nil,
                detail: text(e.detail_off, e.detail_len),
                ahead: Int(e.ahead), behind: Int(e.behind), gone: e.gone != 0)
        }
        var head = v.head
        return Refs(
            generation: v.generation, headBranch: text(v.head_branch_off, v.head_branch_len),
            head: v.has_head != 0 ? withUnsafeBytes(of: &head) { Data($0) } : nil, items: items)
    }
}

extension Repository {
    /// `tips`（20 バイトの ID）から辿れるコミットだけの History。裏のスレッドで呼ぶ。
    public func filteredHistory(tips: [Data]) throws -> History {
        let bytes = tips.reduce(Data(), +)
        let raw = bytes.withUnsafeBytes {
            amonhen_history_filtered(self.raw, $0.baseAddress?.assumingMemoryBound(to: UInt8.self), UInt32(tips.count))
        }
        guard let raw else { throw AmonhenError.last() }
        return History(raw: raw, repository: self)
    }
}

extension History {
    /// `commit`（20 バイト）の行。
    public func row(of commit: Data) -> Int? {
        let row = commit.withUnsafeBytes { amonhen_history_row_of(raw, $0.baseAddress!.assumingMemoryBound(to: UInt8.self)) }
        return row >= 0 ? Int(row) : nil
    }
}

/// 変化の種類。
public struct Changes: OptionSet, Sendable {
    public let rawValue: UInt32
    public init(rawValue: UInt32) { self.rawValue = rawValue }

    public static let refs = Changes(rawValue: UInt32(AMONHEN_CHANGE_REFS))
    public static let index = Changes(rawValue: UInt32(AMONHEN_CHANGE_INDEX))
    public static let worktree = Changes(rawValue: UInt32(AMONHEN_CHANGE_WORKTREE))
    public static let config = Changes(rawValue: UInt32(AMONHEN_CHANGE_CONFIG))
}

/// リポジトリの監視。落とすと止まる。`onChange` は監視用のスレッドから呼ばれる。
public final class RepositoryWatch: @unchecked Sendable {
    private final class Box {
        let onChange: @Sendable (Changes) -> Void
        init(_ onChange: @escaping @Sendable (Changes) -> Void) { self.onChange = onChange }
    }

    private let raw: OpaquePointer
    private let box: Unmanaged<Box>

    public init(repository: Repository, onChange: @escaping @Sendable (Changes) -> Void) throws {
        let box = Unmanaged.passRetained(Box(onChange))
        let callback: @convention(c) (UnsafeMutableRawPointer?, UInt32) -> Void = { context, changes in
            Unmanaged<Box>.fromOpaque(context!).takeUnretainedValue().onChange(Changes(rawValue: changes))
        }
        guard let raw = amonhen_repo_watch(repository.raw, callback, box.toOpaque()) else {
            box.release()
            throw AmonhenError.last()
        }
        self.raw = raw
        self.box = box
    }

    deinit {
        // 監視を止めてから（コールバックが呼ばれなくなってから）文脈を解放する。
        amonhen_watch_free(raw)
        box.release()
    }
}
