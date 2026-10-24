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

extension Repository {
    /// マージ済みブランチを折りたたんだ History。`tips` が nil なら全ブランチ。`expanded`（20 バイトの ID）の
    /// merge は取り込んだ側も見せる。裏のスレッドで呼ぶ。
    public func foldedHistory(tips: [Data]?, expanded: [Data]) throws -> History {
        let tipBytes = (tips ?? []).reduce(Data(), +)
        let expandedBytes = expanded.reduce(Data(), +)
        let raw = tipBytes.withUnsafeBytes { t in
            expandedBytes.withUnsafeBytes { e in
                amonhen_history_folded(
                    self.raw, tips == nil, t.baseAddress?.assumingMemoryBound(to: UInt8.self), UInt32(tips?.count ?? 0),
                    e.baseAddress?.assumingMemoryBound(to: UInt8.self), UInt32(expanded.count))
            }
        }
        guard let raw else { throw AmonhenError.last() }
        return History(raw: raw, repository: self)
    }
}

extension Repository {
    /// `history` の `rows` の行（検索で一致したコミット）だけの History。裏のスレッドで呼ぶ。
    public func matchingHistory(of history: History, rows: [Int]) -> History {
        let raw = rows.map(UInt32.init).withUnsafeBufferPointer {
            amonhen_history_matching(self.raw, history.raw, $0.baseAddress, UInt32($0.count))
        }
        return History(raw: raw!, repository: self)
    }
}

extension History {
    /// 折りたたんだ History で、`row` の merge が隠しているコミットの数（隠していなければ 0）。
    public func hiddenCount(row: Int) -> Int { Int(amonhen_history_hidden(raw, UInt32(row))) }

    /// スクロールバーに描く密度。区間ごとの（頻度, 活動量）を 0〜1 で。
    public func density(buckets: Int) -> [(frequency: Float, activity: Float)] {
        var out = [Float](repeating: 0, count: buckets * 2)
        let n = out.withUnsafeMutableBufferPointer { amonhen_history_density(raw, UInt32(buckets), $0.baseAddress) }
        return (0..<Int(n)).map { (out[$0 * 2], out[$0 * 2 + 1]) }
    }

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

/// ローカルブランチの分け方（`{"op":"branchGroups"}`。`crates/core/src/branches.rs`）。
public struct BranchGroups: Decodable, Equatable, Sendable {
    public struct Branch: Decodable, Equatable, Sendable {
        public let name: String
        /// 先端のコミットの日時（UNIX 時間）。
        public let time: Int64?
        /// 既定のブランチに取り込み済み。
        public let merged: Bool
        /// 既定のブランチの先端と同じ（作ったばかり）。
        public let atBase: Bool
        /// 取り込んでいない自分のコミットがある。
        public let mine: Bool
        /// その上に積んであるブランチ（土台）。既定のブランチの上なら nil。
        public let parent: String?
        /// 手でアーカイブした（`branch.<name>.amonhenArchived`）。
        public let archived: Bool
        /// お気に入りの順位（`branch.<name>.amonhenPinned`。小さいほど上）。お気に入りでなければ nil。
        public let pinned: Int64?
    }

    /// 既定のブランチ（`origin/main`・`main` など）。
    public let base: String?
    public let branches: [Branch]
}

/// 手元のタグごとの、リモートでの様子（`{"op":"tagRemotes"}`。`crates/core/src/remote_tags.rs`）。fetch のときに
/// 読んで覚えたもので、手元の ref からは分からない。
public struct TagRemotes: Decodable, Equatable, Sendable {
    public struct Remote: Decodable, Equatable, Sendable {
        public let name: String
        /// タグを読んだ時刻（UNIX 時刻のミリ秒）。まだ読んでいなければ nil（そのリモートのことは分からない）。
        public let checked: Int64?
    }

    public enum State: String, Decodable, Sendable {
        /// 同じタグ（ref が同じオブジェクトを指す）。
        case same
        /// 同じ名前の別のタグ（違うコミットを指すか、同じコミットを指す別の注釈付きのタグ）。置き換えないと push できない。
        case different
        /// そのリモートにはない。
        case missing
    }

    /// すべてのリモート。
    public let remotes: [Remote]
    /// タグ → 読んだリモート → 様子（読んでいないリモートは入らない）。
    public let tags: [String: [String: State]]

    /// `tag` の `remote` での様子。まだ読んでいないリモートなら nil。
    public func state(of tag: String, on remote: String) -> State? {
        tags[tag]?[remote]
    }

    public func checked(_ remote: String) -> Int64? {
        remotes.first { $0.name == remote }?.checked
    }
}
