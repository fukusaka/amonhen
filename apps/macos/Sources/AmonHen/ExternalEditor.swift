import AppKit

/// 外部のエディタ。アプリのどこから「エディタで開く」ときも、Settings > General で選んだものを使う。
/// 選んでいなければ、入っているコードエディタのうち最初のもの、それもなければ既定のテキストエディタ。
enum ExternalEditor {
    struct App: Identifiable, Hashable {
        let name: String
        let url: URL
        var id: String { url.path }
    }

    /// 知っているコードエディタ（この順に探す）。
    private static let known: [(id: String, name: String)] = [
        ("com.microsoft.VSCode", "VS Code"), ("com.microsoft.VSCodeInsiders", "VS Code Insiders"),
        ("com.todesktop.230313mzl4w4u2", "Cursor"), ("dev.zed.Zed", "Zed"),
        ("com.sublimetext.4", "Sublime Text"), ("com.sublimetext.3", "Sublime Text"),
        ("com.barebones.bbedit", "BBEdit"), ("com.panic.Nova", "Nova"),
        ("com.macromates.TextMate", "TextMate"), ("com.jetbrains.fleet", "Fleet"),
        ("com.apple.dt.Xcode", "Xcode"),
    ]

    /// 入っているコードエディタ（known の順）。最後に既定のテキストエディタ。
    static var installed: [App] {
        var apps: [App] = []
        for app in known {
            if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: app.id), !apps.contains(where: { $0.url == url }) {
                apps.append(App(name: app.name, url: url))
            }
        }
        if let url = NSWorkspace.shared.urlForApplication(toOpen: .plainText), !apps.contains(where: { $0.url == url }) {
            apps.append(App(name: displayName(url), url: url))
        }
        return apps
    }

    /// 自動で選ばれるもの（入っているコードエディタの先頭）。
    static var automatic: App? { installed.first }

    /// Settings で選んだもの。なければ自動。
    static var chosen: App? {
        if let path = Settings.externalEditor, !path.isEmpty {
            let url = URL(fileURLWithPath: path)
            if FileManager.default.fileExists(atPath: path) {
                return known.first { NSWorkspace.shared.urlForApplication(withBundleIdentifier: $0.id) == url }
                    .map { App(name: $0.name, url: url) } ?? App(name: displayName(url), url: url)
            }
        }
        return automatic
    }

    static func displayName(_ url: URL) -> String {
        (FileManager.default.displayName(atPath: url.path) as NSString).deletingPathExtension
    }

    /// ファイルをエディタで開く（`app` がなければ選んだもの）。
    static func open(_ files: [URL], with app: App? = nil) {
        if let app = app ?? chosen {
            NSWorkspace.shared.open(files, withApplicationAt: app.url, configuration: NSWorkspace.OpenConfiguration())
        } else {
            for file in files { NSWorkspace.shared.open(file) }
        }
    }
}
