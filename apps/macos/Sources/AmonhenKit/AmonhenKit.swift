import CAmonhen
import CoreFoundation
import Foundation

/// Rust のコアの薄いラッパー。コアが返すメモリは、対応するオブジェクトが生きている間だけ有効。
/// 生ポインタや NoCopy の CFString は `with…` のクロージャの外に持ち出さないこと。

public struct AmonhenError: Error, CustomStringConvertible {
    public let description: String

    public init(description: String) {
        self.description = description
    }

    static func last() -> AmonhenError {
        AmonhenError(description: String(cString: amonhen_last_error()))
    }
}

public final class Repository: @unchecked Sendable {
    let raw: OpaquePointer

    /// `cacheDirectory` を渡すと、History をそこに永続化し、次に開いたときすぐ表示できる。
    public init(path: String, cacheDirectory: URL? = nil) throws {
        let raw = if let cacheDirectory {
            amonhen_repo_open(path, cacheDirectory.path)
        } else {
            amonhen_repo_open(path, nil)
        }
        guard let raw else { throw AmonhenError.last() }
        self.raw = raw
    }

    /// daemon 越しのリポジトリを開く。`location` は `daemon:/path` か `ssh://[user@]host[:port]/path`、
    /// `daemon` は手元の `amonhend` のパス（`daemon:` のときに起動する）。
    public init(location: String, daemon: String, cacheDirectory: URL? = nil) throws {
        guard let raw = amonhen_repo_open_location(location, daemon, cacheDirectory?.path) else {
            throw AmonhenError.last()
        }
        self.raw = raw
    }

    deinit {
        amonhen_repo_free(raw)
    }

    /// 作業ツリーのルート。bare リポジトリなら nil。
    public var workdir: String? {
        guard let s = amonhen_repo_workdir(raw) else { return nil }
        defer { amonhen_string_free(s) }
        return String(cString: s)
    }

    /// History を読み直して公開する。前回の結果があれば、変わった所だけ計算し直す。
    public func loadHistory() throws -> History {
        guard let raw = amonhen_history_load(raw) else { throw AmonhenError.last() }
        return History(raw: raw, repository: self)
    }

    /// 永続キャッシュにある前回の History。mmap するだけなので、メインスレッドで呼んでよい。
    public func loadCachedHistory() -> History? {
        amonhen_history_load_cached(raw).map { History(raw: $0, repository: self) }
    }

    /// commit-graph がなければ書き出し、書き出したら true を返す。リポジトリの `.git` に書き込むので、
    /// 設定で選ばれたときだけ、裏のスレッドから呼ぶこと。
    public func writeCommitGraphIfMissing() throws -> Bool {
        switch amonhen_repo_write_commit_graph_if_missing(raw) {
        case 1: return true
        case 0: return false
        default: throw AmonhenError.last()
        }
    }

    /// 最新のスナップショット。まだ読んでいなければ nil。
    public func latestHistory() -> History? {
        amonhen_history_acquire(raw).map { History(raw: $0, repository: self) }
    }
}

/// History の不変のスナップショット。どのスレッドから読んでもよい。
/// リポジトリの場所の書き方（手元のパス、`daemon:/path`、`ssh://host/path`）。
public enum RepositoryLocation {
    /// daemon 越しに開く場所か。
    public static func isRemote(_ location: String) -> Bool {
        location.hasPrefix("daemon:") || location.hasPrefix("ssh://")
    }

    /// リポジトリのパス（daemon 越しなら、その向こうのパス）。
    public static func path(_ location: String) -> String {
        if let rest = location.split(separator: ":", maxSplits: 1).last, location.hasPrefix("daemon:") {
            return String(rest)
        }
        if location.hasPrefix("ssh://") {
            let rest = location.dropFirst("ssh://".count)
            return rest.firstIndex(of: "/").map { String(rest[$0...]) } ?? ""
        }
        return location
    }

    /// 接続先（`ssh://` のホスト、`daemon:` は "daemon"）。手元なら nil。
    public static func host(_ location: String) -> String? {
        if location.hasPrefix("daemon:") { return "daemon" }
        guard location.hasPrefix("ssh://") else { return nil }
        let rest = location.dropFirst("ssh://".count)
        return String(rest.prefix { $0 != "/" })
    }
}

public final class History: @unchecked Sendable {
    let raw: OpaquePointer
    let repository: Repository
    let view: AmonhenHistoryView

    init(raw: OpaquePointer, repository: Repository) {
        self.raw = raw
        self.repository = repository
        self.view = amonhen_history_view(raw)
    }

    deinit {
        amonhen_history_release(raw)
    }

