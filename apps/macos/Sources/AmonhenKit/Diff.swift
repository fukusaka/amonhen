import CAmonhen
import CoreFoundation
import Foundation

public enum ChangeKind: UInt8, Sendable {
    case added, deleted, modified, renamed, copied
}

public struct FileChangeItem: Hashable, Sendable {
    public let kind: ChangeKind
    public let path: String
    public let oldPath: String?
    public let isSubmodule: Bool
}

/// コミットで変わったファイルの一覧。差分はファイルごとに `diff(at:)` で取る。
public final class CommitChanges: @unchecked Sendable {
    let raw: OpaquePointer
    let repository: Repository
    public let items: [FileChangeItem]

    init(raw: OpaquePointer, repository: Repository) {
        self.raw = raw
        self.repository = repository
        let v = amonhen_changes_view(raw)
        func text(_ off: UInt32, _ len: UInt32) -> String? {
            guard len != UInt32.max else { return nil }
            return String(decoding: UnsafeBufferPointer(start: v.strings.advanced(by: Int(off)), count: Int(len)), as: UTF8.self)
        }
        items = UnsafeBufferPointer(start: v.entries, count: Int(v.count)).map {
            FileChangeItem(
                kind: ChangeKind(rawValue: $0.kind) ?? .modified, path: text($0.path_off, $0.path_len) ?? "",
                oldPath: text($0.old_path_off, $0.old_path_len), isSubmodule: $0.submodule != 0)
        }
    }

    deinit {
        amonhen_changes_free(raw)
    }

    /// `index` 番目のファイルの差分。大きなファイルでは時間がかかるので、裏のスレッドで呼ぶ。
    public func diff(at index: Int) throws -> FileDiff {
        guard let raw = amonhen_file_diff(repository.raw, raw, UInt32(index), UInt8(AmonhenEncoding_Utf16.rawValue)) else {
            throw AmonhenError.last()
        }
        return FileDiff(raw: raw, changes: self)
    }
}

/// ファイル 1 つの差分。行の文字列はコアのメモリ（blob）を直接指す。
public class FileDiff: @unchecked Sendable {
    let raw: OpaquePointer
    /// コアのメモリを持つ側（解放されないように保持する）。
    let owner: AnyObject
    let view: AmonhenDiffView
    private let free: (OpaquePointer) -> Void
    private let makeSyntax: (OpaquePointer) -> OpaquePointer?

    init(
        raw: OpaquePointer, owner: AnyObject, view: AmonhenDiffView, free: @escaping (OpaquePointer) -> Void,
        syntax: @escaping (OpaquePointer) -> OpaquePointer? = { amonhen_diff_syntax($0) }
    ) {
        self.raw = raw
        self.owner = owner
        self.view = view
        self.free = free
        self.makeSyntax = syntax
    }

    convenience init(raw: OpaquePointer, changes: CommitChanges) {
        self.init(raw: raw, owner: changes, view: amonhen_diff_view(raw), free: { amonhen_diff_free($0) })
    }

    deinit {
        free(raw)
    }

    public var rowCount: Int { Int(view.row_count) }
    public var isBinary: Bool { view.binary != 0 }
    public var added: Int { Int(view.added) }
    public var removed: Int { Int(view.removed) }

    /// 行（コピーしない）。
    public func withRows<R>(_ body: (UnsafeBufferPointer<AmonhenDiffRow>) throws -> R) rethrows -> R {
        try withExtendedLifetime(self) { try body(UnsafeBufferPointer(start: view.rows, count: rowCount)) }
    }

    /// 変わった単語の範囲（行の順、位置は UTF-16 の符号単位）。
    public func withHighlights<R>(_ body: (UnsafeBufferPointer<AmonhenHighlight>) throws -> R) rethrows -> R {
        try withExtendedLifetime(self) {
            try body(UnsafeBufferPointer(start: view.highlights, count: Int(view.highlight_count)))
        }
    }

    /// `row` 行目の文字列を、コアのメモリを指したままの CFString として渡す（コピーしない）。
    public func withText<R>(row: Int, _ body: (CFString) throws -> R) rethrows -> R {
        try withExtendedLifetime(self) {
            try withNoCopyString(view.rows[row].text, bases: view.bases, body)
        }
    }

    public func text(row: Int) -> String {
        withText(row: row) { $0 as String }
    }

