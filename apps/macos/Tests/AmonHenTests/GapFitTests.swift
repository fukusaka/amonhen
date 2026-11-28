import CoreGraphics
import Testing

@testable import AmonHen

/// Working Copy の差分の隙間の行の札とボタンの置き方（GTK4 版の `gap_fit_*` と同じ場合）。
@MainActor
struct GapFitTests {
    private func place(right: CGFloat, chips: [CGFloat] = [40, 40, 30]) -> GapFit.Result {
        GapFit.place(left: 0, center: 300, right: right, pill: 100, shortPill: 30, chips: chips)
    }

    @Test func centersThePillAndPlacesEveryButtonWhenWide() {
        let fit = place(right: 1000)
        #expect(fit.pillX == 250)
        #expect(!fit.short)
        #expect(fit.chips.map(\.index) == [0, 1, 2])
        #expect(!fit.overflows)
    }

    @Test func shiftsThePillLeftSoItDoesNotReachTheHunkActions() {
        let fit = place(right: 320)
        #expect(fit.pillX == 212)
        #expect(fit.pillX! + 100 <= 320 - GapFit.margin)
        #expect(fit.chips.isEmpty)
        #expect(fit.overflows)
    }

    @Test func overflowsWhenSomeButtonsDoNotFit() {
        let fit = place(right: 460)
        #expect(fit.chips.map(\.index) == [0, 1])
        #expect(fit.chips.allSatisfy { $0.x + $0.width < 460 - GapFit.margin })
        #expect(fit.overflows)
    }

    @Test func usesTheShortPillWhenTheFullOneDoesNotFit() {
        let fit = place(right: 60)
        #expect(fit.pillX == 22)
        #expect(fit.short)
        #expect(fit.overflows)
    }

    @Test func dropsThePillWhenEvenTheShortOneDoesNotFit() {
        let fit = place(right: 30)
        #expect(fit.pillX == nil)
        #expect(fit.overflows)
        #expect(!GapFit.place(left: 0, center: 300, right: 30, pill: 100, shortPill: nil, chips: []).overflows)
    }

    @Test func doesNotOverflowWithoutButtons() {
        #expect(!place(right: 320, chips: []).overflows)
    }
}
