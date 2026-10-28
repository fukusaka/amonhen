import CAmonhen
import Foundation

/// 同じ一連のコミットの 2 つの版の比べ合わせ（`git range-diff`）の、組 1 つ。
public struct RangePair: Decodable, Equatable, Sendable {
    public enum Status: String, Decodable, Sendable {
        /// 中身が同じ。
        case same
        /// 対応するが、中身（メッセージ・差分）が変わった。
        case changed
        /// 古い版にだけある。
        case removed
        /// 新しい版にだけある。
        case added
    }

    public struct Commit: Decodable, Equatable, Sendable {
        /// 範囲の中の番号（1 始まり。古い順）。
        public let index: Int
        public let oid: String
    }

    public let status: Status
    public let old: Commit?
    public let new: Commit?
    public let subject: String
    /// 差分の差分（changed のときだけ）。先頭の 1 文字が外側の記号、残りがそれぞれの版のパッチの行。
    public let diff: [String]
}

/// ブランチの、最後に書き換える前の版（reflog から）。
public struct PreviousVersion: Decodable, Equatable, Sendable {
    public let branch: String
    /// 前の版の `<branch>@{n}`。
    public let selector: String
    /// 書き換えた動きの reflog の行（`rebase (finish): …` など）。
    public let message: String
    public let time: Int64
    public let old: String
    public let new: String
    /// rebase の載せ先（わからなければ nil）。
    public let onto: String?
    /// `rangeDiff` に渡す範囲。
    public let ranges: [String]
}

extension Repository {
    /// 2 つの版を比べる。`ranges` は git range-diff と同じ形。裏のスレッドで呼ぶ。
    public func rangeDiff(_ ranges: [String]) throws -> [RangePair] {
        struct Pairs: Decodable { let pairs: [RangePair] }
        return try info(["op": "rangeDiff", "ranges": ranges], as: Pairs.self).pairs
    }

    /// `branch`（nil なら今のブランチ）の、最後に書き換える前の版。なければ nil。裏のスレッドで呼ぶ。
    public func previousVersion(branch: String?) throws -> PreviousVersion? {
        struct Answer: Decodable { let previous: PreviousVersion? }
        var command: [String: Any] = ["op": "previousVersion"]
        if let branch { command["branch"] = branch }
        return try info(command, as: Answer.self).previous
    }

    /// 組の差分の差分を、差分の表示と同じ形にする。
    public func rangeDiffView(_ pair: RangePair) -> FileDiff? {
        guard let raw = amonhen_range_diff_view(pair.diff.joined(separator: "\n"), UInt8(AmonhenEncoding_Utf16.rawValue))
        else { return nil }
        return FileDiff(raw: raw, owner: self, view: amonhen_diff_view(raw), free: { amonhen_diff_free($0) })
    }
}
