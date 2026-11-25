import AmonhenKit
import AppKit
import Testing

@testable import AmonHen

/// Markdown の整形表示の検索: 見えている文字（`<kbd>` も表の中も）を大文字・小文字を区別せずに探し、すべての一致の地を
/// 塗り、次・前へ回る。閉じた `<details>` の中は探さない。
@MainActor @Test func findsVisibleTextInMarkdown() throws {
    let blocks = MarkdownBlock.parse(
        "Press <kbd>Tab</kbd> then tab again.\n\n| key |\n|---|\n| TAB |\n\n"
            + "<details><summary>More</summary>\n\nhidden tab\n\n</details>\n")
    let view = MarkdownView(blocks: blocks, path: "README.md") { _ in nil }
    view.frame = NSRect(x: 0, y: 0, width: 400, height: 300)
    view.layoutSubtreeIfNeeded()
    view.find("tab")
    #expect(view.isFindBarVisible)
    let string = view.text.string as NSString
    #expect(view.matches.map { string.substring(with: $0) } == ["Tab", "tab", "TAB"])
    #expect(view.currentMatch == 0)
    let layout = try #require(view.text.layoutManager)
    for range in view.matches {
        #expect(layout.temporaryAttribute(.backgroundColor, atCharacterIndex: range.location, effectiveRange: nil) != nil)
    }
    view.findPrevious()
    #expect(view.currentMatch == 2)
    view.findNext()
    #expect(view.currentMatch == 0)

    view.hideFindBar()
    #expect(view.matches.isEmpty)
    #expect(layout.temporaryAttribute(.backgroundColor, atCharacterIndex: view.text.string.utf16.count - 2, effectiveRange: nil) == nil)
    view.find("zzz")
    #expect(view.matches.isEmpty && view.currentMatch == nil)
}
