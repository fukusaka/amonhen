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

    /// 開く隙間を C に渡す（nil なら NULL: 畳まない。空でも NULL でないポインタを渡す）。
    private func withGaps<R>(_ gaps: [Int: GapOpening]?, _ body: (UnsafePointer<UInt32>?, UInt32) -> R) -> R {
        guard let gaps else { return body(nil, 0) }
        var values: [UInt32] = []
        for (gap, opening) in gaps.sorted(by: { $0.key < $1.key }) {
            values += [UInt32(gap), opening.top, opening.bottom]
        }
        if values.isEmpty { values = [UInt32.max] }
        return values.withUnsafeBufferPointer { body($0.baseAddress, UInt32(gaps.count)) }
    }

    /// `item` の差分。`staged` なら HEAD と index、そうでなければ index と作業ツリー。空白を無視した差分は、行ごとの
    /// stage・破棄に使えない。`context` は前後の行数（`.max` ならファイル全体）。`openGaps` があれば、変更のない所
    /// （隙間）を少しずつ開いたり閉じたりできる形にする（hunk の見出しの行の代わりに隙間の行。`old_no` が隠して
    /// いる行数、`new_no` が見せている行数、文字は関数名）。キーは隙間の番号（頭から 0, 1, …）。裏のスレッドで呼ぶ。
    public func diff(
        of item: StatusItem, staged: Bool, context: UInt32 = 3, openGaps: [Int: GapOpening]? = nil,
        whitespace: DiffWhitespace = .exact
    ) throws -> WorkingFileDiff {
        let raw = withGaps(openGaps) { open, count in
            amonhen_working_diff(
                repository.raw, self.raw, UInt32(item.index), staged, context, open, count, whitespace.rawValue,
                UInt8(AmonhenEncoding_Utf16.rawValue))
        }
        guard let raw
        else { throw AmonhenError.last() }
        return WorkingFileDiff(raw: raw, status: self, item: item, staged: staged)
    }

    /// `item` の HEAD と作業ツリーの差分。変更の行ごとに index に入っているか（`WorkingFileDiff.isStaged(row:)`）が
    /// わかる。index がその形で表せないファイル（作業ツリーで書き戻した変更が index にだけあるなど）・
    /// サブモジュール・バイナリ・コンフリクトでは nil。`context` は前後の行数（`.max` ならファイル全体）。
    /// 裏のスレッドで呼ぶ。
    public func combinedDiff(of item: StatusItem, context: UInt32 = 3, openGaps: [Int: GapOpening]? = nil) throws -> WorkingFileDiff? {
        let raw = withGaps(openGaps) { open, count in
            amonhen_working_diff_combined(
                repository.raw, self.raw, UInt32(item.index), context, open, count, UInt8(AmonhenEncoding_Utf16.rawValue))
        }
        guard let raw
        else {
            // 理由が空の NULL は「表せない」。
            let error = AmonhenError.last()
            if error.description.isEmpty { return nil }
            throw error
        }
        return WorkingFileDiff(raw: raw, status: self, item: item, staged: false, combined: true)
    }
}

/// 変更のない所（隙間）の開き方: 上（前の変更に続く所）から `top` 行、下（次の変更の手前）から `bottom` 行を
/// 見せる。足して隙間の行数以上なら、すべて見せる。
public struct GapOpening: Hashable, Sendable {
    public var top: UInt32
    public var bottom: UInt32
    public init(top: UInt32 = 0, bottom: UInt32 = 0) {
        self.top = top
        self.bottom = bottom
    }
}

/// ファイルの改行の種類。
public enum LineEnding: UInt8, Sendable {
    case none = 0, lf, crlf, mixed
}

/// Working Copy の 1 ファイルの改行。差分は git に入れる形どうしで比べるので、改行の変換や混ざった改行は差分に
/// 出ない。それを見せるためのもの。
public struct LineEndings: Equatable, Sendable {
    /// git（index）に入っている形。
    public let index: LineEnding
    public let worktree: LineEnding
    /// index の内容を checkout したときの形。
    public let checkout: LineEnding
    /// かかっている変換（`text=auto eol=crlf`・`core.autocrlf=true` など。なければ空）。
    public let rule: String
    /// 見せる値打ちがある（改行を変換している、または `warning`）。
    public let notable: Bool
    /// 改行が混ざっている、または作業ツリーが CRLF なのに checkout すると LF になる。
    public let warning: Bool
}

