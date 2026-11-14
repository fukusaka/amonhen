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

    /// Open in Editor のツールチップ（どのエディタで開くか）。
    static var toolTip: String { L("Opens in %@", chosen?.name ?? L("Editor")) }

    /// フォルダ（リポジトリ・worktree）を選んだエディタで開く。開けなければ、そのわけ（開いたら nil）。
    /// 知っているコードエディタはどれもフォルダを開ける。ほか（既定のテキストエディタ、Other… で選んだもの）は、
    /// フォルダを開けると宣言しているものだけ（TextEdit に渡すと、開けないと言われるだけ）。
    @discardableResult
    static func openFolder(_ path: String) -> String? {
        let folder = URL(fileURLWithPath: path, isDirectory: true)
        guard let app = chosen else { return L("No editor is installed. Choose one in Settings > General.") }
        let isKnown = known.contains { NSWorkspace.shared.urlForApplication(withBundleIdentifier: $0.id) == app.url }
        let opensFolders = isKnown || NSWorkspace.shared.urlsForApplications(toOpen: folder)
            .contains { $0.standardizedFileURL == app.url.standardizedFileURL }
        guard opensFolders else { return L("%@ can’t open folders. Choose a code editor in Settings > General.", app.name) }
        NSWorkspace.shared.open([folder], withApplicationAt: app.url, configuration: NSWorkspace.OpenConfiguration())
        return nil
    }

    /// `openFolder` で開けなかったわけを、`window` のシート（なければ窓）で知らせる。
    @MainActor
    static func openFolder(_ path: String, in window: NSWindow?) {
        guard let message = openFolder(path) else { return }
        let alert = NSAlert()
        alert.messageText = L("Couldn’t Open in Editor")
        alert.informativeText = message
        if let window { alert.beginSheetModal(for: window) } else { alert.runModal() }
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
