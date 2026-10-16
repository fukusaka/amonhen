import AmonhenKit
import AppKit

/// 個別に登録したリポジトリ。
struct Bookmark: Codable, Hashable {
    var path: String
    /// 表示名の上書き（なければフォルダの名前）。
    var name: String?
    /// 手で作ったグループ（なければグループなし）。
    var group: String?
    var favorite = false
}

/// リポジトリを探すフォルダ。
struct ScanRoot: Codable, Hashable {
    var path: String
    /// フォルダから何段下まで探すか。
    var depth = 3
    /// 入らないフォルダの名前。
    var excludes = ["node_modules", "target", "build", ".build", "vendor", "Pods", "DerivedData", "dist"]
}

struct BookmarkData: Codable {
    var items: [Bookmark] = []
    var groups: [String] = []
    var scanRoots: [ScanRoot] = []
}

/// ブックマーク: 個別に登録したリポジトリ（グループ・お気に入り・表示名）と、リポジトリを探すフォルダ。
/// `~/Library/Application Support/Amon Hen/bookmarks.json` に置く（構造があり、手で直せるように）。
/// スキャンの結果は保存せず、一覧を開くたびに裏で取り直す。
final class Bookmarks {
    static let shared = Bookmarks()

    private(set) var data: BookmarkData
    /// スキャンで見つけたリポジトリ（フォルダ → リポジトリの並び）。
    private(set) var scanned: [String: [String]] = [:]
    private(set) var isScanning = false
    /// 一覧の行に出す状態（読んだもの）。
    private var peeks: [String: RepositoryPeek] = [:]
    private var peeking = Set<String>()
    /// 中身・スキャンの結果・行の状態が変わった（一覧を描き直す）。
    var onChange: (() -> Void)?

    private static var file: URL {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Amon Hen")
        return dir.appendingPathComponent("bookmarks.json")
    }

    private init() {
        data = (try? Data(contentsOf: Self.file)).flatMap { try? JSONDecoder().decode(BookmarkData.self, from: $0) }
            ?? BookmarkData()
    }

    private func save() {
        do {
            try FileManager.default.createDirectory(
                at: Self.file.deletingLastPathComponent(), withIntermediateDirectories: true)
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try encoder.encode(data).write(to: Self.file, options: .atomic)
        } catch {
            NSLog("ブックマークを保存できない: \(error)")
        }
        onChange?()
    }

    // MARK: - 編集

    func bookmark(_ path: String) -> Bookmark? { data.items.first { $0.path == path } }

    func add(_ path: String, group: String? = nil) {
        guard bookmark(path) == nil else { return }
        data.items.append(Bookmark(path: path, group: group))
        save()
    }

    func remove(_ path: String) {
        data.items.removeAll { $0.path == path }
        save()
    }

    private func update(_ path: String, _ change: (inout Bookmark) -> Void) {
        if bookmark(path) == nil { data.items.append(Bookmark(path: path)) }
        guard let i = data.items.firstIndex(where: { $0.path == path }) else { return }
        change(&data.items[i])
        save()
    }

    func toggleFavorite(_ path: String) { update(path) { $0.favorite.toggle() } }
    func rename(_ path: String, to name: String?) { update(path) { $0.name = name } }
    func move(_ path: String, to group: String?) { update(path) { $0.group = group } }

    func addGroup(_ name: String) {
        guard !data.groups.contains(name) else { return }
        data.groups.append(name)
        save()
    }

    /// グループを消す（中のリポジトリはグループなしに戻す）。
    func removeGroup(_ name: String) {
        data.groups.removeAll { $0 == name }
        for i in data.items.indices where data.items[i].group == name { data.items[i].group = nil }
        save()
    }

    func addScanRoot(_ path: String) {
        guard !data.scanRoots.contains(where: { $0.path == path }) else { return }
        data.scanRoots.append(ScanRoot(path: path))
        save()
        rescan()
    }

    func removeScanRoot(_ path: String) {
        data.scanRoots.removeAll { $0.path == path }
        scanned[path] = nil
        save()
    }

    func setDepth(_ depth: Int, of path: String) {
        guard let i = data.scanRoots.firstIndex(where: { $0.path == path }) else { return }
        data.scanRoots[i].depth = depth
        save()
        rescan()
    }

