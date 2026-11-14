import AmonhenKit
import AppKit
import CAmonhen

extension Notification.Name {
    /// 構文の色のテーマ（明るい・暗い外観それぞれ）を変えた、テーマを読み込んだ・消した。
    static let syntaxThemeChanged = Notification.Name("syntaxThemeChanged")
}

/// 構文の種類ごとの色（差分・マージエディタで共通）。明るい・暗い外観で、それぞれ選んだテーマの色になる。
enum SyntaxColors {
    /// `AmonhenSyntaxSpan` の種類の定数を添字にした 16 色。外観に合わせて変わる色（描くときの外観で決まる）。
    static var palette: [NSColor] {
        let light = SyntaxTheme.named(Settings.syntaxThemeLight) ?? .system
        let dark = SyntaxTheme.named(Settings.syntaxThemeDark) ?? .system
        return (0..<16).map { i in
            let l = light.color(Int32(i)), d = dark.color(Int32(i))
            if l == d { return l }
            return NSColor(name: nil) { appearance in
                appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? d : l
            }
        }
    }
}

/// 構文の色のテーマ。種類ごとの色（なければ文字の色）。
struct SyntaxTheme: Codable, Hashable {
    var name: String
    /// 暗い外観向けか（一覧で明るい・暗いに分けて出す）。
    var dark: Bool
    /// 種類（`keyword` など）→ `#rrggbb`。
    var colors: [String: String]

    /// 種類の名前と `AmonhenSyntaxSpan` の定数。
    static let kinds: [(String, Int32)] = [
        ("keyword", AmonhenSyntaxSpan_KEYWORD), ("string", AmonhenSyntaxSpan_STRING), ("comment", AmonhenSyntaxSpan_COMMENT),
        ("number", AmonhenSyntaxSpan_NUMBER), ("function", AmonhenSyntaxSpan_FUNCTION), ("type", AmonhenSyntaxSpan_TYPE),
        ("constant", AmonhenSyntaxSpan_CONSTANT), ("property", AmonhenSyntaxSpan_PROPERTY),
        ("attribute", AmonhenSyntaxSpan_ATTRIBUTE), ("tag", AmonhenSyntaxSpan_TAG), ("operator", AmonhenSyntaxSpan_OPERATOR),
        ("variable", AmonhenSyntaxSpan_VARIABLE), ("escape", AmonhenSyntaxSpan_ESCAPE), ("module", AmonhenSyntaxSpan_MODULE),
    ]

    /// これまでの色（システムの色。明るい・暗いで OS が変える）。
    static let system = SyntaxTheme(name: "System", dark: false, colors: [:])

    func color(_ kind: Int32) -> NSColor {
        if self == .system { return Self.systemColors[kind] ?? .labelColor }
        guard let name = Self.kinds.first(where: { $0.1 == kind })?.0 else { return .labelColor }
        return colors[name].flatMap(NSColor.init(hex:)) ?? .labelColor
    }

    private static let systemColors: [Int32: NSColor] = [
        AmonhenSyntaxSpan_KEYWORD: .systemPink, AmonhenSyntaxSpan_STRING: .systemRed, AmonhenSyntaxSpan_COMMENT: .systemGray,
        AmonhenSyntaxSpan_NUMBER: .systemPurple, AmonhenSyntaxSpan_FUNCTION: .systemBlue, AmonhenSyntaxSpan_TYPE: .systemTeal,
        AmonhenSyntaxSpan_CONSTANT: .systemPurple, AmonhenSyntaxSpan_PROPERTY: .systemCyan,
        AmonhenSyntaxSpan_ATTRIBUTE: .systemOrange, AmonhenSyntaxSpan_TAG: .systemPink, AmonhenSyntaxSpan_OPERATOR: .labelColor,
        AmonhenSyntaxSpan_VARIABLE: .systemPink, AmonhenSyntaxSpan_ESCAPE: .systemOrange, AmonhenSyntaxSpan_MODULE: .systemBrown,
    ]

    private static func theme(_ name: String, dark: Bool, _ hex: [String]) -> SyntaxTheme {
        SyntaxTheme(name: name, dark: dark, colors: Dictionary(uniqueKeysWithValues: zip(kinds.map(\.0), hex)))
    }

