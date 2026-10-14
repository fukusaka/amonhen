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

    deinit {
        amonhen_repo_free(raw)
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

    /// 最新のスナップショット。まだ読んでいなければ nil。
    public func latestHistory() -> History? {
        amonhen_history_acquire(raw).map { History(raw: $0, repository: self) }
    }
}

/// History の不変のスナップショット。どのスレッドから読んでもよい。
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
        let length = Int(s.len_and_kind & 0x7fff_ffff)
        let isASCII = s.len_and_kind & 0x8000_0000 == 0
        let start = view.bases[Int(s.buf)]!.advanced(by: Int(s.off))
        let string: CFString = if isASCII {
            CFStringCreateWithBytesNoCopy(
                nil, start, length, CFStringBuiltInEncodings.ASCII.rawValue, false, kCFAllocatorNull)
        } else {
            UnsafeRawPointer(start).withMemoryRebound(to: UniChar.self, capacity: length) {
                CFStringCreateWithCharactersNoCopy(nil, $0, length, kCFAllocatorNull)
            }
        }
        return try withExtendedLifetime(self) { try body(string) }
    }
}
