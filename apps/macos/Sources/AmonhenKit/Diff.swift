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

    /// 一覧に出す名前。名前を変えた・コピーしたファイルは、git の `--stat` と同じく `src/{old.rs → new.rs}`。
    public var label: String {
        guard let oldPath, oldPath != path, let out = amonhen_rename_label(oldPath, path) else { return path }
        defer { amonhen_string_free(out) }
        return String(cString: out)
    }
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

    /// `index` 番目のファイルの、前後 `context` 行の差分（`UInt32.max` ならファイル全体）。裏のスレッドで呼ぶ。
    public func diff(at index: Int, context: UInt32, whitespace: DiffWhitespace = .exact) throws -> FileDiff {
        guard let raw = amonhen_file_diff_context(
            repository.raw, raw, UInt32(index), context, whitespace.rawValue, UInt8(AmonhenEncoding_Utf16.rawValue)) else {
            throw AmonhenError.last()
        }
        return FileDiff(raw: raw, changes: self)
    }

    /// `index` 番目のファイルの、変更前（`after` が false）か変更後の中身。その側にない（追加の前・削除の後）か、
    /// 読めなければ nil。裏のスレッドで呼ぶ。
    public func blob(at index: Int, after: Bool) -> Data? {
        var length: UInt = 0
        guard let bytes = amonhen_change_blob(repository.raw, raw, UInt32(index), after, &length) else { return nil }
        defer { amonhen_blob_free(bytes, length) }
        return Data(bytes: bytes, count: Int(length))
    }

    /// すべてのファイルの差分（全ファイルを縦に並べて見せる）。ファイルの数だけ blob を読むので、裏のスレッドで呼ぶ。
    public func stacked(whitespace: DiffWhitespace = .exact) throws -> StackedDiff {
        guard let raw = amonhen_stacked_diff(repository.raw, raw, whitespace.rawValue) else { throw AmonhenError.last() }
        return StackedDiff(raw: raw, owner: self)
    }
}

/// コミット（や選んだ複数のコミット）のすべてのファイルの差分。畳むファイルを変えるたびに `view(collapsed:)` で
/// 表示を作り直す。
public final class StackedDiff: @unchecked Sendable {
    let raw: OpaquePointer
    let owner: AnyObject?

    init(raw: OpaquePointer, owner: AnyObject?) {
        self.raw = raw
        self.owner = owner
    }

    deinit {
        amonhen_stacked_free(raw)
    }

    public var count: Int { Int(amonhen_stacked_count(raw)) }

    /// `file` 番目のファイルの差分を、前後 `context` 行で取り直す（`UInt32.max` ならファイル全体）。まだ差分を
    /// 取っていないファイル（ファイルの多いコミット）なら、ここで取る。表示は `view(collapsed:)` で作り直す。
    /// 表示を作っている間に呼ばないこと（裏のスレッドで呼ぶなら、終わるまで `view` を呼ばない）。
    public func setContext(_ file: Int, _ context: UInt32, in repository: Repository) throws {
        guard amonhen_stacked_set_context(repository.raw, raw, UInt32(file), context) else { throw AmonhenError.last() }
    }

    /// `file` 番目のファイルが生成物か（最初は畳む）。`.gitattributes` の `linguist-generated`、なければ lock ファイルなど。
    public func isGenerated(_ file: Int) -> Bool { amonhen_stacked_generated(raw, UInt32(file)) }

    /// 各ファイルの見出し（ふつうはパス。とびとびに選んだコミットでは「パス — 短い ID」）。
    public private(set) lazy var names: [String] = (0..<headers.rowCount).map { headers.text(row: $0) }

    /// 全ファイルの追加と削除の行数の合計（バイナリのファイルは数えない）。
    public private(set) lazy var totals: (added: Int, removed: Int) = headers.withRows { rows in
        rows.reduce((0, 0)) { ($0.0 + Int($1.new_no), $0.1 + Int($1.old_no)) }
    }

