import AmonhenKit
import AppKit

/// サイドバーのタグの行の右端の印（どのリモートにもない・違うコミットを指すリモートがある）を押したときの吹き出し
/// （ブランチの吹き出しと同じ作り）。タグはブランチと違って手元にリモートの写しがないので、どこから分かったか
/// （fetch のときに読んだ）と、リモートごとの様子、次にできること（push・置き換え）を出す。
final class TagStatusPopover: NSViewController {
    struct Row {
        let remote: String
        /// まだ読んでいなければ nil。
        let state: TagRemotes.State?
        /// 読んだ時刻。まだ読んでいなければ nil。
        let checked: Date?
    }

    private let item: RefItem
    /// 行の印（SF Symbols の名前・見出し・色）。
    private let mark: (String, String, NSColor)
    private let rows: [Row]
    private let onPush: (String) -> Void
    private let onReplace: (String) -> Void

    init(item: RefItem, mark: (String, String, NSColor), rows: [Row],
         onPush: @escaping (String) -> Void, onReplace: @escaping (String) -> Void) {
        self.item = item
        self.mark = mark
        self.rows = rows
        self.onPush = onPush
        self.onReplace = onReplace
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let (symbol, title, color) = mark
        let different = rows.contains { $0.state == .different }
        let icon = NSImageView()
        icon.image = CommitHeaderRenderer.symbolImage(symbol, color: color, size: 18)
        let titleField = NSTextField(labelWithString: title)
        titleField.font = .systemFont(ofSize: 14, weight: .semibold)
        let header = NSStackView(views: [icon, titleField])
        header.spacing = 6

        let explanation = different
            ? L("A tag with the same name points to another commit there. Fetch doesn’t overwrite your tag, so the two stay apart until one of them is replaced.")
            : L("Tags have no remote copies here the way branches do, so this was read from each remote at the last Fetch. Push doesn’t send tags; push this one to share it.")
        let explanationField = NSTextField(wrappingLabelWithString: explanation)
        explanationField.font = .systemFont(ofSize: 12)
        explanationField.textColor = .secondaryLabelColor
        explanationField.preferredMaxLayoutWidth = 380

        func value(_ s: String, mono: Bool = false, color: NSColor = .labelColor) -> NSTextField {
            let f = NSTextField(wrappingLabelWithString: s)
            f.isSelectable = true
            f.font = mono ? .monospacedSystemFont(ofSize: 11, weight: .regular) : .systemFont(ofSize: 12)
            f.textColor = color
            f.preferredMaxLayoutWidth = 290
            return f
        }
        var grid: [(String, NSView)] = [(L("Tag"), value(item.name))]
        if let target = item.target {
            let hex = target.map { String(format: "%02x", $0) }.joined()
            grid.append((L("Commit"), value(String(hex.prefix(12)), mono: true)))
        }
        for row in rows {
            let (text, color): (String, NSColor) = switch row.state {
            case .same?: (L("Has it"), .labelColor)
            case .different?: (L("Has this name on another commit"), .systemOrange)
            case .missing?: (L("Doesn’t have it"), .secondaryLabelColor)
            case nil: (L("Not checked yet (Fetch reads it)"), .tertiaryLabelColor)
            }
            let when = row.checked.map { " · " + L("read %@", Self.relative.localizedString(for: $0, relativeTo: Date())) } ?? ""
            grid.append((row.remote, value(text + when, color: color)))
        }
        let gridView = NSGridView(views: grid.map { label, view in
            let l = NSTextField(labelWithString: label)
            l.font = .systemFont(ofSize: 11, weight: .medium)
            l.textColor = .secondaryLabelColor
            return [l, view]
        })
        gridView.column(at: 0).xPlacement = .trailing
        gridView.rowSpacing = 5
        gridView.columnSpacing = 10
        for i in 0..<gridView.numberOfRows { gridView.row(at: i).rowAlignment = .firstBaseline }

        // 次にできること: ないリモートへ push、違うコミットを指すリモートは置き換え（確かめてから）。
        let buttons = NSStackView()
        buttons.spacing = 8
        for row in rows where row.state == .missing {
            buttons.addArrangedSubview(ClosureButton(title: "Push to \(row.remote)") { [weak self] in
                self?.dismiss(nil)
                self?.onPush(row.remote)
            })
        }
        for row in rows where row.state == .different {
            buttons.addArrangedSubview(ClosureButton(title: L("Replace on %@…", row.remote)) { [weak self] in
                self?.dismiss(nil)
                self?.onReplace(row.remote)
            })
        }
        (buttons.arrangedSubviews.first as? NSButton)?.bezelColor = .controlAccentColor
        for b in buttons.arrangedSubviews { (b as? NSButton)?.controlSize = .small }

        let stack = NSStackView(views: [header, explanationField, gridView]
            + (buttons.arrangedSubviews.isEmpty ? [] : [buttons]))
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        stack.edgeInsets = NSEdgeInsets(top: 14, left: 16, bottom: 14, right: 16)
        stack.widthAnchor.constraint(equalToConstant: 420).isActive = true
        view = stack
    }

    private static let relative: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f
    }()
}

/// 押したら `action` を呼ぶボタン。
private final class ClosureButton: NSButton {
    private var handler: (() -> Void)?

    convenience init(title: String, action: @escaping () -> Void) {
        self.init(frame: .zero)
        self.title = title
        bezelStyle = .push
        handler = action
        target = self
        self.action = #selector(run)
    }

    @objc private func run() { handler?() }
}