/// Working Copy の 1 ファイルの差分。行を選んで stage・unstage できる。
public final class WorkingFileDiff: FileDiff, @unchecked Sendable {
    public let item: StatusItem
    public let staged: Bool
    /// HEAD と作業ツリーの差分（`WorkingStatus.combinedDiff`）か。
    public let isCombined: Bool
    /// ファイルの改行（stage していない差分だけ）。
    public let lineEndings: LineEndings?
    let repository: Repository

    init(raw: OpaquePointer, status: WorkingStatus, item: StatusItem, staged: Bool, combined: Bool = false) {
        self.item = item
        self.staged = staged
        self.isCombined = combined
        self.repository = status.repository
        let le = amonhen_working_diff_line_endings(raw)
        if le.present {
            let rule = amonhen_working_diff_line_ending_rule(raw)
            defer { if let rule { amonhen_string_free(rule) } }
            self.lineEndings = LineEndings(
                index: LineEnding(rawValue: le.index) ?? .none,
                worktree: LineEnding(rawValue: le.worktree) ?? .none,
                checkout: LineEnding(rawValue: le.checkout) ?? .none,
                rule: rule.map { String(cString: $0) } ?? "",
                notable: le.notable,
                warning: le.warning)
        } else {
            self.lineEndings = nil
        }
        super.init(
            raw: raw, owner: status, view: amonhen_working_diff_view(raw), free: { amonhen_working_diff_free($0) },
            syntax: { amonhen_working_diff_syntax($0) })
    }

    /// `row` 行目の変更が index に入っているか（`isCombined` の差分だけ）。
    public func isStaged(row: Int) -> Bool {
        withRows { $0[row].flags & UInt8(AmonhenDiffRow_STAGED) != 0 }
    }

    /// `rows` 行目の変更を、`staged` なら index に入れ、そうでなければ外す（`isCombined` の差分だけ）。
    public func setStaged(rows: [Int], staged: Bool) throws {
        let ok = rows.map(UInt32.init).withUnsafeBufferPointer {
            amonhen_set_rows_staged(repository.raw, raw, $0.baseAddress, UInt32($0.count), staged)
        }
        if !ok { throw AmonhenError.last() }
    }

