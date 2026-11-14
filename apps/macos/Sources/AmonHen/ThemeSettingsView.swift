import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// 窓の背景の設定の欄（Settings > Theme と、リポジトリごとの上書きのシートで同じ）。
struct BackdropSections: View {
    @Binding var settings: BackdropSettings

    var body: some View {
        // 3 つは互いに独立: 窓の後ろを透かす・背景画像を重ねる・ツールバーに色を付ける。
        Section(L("Window Transparency")) {
            Picker(L("See through in"), selection: $settings.style) {
                ForEach(BackdropStyle.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            if settings.style != .system {
                Percent(L("Transparency"), value: $settings.transparency, in: 0.1...0.9)
                Percent(L("Blur"), value: $settings.blur, in: 0...1)
            }
            Note(transparencyNote)
        }
        Section(L("Background Image")) {
            LabeledContent(L("Image")) {
                HStack {
                    Text(settings.image.map { ($0 as NSString).lastPathComponent } ?? L("None"))
                        .foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                    Button(L("Choose…")) { chooseImage() }
                    if settings.image != nil { Button(L("Remove")) { settings.image = nil } }
                }
            }
            if settings.image != nil {
                Picker(L("Position"), selection: $settings.imagePlacement) {
                    ForEach(BackdropImagePlacement.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                Percent(L("Transparency"), value: $settings.imageTransparency, in: 0.5...0.95)
            }
            Note(L("Laid thinly over the whole window, on top of the lists and diffs."))
        }
        Section(L("Toolbar")) {
            Toggle(L("Tint the toolbar"), isOn: Binding(
                get: { settings.toolbarColor != nil },
                set: { settings.toolbarColor = $0 ? (settings.toolbarColor ?? NSColor.controlAccentColor.hex) : nil }))
            if let hex = settings.toolbarColor {
                ColorPicker(L("Toolbar color"), selection: Binding(
                    get: { Color(nsColor: NSColor(hex: hex) ?? .controlAccentColor) },
                    set: { settings.toolbarColor = NSColor($0).hex }), supportsOpacity: false)
                Percent(L("Toolbar tint"), value: $settings.toolbarOpacity, in: 0.1...1)
            }
        }
    }

    private var transparencyNote: String {
        switch settings.style {
        case .system: L("macOS’s standard window. Nothing shows through.")
        case .chrome: L("The desktop and windows behind show through the sidebar and toolbar. The lists and diffs stay opaque.")
        case .single: L("The desktop and windows behind show through the whole window. Changed lines and selections stay opaque.")
        }
    }

    private func chooseImage() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.image]
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        settings.image = url.path
    }
}

/// 割合の滑り棒（右に % を出す）。
private struct Percent: View {
    let title: String
    @Binding var value: Double
    let range: ClosedRange<Double>

    init(_ title: String, value: Binding<Double>, in range: ClosedRange<Double>) {
        self.title = title
        _value = value
        self.range = range
    }

    var body: some View {
        LabeledContent(title) {
            HStack {
                Slider(value: $value, in: range, step: 0.05)
                Text("\(Int((value * 100).rounded()))%").monospacedDigit().frame(width: 40)
            }
        }
    }
}

/// Theme: 窓の背景と構文の色（アプリ全体。リポジトリごとの背景は、リポジトリの一覧の右クリックから）。
struct ThemeSettings: View {
    @Bindable var model: SettingsModel

    var body: some View {
        Form {
            BackdropSections(settings: $model.backdrop)
            SyntaxThemeSection(model: model)
        }
        .formStyle(.grouped)
    }
}

/// 構文の色のテーマ（Settings > Theme）。明るい・暗い外観で別に選ぶ。VSCode のテーマを読み込める。
struct SyntaxThemeSection: View {
    @Bindable var model: SettingsModel
    @State private var failure: String?

    var body: some View {
        Section(L("Syntax Colors")) {
            Picker(L("Light appearance"), selection: $model.syntaxThemeLight) { options(dark: false) }
            Picker(L("Dark appearance"), selection: $model.syntaxThemeDark) { options(dark: true) }
            ThemePreview(light: model.syntaxThemeLight, dark: model.syntaxThemeDark)
            HStack {
                Button(L("Import VSCode Theme…")) { importTheme() }
                Spacer()
            }
            Note(L("Reads tokenColors from a VSCode color theme (.json). Scopes map to Amon Hen’s 14 kinds, so colors are close but not exact."))
            if let failure { Note(failure).foregroundStyle(.red) }
            ForEach(model.importedThemes, id: \.name) { theme in
                HStack {
                    Text(theme.name)
                    Text(theme.dark ? L("Dark") : L("Light")).foregroundStyle(.secondary)
                    Spacer()
                    Button(L("Remove")) { model.importedThemes.removeAll { $0.name == theme.name } }
                }
            }
        }
    }

    @ViewBuilder private func options(dark: Bool) -> some View {
        Text(SyntaxTheme.system.name).tag(SyntaxTheme.system.name)
        Divider()
        // 合う外観のテーマを先に、ほかは後に（暗い外観に明るいテーマを選んでもよい）。
        let themes = SyntaxTheme.builtIn + model.importedThemes
        ForEach(themes.filter { $0.dark == dark }, id: \.name) { Text($0.name).tag($0.name) }
        Divider()
        ForEach(themes.filter { $0.dark != dark }, id: \.name) { Text($0.name).tag($0.name) }
    }

    private func importTheme() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.json]
        panel.allowsMultipleSelection = true
        guard panel.runModal() == .OK else { return }
        failure = nil
        for url in panel.urls {
            do {
                let theme = try SyntaxTheme.importVSCode(url)
                model.importedThemes.removeAll { $0.name == theme.name }
                model.importedThemes.append(theme)
                if theme.dark { model.syntaxThemeDark = theme.name } else { model.syntaxThemeLight = theme.name }
            } catch {
                failure = L("Couldn’t read %@: %@", url.lastPathComponent, error.localizedDescription)
            }
        }
    }
}