    public var generation: UInt64 { view.generation }
    public var rowCount: Int { Int(view.row_count) }
    public var laneCount: Int { Int(view.lane_count) }
    public var edgeCount: Int { Int(view.edge_count) }

    /// 行ごとの点（コピーしない）。
    public func withNodes<R>(_ body: (UnsafeBufferPointer<AmonhenNode>) throws -> R) rethrows -> R {
        try withExtendedLifetime(self) {
            try body(UnsafeBufferPointer(start: view.nodes, count: rowCount))
        }
    }

    /// すべての線（コピーしない）。
    public func withEdges<R>(_ body: (UnsafeBufferPointer<AmonhenEdge>) throws -> R) rethrows -> R {
        try withExtendedLifetime(self) {
            try body(UnsafeBufferPointer(start: view.edges, count: edgeCount))
        }
    }

    /// `rows` に重なる線の添字。
    public func edgeIndices(in rows: Range<Int>) -> [UInt32] {
        var indices = [UInt32](repeating: 0, count: 64)
        while true {
            let total = indices.withUnsafeMutableBufferPointer {
                amonhen_history_edges_in(raw, UInt32(rows.lowerBound), UInt32(rows.upperBound), $0.baseAddress, UInt32($0.count))
            }
            if Int(total) <= indices.count {
                return Array(indices.prefix(Int(total)))
            }
            indices = [UInt32](repeating: 0, count: Int(total))
        }
    }

    /// `rows` を表示するときの、レーンごとの中心の横位置（魚眼）を `positions` に書く（レーンの数に合わせる）。
    public func lanePositions(rows: Range<Int>, metrics: AmonhenLaneMetrics, into positions: inout [Float]) {
        if positions.count != laneCount { positions = [Float](repeating: 0, count: laneCount) }
        _ = positions.withUnsafeMutableBufferPointer {
            amonhen_history_lane_positions(
                raw, UInt32(rows.lowerBound), UInt32(rows.upperBound), metrics, $0.baseAddress, UInt32($0.count))
        }
    }

    /// `query` に合うコミットの行（昇順）。構文は `author:` `subject:` `commit:` `path:` `contents:` と
    /// AND（空白）・OR・括弧。すべてのコミットを読むので裏のスレッドで呼ぶ。取り消されたら投げる。
    public func search(_ query: String) throws -> [Int] {
        guard let rows = amonhen_history_search(repository.raw, raw, query) else { throw AmonhenError.last() }
        defer { amonhen_rows_free(rows) }
        let view = amonhen_rows_view(rows)
        return UnsafeBufferPointer(start: view.rows, count: Int(view.count)).map(Int.init)
    }

    /// `rows` の件名・作者・日時。
    public func chunk(rows: Range<Int>) throws -> HistoryChunk {
        let start = UInt32(clamping: rows.lowerBound)
        let count = UInt32(clamping: rows.count)
        guard let chunk = amonhen_history_chunk(repository.raw, raw, start, count, UInt8(AmonhenEncoding_Utf16.rawValue)) else {
            throw AmonhenError.last()
        }
        return HistoryChunk(raw: chunk, history: self)
    }
}

/// History の連続した行の列。文字列はコアのメモリを直接指す。
public final class HistoryChunk: @unchecked Sendable {
    let raw: OpaquePointer
    let history: History
    let view: AmonhenChunkView

    init(raw: OpaquePointer, history: History) {
        self.raw = raw
        self.history = history
        self.view = amonhen_chunk_view(raw)
    }

    deinit {
        amonhen_chunk_free(raw)
    }

    public var startRow: Int { Int(view.start_row) }
    public var count: Int { Int(view.row_count) }
    public var rows: Range<Int> { startRow..<startRow + count }

    public func authorTime(row: Int) -> Date {
        Date(timeIntervalSince1970: TimeInterval(view.author_times[row - startRow]))
    }

    /// 件名を、コアのメモリを指したままの CFString として渡す（コピーしない）。
    public func withSubject<R>(row: Int, _ body: (CFString) throws -> R) rethrows -> R {
        try withString(view.subjects[row - startRow], body)
    }

    public func withAuthorName<R>(row: Int, _ body: (CFString) throws -> R) rethrows -> R {
        try withString(view.author_names[row - startRow], body)
    }

    /// 件名のコピー。描画以外（テストやログ）に使う。
    public func subject(row: Int) -> String {
        withSubject(row: row) { $0 as String }
    }

    public func authorName(row: Int) -> String {
        withAuthorName(row: row) { $0 as String }
    }

    func withString<R>(_ s: AmonhenStrRef, _ body: (CFString) throws -> R) rethrows -> R {
        try withExtendedLifetime(self) { try withNoCopyString(s, bases: view.bases, body) }
    }
}