    /// 差分の前（`after` が false）か後の中身（画像の比較など）。空なら nil。
    public func blob(after: Bool) -> Data? {
        var length: UInt = 0
        guard let bytes = amonhen_working_diff_blob(raw, after, &length) else { return nil }
        defer { amonhen_blob_free(bytes, length) }
        return Data(bytes: bytes, count: Int(length))
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

/// リポジトリの一覧に出す軽い状態（`Repository` を開かずに読む）。
public struct RepositoryPeek: Sendable, Equatable {
    /// 今のブランチ。detached なら nil。
    public let branch: String?
    public let hasUpstream: Bool
    public let ahead: Int
    public let behind: Int
    /// 変更のあるファイルの数（untracked も数える）。
    public let changed: Int
    /// HEAD のコミットの日時と件名。まだコミットがなければ nil。
    public let lastCommitDate: Date?
    public let lastCommitSubject: String?

    /// `path` の状態。git を 2 回起動するので、裏のスレッドで呼ぶ。読めなければ nil。
    public static func read(path: String) -> RepositoryPeek? {
        guard let out = amonhen_peek(path) else { return nil }
        defer { amonhen_string_free(out) }
        let fields = String(cString: out).components(separatedBy: "\n")
        guard fields.count == 7 else { return nil }
        let time = TimeInterval(fields[5])
        return RepositoryPeek(
            branch: fields[0].isEmpty ? nil : fields[0], hasUpstream: fields[1] == "1",
            ahead: Int(fields[2]) ?? 0, behind: Int(fields[3]) ?? 0, changed: Int(fields[4]) ?? 0,
            lastCommitDate: time.map { Date(timeIntervalSince1970: $0) },
            lastCommitSubject: time == nil ? nil : fields[6])
    }
}

extension Repository {
    /// `url` を `dest`（まだないフォルダ）に clone する。`filter`（`blob:none` など）があれば partial clone、`depth`
    /// があれば最近のその数のコミットだけ、`sparse` なら根のファイルだけを取り出す（git 2.27 から）。`branch` は取り出す
    /// ブランチ（`-b`。HEAD のない bundle なら、なくても main・master・最初のブランチ）。`recurseSubmodules` なら
    /// submodule も取り出す。時間がかかるので裏のスレッドで呼ぶ。
    public static func clone(url: String, to dest: String, filter: String? = nil, depth: Int? = nil,
                             sparse: Bool = false, branch: String? = nil,
                             recurseSubmodules: Bool = false) throws {
        if !amonhen_clone(url, dest, filter, UInt32(clamping: max(0, depth ?? 0)), sparse, branch,
                          recurseSubmodules) {
            throw AmonhenError.last()
        }
    }
}

/// bundle のファイルの見出し（git を動かさずに読む）。
public struct BundleFileInfo: Decodable, Sendable {
    /// 入っている ref（`refs/heads/main` など。HEAD は除く）。
    public let refs: [String]
    /// HEAD が入っている（clone したとき、それを取り出す）。
    public let head: Bool
    /// 前提のコミットの数（あれば clone できない）。
    public let prerequisites: Int
    /// HEAD がないとき取り出すブランチ（main・master・最初のブランチ）。
    public let branch: String?

    /// `path` が bundle のファイルなら、その見出し。bundle でなければ nil。
    public static func read(path: String) -> BundleFileInfo? {
        guard let out = amonhen_bundle_info(path) else { return nil }
        defer { amonhen_string_free(out) }
        return try? JSONDecoder().decode(BundleFileInfo.self, from: Data(String(cString: out).utf8))
    }
}

/// SSH 先のフォルダの下のリポジトリを探す（リポジトリの一覧の、SSH 先のスキャンするフォルダ）。
public enum RemoteScan {
    /// `location`（`ssh://host/path`）の下を `depth` 段まで探し、見つけたリポジトリの場所を返す。SSH 先のエンジンを
    /// 用意する（なければ送り込む）ので時間がかかる。裏のスレッドで呼ぶ。
    public static func findRepositories(
        location: String, depth: Int, excludes: [String], daemon: String, daemons: String, hostOptions: String? = nil
    ) throws -> [String] {
        guard let out = amonhen_find_remote_repositories(location, UInt32(max(0, depth)), excludes.joined(separator: "\n"), daemon, daemons, hostOptions)
        else { throw AmonhenError.last() }
        defer { amonhen_string_free(out) }
        return String(cString: out).split(separator: "\n").map(String.init)
    }
}

/// この Mac のフォルダ（パスを選ぶシートが読む）。
public enum LocalFolders {
    /// `path`（`~` 可）のフォルダの中身。JSON（`path`・`home`・`entries`。SSH 先のエンジンの `listDirectory` と同じ形）。
    public static func listJSON(_ path: String) throws -> Data {
        guard let out = amonhen_list_directory(path) else { throw AmonhenError.last() }
        defer { amonhen_string_free(out) }
        return Data(String(cString: out).utf8)
    }
}

/// この Mac の git（リポジトリに依らないもの）。どれも git を起動することがあるので、裏のスレッドで呼ぶ。
public enum LocalGit {
    /// git の版（`2.39.5`）。読めなければ nil。
    public static func version() -> String? {
        guard let out = amonhen_git_version() else { return nil }
        defer { amonhen_string_free(out) }
        return String(cString: out)
    }

    /// ユーザー全体（`git config --global`）の `key` の値。なければ nil。
    public static func globalConfig(_ key: String) -> String? {
        guard let out = amonhen_global_config(key) else { return nil }
        defer { amonhen_string_free(out) }
        return String(cString: out)
    }

    /// この機械で署名に使える鍵（`format` は openpgp・ssh・x509）。JSON の配列（id・label・emails）。読めなければ nil。
    /// gpg などを起動するので、裏のスレッドで呼ぶ。
    public static func signingKeysJSON(format: String) -> String? {
        guard let out = amonhen_signing_keys(format) else { return nil }
        defer { amonhen_string_free(out) }
        return String(cString: out)
    }

    /// ユーザー全体（`git config --global`）の `key` を変える。nil なら消す。
    public static func setGlobalConfig(_ key: String, _ value: String?) throws {
        let ok = value.map { amonhen_set_global_config(key, $0) } ?? amonhen_set_global_config(key, nil)
        if !ok { throw AmonhenError.last() }
    }
}