    /// 内蔵のテーマ（どれも MIT などで公開された配色）。並びは kinds の順。
    static let builtIn: [SyntaxTheme] = [
        theme("Solarized Light", dark: false, ["#859900", "#2aa198", "#93a1a1", "#d33682", "#268bd2", "#b58900", "#cb4b16",
                                                "#268bd2", "#b58900", "#268bd2", "#657b83", "#b58900", "#dc322f", "#6c71c4"]),
        theme("Solarized Dark", dark: true, ["#859900", "#2aa198", "#586e75", "#d33682", "#268bd2", "#b58900", "#cb4b16",
                                              "#268bd2", "#b58900", "#268bd2", "#839496", "#b58900", "#dc322f", "#6c71c4"]),
        theme("Gruvbox Light", dark: false, ["#9d0006", "#79740e", "#928374", "#8f3f71", "#79740e", "#b57614", "#8f3f71",
                                              "#076678", "#427b58", "#9d0006", "#af3a03", "#076678", "#af3a03", "#427b58"]),
        theme("Gruvbox Dark", dark: true, ["#fb4934", "#b8bb26", "#928374", "#d3869b", "#b8bb26", "#fabd2f", "#d3869b",
                                            "#83a598", "#8ec07c", "#fb4934", "#fe8019", "#83a598", "#fe8019", "#8ec07c"]),
        theme("Tomorrow", dark: false, ["#8959a8", "#718c00", "#8e908c", "#f5871f", "#4271ae", "#c99e00", "#f5871f",
                                         "#c82829", "#f5871f", "#c82829", "#3e999f", "#c82829", "#f5871f", "#3e999f"]),
        theme("Tomorrow Night", dark: true, ["#b294bb", "#b5bd68", "#969896", "#de935f", "#81a2be", "#f0c674", "#de935f",
                                              "#cc6666", "#de935f", "#cc6666", "#8abeb7", "#cc6666", "#de935f", "#8abeb7"]),
        theme("Nord", dark: true, ["#81a1c1", "#a3be8c", "#616e88", "#b48ead", "#88c0d0", "#8fbcbb", "#b48ead",
                                    "#88c0d0", "#d08770", "#81a1c1", "#81a1c1", "#d8dee9", "#ebcb8b", "#8fbcbb"]),
    ]

    /// 読み込んだテーマ（VSCode のテーマから。`importedSyntaxThemes` に JSON で置く）。
    static var imported: [SyntaxTheme] {
        get {
            UserDefaults.standard.data(forKey: "importedSyntaxThemes")
                .flatMap { try? JSONDecoder().decode([SyntaxTheme].self, from: $0) } ?? []
        }
        set {
            UserDefaults.standard.set(try? JSONEncoder().encode(newValue), forKey: "importedSyntaxThemes")
            NotificationCenter.default.post(name: .syntaxThemeChanged, object: nil)
        }
    }

    static var all: [SyntaxTheme] { [.system] + builtIn + imported }

    static func named(_ name: String) -> SyntaxTheme? { all.first { $0.name == name } }
}

