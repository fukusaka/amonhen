import Testing

@testable import AmonHen

/// Working Copy の一覧を読み直した後に選び直す行（GTK4 版の `rows_to_reselect` と同じ場合）。
@MainActor
struct WorkingCopyReselectTests {
    private let staged = 1
    private let unstaged = 0
    private let all = 2

    private func key(_ path: String, _ section: Int?) -> WorkingCopyReselect.Key {
        WorkingCopyReselect.Key(path: path, section: section)
    }

    @Test func keepsTheSameRows() {
        let rows = [key("a", staged), key("a", unstaged), key("b", unstaged)]
        #expect(WorkingCopyReselect.rows(kept: [key("a", unstaged), key("b", unstaged)], in: rows) == [1, 2])
    }

    @Test func followsAFileThatMovedToTheOtherSection() {
        // a をすべて stage した: Unstaged の a はなくなり、Staged の a を選ぶ。
        let rows = [key("a", staged), key("b", unstaged)]
        #expect(WorkingCopyReselect.rows(kept: [key("a", unstaged), key("b", unstaged)], in: rows) == [0, 1])
    }

    @Test func doesNotAddTheOtherSectionOfAPartlyStagedFile() {
        // a は一部だけ stage してある（両方の節にある）。b の hunk を stage しても、Staged の a は選ばない。
        let rows = [key("a", staged), key("b", staged), key("a", unstaged), key("b", unstaged)]
        #expect(WorkingCopyReselect.rows(kept: [key("a", unstaged), key("b", unstaged)], in: rows) == [2, 3])
    }

    @Test func dropsAFileThatIsGone() {
        let rows = [key("b", unstaged)]
        #expect(WorkingCopyReselect.rows(kept: [key("a", unstaged), key("b", unstaged)], in: rows) == [0])
    }

    @Test func withoutSectionsMatchesByPath() {
        let rows = [key("a", nil), key("b", nil), key("c", nil)]
        #expect(WorkingCopyReselect.rows(kept: [key("a", nil), key("c", nil)], in: rows) == [0, 2])
    }

    @Test func firstPrefersTheViewedSection() {
        let rows = [key("a", staged), key("a", unstaged), key("b", unstaged)]
        #expect(WorkingCopyReselect.first(in: rows, side: unstaged) == 1)
        #expect(WorkingCopyReselect.first(in: rows, side: staged) == 0)
    }

    @Test func firstFallsBackToTheFirstRow() {
        #expect(WorkingCopyReselect.first(in: [key("a", staged)], side: unstaged) == 0)
        #expect(WorkingCopyReselect.first(in: [key("a", nil), key("b", nil)], side: unstaged) == 0)
        #expect(WorkingCopyReselect.first(in: [], side: unstaged) == nil)
    }

    @Test func firstKeepsConflictsFirst() {
        let rows = [key("c", nil), key("a", staged), key("b", unstaged)]
        #expect(WorkingCopyReselect.first(in: rows, side: unstaged) == 0)
    }

    @Test func reloadKeepsAll() {
        // 見る側が All なら、選び直した行がどちらの節でも All のまま。
        #expect(WorkingCopyReselect.side(all, all: all, reselected: staged) == all)
        #expect(WorkingCopyReselect.side(all, all: all, reselected: unstaged) == all)
        #expect(WorkingCopyReselect.side(all, all: all, reselected: nil) == all)
    }

    @Test func reloadFollowsAFileThatMovedToTheOtherSection() {
        // Unstaged で見ていた a をすべて stage した: Staged の節の a を選び直し、側も Staged。
        #expect(WorkingCopyReselect.side(unstaged, all: all, reselected: staged) == staged)
        #expect(WorkingCopyReselect.side(staged, all: all, reselected: unstaged) == unstaged)
        // 同じ節に残っていればそのまま。節分けでなければ替えない。
        #expect(WorkingCopyReselect.side(unstaged, all: all, reselected: unstaged) == unstaged)
        #expect(WorkingCopyReselect.side(staged, all: all, reselected: nil) == staged)
    }

    @Test func preferredSectionUnderAllIsTheSelectedRow() {
        // All で Unstaged の a を選んでいた: 一部を stage して両方の節にあっても、Unstaged の a を選び直す。
        let kept = [key("a", unstaged)]
        #expect(WorkingCopyReselect.preferredSection(side: all, all: all, path: "a", kept: kept) == unstaged)
        #expect(WorkingCopyReselect.preferredSection(side: all, all: all, path: "b", kept: kept) == nil)
        // Unstaged・Staged なら見る側の節。
        #expect(WorkingCopyReselect.preferredSection(side: staged, all: all, path: "a", kept: kept) == staged)
    }
}