    /// すべて畳んだ表示（見出しの行だけ。`old_no` は削除、`new_no` は追加の行数）。
    private lazy var headers = view(collapsed: [Bool](repeating: true, count: count))

    /// 各ファイルの見出しの行（`AmonhenDiffRow_FILE`）に、畳まないファイルの差分の行を続けたもの。
    /// `collapsed[i]` が true のファイルは見出しだけ。
    public func view(collapsed: [Bool]) -> FileDiff {
        let raw = collapsed.withUnsafeBufferPointer {
            amonhen_stacked_view(self.raw, $0.baseAddress, UInt32($0.count), UInt8(AmonhenEncoding_Utf16.rawValue))!
        }
        return FileDiff(raw: raw, owner: self, view: amonhen_diff_view(raw), free: { amonhen_diff_free($0) })
    }
}

/// History で選んだ複数のコミットの差分の見せ方（`History.selectionDiff`）。
public enum SelectionKind: UInt8, Sendable {
    /// first-parent でつながる範囲（一番古いものの親から一番新しいものまで）。
    case range
    /// つながっていない 2 つ（古いほうから新しいほうまで）。
    case pair
    /// とびとびの 3 つ以上（コミットごとの差分を、パス順に）。
    case each
}

/// 2 つ選んだときの差分の取り方。
/// 行を比べるときに無視する空白（git diff の `--ignore-space-at-eol`・`-b`・`-w`）。
public enum DiffWhitespace: UInt8, Sendable, CaseIterable {
    /// 無視しない。
    case exact
    /// 行末の空白。
    case atEol
    /// 空白の量の違いと行末の空白（`-b`）。
    case change
    /// すべての空白（`-w`）。
    case all
}

public enum PairMode: UInt8, Sendable {
    /// 古いほうから新しいほうまで（A..B）。
    case forward
    /// 新しいほうから古いほうまで。
    case reverse
    /// 共通の祖先から新しいほうまで（A...B）。
    case mergeBase
}

