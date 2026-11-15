import AppKit

/// 入力欄のあるシート（NSAlert）の中身を、リモート・SSH のシートと同じ形にする: ラベルを左に、欄を右に並べ、
/// 欠かせない欄が空のあいだは実行のボタンを押せなくする（空のまま押して何も起きない、をなくす）。
@MainActor
enum AlertForm {
    /// 1 行の入力欄。`placeholder` は欄の中の薄い文字（任意の欄の「（任意）」や、空のときに使う既定の値）。
    static func field(_ value: String = "", placeholder: String = "") -> NSTextField {
        let field = NSTextField(string: value)
        field.placeholderString = placeholder
        // 表の中でも、幅が文字の分に縮まないように。
        field.widthAnchor.constraint(equalToConstant: 300).isActive = true
        return field
    }

    /// ラベルと欄の表。欄でない行（チェックボックスなど）はラベルを空にすると、欄の列にそろえて置く。
    static func grid(_ rows: [(String, NSView)]) -> NSGridView {
        let grid = NSGridView(views: rows.map { label, view in
            [label.isEmpty ? NSGridCell.emptyContentView : NSTextField(labelWithString: label), view]
        })
        grid.rowSpacing = 8
        grid.columnSpacing = 8
        grid.column(at: 0).xPlacement = .trailing
        for i in 0..<grid.numberOfRows { grid.row(at: i).rowAlignment = .firstBaseline }
        grid.frame.size = grid.fittingSize
        return grid
    }

    /// `required` のどれかが空のあいだ、`button` を押せなくする。返すものは、シートを閉じるまで持っておく。
    static func require(_ required: [NSTextField], for button: NSButton) -> [NSObjectProtocol] {
        let update = ClosureTarget {
            button.isEnabled = required.allSatisfy { !$0.stringValue.trimmingCharacters(in: .whitespaces).isEmpty }
        }
        update.fire()
        return required.map { field in
            NotificationCenter.default.addObserver(forName: NSControl.textDidChangeNotification, object: field, queue: .main) { _ in
                MainActor.assumeIsolated { update.fire() }
            }
        }
    }

    /// `require` で付けた見張りを外す（シートを閉じたとき）。
    static func release(_ observers: [NSObjectProtocol]) {
        observers.forEach(NotificationCenter.default.removeObserver)
    }
}