/// テーマの見本（短いコード）。今の外観のテーマで色を付ける。
private struct ThemePreview: View {
    let light: String
    let dark: String
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let theme = SyntaxTheme.named(scheme == .dark ? dark : light) ?? .system
        let parts: [(String, String?)] = [
            ("// ", "comment"), ("Rank the results\n", "comment"),
            ("pub fn ", "keyword"), ("rank", "function"), ("(", nil), ("hits", "variable"), (": &[", nil), ("Hit", "type"),
            ("]) -> ", nil), ("usize", "type"), (" {\n    ", nil), ("let ", "keyword"), ("limit", "variable"), (" = ", "operator"),
            ("10", "number"), (";\n    ", nil), ("println!", "function"), ("(", nil), ("\"top {}\\n\"", "string"),
            (", ", nil), ("self", "constant"), (".", nil), ("count", "property"), (")\n}", nil),
        ]
        parts.reduce(Text("")) { text, part in
            let kind = part.1.flatMap { name in SyntaxTheme.kinds.first { $0.0 == name }?.1 }
            let color = kind.map { Color(nsColor: theme.color($0)) } ?? .primary
            return text + Text(part.0).foregroundColor(color)
        }
        .font(.system(size: 12, design: .monospaced))
        .padding(8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(nsColor: .textBackgroundColor))
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }
}

/// このリポジトリの窓の背景（リポジトリの一覧の右クリック「Window Background…」）。アプリの設定を使うか、上書きするか。
struct RepositoryBackdropView: View {
    let location: String
    let title: String
    let onClose: () -> Void
    @State private var override: Bool
    @State private var settings: BackdropSettings

    init(location: String, title: String, onClose: @escaping () -> Void) {
        self.location = location
        self.title = title
        self.onClose = onClose
        let own = Bookmarks.shared.bookmark(location)?.backdrop
        _override = State(initialValue: own != nil)
        _settings = State(initialValue: own ?? BackdropSettings.app)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Form {
                Section {
                    Toggle(L("Use a different background for %@", title), isOn: $override)
                    Note(L("Otherwise this window follows Settings > Theme."))
                }
                if override { BackdropSections(settings: $settings) }
            }
            .formStyle(.grouped)
            HStack {
                Spacer()
                Button(L("Done")) { onClose() }.keyboardShortcut(.defaultAction)
            }
            .padding()
        }
        // Form は自分の高さを持たないので、シートの高さを決めておく（溢れたら Form の中で送る）。
        .frame(width: 520, height: !override ? 200 : 600)
        .onChange(of: override) { save() }
        .onChange(of: settings) { save() }
    }

    private func save() {
        Bookmarks.shared.setBackdrop(location, override ? settings : nil)
    }
}
