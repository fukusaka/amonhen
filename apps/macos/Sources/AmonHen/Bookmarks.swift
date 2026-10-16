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
    /// 色ラベル（`Bookmarks.colors` の名前）。
    var color: String?
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
/// スキャンの結果は保存せず、一覧を開くたびと、フォルダの中でリポジトリが増えた・消えたとき（FSEvents）に裏で取り直す。
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

    /// 開いたリポジトリを、まだどこにもなければ（ブックマークにも、スキャンするフォルダの下にもない）ブックマークに入れる。
    func register(_ path: String) {
        let real = Self.real(path)
        guard bookmark(path) == nil, !data.scanRoots.contains(where: { real.hasPrefix(Self.real($0.path) + "/") }) else { return }
        add(path)
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
    func setColor(_ path: String, _ color: String?) { update(path) { $0.color = color } }

    /// 色ラベルの名前と色（Finder のタグの並び）。
    static let colors: [(name: String, title: String, color: NSColor)] = [
        ("red", "赤", .systemRed), ("orange", "オレンジ", .systemOrange), ("yellow", "黄", .systemYellow),
        ("green", "緑", .systemGreen), ("blue", "青", .systemBlue), ("purple", "紫", .systemPurple),
        ("gray", "グレー", .systemGray),
    ]

    static func color(named name: String?) -> NSColor? { colors.first { $0.name == name }?.color }

    /// `path` を `target` の前（nil ならそのグループの末尾）に動かし、グループを `group` にする（ドラッグでの並べ替え）。
    func reorder(_ path: String, before target: String?, group: String?) {
        if bookmark(path) == nil { data.items.append(Bookmark(path: path)) }
        guard let i = data.items.firstIndex(where: { $0.path == path }), path != target else { return }
        var item = data.items.remove(at: i)
        item.group = group
        let j = target.flatMap { t in data.items.firstIndex { $0.path == t } }
            ?? data.items.lastIndex { $0.group == group }.map { $0 + 1 }
            ?? data.items.endIndex
        data.items.insert(item, at: j)
        save()
    }
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
        watchScanRoots()
    }

    func removeScanRoot(_ path: String) {
        data.scanRoots.removeAll { $0.path == path }
        scanned[path] = nil
        save()
        watchScanRoots()
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

    // MARK: - まとめて Fetch

    /// Fetch している・失敗したリポジトリ（行に出す）。
    private(set) var fetching = Set<String>()
    private(set) var fetchErrors: [String: String] = [:]

    /// 選んだリポジトリをまとめて fetch する（同時に 4 つまで）。終わったら行の状態を読み直す。SSH 先は除く。
    func fetch(_ paths: [String]) {
        for path in paths where !RepositoryLocation.isRemote(path) && !fetching.contains(path) && RepositoryList.isRepository(path) {
            fetching.insert(path)
            fetchErrors[path] = nil
            Task {
                let error = await Self.peekQueue.run { () -> String? in
                    do {
                        try Repository(path: path).run(json: #"{"op":"fetch"}"#)
                        return nil
                    } catch {
                        return "\(error)"
                    }
                }
                fetching.remove(path)
                fetchErrors[path] = error
                peeks[path] = nil
                onChange?()
            }
        }
        onChange?()
    }

    // MARK: - スキャンするフォルダの見張り

    private var stream: FSEventStreamRef?
    private var rescanPending = false

    /// スキャンするフォルダを FSEvents で見張り、リポジトリが増えた・消えたらしいときに探し直す。
    /// 見つけたリポジトリの中の変化（ビルドなど）は見ない。
    func watchScanRoots() {
        if let stream {
            FSEventStreamStop(stream)
            FSEventStreamInvalidate(stream)
            FSEventStreamRelease(stream)
            self.stream = nil
        }
        let roots = data.scanRoots.map(\.path).filter { FileManager.default.fileExists(atPath: $0) }
        guard !roots.isEmpty else { return }
        var context = FSEventStreamContext(
            version: 0, info: Unmanaged.passUnretained(self).toOpaque(), retain: nil, release: nil, copyDescription: nil)
        let callback: FSEventStreamCallback = { _, info, count, paths, _, _ in
            guard let info else { return }
            let changed = (unsafeBitCast(paths, to: NSArray.self) as? [String]) ?? []
            let bookmarks = Unmanaged<Bookmarks>.fromOpaque(info).takeUnretainedValue()
            MainActor.assumeIsolated { bookmarks.scanRootsChanged(Array(changed.prefix(count))) }
        }
        guard let stream = FSEventStreamCreate(
            nil, callback, &context, roots as CFArray, FSEventStreamEventId(kFSEventStreamEventIdSinceNow), 2.0,
            FSEventStreamCreateFlags(kFSEventStreamCreateFlagUseCFTypes | kFSEventStreamCreateFlagNoDefer))
        else { return }
        FSEventStreamSetDispatchQueue(stream, .main)
        FSEventStreamStart(stream)
        self.stream = stream
    }

    /// シンボリックリンクを解いたパス（/tmp と /private/tmp のような違いをなくして比べる）。
    nonisolated static func real(_ path: String) -> String {
        RepositoryLocation.isRemote(path) ? path : URL(fileURLWithPath: path).resolvingSymlinksInPath().path
    }

    private func scanRootsChanged(_ paths: [String]) {
        // FSEvents のパスはリンクを解いた形で来る。
        let known = data.scanRoots.flatMap { scanned[$0.path] ?? [] }.map(Self.real)
        let roots = data.scanRoots.map { root in
            var root = root
            root.path = Self.real(root.path)
            return root
        }
        let relevant = paths.contains { raw in
            let path = raw.hasSuffix("/") ? String(raw.dropLast()) : raw
            // 見つけたリポジトリの中（そのもの以外）の変化は、リポジトリの増減ではない。
            if known.contains(where: { path.hasPrefix($0 + "/") }) { return false }
            // 探す深さより下は見ない。
            return roots.contains { root in
                guard path == root.path || path.hasPrefix(root.path + "/") else { return false }
                let depth = path.dropFirst(root.path.count).split(separator: "/").count
                let parts = path.dropFirst(root.path.count).split(separator: "/").map(String.init)
                return depth < root.depth && !parts.contains { $0.hasPrefix(".") || root.excludes.contains($0) }
            }
        }
        guard relevant, !rescanPending else { return }
        rescanPending = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in
            self?.rescanPending = false
            self?.rescan()
        }
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