    /// 構文の色付け。ファイル全体を解析するので、裏のスレッドで呼ぶ。
    public func syntax() -> SyntaxSpans? {
        withExtendedLifetime(self) { makeSyntax(raw).map(SyntaxSpans.init) }
    }
}

/// 構文の色付け（差分なら行の順、位置は UTF-16 の符号単位）。
public final class SyntaxSpans: @unchecked Sendable {
    let raw: OpaquePointer
    let view: AmonhenSyntaxView

    init(raw: OpaquePointer) {
        self.raw = raw
        self.view = amonhen_syntax_view(raw)
    }

    deinit {
        amonhen_syntax_free(raw)
    }

    /// テキスト全体の構文の色付け（位置はテキストの先頭からの UTF-16 の符号単位）。言語は `path` の拡張子で
    /// 決める。ファイル全体を解析するので、裏のスレッドで呼ぶ。
    public static func highlight(path: String, text: String) -> SyntaxSpans? {
        var text = text
        return text.withUTF8 { buffer in
            amonhen_text_syntax(path, buffer.baseAddress, UInt(buffer.count)).map(SyntaxSpans.init)
        }
    }

    public var count: Int { Int(view.span_count) }

    public func withSpans<R>(_ body: (UnsafeBufferPointer<AmonhenSyntaxSpan>) throws -> R) rethrows -> R {
        try withExtendedLifetime(self) { try body(UnsafeBufferPointer(start: view.spans, count: count)) }
    }
}

extension Repository {
    /// `commit`（20 バイト）で変わったファイル。
    public func changes(of commit: Data) throws -> CommitChanges {
        let raw = commit.withUnsafeBytes { amonhen_commit_changes(self.raw, $0.baseAddress!.assumingMemoryBound(to: UInt8.self)) }
        guard let raw else { throw AmonhenError.last() }
        return CommitChanges(raw: raw, repository: self)
    }
}

extension Repository {
    /// stash（20 バイト）の中身。untracked のファイルを含めていればそれも。
    public func stashChanges(of stash: Data) throws -> CommitChanges {
        let raw = stash.withUnsafeBytes { amonhen_stash_changes(self.raw, $0.baseAddress!.assumingMemoryBound(to: UInt8.self)) }
        guard let raw else { throw AmonhenError.last() }
        return CommitChanges(raw: raw, repository: self)
    }
}

extension History {
    /// `row` 行目のコミット ID（20 バイト）。
    public func commitID(row: Int) -> Data? {
        var id = Data(count: 20)
        let ok = id.withUnsafeMutableBytes {
            amonhen_history_commit_id(raw, UInt32(row), $0.baseAddress!.assumingMemoryBound(to: UInt8.self))
        }
        return ok ? id : nil
    }
}

/// コアの文字列置き場を指したままの CFString を作る（コピーしない）。`body` の外に持ち出さないこと。
func withNoCopyString<R>(
    _ s: AmonhenStrRef, bases: UnsafePointer<UnsafePointer<UInt8>?>, _ body: (CFString) throws -> R
) rethrows -> R {
    let length = Int(s.len_and_kind & 0x7fff_ffff)
    let isASCII = s.len_and_kind & 0x8000_0000 == 0
    let start = bases[Int(s.buf)]!.advanced(by: Int(s.off))
    let string: CFString = if isASCII {
        CFStringCreateWithBytesNoCopy(nil, start, length, CFStringBuiltInEncodings.ASCII.rawValue, false, kCFAllocatorNull)
    } else {
        UnsafeRawPointer(start).withMemoryRebound(to: UniChar.self, capacity: length) {
            CFStringCreateWithCharactersNoCopy(nil, $0, length, kCFAllocatorNull)
        }
    }
    return try body(string)
}

extension Repository {
    /// コンフリクトのある作業ツリーのファイル。行の `kind` は Current・Incoming・Base・マーカーで、
    /// コンフリクトの行の `old_no` はその番号 + 1。裏のスレッドで呼ぶ。
    public func conflictView(path: String) throws -> FileDiff {
        guard let raw = amonhen_conflict_view(self.raw, path, UInt8(AmonhenEncoding_Utf16.rawValue)) else {
            throw AmonhenError.last()
        }
        return FileDiff(raw: raw, owner: self, view: amonhen_diff_view(raw), free: { amonhen_diff_free($0) })
    }
}
