import CAmonhen
import Foundation

public enum StatusKind: UInt8, Sendable {
    case changed, renamed, unmerged, untracked
}

public struct StatusItem: Hashable, Sendable {
    /// `WorkingStatus` の中の番号（差分を取るときに使う）。
    public let index: Int
    public let kind: StatusKind
    public let path: String
    public let origPath: String?
    /// git の XY。変更なしは `.`、untracked は `?`。
    public let indexState: Character
    public let worktreeState: Character
    public let isSubmodule: Bool

    public var isStaged: Bool { indexState != "." && indexState != "?" }
    public var isUnstaged: Bool { worktreeState != "." }
}

/// 作業ツリーと index の状態。
public final class WorkingStatus: @unchecked Sendable {
    let raw: OpaquePointer
    let repository: Repository
    public let generation: UInt64
    public let branch: String?
    /// 上流ブランチ（`origin/main`）。
    public let upstream: String?
    public let ahead: Int
    public let behind: Int
    public let items: [StatusItem]

    init(raw: OpaquePointer, repository: Repository) {
        self.raw = raw
        self.repository = repository
        let v = amonhen_status_view(raw)
        func text(_ off: UInt32, _ len: UInt32) -> String? {
            guard len != UInt32.max else { return nil }
            return String(decoding: UnsafeBufferPointer(start: v.strings.advanced(by: Int(off)), count: Int(len)), as: UTF8.self)
        }
        generation = v.generation
        branch = text(v.branch_off, v.branch_len)
        upstream = text(v.upstream_off, v.upstream_len)
        ahead = Int(v.ahead)
        behind = Int(v.behind)
        items = UnsafeBufferPointer(start: v.entries, count: Int(v.count)).enumerated().map { i, e in
            StatusItem(
                index: i, kind: StatusKind(rawValue: e.kind) ?? .changed, path: text(e.path_off, e.path_len) ?? "",
                origPath: text(e.orig_path_off, e.orig_path_len),
                indexState: Character(UnicodeScalar(e.index)), worktreeState: Character(UnicodeScalar(e.worktree)),
                isSubmodule: e.submodule != 0)
        }
    }

    deinit {
        amonhen_status_release(raw)
    }

    /// `item` の差分。`staged` なら HEAD と index、そうでなければ index と作業ツリー。裏のスレッドで呼ぶ。
    public func diff(of item: StatusItem, staged: Bool) throws -> WorkingFileDiff {
        guard let raw = amonhen_working_diff(
            repository.raw, raw, UInt32(item.index), staged, UInt8(AmonhenEncoding_Utf16.rawValue))
        else { throw AmonhenError.last() }
        return WorkingFileDiff(raw: raw, status: self, item: item, staged: staged)
    }
}

/// Working Copy の 1 ファイルの差分。行を選んで stage・unstage できる。
public final class WorkingFileDiff: FileDiff, @unchecked Sendable {
    public let item: StatusItem
    public let staged: Bool
    let repository: Repository

    init(raw: OpaquePointer, status: WorkingStatus, item: StatusItem, staged: Bool) {
        self.item = item
        self.staged = staged
        self.repository = status.repository
        super.init(raw: raw, owner: status, view: amonhen_working_diff_view(raw), free: { amonhen_working_diff_free($0) })
    }

    /// `rows` 行目の変更だけを stage する（stage 済みの差分なら unstage する）。
    public func stage(rows: [Int]) throws {
        let ok = rows.map(UInt32.init).withUnsafeBufferPointer {
            amonhen_stage_rows(repository.raw, raw, $0.baseAddress, UInt32($0.count))
        }
        if !ok { throw AmonhenError.last() }
    }
}

extension Repository {
    public func loadStatus() throws -> WorkingStatus {
        guard let raw = amonhen_status_load(raw) else { throw AmonhenError.last() }
        return WorkingStatus(raw: raw, repository: self)
    }

    private func withPaths(_ paths: [String], _ body: (UnsafePointer<UnsafePointer<CChar>?>, UInt32) -> Bool) throws {
        let copies = paths.map { strdup($0) }
        defer { copies.forEach { free($0) } }
        let ok = copies.map { UnsafePointer($0) }.withUnsafeBufferPointer { body($0.baseAddress!, UInt32($0.count)) }
        if !ok { throw AmonhenError.last() }
    }

    public func stage(paths: [String]) throws {
        try withPaths(paths) { amonhen_stage(raw, $0, $1) }
    }

    public func unstage(paths: [String]) throws {
        try withPaths(paths) { amonhen_unstage(raw, $0, $1) }
    }

    public func commit(message: String, amend: Bool) throws {
        if !amonhen_commit(raw, message, amend) { throw AmonhenError.last() }
    }
}
