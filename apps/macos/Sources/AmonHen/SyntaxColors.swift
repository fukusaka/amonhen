import AmonhenKit
import AppKit
import CAmonhen

/// 構文の種類ごとの色（差分・マージエディタで共通）。明るい・暗い外観で変わる。
enum SyntaxColors {
    /// `AmonhenSyntaxSpan` の種類の定数を添字にした 16 色。
    static var palette: [NSColor] {
        var colors = [NSColor](repeating: .labelColor, count: 16)
        let pairs: [(Int32, NSColor)] = [
            (AmonhenSyntaxSpan_KEYWORD, .systemPink),
            (AmonhenSyntaxSpan_STRING, .systemRed),
            (AmonhenSyntaxSpan_COMMENT, .systemGray),
            (AmonhenSyntaxSpan_NUMBER, .systemPurple),
            (AmonhenSyntaxSpan_FUNCTION, .systemBlue),
            (AmonhenSyntaxSpan_TYPE, .systemTeal),
            (AmonhenSyntaxSpan_CONSTANT, .systemPurple),
            (AmonhenSyntaxSpan_PROPERTY, .systemCyan),
            (AmonhenSyntaxSpan_ATTRIBUTE, .systemOrange),
            (AmonhenSyntaxSpan_TAG, .systemPink),
            (AmonhenSyntaxSpan_OPERATOR, .labelColor),
            (AmonhenSyntaxSpan_VARIABLE, .systemPink),
            (AmonhenSyntaxSpan_ESCAPE, .systemOrange),
            (AmonhenSyntaxSpan_MODULE, .systemBrown),
        ]
        for (kind, color) in pairs { colors[Int(kind)] = color }
        return colors
    }
}
