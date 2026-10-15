import CAmonhen
import Foundation

/// 途中で止まりうる操作の結果。
public enum Outcome: String, Sendable {
    case done, conflict, stopped
}

public struct CommandResult: Sendable {
    public let outcome: Outcome?
    /// Undo した・次に Undo される操作の名前。
    public let label: String?
    /// コミットのメッセージなど、命令ごとの結果。
    public let text: String?
}

extension Repository {
    /// 頻度の低い操作（merge・rebase・ブランチ・stash・Undo など）を実行する。裏のスレッドで呼ぶ。
    /// 命令の形は `crates/core/src/command.rs` の `Command`。
    @discardableResult
    public func run(_ command: [String: Any]) throws -> CommandResult {
        try run(json: Self.json(command))
    }

    /// 命令を JSON の文字列にする（スレッドをまたいで渡すため）。
    public static func json(_ command: [String: Any]) throws -> String {
        String(decoding: try JSONSerialization.data(withJSONObject: command), as: UTF8.self)
    }

    @discardableResult
    public func run(json: String) throws -> CommandResult {
        guard let out = amonhen_command(raw, json) else { throw AmonhenError(description: "結果がない") }
        defer { amonhen_string_free(out) }
        let response = try JSONSerialization.jsonObject(with: Data(String(cString: out).utf8)) as? [String: Any] ?? [:]
        guard response["ok"] as? Bool == true else {
            throw AmonhenError(description: response["error"] as? String ?? "失敗した")
        }
        return CommandResult(
            outcome: (response["outcome"] as? String).flatMap(Outcome.init), label: response["label"] as? String,
            text: response["text"] as? String)
    }
}

extension WorkingFileDiff {
    /// `rows` 行目の変更を作業ツリーから取り除く（Undo できる）。stage していない差分にだけ使える。
    public func discard(rows: [Int]) throws {
        let ok = rows.map(UInt32.init).withUnsafeBufferPointer {
            amonhen_discard_rows(repository.raw, raw, $0.baseAddress, UInt32($0.count))
        }
        if !ok { throw AmonhenError.last() }
    }
}

/// コミットの詳細。
public struct CommitInfo: Decodable, Sendable {
    public let oid: String
    public let message: String
    public let author: String
    public let authorEmail: String
    public let authorTime: Int64
    public let committer: String
    public let committerTime: Int64
    public let parents: [String]
    public let refs: [String]
}

extension Repository {
    /// 走っている History の検索をやめる。
    public func cancelSearch() {
        amonhen_search_cancel(raw)
    }

    /// 命令を実行し、結果の `info` を `T` として読む。裏のスレッドで呼ぶ。
    public func info<T: Decodable>(_ command: [String: Any], as type: T.Type = T.self) throws -> T {
        try info(json: try Repository.json(command), as: type)
    }

    /// `info(_:as:)` の、命令を JSON の文字列で渡す版（スレッドをまたいで渡すため）。
    public func info<T: Decodable>(json: String, as type: T.Type = T.self) throws -> T {
        guard let out = amonhen_command(raw, json) else { throw AmonhenError(description: "結果がない") }
        defer { amonhen_string_free(out) }
        let response = try JSONDecoder().decode(InfoResponse<T>.self, from: Data(String(cString: out).utf8))
        guard response.ok, let info = response.info else {
            throw AmonhenError(description: response.error ?? "失敗した")
        }
        return info
    }

    /// コミットの詳細。裏のスレッドで呼ぶ。
    public func commitInfo(oid: String) throws -> CommitInfo {
        try info(["op": "commitInfo", "oid": oid])
    }
}

private struct InfoResponse<T: Decodable>: Decodable {
    let ok: Bool
    let info: T?
    let error: String?
}

/// マージエディタの中身（`mergeSides`）。
public struct MergeSides: Decodable, Sendable {
    /// すべてのコンフリクトをその側にした内容と、各コンフリクトの行範囲（`[開始, 終わり)`、0 始まり）。
    public struct Side: Decodable, Sendable {
        public let text: String
        public let regions: [[Int]]
    }

    public let current: Side?
    public let incoming: Side?
    /// diff3 のときだけ。
    public let base: Side?
    /// 今の作業ツリーの内容（マーカーつき）。
    public let result: String
}

/// 結果の文章に残っているコンフリクト（`conflictRegions`）。
public struct ConflictRegions: Decodable, Sendable {
    public struct Conflict: Decodable, Sendable {
        /// マーカーを含む行範囲。
        public let lines: [Int]
        public let current: String
        public let incoming: String
    }

    public let conflicts: [Conflict]
}

/// 各行を最後に変えたコミット（`blame`）。
public struct Blame: Decodable, Sendable {
    public struct Commit: Decodable, Sendable {
        /// 40 桁の ID。まだコミットしていない行は 0 が並ぶ。
        public let oid: String
        public let author: String
        public let time: Int64
        public let summary: String

        public var isCommitted: Bool { oid.contains { $0 != "0" } }
    }

    public struct Line: Decodable, Sendable {
        /// `commits` の添字。
        public let c: Int
        public let t: String
    }

    public let commits: [Commit]
    public let lines: [Line]
}

/// ファイルを変えたコミット（`fileHistory`、新しい順）。
public struct FileHistory: Decodable, Sendable {
    public struct Entry: Decodable, Sendable {
        public let oid: String
        public let subject: String
        public let author: String
        public let time: Int64
        /// そのコミットの時点のパス。
        public let path: String
    }

    public let entries: [Entry]
}
