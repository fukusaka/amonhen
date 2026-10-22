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
            throw AmonhenError(
                description: response["error"] as? String ?? "失敗した", kind: response["errorKind"] as? String,
                path: response["errorPath"] as? String)
        }
        return CommandResult(
            outcome: (response["outcome"] as? String).flatMap(Outcome.init), label: response["label"] as? String,
            text: response["text"] as? String)
    }
}

/// 途中の操作（rebase・merge・cherry-pick・revert）。`{"op":"sequence"}` の `info`。
public struct SequenceInfo: Decodable, Equatable, Sendable {
    /// 止まっている手順（`pick`・`edit`・`exec`・`merge`・`cherry-pick` など）と、そのコミット。
    public struct Step: Decodable, Equatable, Sendable {
        public let action: String
        public let oid: String?
        public let subject: String?
    }

    /// `rebase`・`merge`・`cherry-pick`・`revert`。
    public let kind: String
    /// rebase の何番目か・全部でいくつか。
    public var current: Int?
    public var total: Int?
    /// 操作しているブランチ。
    public var branch: String?
    /// rebase の先・merge で取り込む側（ref の名前、なければ短い ID）。
    public var onto: String?
    public var from: String?
    public var step: Step?
    /// edit・break で止めた（コンフリクトではない）。
    public var editing: Bool?

    public init(kind: String) {
        self.kind = kind
    }
}