extension History {
    /// 選んだ行（順不同）のコミットの、全ファイルの差分。裏のスレッドで呼ぶ。
    public func selectionDiff(
        rows: [Int], pair: PairMode = .forward, whitespace: DiffWhitespace = .exact
    ) throws -> (SelectionKind, StackedDiff) {
        var kind: UInt8 = 0
        let raw = rows.map(UInt32.init).withUnsafeBufferPointer {
            amonhen_selection_diff(
                repository.raw, self.raw, $0.baseAddress, UInt32($0.count), pair.rawValue, whitespace.rawValue, &kind)
        }
        guard let raw else { throw AmonhenError.last() }
        return (SelectionKind(rawValue: kind) ?? .range, StackedDiff(raw: raw, owner: self))
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

    /// hunk の見出しの行の数（変更のない所を開閉できる差分では、隙間の数）。
    public var hunkCount: Int {
        withRows { $0.reduce(0) { $0 + ($1.kind == UInt8(AmonhenDiffRow_HUNK) ? 1 : 0) } }
    }

    /// 変更のない所を開閉できる差分で、まだ隠している行のある隙間の数。
    public var foldedGapCount: Int {
        withRows { $0.reduce(0) { $0 + ($1.kind == UInt8(AmonhenDiffRow_HUNK) && $1.old_no > 0 ? 1 : 0) } }
    }

    /// 同じ中身の差分か（行の種類・印・行番号・文字列が同じ）。読み直したとき、出し直すかを決める。
    public func hasSameRows(as other: FileDiff) -> Bool {
        guard rowCount == other.rowCount, isBinary == other.isBinary else { return false }
        return withRows { a in
            other.withRows { b in
                (0..<rowCount).allSatisfy {
                    a[$0].kind == b[$0].kind && a[$0].flags == b[$0].flags && a[$0].old_no == b[$0].old_no
                        && a[$0].new_no == b[$0].new_no && text(row: $0) == other.text(row: $0)
                }
            }
        }
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

/// コミットの木の 1 段にあるもの（`Repository.treeEntries`）。
public struct TreeEntry: Decodable, Sendable {
    public let name: String
    /// `folder`・`file`・`executable`・`link`・`submodule`。
    public let kind: String
    /// blob・木の ID（40 桁）。
    public let oid: String
    public var isFolder: Bool { kind == "folder" }
}

extension Repository {
    /// コミット `commit`（40 桁）の木の `path`（根からの相対パス。空なら根）にあるもの。フォルダを先に、名前の順。
    public func treeEntries(commit: String, path: String) throws -> [TreeEntry] {
        struct Info: Decodable { let entries: [TreeEntry] }
        return try info(["op": "treeEntries", "oid": commit, "path": path], as: Info.self).entries
    }

    /// コミット `commit`（20 バイト）の `path` の中身を、変わっていない行として並べた表示。裏のスレッドで呼ぶ。
    public func treeFile(commit: Data, path: String) throws -> FileDiff {
        let raw = commit.withUnsafeBytes {
            amonhen_tree_file(self.raw, $0.baseAddress!.assumingMemoryBound(to: UInt8.self), path, UInt8(AmonhenEncoding_Utf16.rawValue))
        }
        guard let raw else { throw AmonhenError.last() }
        return FileDiff(raw: raw, owner: self, view: amonhen_diff_view(raw), free: { amonhen_diff_free($0) })
    }

    /// blob `oid`（40 桁）の中身。読めなければ nil。裏のスレッドで呼ぶ。
    public func blob(oid: String) -> Data? {
        guard let id = Data(hex: oid), id.count == 20 else { return nil }
        var length: UInt = 0
        let bytes = id.withUnsafeBytes {
            amonhen_blob_read(raw, $0.baseAddress!.assumingMemoryBound(to: UInt8.self), &length)
        }
        guard let bytes else { return nil }
        defer { amonhen_blob_free(bytes, length) }
        return Data(bytes: bytes, count: Int(length))
    }
}

extension Data {
    /// 16 進の文字列から（偶数桁、16 進でない文字があれば nil）。
    init?(hex: String) {
        var bytes: [UInt8] = []
        bytes.reserveCapacity(hex.utf8.count / 2)
        var high: UInt8?
        for c in hex.utf8 {
            let v: UInt8
            switch c {
            case 48...57: v = c - 48
            case 97...102: v = c - 87
            case 65...70: v = c - 55
            default: return nil
            }
            if let h = high { bytes.append(h << 4 | v); high = nil } else { high = v }
        }
        guard high == nil else { return nil }
        self.init(bytes)
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
    /// コミット `base` から `tip` への変更で変わったファイル（push で送る範囲 upstream..HEAD など）。`base` が nil なら
    /// 全ファイルの追加。差分は `CommitChanges.diff(at:)` で。裏のスレッドで呼ぶ。
    public func changes(from base: Data?, to tip: Data) throws -> CommitChanges {
        let raw: OpaquePointer? = tip.withUnsafeBytes { tipBytes in
            let tipPtr = tipBytes.baseAddress!.assumingMemoryBound(to: UInt8.self)
            guard let base else { return amonhen_range_changes(self.raw, nil, tipPtr) }
            return base.withUnsafeBytes {
                amonhen_range_changes(self.raw, $0.baseAddress!.assumingMemoryBound(to: UInt8.self), tipPtr)
            }
        }
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

/// Android の VectorDrawable を絵にしたもの（History の「すべてのファイル」）。
public struct VectorDrawablePicture: Sendable {
    /// 描ける SVG（大きさは dp）。
    public let svg: Data
    /// 絵の下に添える説明（引けなかった色・SVG では同じに描けないもの）。
    public let notes: [String]
}

/// 透けた絵の下の市松模様の既定（コアの `checker::Tone`）。`pixels` は `height` 行 × `stride` バイトの、不透明度を
/// 掛けた RGBA の画素（比べる 2 枚は並べて 1 つに）。暗くするのが既定か（`dark`）と、透けた画素があったか。
public func checkerTone(pixels: UnsafePointer<UInt8>?, width: Int, height: Int, stride: Int)
    -> (dark: Bool, transparent: Bool)
{
    var transparent = false
    let dark = amonhen_checker_dark(pixels, UInt(width), UInt(height), UInt(stride), false, &transparent)
    return (dark, transparent)
}

extension Repository {
    /// `path` の中身 `data` が、絵にする Android の VectorDrawable か（`.xml` で、根が `<vector>` か
    /// `<animated-vector>`。頭だけを見る）。
    public static func isVectorDrawable(path: String, data: Data) -> Bool {
        data.withUnsafeBytes { amonhen_is_vector_drawable(path, $0.bindMemory(to: UInt8.self).baseAddress, UInt($0.count)) }
    }

    /// コミット `commit`（20 バイト）の `path` の VectorDrawable（中身 `data`）を絵にする。色の参照は同じコミットの
    /// リソースから（`dark` なら暗い外観の `values-night` などを先に）引き、引けない色は `fallback`（画面の文字の色の
    /// `#rrggbb`。nil なら灰色）で描く。裏のスレッドで呼ぶ。
    public func vectorDrawable(commit: Data, path: String, data: Data, fallback: String? = nil, dark: Bool = false) throws
        -> VectorDrawablePicture
    {
        var length: UInt = 0
        var notes: UnsafeMutablePointer<CChar>?
        let raw = commit.withUnsafeBytes { id in
            data.withUnsafeBytes { bytes in
                amonhen_vector_drawable(
                    self.raw, id.baseAddress!.assumingMemoryBound(to: UInt8.self), path,
                    bytes.bindMemory(to: UInt8.self).baseAddress, UInt(bytes.count), fallback, dark, &length, &notes)
            }
        }
        guard let raw else { throw AmonhenError.last() }
        defer { amonhen_blob_free(raw, length) }
        var list: [String] = []
        if let notes {
            list = (try? JSONDecoder().decode([String].self, from: Data(String(cString: notes).utf8))) ?? []
            amonhen_string_free(notes)
        }
        return VectorDrawablePicture(svg: Data(bytes: raw, count: Int(length)), notes: list)
    }
}

/// SVG の、モバイル向けの描画部品で崩れる機能 1 つ。
public struct SVGIssue: Sendable, Decodable {
    /// 崩れる部品（`android`・`ios`・`flutter`・`reactNative`）。
    public let targets: [String]
    /// 機能（`<filter>`・`stroke-dasharray` など）。
    public let feature: String
    /// どう崩れるか（訳した文）。
    public let message: String
    /// 使っている数。
    public let count: Int

    /// `svg` の中の、Android VectorDrawable・iOS（CoreSVG）・flutter_svg・react-native-svg で崩れる機能。
    public static func lint(_ svg: Data) -> [SVGIssue] {
        let raw = svg.withUnsafeBytes { buffer in
            amonhen_svg_lint(buffer.bindMemory(to: UInt8.self).baseAddress, UInt(buffer.count))
        }
        guard let raw else { return [] }
        defer { amonhen_string_free(raw) }
        return (try? JSONDecoder().decode([SVGIssue].self, from: Data(String(cString: raw).utf8))) ?? []
    }
}

/// モバイル向けの SVG の描画部品。
public enum MobileRenderer: UInt8, Sendable, CaseIterable {
    case android, ios, flutter, reactNative

    /// 画面に出す名前。
    public var name: String {
        switch self {
        case .android: "Android VectorDrawable"
        case .ios: "iOS (CoreSVG)"
        case .flutter: "flutter_svg"
        case .reactNative: "react-native-svg"
        }
    }

    /// `svgIssue.targets` の名前。
    public var key: String {
        switch self {
        case .android: "android"
        case .ios: "ios"
        case .flutter: "flutter"
        case .reactNative: "reactNative"
        }
    }

    /// `svg` がこの部品でどう見えるか。Android と flutter_svg は本物の取り込みを移したもの、ほかは近似。
    public func preview(_ svg: Data) -> MobilePreview {
        var length: UInt = 0
        var info: UnsafeMutablePointer<CChar>?
        let raw = svg.withUnsafeBytes { buffer in
            amonhen_svg_preview(buffer.bindMemory(to: UInt8.self).baseAddress, UInt(buffer.count), rawValue, &length, &info)
        }
        var picture: Data?
        if let raw {
            picture = Data(bytes: raw, count: Int(length))
            amonhen_blob_free(raw, length)
        }
        var meta = MobilePreview.Info(title: name, caption: "", messages: [], error: nil, exact: false)
        if let info {
            if let decoded = try? JSONDecoder().decode(MobilePreview.Info.self, from: Data(String(cString: info).utf8)) {
                meta = decoded
            }
            amonhen_string_free(info)
        }
        return MobilePreview(svg: picture, info: meta)
    }
}

/// ある部品での SVG の見え方（コアの `svgmobile::Preview`）。
public struct MobilePreview: Sendable {
    struct Info: Decodable, Sendable {
        let title: String
        let caption: String
        let messages: [String]
        let error: String?
        let exact: Bool
    }

    /// 描ける SVG。部品が読めない SVG なら `nil`（理由は `error`）。
    public let svg: Data?
    let info: Info

    /// 絵の見出し。
    public var title: String { info.title }
    /// 絵の作り方の説明。
    public var caption: String { info.caption }
    /// 取り込みの警告と、同じに描けなかったこと。
    public var messages: [String] { info.messages }
    /// 部品が読めなかった理由。
    public var error: String? { info.error }
    /// 本物の取り込みを移したものか（`false` なら近似）。
    public var exact: Bool { info.exact }
}

/// Markdown のブロック（コアの `markdown::Block`）。
public indirect enum MarkdownBlock: Decodable, Sendable {
    /// 描いた数式（コアの `math::MathPicture`）。`svg` は色を CSS の変数のままにした絵（描くときに `colorizeMath` で
    /// 文字の色を当てる）。大きさは em（本文の字の大きさ × `Formula.fontScale` を 1 とする）。
    public struct Formula: Decodable, Sendable {
        public let svg: String
        public let width: Double
        /// 基線から上の高さと、下の深さ（文中の式は深さだけ基線より下に出す）。
        public let height: Double
        public let depth: Double

        /// 本文の字の大きさに対する、数式の 1 em の大きさの既定（コアの `math::FONT_SCALE`。設定で変えられる）。
        public static let fontScale: Double = 1.21
        /// 設定で選べる大きさ（コアの `math::FONT_SCALES`）と、1 つのファイルで式を描く時間の上限（秒。0 は上限なし。
        /// コアの `math::TIME_LIMITS`）。
        public static let fontScales: [Double] = [1.0, 1.1, fontScale, 1.3, 1.4, 1.5]
        public static let timeLimits: [Int] = [1, 2, 5, 10, 0]
        public static let defaultTimeLimit = 2

        /// 1 つの Markdown のファイルで式を描く時間の上限を `seconds` 秒にする（0 は上限なし）。次に読む Markdown から効く。
        public static func setTimeLimit(seconds: Int) {
            amonhen_math_set_time_limit(UInt64(max(0, seconds)))
        }
    }

    /// ひと続きの飾りの文字。
    public struct Run: Decodable, Sendable {
        public let text: String
        public let bold: Bool?
        public let italic: Bool?
        public let strike: Bool?
        public let code: Bool?
        /// キー（HTML の `<kbd>`）。小さなキーの札で見せる。
        public let kbd: Bool?
        /// 下付き・上付き（`<sub>`・`<sup>`）。
        public let sub: Bool?
        public let sup: Bool?
        /// 下線（`<ins>`）。
        public let underline: Bool?
        public let link: String?
        public let image: String?
        /// `<picture>` の、暗い外観・明るい外観で見せる画像（合うものがなければ `image`）。
        public let imageDark: String?
        public let imageLight: String?
        /// 画像の幅・高さ（HTML の `width`・`height`、pt）。片方だけなら、もう片方は比を保つ。
        public let width: Int?
        public let height: Int?
        /// 画像の幅の割合（HTML の `width="50%"`、1〜100）。表示する列の幅（表のマスの中ならマスの幅）に対して決める。
        public let widthPercent: Int?
        public let footnote: String?
        public let lineBreak: Bool?
        /// GitHub 独自の絵文字（画像だけのもの）の名前。`text`（`:octocat:`）を札で見せる。
        public let customEmoji: String?
        /// 文中の数式の絵（`text` は書いたままの原文、`code` が付いている）。描けなければ nil で、`mathError` に理由。
        public let math: Formula?
        public let mathError: String?
        enum CodingKeys: String, CodingKey {
            case text, bold, italic, strike, code, kbd, sub, sup, underline, link, image, width, height, footnote, math
            case imageDark = "image_dark"
            case imageLight = "image_light"
            case lineBreak = "line_break"
            case customEmoji = "custom_emoji"
            case widthPercent = "width_percent"
            case mathError = "math_error"
        }

        /// 外観 `dark` で見せる画像。
        public func image(dark: Bool) -> String? {
            (dark ? imageDark : imageLight) ?? image
        }
    }

    /// HTML の表のマス 1 つ（`colspan`・`rowspan` を解いた後の位置）。
    public struct TableCell: Decodable, Sendable {
        public let row: Int
        public let column: Int
        public let rowspan: Int
        public let colspan: Int
        public let header: Bool?
        public let align: String?
        public let runs: [Run]
    }

    public struct Item: Decodable, Sendable {
        public let task: Bool?
        public let blocks: [MarkdownBlock]
    }

    /// 見出し・段落。`align` は HTML の `align`（left・center・right。なければ nil）。
    case heading(level: Int, runs: [Run], align: String?)
    case paragraph(runs: [Run], align: String?)
    case code(language: String, text: String)
    /// 図（` ```mermaid ` など）。`svg` は色を CSS の変数のままにした絵（描くときに `colorizeDiagram` で配色を当てる）。
    /// 近いもので描いた図（DOT の HTML の表など）なら `note` にそのことを伝える一文。描けなければ `error` に理由。
    case diagram(language: String, text: String, svg: String?, config: MermaidThemeConfig?, note: String?, error: String?)
    /// 数式のブロック（```` ```math ````・`$$…$$`）。`text` は TeX。描けなければ `formula` は nil で、`error` に理由
    /// （どちらもなければ、数式を描かない作り）。
    case math(text: String, formula: Formula?, error: String?)
    case quote(alert: String?, blocks: [MarkdownBlock])
    case list(start: Int?, items: [Item])
    case table(aligns: [String], head: [[Run]], rows: [[[Run]]])
    case rule
    case footnote(label: String, blocks: [MarkdownBlock])
    /// HTML の `<details>`。`summary` が空なら「Details」、`open` は最初から開いているか。
    case details(summary: [Run], open: Bool, blocks: [MarkdownBlock])
    /// HTML の表。マスは行・列の順で、隙間は空のマスで埋めてある。
    case htmlTable(columns: Int, rows: Int, cells: [TableCell])

    enum Key: String, CodingKey {
        case kind, level, runs, language, text, alert, blocks, start, items, aligns, head, rows, label, svg, config, note, error
        case align, summary, open, columns, cells
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "heading":
            self = .heading(
                level: try c.decode(Int.self, forKey: .level), runs: try c.decode([Run].self, forKey: .runs),
                align: try c.decodeIfPresent(String.self, forKey: .align))
        case "paragraph":
            self = .paragraph(runs: try c.decode([Run].self, forKey: .runs), align: try c.decodeIfPresent(String.self, forKey: .align))
        case "code": self = .code(language: try c.decode(String.self, forKey: .language), text: try c.decode(String.self, forKey: .text))
        case "diagram":
            self = .diagram(
                language: try c.decode(String.self, forKey: .language), text: try c.decode(String.self, forKey: .text),
                svg: try c.decodeIfPresent(String.self, forKey: .svg),
                config: try c.decodeIfPresent(MermaidThemeConfig.self, forKey: .config),
                note: try c.decodeIfPresent(String.self, forKey: .note),
                error: try c.decodeIfPresent(String.self, forKey: .error))
        case "math":
            // 絵は同じ階層の svg・width・height・depth
            let formula = c.contains(.svg) ? try? Formula(from: decoder) : nil
            self = .math(
                text: try c.decode(String.self, forKey: .text), formula: formula,
                error: try c.decodeIfPresent(String.self, forKey: .error))
        case "quote":
            self = .quote(alert: try c.decodeIfPresent(String.self, forKey: .alert), blocks: try c.decode([MarkdownBlock].self, forKey: .blocks))
        case "list": self = .list(start: try c.decodeIfPresent(Int.self, forKey: .start), items: try c.decode([Item].self, forKey: .items))
        case "table":
            self = .table(
                aligns: try c.decode([String].self, forKey: .aligns), head: try c.decode([[Run]].self, forKey: .head),
                rows: try c.decode([[[Run]]].self, forKey: .rows))
        case "details":
            self = .details(
                summary: try c.decode([Run].self, forKey: .summary), open: try c.decodeIfPresent(Bool.self, forKey: .open) ?? false,
                blocks: try c.decode([MarkdownBlock].self, forKey: .blocks))
        case "htmlTable":
            self = .htmlTable(
                columns: try c.decode(Int.self, forKey: .columns), rows: try c.decode(Int.self, forKey: .rows),
                cells: try c.decode([TableCell].self, forKey: .cells))
        case "footnote": self = .footnote(label: try c.decode(String.self, forKey: .label), blocks: try c.decode([MarkdownBlock].self, forKey: .blocks))
        default: self = .rule
        }
    }

    /// Markdown として整形して見せるファイルか（拡張子で）。
    public static func isMarkdown(_ path: String) -> Bool {
        ["md", "markdown", "mdown", "mkd", "mkdn"].contains((path as NSString).pathExtension.lowercased())
    }

    /// `text` を読む。`gitHub`（`https://github.com/owner/repo`）があれば `#123`・`@user`・コミットの ID もリンクにする。
    /// 図（gantt）の日付は `timeZone` で数える（nil なら UTC）。DOT の図の画像は、`path`（Markdown のファイルの
    /// リポジトリの根からのパス）のフォルダからの相対パスを、`image`（リポジトリの根からのパスで、同じコミットの
    /// 中身を読む）で読む。図を描くスレッドから呼ばれる。
    public static func parse(
        _ text: String, gitHub: String? = nil, timeZone: TimeZone? = nil, path: String? = nil,
        image: (@Sendable (String) -> Data?)? = nil
    ) -> [MarkdownBlock] {
        var text = text
        let box = timeZone.map(ZoneBox.init)
        let reader = path == nil ? nil : image.map(ImageBox.init)
        let raw = withExtendedLifetime((box, reader)) {
            text.withUTF8 { buffer in
                amonhen_markdown_parse_with_images(
                    buffer.baseAddress, UInt(buffer.count), gitHub,
                    box.map { UnsafeRawPointer(Unmanaged.passUnretained($0).toOpaque()) },
                    box == nil ? nil : { context, utc in
                        // pele の描画のスレッドから呼ばれる（TimeZone はどのスレッドから使ってもよい）。
                        guard let context, utc.isFinite else { return 0 }
                        let zone = Unmanaged<ZoneBox>.fromOpaque(context).takeUnretainedValue().zone
                        return Double(zone.secondsFromGMT(for: Date(timeIntervalSince1970: utc / 1000))) * 1000
                    },
                    reader == nil ? nil : path,
                    reader.map { UnsafeRawPointer(Unmanaged.passUnretained($0).toOpaque()) },
                    reader == nil ? nil : { context, path, sink, sinkContext in
                        // DOT の図を描くスレッドから呼ばれる。読めたときだけ sink に渡す。
                        guard let context, let path, let sink else { return }
                        let read = Unmanaged<ImageBox>.fromOpaque(context).takeUnretainedValue().read
                        guard let data = read(String(cString: path)) else { return }
                        data.withUnsafeBytes { bytes in
                            sink(sinkContext, bytes.bindMemory(to: UInt8.self).baseAddress, UInt(bytes.count))
                        }
                    })
            }
        }
        guard let raw else { return [] }
        defer { amonhen_string_free(raw) }
        return (try? JSONDecoder().decode([MarkdownBlock].self, from: Data(String(cString: raw).utf8))) ?? []
    }
}

/// FFI の画像の読み手に渡す箱。
private final class ImageBox: @unchecked Sendable {
    let read: @Sendable (String) -> Data?
    init(_ read: @escaping @Sendable (String) -> Data?) { self.read = read }
}

/// FFI の時間帯の呼び出しに渡す箱。
private final class ZoneBox: @unchecked Sendable {
    let zone: TimeZone
    init(_ zone: TimeZone) { self.zone = zone }
}

/// Mermaid のテーマの指定（mermaid-cli の設定ファイルと同じ中身。図の中の指定と、読み込んだテーマ）。
public struct MermaidThemeConfig: Codable, Hashable, Sendable {
    public var theme: String?
    public var themeVariables: [String: String]?

    /// JSON の文字（`amonhen_diagram_colorize` に渡す形）。
    public var json: String {
        (try? JSONEncoder().encode(self)).flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
    }

    /// mermaid-cli の設定ファイル（`{"theme": …, "themeVariables": {…}}`、または変数だけのオブジェクト）を読む。
    public static func parse(_ text: String) throws -> MermaidThemeConfig {
        guard let raw = amonhen_mermaid_theme_parse(text) else { throw AmonhenError.last() }
        defer { amonhen_string_free(raw) }
        return try JSONDecoder().decode(MermaidThemeConfig.self, from: Data(String(cString: raw).utf8))
    }
}

extension MarkdownBlock {
    /// 設定の見本の図の SVG（色は `colorizeDiagram` で当てる）。
    public static let sampleDiagram: String? = {
        guard let raw = amonhen_diagram_sample() else { return nil }
        defer { amonhen_string_free(raw) }
        return String(cString: raw)
    }()

    /// 数式の SVG（`math` の絵の `svg`）に文字の色 `foreground`（`#rrggbb`）を当て、CoreSVG で描ける SVG にする。
    public static func colorizeMath(_ svg: String, foreground: String) -> Data? {
        guard let raw = amonhen_math_colorize(svg, foreground) else { return nil }
        defer { amonhen_string_free(raw) }
        return Data(String(cString: raw).utf8)
    }

    /// 図の SVG（`diagram` の `svg`）に配色を当て、CoreSVG で描ける SVG にする。`scheme` は設定の「図の配色」の鍵、
    /// `theme` は構文の色のテーマ（`text`・`background` と種類の名前 → `#rrggbb`）、`own` は図の中で指定した
    /// テーマ（設定より先）、`imported` は `scheme` が読み込んだテーマのときの中身。
    public static func colorizeDiagram(
        _ svg: String, scheme: String, dark: Bool, theme: [String: Any], own: MermaidThemeConfig? = nil,
        imported: MermaidThemeConfig? = nil
    ) -> Data? {
        let json = (try? JSONSerialization.data(withJSONObject: theme)).flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
        guard let raw = amonhen_diagram_colorize(svg, scheme, dark, json, own?.json, imported?.json) else { return nil }
        defer { amonhen_string_free(raw) }
        return Data(String(cString: raw).utf8)
    }
}
