import AmonhenKit
import Foundation

/// 最近開いたリポジトリのファイル（`recent.json`）。設定と同じフォルダ（`~/.config/amonhen/`）に置き、GTK4 のアプリ・
/// TUI も同じものを読み書きする（`amonhen_model::recent`）。形は `{"repositories": [{"path": …, "openedAt": 秒}, …]}`
/// で新しい順。書くときは隣の `recent.lock` を取ってから読み直す（ほかの Amon Hen の変更を消さない）。知らないキーは
/// 残す。前の記録（UserDefaults の `repositories`・`repositoryOpenedAt`）しかなければ、初めに写す（前のものは残す）。
final class RecentFile {
    static let shared = RecentFile(file: location(settings: MachineGit.thisMac?.file), defaults: .standard)

    let file: URL
    private var entries: [[String: Any]] = []
    /// 最後に読んだか書いたときの、ファイルの更新時刻（ほかのプロセスが書いたかを見分ける）。
    private var stamp: Date?

    /// 設定のファイル `settings` と同じフォルダの `recent.json`。設定の場所が決められなければ Application Support の下。
    nonisolated static func location(settings: String?) -> URL {
        guard let settings else {
            return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("Amon Hen").appendingPathComponent("recent.json")
        }
        return URL(fileURLWithPath: settings).deletingLastPathComponent().appendingPathComponent("recent.json")
    }

    init(file: URL, defaults: UserDefaults) {
        self.file = file
        importLegacy(from: defaults)
        refresh()
    }

    /// 最近開いた場所（新しい順）。
    var paths: [String] {
        refresh()
        return entries.compactMap(Self.path)
    }

    /// 最後に開いた日時（場所 → UNIX 時間の秒）。この記録を始める前に開いたものはない。
    var openedAt: [String: Double] {
        refresh()
        var out: [String: Double] = [:]
        for entry in entries {
            if let path = Self.path(entry), let time = (entry["openedAt"] as? NSNumber)?.doubleValue, out[path] == nil {
                out[path] = time
            }
        }
        return out
    }

    /// 開いたことを覚える: 先頭に置き、開いた日時を今にする。
    func touch(_ path: String) {
        let now = Date().timeIntervalSince1970
        edit { list in
            var entry = list.first { Self.path($0) == path } ?? [:]
            list.removeAll { Self.path($0) == path }
            entry["path"] = path
            entry["openedAt"] = now
            list.insert(entry, at: 0)
        }
    }

    /// 一覧から外す。
    func remove(_ paths: Set<String>) {
        edit { list in list.removeAll { Self.path($0).map(paths.contains) ?? false } }
    }

    private nonisolated static func path(_ entry: [String: Any]) -> String? {
        (entry["path"] as? String).flatMap { $0.isEmpty ? nil : $0 }
    }

    private func modified() -> Date? {
        (try? FileManager.default.attributesOfItem(atPath: file.path))?[.modificationDate] as? Date
    }

    /// 読めない（JSON のオブジェクトでない）ときは nil。ないときは空。
    private func readObject() -> [String: Any]? {
        guard let data = try? Data(contentsOf: file) else { return [:] }
        if data.allSatisfy({ $0 == 0x20 || $0 == 0x0A || $0 == 0x0D || $0 == 0x09 }) { return [:] }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }

    /// ほかのプロセスが書き換えていたら読み直す。
    private func refresh() {
        let now = modified()
        guard now != stamp else { return }
        stamp = now
        if let object = readObject() {
            entries = object["repositories"] as? [[String: Any]] ?? []
        }
    }

    /// ロックを取ってから読み直し、`proceed(ファイルがあるか)` なら `change` を当てて書く。読めないファイルは書き換えない。
    private func edit(if proceed: (Bool) -> Bool = { _ in true }, _ change: (inout [[String: Any]]) -> Void) {
        let directory = file.deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let lock = open(directory.appendingPathComponent("recent.lock").path, O_CREAT | O_RDWR, 0o644)
        if lock >= 0 { flock(lock, LOCK_EX) }
        defer {
            if lock >= 0 {
                flock(lock, LOCK_UN)
                close(lock)
            }
        }
        let exists = FileManager.default.fileExists(atPath: file.path)
        guard proceed(exists), var object = readObject() else { return }
        var list = object["repositories"] as? [[String: Any]] ?? []
        change(&list)
        object["repositories"] = list
        do {
            let data = try JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
            try (data + Data("\n".utf8)).write(to: file, options: .atomic)
            entries = list
            stamp = modified()
        } catch {
            NSLog("最近開いたものを保存できない: \(error)")
        }
    }

    /// まだファイルがなければ、前の記録（UserDefaults）を写す。
    private func importLegacy(from defaults: UserDefaults) {
        let paths = defaults.stringArray(forKey: "repositories") ?? []
        guard !paths.isEmpty else { return }
        let opened = defaults.dictionary(forKey: "repositoryOpenedAt") ?? [:]
        edit(if: { !$0 }) { list in
            list = paths.map { path in
                var entry: [String: Any] = ["path": path]
                if let time = (opened[path] as? NSNumber)?.doubleValue { entry["openedAt"] = time }
                return entry
            }
        }
    }
}
