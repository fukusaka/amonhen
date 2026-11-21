import Foundation
import Testing

@testable import AmonHen

/// 最近開いたもの（共有の `recent.json`）: 開いたものを先頭に、前の記録（UserDefaults）は一度だけ写す。
@MainActor
struct RecentFileTests {
    private func temporary() throws -> (URL, UserDefaults, String) {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let suite = "RecentFileTests.\(UUID().uuidString)"
        return (dir, UserDefaults(suiteName: suite)!, suite)
    }

    @Test func importsTheOldRecordsOnceAndTouches() throws {
        let (dir, defaults, suite) = try temporary()
        defer {
            try? FileManager.default.removeItem(at: dir)
            defaults.removePersistentDomain(forName: suite)
        }
        defaults.set(["/w/b", "/w/a"], forKey: "repositories")
        defaults.set(["/w/b": 20.0], forKey: "repositoryOpenedAt")
        let file = RecentFile.location(settings: dir.appendingPathComponent("amonhen/settings.json").path)
        #expect(file == dir.appendingPathComponent("amonhen/recent.json"))

        let recent = RecentFile(file: file, defaults: defaults)
        #expect(recent.paths == ["/w/b", "/w/a"])
        #expect(recent.openedAt == ["/w/b": 20.0])
        recent.touch("/w/a")
        #expect(recent.paths == ["/w/a", "/w/b"])
        #expect((recent.openedAt["/w/a"] ?? 0) > 1e9)

        // もうファイルがあれば、前の記録は写し直さない。
        defaults.set(["/w/c"], forKey: "repositories")
        let again = RecentFile(file: file, defaults: defaults)
        #expect(again.paths == ["/w/a", "/w/b"])
        #expect(!recent.reloadIfChanged(), "nothing changed")
        again.remove(["/w/b"])
        #expect(recent.reloadIfChanged(), "another writer's change is noticed")
        #expect(recent.paths == ["/w/a"])
        #expect(!recent.reloadIfChanged())
    }

    @Test func keepsUnknownKeysAndLeavesABrokenFile() throws {
        let (dir, defaults, suite) = try temporary()
        defer {
            try? FileManager.default.removeItem(at: dir)
            defaults.removePersistentDomain(forName: suite)
        }
        let file = dir.appendingPathComponent("recent.json")
        try Data(#"{"other":1,"repositories":[{"path":"/w/a","color":"blue"},{"openedAt":5}]}"#.utf8).write(to: file)
        let recent = RecentFile(file: file, defaults: defaults)
        #expect(recent.paths == ["/w/a"])
        recent.touch("/w/b")
        let object = try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any]
        #expect(object?["other"] as? Int == 1)
        let a = (object?["repositories"] as? [[String: Any]])?.first { $0["path"] as? String == "/w/a" }
        #expect(a?["color"] as? String == "blue")

        try Data("[".utf8).write(to: file)
        recent.touch("/w/c")
        #expect(try String(contentsOf: file, encoding: .utf8) == "[", "a broken file isn't overwritten")
    }
}