extension Repository {
    /// 途中の操作。なければ nil。裏のスレッドで呼ぶ。
    public func sequence() throws -> SequenceInfo? {
        struct Response: Decodable {
            let text: String?
            let info: SequenceInfo?
        }
        guard let out = amonhen_command(raw, #"{"op":"sequence"}"#) else { throw AmonhenError(description: "結果がない") }
        defer { amonhen_string_free(out) }
        let response = try JSONDecoder().decode(Response.self, from: Data(String(cString: out).utf8))
        // 詳しいことを返さない古いエンジン（SSH 先）なら、種類だけ（"rebase (2/5)" の頭）。
        return response.info ?? response.text.map { SequenceInfo(kind: String($0.prefix { $0 != " " })) }
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

/// コミットの詳細（すぐ返せる分）。
public struct CommitInfo: Decodable, Equatable, Sendable {
    public let oid: String
    public let message: String
    public let author: String
    public let authorEmail: String
    public let authorTime: Int64
    /// `+0900` の形。
    public let authorTz: String
    public let committer: String
    public let committerEmail: String
    public let committerTime: Int64
    public let committerTz: String
    public let parents: [String]
    public let refs: [String]
    /// 署名の種類（`gpg`・`ssh`・`x509`）。署名がなければ nil。検証の結果は `CommitExtras.signature`。
    public let signed: String?
    /// 署名付きのタグを merge したコミットか。
    public let mergetag: Bool
    /// merge した署名付きのタグ（ヘッダーの mergetag）。
    public let mergedTags: [MergedTag]
    public let trailers: [Trailer]

    public struct MergedTag: Decodable, Equatable, Sendable {
        public let name: String
        public let tagger: String
        /// タグのメッセージの 1 行目。
        public let subject: String
        public let signed: Bool
    }

    public struct Trailer: Decodable, Equatable, Sendable {
        public let key: String
        public let value: String
    }
}

/// 署名の検証の結果（History の一覧用）。
public struct SignatureCheck: Decodable, Sendable {
    /// `git log --format=%G?` の 1 文字（署名がなければ N）。
    public let status: String
    public let signer: String
    public let key: String
    public let fingerprint: String
    public let trust: String
    /// コミッターのメールアドレス（正しい署名のときだけ。署名者と一致するかを確かめる）。
    public let committerEmail: String
    /// 署名した鍵に登録されたメールアドレス（GPG はすべてのユーザー ID、SSH はプリンシパル）。
    public let emails: [SignerEmail]
}

/// 署名した鍵のメールアドレスと、その有効性（gpg の uid の欄。u・f・m は有効、r は失効、e は期限切れ、`-` は不明）。
public struct SignerEmail: Decodable, Sendable {
    public let email: String
    public let validity: String
}

/// コミットの詳細のうち時間のかかる分（署名の検証・notes・行数・子・含むブランチとタグ）。
/// stash の土台（作ったときの HEAD）と、今の HEAD の離れ具合。
public struct StashBase: Decodable, Equatable, Sendable {
    public let name: String
    public let base: String
    /// HEAD にだけあるコミット（stash を作った後に HEAD が進んだ分）。
    public let ahead: Int
    /// 土台にだけあるコミット（0 でなければ、土台は今の HEAD の履歴にない）。
    public let behind: Int
}

private struct StashBases: Decodable {
    let stashes: [StashBase]
}

/// reflog の 1 行（`git reflog` の 1 行）。
public struct ReflogEntry: Decodable, Equatable, Sendable {
    /// `HEAD@{0}` など。
    public let selector: String
    /// 前に指していたコミット。ref を作った行なら `nil`。
    public let old: String?
    /// 新しく指したコミット。
    public let new: String
    /// 動かした日時（UNIX 時刻、秒）。
    public let time: Int64
    /// 何をしたか（`commit: …`、`checkout: moving from a to b`、`reset: moving to HEAD~1` など）。
    public let message: String
    /// `new` の件名（読めなければ空）。
    public let subject: String
    /// ref から辿れる（History にある）。`false` なら、どのブランチにも残っていない。
    public let reachable: Bool
}

private struct Reflog: Decodable {
    let entries: [ReflogEntry]
}

public struct CommitExtras: Decodable, Sendable {
    public let signature: Signature?
    public let notes: String
    public let stats: [FileStat]
    public let children: [String]
    public let branches: [String]
    /// 含むタグは古い順で、多ければ先頭だけ。数は `tagCount`。
    public let tags: [String]
    public let tagCount: Int
    /// このコミットを最初に含んだタグと、このコミットより前でいちばん近いタグ。
    public let firstTag: String?
    public let previousTag: String?
    public let originUrl: String?
    public let localBranches: [String]
    public let remoteBranches: [String]
    /// HEAD から何コミット前か。HEAD が含まなければ nil。
    public let headDistance: Int?
    /// HEAD からの道のり（`HEAD~2^2~1` の形。そのまま git に渡せる）。HEAD が含まなければ nil。
    public let headRev: String?
    /// merge が取り込んだコミットの数と、その先頭（多ければ 50 件まで）。
    public let mergedCount: Int
    public let merged: [CommitSummary]
    /// メッセージに書かれた ID（書かれたままの文字列）から、引けたコミット。
    public let references: [String: CommitSummary]

    public struct CommitSummary: Decodable, Sendable {
        public let oid: String
        public let subject: String
        public let author: String?
    }

    public struct Signature: Decodable, Sendable {
        /// `git log --format=%G?` の 1 文字（G・B・U・X・Y・R・E）。
        public let status: String
        public let signer: String
        public let key: String
        public let fingerprint: String
        public let trust: String
        public let emails: [SignerEmail]
    }

    public struct FileStat: Decodable, Sendable {
        public let path: String
        /// バイナリなら nil。
        public let added: Int?
        public let removed: Int?
    }
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

    /// stash ごとの土台と、今の HEAD の離れ具合。HEAD は、このリポジトリ（worktree）のもの。裏のスレッドで呼ぶ。
    public func stashBases() throws -> [StashBase] {
        let bases: StashBases = try info(["op": "stashBases"])
        return bases.stashes
    }

    /// HEAD（このリポジトリ・worktree のもの）の reflog を新しい順に。裏のスレッドで呼ぶ。
    public func reflog(limit: Int = 500) throws -> [ReflogEntry] {
        let reflog: Reflog = try info(["op": "reflog", "limit": limit])
        return reflog.entries
    }

    /// コミットの詳細のうち時間のかかる分（git で署名を検証するなど）。裏のスレッドで呼ぶ。
    public func commitExtras(oid: String) throws -> CommitExtras {
        try info(["op": "commitExtras", "oid": oid])
    }

    /// 署名の検証の結果を `oids` と同じ順に返す。一度検証したものはコアが覚えているので速い。覚えていない
    /// ものは 1 件 15 ms ほどかかるので、裏のスレッドで少しずつ呼ぶ。
    public func verifySignatures(_ oids: [String]) throws -> [SignatureCheck] {
        struct Checks: Decodable { let checks: [SignatureCheck] }
        return try info(["op": "verifySignatures", "oids": oids], as: Checks.self).checks
    }

    /// 名前や短い ID が指すコミット（40 桁）。裏のスレッドで呼ぶ。
    public func resolveCommit(_ rev: String) throws -> String {
        guard let text = try run(["op": "resolveCommit", "rev": rev]).text else {
            throw AmonhenError(description: "コミットが見つからない: \(rev)")
        }
        return text
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

/// Activity の記録（git の起動とコマンドの実行）。
public struct ActivityRecord: Decodable, Sendable, Identifiable, Hashable {
    public let seq: UInt64
    /// "git"・"command"。
    public let kind: String
    /// 作業ツリーのパス（わからなければ空）。
    public let repo: String
    public let label: String
    /// 始めた時刻（UNIX 時刻のミリ秒）。
    public let started: UInt64
    /// かかった時間（ミリ秒）。走っている間は nil。
    public let duration: Double?
    public let ok: Bool?
    public let error: String?
    /// 走っている間の進み具合（fetch の受信量など）。
    public let progress: String?
    /// 取り消せるか（走っている間）。
    public let cancellable: Bool
    /// 成功したときに git が標準エラーに書いたもの（hooks の出力など）。
    public let output: String?
    /// SSH 先の daemon の記録なら、そのリポジトリの名前（手元の記録なら nil）。
    public var remote: String?
    public var id: String { "\(remote ?? ""):\(seq)" }

    enum CodingKeys: String, CodingKey {
        case seq, kind, repo, label, started, duration, ok, error, progress, cancellable, output
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        seq = try c.decode(UInt64.self, forKey: .seq)
        kind = try c.decode(String.self, forKey: .kind)
        repo = try c.decode(String.self, forKey: .repo)
        label = try c.decode(String.self, forKey: .label)
        started = try c.decode(UInt64.self, forKey: .started)
        duration = try c.decodeIfPresent(Double.self, forKey: .duration)
        ok = try c.decodeIfPresent(Bool.self, forKey: .ok)
        error = try c.decodeIfPresent(String.self, forKey: .error)
        progress = try c.decodeIfPresent(String.self, forKey: .progress)
        cancellable = try c.decodeIfPresent(Bool.self, forKey: .cancellable) ?? false
        output = try c.decodeIfPresent(String.self, forKey: .output)
    }

    /// 今残っている記録（古い順、最大 1000 件）。
    public static func all() -> [ActivityRecord] {
        guard let out = amonhen_activity() else { return [] }
        defer { amonhen_string_free(out) }
        return (try? JSONDecoder().decode([ActivityRecord].self, from: Data(String(cString: out).utf8))) ?? []
    }

    /// 手元で走っている操作を取り消す。
    @discardableResult
    public static func cancel(seq: UInt64) -> Bool { amonhen_activity_cancel(seq) }
}