extension SyntaxTheme {
    /// VSCode のテーマの JSON（コメント・末尾のカンマあり、`include` で親を読む）から、種類ごとの色を拾う。
    /// TextMate のスコープを種類に当てはめるので近似になる。
    static func importVSCode(_ url: URL) throws -> SyntaxTheme {
        let json = try loadVSCode(url, depth: 0)
        let type = json["type"] as? String
        let background = (json["colors"] as? [String: Any])?["editor.background"] as? String
        let dark = type.map { $0 != "light" && $0 != "hcLight" }
            ?? background.flatMap(NSColor.init(hex:)).map { ($0.usingColorSpace(.sRGB)?.brightnessComponent ?? 0) < 0.5 }
            ?? true
        // スコープ → 色（後に書いたものが勝つ）。
        var scopes: [String: String] = [:]
        for entry in json["tokenColors"] as? [[String: Any]] ?? [] {
            guard let color = (entry["settings"] as? [String: Any])?["foreground"] as? String else { continue }
            let names: [String] = (entry["scope"] as? [String])
                ?? (entry["scope"] as? String)?.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) } ?? []
            // 「a b」（子孫の指定）は最後のスコープだけで見る。
            for name in names { scopes[String(name.split(separator: " ").last ?? "")] = String(color.prefix(7)) }
        }
        // 種類ごとに、当てはめるスコープを優先の順に。
        let targets: [String: [String]] = [
            "keyword": ["keyword.control", "keyword", "storage.modifier", "storage"],
            "string": ["string.quoted", "string"],
            "comment": ["comment.line", "comment"],
            "number": ["constant.numeric"],
            "function": ["entity.name.function", "support.function", "meta.function-call"],
            "type": ["entity.name.type", "support.type", "storage.type", "entity.name.class"],
            "constant": ["constant.language", "variable.other.constant", "constant"],
            "property": ["variable.other.property", "support.type.property-name", "variable.other.object.property"],
            "attribute": ["entity.other.attribute-name", "meta.attribute"],
            "tag": ["entity.name.tag"],
            "operator": ["keyword.operator"],
            "variable": ["variable.other", "variable"],
            "escape": ["constant.character.escape"],
            "module": ["entity.name.namespace", "entity.name.module", "support.module", "entity.name.type.module"],
        ]
        var colors: [String: String] = [:]
        for (kind, wanted) in targets {
            for target in wanted {
                // いちばん詳しく当たるスコープ（target そのものか、その親）。
                let hit = scopes.keys
                    .filter { target == $0 || target.hasPrefix($0 + ".") }
                    .max { $0.count < $1.count }
                if let hit, let color = scopes[hit] {
                    colors[kind] = color
                    break
                }
            }
        }
        let name = json["name"] as? String ?? url.deletingPathExtension().lastPathComponent
        return SyntaxTheme(name: name, dark: dark, colors: colors)
    }

    private static func loadVSCode(_ url: URL, depth: Int) throws -> [String: Any] {
        let text = String(decoding: try Data(contentsOf: url), as: UTF8.self)
        guard let object = try JSONSerialization.jsonObject(with: Data(stripJSONC(text).utf8)) as? [String: Any] else {
            throw CocoaError(.fileReadCorruptFile)
        }
        guard let include = object["include"] as? String, depth < 4 else { return object }
        // 親を先に、子で上書きする（tokenColors はつなげる）。
        var merged = try loadVSCode(url.deletingLastPathComponent().appendingPathComponent(include), depth: depth + 1)
        for (key, value) in object where key != "include" {
            if key == "tokenColors", let mine = value as? [Any], let parent = merged[key] as? [Any] {
                merged[key] = parent + mine
            } else if key == "colors", let mine = value as? [String: Any], let parent = merged[key] as? [String: Any] {
                merged[key] = parent.merging(mine) { $1 }
            } else {
                merged[key] = value
            }
        }
        return merged
    }

    /// JSONC（`//`・`/* */` のコメントと、閉じる前のカンマ）をふつうの JSON にする。文字列の中は触らない。
    static func stripJSONC(_ text: String) -> String {
        var out = ""
        var chars = Array(text)[...]
        var inString = false
        while let c = chars.first {
            chars = chars.dropFirst()
            if inString {
                out.append(c)
                if c == "\\", let next = chars.first {
                    out.append(next)
                    chars = chars.dropFirst()
                } else if c == "\"" {
                    inString = false
                }
                continue
            }
            if c == "\"" {
                inString = true
                out.append(c)
            } else if c == "/", chars.first == "/" {
                while let n = chars.first, n != "\n" { chars = chars.dropFirst() }
            } else if c == "/", chars.first == "*" {
                chars = chars.dropFirst()
                while let n = chars.first, !(n == "*" && chars.dropFirst().first == "/") { chars = chars.dropFirst() }
                chars = chars.dropFirst(2)
            } else if c == "," {
                // 次の空白でない文字が閉じかっこなら落とす。
                let rest = chars.drop { $0.isWhitespace }
                if rest.first != "}", rest.first != "]" { out.append(c) }
            } else {
                out.append(c)
            }
        }
        return out
    }
}

extension NSColor {
    /// `#rrggbb` から。
    convenience init?(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("#") { s.removeFirst() }
        guard s.count == 6 || s.count == 8, let v = UInt64(s, radix: 16) else { return nil }
        let rgb = s.count == 8 ? v >> 8 : v
        let a = s.count == 8 ? CGFloat(v & 0xff) / 255 : 1
        self.init(
            srgbRed: CGFloat((rgb >> 16) & 0xff) / 255, green: CGFloat((rgb >> 8) & 0xff) / 255,
            blue: CGFloat(rgb & 0xff) / 255, alpha: a)
    }

    var hex: String {
        let c = usingColorSpace(.sRGB) ?? self
        return String(format: "#%02x%02x%02x", Int(round(c.redComponent * 255)), Int(round(c.greenComponent * 255)),
                      Int(round(c.blueComponent * 255)))
    }
}
