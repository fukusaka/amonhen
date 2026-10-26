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
    /// 開いている間、何分ごとに自動で fetch するか（なければしない）。
    var autoFetch: Int?
}

/// リポジトリを探すフォルダ。この Mac のパスか、SSH 先のフォルダ（`ssh://host/path`。SSH 先のエンジンが探す）。
struct ScanRoot: Codable, Hashable {
    var path: String
    /// フォルダから何段下まで探すか。
    var depth = 3
    /// 入らないフォルダの名前。
    var excludes = ["node_modules", "target", "build", ".build", "vendor", "Pods", "DerivedData", "dist"]
}

struct BookmarkData: Codable, Equatable {
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
    /// SSH 先のフォルダを探せなかった理由（フォルダ → エラー。一覧に出す）。
    private(set) var scanErrors: [String: String] = [:]
    /// 一覧の行に出す状態（読んだもの）。
    private var peeks: [String: RepositoryPeek] = [:]
    private var peeking = Set<String>()
    /// 中身・スキャンの結果・行の状態が変わった（一覧を描き直す）。
    var onChange: (() -> Void)?
    /// 行の状態（peek・fetch の結果）が変わった。一覧はその行だけ描き直す（全部作り直すとスクロールが戻る）。
    var onRowChange: ((String) -> Void)?

    private static var file: URL {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Amon Hen")
        return dir.appendingPathComponent("bookmarks.json")
    }

    private init() {
        data = Self.read() ?? BookmarkData()
        stamp = Self.modified()
        // ほかの Amon Hen（正式版と snapshot、開発中のビルド）も同じファイルを書く。前に来たときに読み直す。
        NotificationCenter.default.addObserver(
            forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main
        ) { _ in
            MainActor.assumeIsolated { Bookmarks.shared.reloadIfChanged() }
        }
    }

    /// 最後に読んだか書いたときの、ファイルの更新時刻（ほかのプロセスが書いたかを見分ける）。
    private var stamp: Date?

    private static func read() -> BookmarkData? {
        (try? Data(contentsOf: file)).flatMap { try? JSONDecoder().decode(BookmarkData.self, from: $0) }
    }

    private static func modified() -> Date? {
        (try? FileManager.default.attributesOfItem(atPath: file.path))?[.modificationDate] as? Date
    }

    /// ほかのプロセスが書き換えていたら読み直す。変わったら `true`。
    @discardableResult
    private func refresh() -> Bool {
        let now = Self.modified()
        guard now != stamp else { return false }
        stamp = now
        guard let fresh = Self.read(), fresh != data else { return false }
        data = fresh
        return true
    }

    /// ほかのプロセスが書き換えていたら読み直し、一覧を描き直す（スキャンするフォルダが変わっていれば探し直す）。
    func reloadIfChanged() {
        let roots = data.scanRoots
        guard refresh() else { return }
        if data.scanRoots != roots {
            rescan()
            watchScanRoots()
        }
        onChange?()
    }

    /// 変える。ファイルをロックしてから最新を読み直し、その上で `change` を当てて書く（ほかの Amon Hen が同時に
    /// 動いていても、相手の変更を手元の古い内容で消さない。前は手元の内容で丸ごと書き直していた）。
    private func edit(_ change: (inout BookmarkData) -> Void) {
        guard write(change) else { return }
        // ロックを外してから知らせる（知らせを受けた側がまた変えても、待ち合わない）。
        onChange?()
    }

    /// ロックの中で読み直し、`change` を当てて書く。変えたら `true`。
    private func write(_ change: (inout BookmarkData) -> Void) -> Bool {
        let directory = Self.file.deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let lock = open(directory.appendingPathComponent("bookmarks.lock").path, O_CREAT | O_RDWR, 0o644)
        if lock >= 0 { flock(lock, LOCK_EX) }
        defer {
            if lock >= 0 {
                flock(lock, LOCK_UN)
                close(lock)
            }
        }
        refresh()
        var next = data
        change(&next)
        guard next != data else { return false }
        data = next
        do {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try encoder.encode(data).write(to: Self.file, options: .atomic)
            stamp = Self.modified()
        } catch {
            NSLog("ブックマークを保存できない: \(error)")
        }
        return true
    }

    // MARK: - 編集

    func bookmark(_ path: String) -> Bookmark? { data.items.first { $0.path == path } }

    func add(_ path: String, group: String? = nil) {
        edit { data in
            guard !data.items.contains(where: { $0.path == path }) else { return }
            data.items.append(Bookmark(path: path, group: group))
        }
    }

    /// 開いたリポジトリを、まだどこにもなければ（ブックマークにも、スキャンするフォルダの下にもない）ブックマークに入れる。
    func register(_ path: String) {
        let real = Self.real(path)
        guard bookmark(path) == nil, !data.scanRoots.contains(where: { real.hasPrefix(Self.real($0.path) + "/") }) else { return }
        add(path)
    }

    func remove(_ path: String) {
        edit { $0.items.removeAll { $0.path == path } }
    }

    private func update(_ path: String, _ change: (inout Bookmark) -> Void) {
        edit { data in
            if !data.items.contains(where: { $0.path == path }) { data.items.append(Bookmark(path: path)) }
            guard let i = data.items.firstIndex(where: { $0.path == path }) else { return }
            change(&data.items[i])
        }
    }

    func toggleFavorite(_ path: String) { update(path) { $0.favorite.toggle() } }
    func setColor(_ path: String, _ color: String?) { update(path) { $0.color = color } }
    func setAutoFetch(_ path: String, minutes: Int?) { update(path) { $0.autoFetch = minutes } }

