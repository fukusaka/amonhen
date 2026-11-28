import Testing

@testable import AmonHen

/// Space で含める・外した後に選ぶ hunk（GTK4 版の `next_hunk_*` と同じ場合）。
struct NextHunkTests {
    @Test func selectsTheHunkAfterTheToggledOne() {
        #expect(NextHunk.after(rows: [3, 4], hunks: [3, 10, 20]) == 1)
        #expect(NextHunk.after(rows: [11], hunks: [3, 10, 20]) == 2)
    }

    @Test func usesTheLastToggledRowWhenRowsSpanHunks() {
        #expect(NextHunk.after(rows: [4, 12], hunks: [3, 10, 20]) == 2)
    }

    @Test func selectsNothingAfterTheLastHunk() {
        #expect(NextHunk.after(rows: [21, 22], hunks: [3, 10, 20]) == nil)
        #expect(NextHunk.after(rows: [3], hunks: [3]) == nil)
    }

    @Test func selectsNothingWithoutRowsOrHunks() {
        #expect(NextHunk.after(rows: [], hunks: [3, 10]) == nil)
        #expect(NextHunk.after(rows: [5], hunks: []) == nil)
        #expect(NextHunk.after(rows: [1], hunks: [3, 10]) == nil)
    }
}