    /// 表示名（上書きがなければフォルダの名前。SSH 先ならホストを添える）。
    func displayName(_ path: String) -> String {
        if let name = bookmark(path)?.name, !name.isEmpty { return name }
        return (RepositoryLocation.path(path) as NSString).lastPathComponent
            + (RepositoryLocation.host(path).map { " — \($0)" } ?? "")
    }

    /// Quick Open で探す候補: お気に入り、ブックマーク、最近開いたもの、スキャンで見つけたもの（重複なし）。
    var allLocations: [String] {
        var seen = Set<String>()
        let ordered = data.items.filter(\.favorite).map(\.path) + data.items.map(\.path) + RepositoryList.paths
            + data.scanRoots.flatMap { scanned[$0.path] ?? [] }
        return ordered.filter { seen.insert($0).inserted }
    }

    // MARK: - スキャン

    /// すべてのフォルダを裏で探し直す。
    func rescan() {
        guard !isScanning else { return }
        isScanning = true
        onChange?()
        let roots = data.scanRoots
        Task {
            let found = await Task.detached(priority: .utility) {
                Dictionary(uniqueKeysWithValues: roots.map { ($0.path, Self.scan($0)) })
            }.value
            scanned = found
            isScanning = false
            onChange?()
        }
    }

    /// `root` の下のリポジトリ（作業ツリーの直下に `.git` があるフォルダ）。リポジトリの中には入らない。
    /// 隠しフォルダと `excludes` の名前のフォルダには入らない。
    nonisolated static func scan(_ root: ScanRoot) -> [String] {
        let fm = FileManager.default
        var found: [String] = []
        func walk(_ dir: URL, depth: Int) {
            if fm.fileExists(atPath: dir.appendingPathComponent(".git").path) {
                found.append(dir.standardizedFileURL.path)
                return
            }
            guard depth > 0, let children = try? fm.contentsOfDirectory(
                at: dir, includingPropertiesForKeys: [.isDirectoryKey, .isSymbolicLinkKey], options: [.skipsHiddenFiles])
            else { return }
            for child in children {
                let values = try? child.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
                guard values?.isDirectory == true, values?.isSymbolicLink != true,
                      !root.excludes.contains(child.lastPathComponent) else { continue }
                walk(child, depth: depth - 1)
            }
        }
        walk(URL(fileURLWithPath: (root.path as NSString).expandingTildeInPath), depth: root.depth)
        return found.sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }

    // MARK: - 行の状態

    /// `path` の状態。まだ読んでいなければ裏で読み始めて nil を返し、読めたら `onChange` で知らせる
    /// （見えている行だけを読むため、一覧が描くときに呼ぶ）。SSH 先は読まない。
    func peek(_ path: String) -> RepositoryPeek? {
        if let cached = peeks[path] { return cached }
        guard !RepositoryLocation.isRemote(path), !peeking.contains(path), RepositoryList.isRepository(path) else { return nil }
        peeking.insert(path)
        Task {
            let peek = await Self.peekQueue.run { RepositoryPeek.read(path: path) }
            peeking.remove(path)
            if let peek { peeks[path] = peek }
            onChange?()
        }
        return nil
    }

    /// 一覧を開き直したら、状態を読み直す。
    func forgetPeeks() {
        peeks.removeAll()
    }

    /// git を同時に起動しすぎないよう、行の状態は 4 つずつ読む。
    private static let peekQueue = LimitedQueue(limit: 4)
}

/// 同時に走らせる数を制限して、裏で処理する。
actor LimitedQueue {
    private let limit: Int
    private var running = 0
    private var waiting: [CheckedContinuation<Void, Never>] = []

    init(limit: Int) { self.limit = limit }

    func run<T: Sendable>(_ work: @escaping @Sendable () -> T) async -> T {
        if running >= limit {
            // 空いた枠をそのまま引き継ぐ（起こされたときには running は増えたまま）。
            await withCheckedContinuation { waiting.append($0) }
        } else {
            running += 1
        }
        let result = await Task.detached(priority: .utility) { work() }.value
        if waiting.isEmpty {
            running -= 1
        } else {
            waiting.removeFirst().resume()
        }
        return result
    }
}
