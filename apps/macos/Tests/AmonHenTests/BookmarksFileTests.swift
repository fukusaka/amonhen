import Foundation
import Testing

@testable import AmonHen

/// ブックマークの置き場所: 設定と同じフォルダ。そこになく前の置き場所にあれば、一度だけ写す。
struct BookmarksFileTests {
    private func temporary() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    @Test func copiesTheOldFileOnceAndKeepsIt() throws {
        let dir = try temporary()
        defer { try? FileManager.default.removeItem(at: dir) }
        let old = dir.appendingPathComponent("old/bookmarks.json")
        try FileManager.default.createDirectory(at: old.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(#"{"items":[{"path":"/w/a"}]}"#.utf8).write(to: old)
        let settings = dir.appendingPathComponent("config/amonhen/settings.json").path
        let file = Bookmarks.sharedFile(settings: settings, old: old)
        #expect(file.path == dir.appendingPathComponent("config/amonhen/bookmarks.json").path)
        #expect(try String(contentsOf: file, encoding: .utf8).contains("/w/a"))
        #expect(FileManager.default.fileExists(atPath: old.path), "the old file stays")
        // 新しい場所にあれば、写さない（前のものが変わっても上書きしない）。
        try Data(#"{"items":[]}"#.utf8).write(to: old)
        _ = Bookmarks.sharedFile(settings: settings, old: old)
        #expect(try String(contentsOf: file, encoding: .utf8).contains("/w/a"))
    }

    @Test func noFilesAnywhereCopiesNothing() throws {
        let dir = try temporary()
        defer { try? FileManager.default.removeItem(at: dir) }
        let old = dir.appendingPathComponent("old/bookmarks.json")
        let file = Bookmarks.sharedFile(settings: dir.appendingPathComponent("config/settings.json").path, old: old)
        #expect(!FileManager.default.fileExists(atPath: file.path))
        // 設定の場所が決められなければ、前の置き場所。
        #expect(Bookmarks.sharedFile(settings: nil, old: old) == old)
    }
}
