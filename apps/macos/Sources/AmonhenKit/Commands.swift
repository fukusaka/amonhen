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
            outcome: (response["outcome"] as? String).flatMap(Outcome.init), label: response["label"] as? String)
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
