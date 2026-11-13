import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// Theme: 構文の色。
struct ThemeSettings: View {
    @Bindable var model: SettingsModel

    var body: some View {
        Form {
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
