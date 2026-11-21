import CoreGraphics
import Testing

@testable import AmonHen

/// History の行の札の置き方（GTK4 版の `labels_fit_before_the_limit_and_fold_the_rest` と同じ場合）。
@MainActor
struct LabelFitTests {
    private func place(_ widths: [CGFloat], count: Int, maxX: CGFloat) -> [LabelFit.Slot] {
        LabelFit.place(widths: widths, count: count, start: 0, maxX: maxX, rest: { _ in 20 })
    }

    private func name(_ i: Int, _ x: CGFloat, _ width: CGFloat) -> LabelFit.Slot {
        LabelFit.Slot(content: .name(i), x: x, width: width)
    }

    private func rest(_ n: Int, _ x: CGFloat) -> LabelFit.Slot {
        LabelFit.Slot(content: .rest(n), x: x, width: 20)
    }

    @Test func placesLabelsThatFit() {
        #expect(place([30, 30], count: 2, maxX: 100) == [name(0, 0, 30), name(1, 34, 30)])
    }

    @Test func foldsLabelsAfterTheThirdIntoRest() {
        #expect(
            place([10, 10, 10], count: 5, maxX: 100) == [name(0, 0, 10), name(1, 14, 10), name(2, 28, 10), rest(2, 42)])
    }

    @Test func foldsALabelThatDoesNotFitInsteadOfDroppingIt() {
        #expect(place([30, 60], count: 2, maxX: 80) == [name(0, 0, 30), rest(1, 34)])
    }

    @Test func clipsALongFirstLabelAndKeepsRoomForRest() {
        #expect(place([300], count: 1, maxX: 100) == [name(0, 0, 100)])
        #expect(place([300, 30], count: 2, maxX: 100) == [name(0, 0, 76), rest(1, 80)])
    }

    @Test func keepsTheMinimumWidthWithAlmostNoRoom() {
        #expect(place([300], count: 1, maxX: 10) == [name(0, 0, LabelFit.minimum)])
    }

    @Test(arguments: [50, 80, 120, 200] as [CGFloat])
    func restStaysWithinTheLimit(maxX: CGFloat) {
        let slots = place([40, 40, 40], count: 6, maxX: maxX)
        let last = slots.last!
        #expect(last.x + last.width <= max(maxX, LabelFit.minimum + 4 + 20), "\(slots)")
        // 名前を出した札と「+N」で、札の総数になる。
        let shown = slots.reduce(0) { n, slot in
            switch slot.content {
            case .name: n + 1
            case .rest(let k): n + k
            }
        }
        #expect(shown == 6)
    }
}