    /// 色ラベルの名前と色（Finder のタグの並び）。
    static let colors: [(name: String, title: String, color: NSColor)] = [
        ("red", L("Red"), .systemRed), ("orange", L("Orange"), .systemOrange), ("yellow", L("Yellow"), .systemYellow),
        ("green", L("Green"), .systemGreen), ("blue", L("Blue"), .systemBlue), ("purple", L("Purple"), .systemPurple),
        ("gray", L("Gray"), .systemGray),
    ]

    static func color(named name: String?) -> NSColor? { colors.first { $0.name == name }?.color }

    /// `path` を `target` の前（nil ならそのグループの末尾）に動かし、グループを `group` にする（ドラッグでの並べ替え）。
    func reorder(_ path: String, before target: String?, group: String?) {
        edit { data in
            if !data.items.contains(where: { $0.path == path }) { data.items.append(Bookmark(path: path)) }
            guard let i = data.items.firstIndex(where: { $0.path == path }), path != target else { return }
            var item = data.items.remove(at: i)
            item.group = group
            let j = target.flatMap { t in data.items.firstIndex { $0.path == t } }
                ?? data.items.lastIndex { $0.group == group }.map { $0 + 1 }
                ?? data.items.endIndex
            data.items.insert(item, at: j)
        }
    }
    func rename(_ path: String, to name: String?) { update(path) { $0.name = name } }
    func move(_ path: String, to group: String?) { update(path) { $0.group = group } }

    func addGroup(_ name: String) {
        edit { data in
            guard !data.groups.contains(name) else { return }
            data.groups.append(name)
        }
    }

    /// グループを消す（中のリポジトリはグループなしに戻す）。
    func removeGroup(_ name: String) {
        edit { data in
            data.groups.removeAll { $0 == name }
            for i in data.items.indices where data.items[i].group == name { data.items[i].group = nil }
        }
    }

    func addScanRoot(_ path: String) {
        edit { data in
            guard !data.scanRoots.contains(where: { $0.path == path }) else { return }
            data.scanRoots.append(ScanRoot(path: path))
        }
        rescan()
        watchScanRoots()
    }

    func removeScanRoot(_ path: String) {
        scanned[path] = nil
        edit { $0.scanRoots.removeAll { $0.path == path } }
        watchScanRoots()
    }

    func setDepth(_ depth: Int, of path: String) {
        edit { data in
            guard let i = data.scanRoots.firstIndex(where: { $0.path == path }) else { return }
            data.scanRoots[i].depth = depth
        }
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
        // SSH 先のエンジンを用意するための場所（アプリの中の、SSH 先に送るエンジン）。
        let daemon = Daemon.path, daemons = Daemon.bundledDirectory
        Task {
            let results = await Task.detached(priority: .utility) {
                // SSH 先は 1 つずつ時間がかかるので、フォルダごとに並べて探す。
                await withTaskGroup(of: (String, Result<[String], Error>).self) { group in
                    for root in roots {
                        group.addTask {
                            if RepositoryLocation.isRemote(root.path) {
                                return (root.path, Result {
                                    try RemoteScan.findRepositories(
                                        location: root.path, depth: root.depth, excludes: root.excludes, daemon: daemon, daemons: daemons,
                                        hostOptions: Settings.hostOptionsJSON(for: root.path))
                                })
                            }
                            return (root.path, .success(Self.scan(root)))
                        }
                    }
                    var all: [(String, Result<[String], Error>)] = []
                    for await result in group { all.append(result) }
                    return all
                }
            }.value
            var found: [String: [String]] = [:]
            var errors: [String: String] = [:]
            for (path, result) in results {
                switch result {
                case let .success(list): found[path] = list
                case let .failure(error):
                    // 探せなかったら、前に見つけたものを残す（つながらないだけで消えたわけではない）。
                    found[path] = scanned[path] ?? []
                    errors[path] = "\(error)"
                }
            }
            scanned = found
            scanErrors = errors
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
            onRowChange?(path)
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
                onRowChange?(path)
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

    /// 見つけたリポジトリとスキャンするフォルダの、リンクを解いたパス（解くのはファイルシステムを引くので、
    /// 探し直したときだけ作り直す）。
    private var realScanned: (key: [String: [String]], known: Set<String>, roots: [ScanRoot])?

    private func scanRootsChanged(_ paths: [String]) {
        // FSEvents のパスはリンクを解いた形で来る。
        if realScanned?.key != scanned {
            realScanned = (scanned, Set(data.scanRoots.flatMap { scanned[$0.path] ?? [] }.map(Self.real)),
                           data.scanRoots.map { root in
                               var root = root
                               root.path = Self.real(root.path)
                               return root
                           })
        }
        guard let (_, known, roots) = realScanned else { return }
        let relevant = paths.contains { raw in
            let path = raw.hasSuffix("/") ? String(raw.dropLast()) : raw
            // 探す深さより下（ビルドの出力など、ほとんどの知らせ）は、先に安く捨てる。
            guard let root = roots.first(where: { path == $0.path || path.hasPrefix($0.path + "/") }) else { return false }
            let parts = path.dropFirst(root.path.count).split(separator: "/").map(String.init)
            guard parts.count < root.depth, !parts.contains(where: { $0.hasPrefix(".") || root.excludes.contains($0) })
            else { return false }
            // 見つけたリポジトリの中（そのもの以外）の変化は、リポジトリの増減ではない。上のフォルダを順に引く。
            var parent = (path as NSString).deletingLastPathComponent
            while parent.count > root.path.count {
                if known.contains(parent) { return false }
                parent = (parent as NSString).deletingLastPathComponent
            }
            return true
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
