import AmonhenKit
import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow?
    private var history: HistoryViewController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        guard let path = repositoryPath() else {
            NSApp.terminate(nil)
            return
        }
        do {
            let cacheDirectory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
                .appendingPathComponent(Bundle.main.bundleIdentifier ?? "io.github.fukusaka.amonhen")
            let repository = try Repository(path: path, cacheDirectory: cacheDirectory)
            guard let renderer = GraphRenderer() else { throw AmonhenError(description: "Metal を使えない") }
            let controller = HistoryViewController(repository: repository, renderer: renderer)
            let window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 1100, height: 720),
                styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
            let name = URL(fileURLWithPath: path).lastPathComponent
            window.title = name
            window.contentViewController = controller
            window.setContentSize(NSSize(width: 1100, height: 720))
            window.center()
            window.makeKeyAndOrderFront(nil)
            controller.onLatency = { latency in
                window.subtitle = String(format: "入力→表示 %.1f ms", latency * 1000)
            }
            let start = Date()
            controller.load { error in
                if let error { NSAlert(error: error).runModal() }
                NSLog("History を最新にするまで %.0f ms", Date().timeIntervalSince(start) * 1000)
            }
            NSLog("History を %.0f ms で表示（キャッシュ）", Date().timeIntervalSince(start) * 1000)
            self.window = window
            self.history = controller
            if CommandLine.arguments.contains("--measure-scroll") {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                    controller.measureScroll(frames: 600, rowsPerFrame: 1.5) { summary in
                        print(summary)
                        NSApp.terminate(nil)
                    }
                }
            }
            if let output = Self.option("--snapshot") {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                    let png = controller.snapshot()?.representation(using: .png, properties: [:])
                    try? png?.write(to: URL(fileURLWithPath: output))
                    NSApp.terminate(nil)
                }
            }
        } catch {
            let alert = NSAlert(error: error)
            alert.runModal()
            NSApp.terminate(nil)
        }
        NSApp.activate()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    private static func option(_ name: String) -> String? {
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
        return args[i + 1]
    }

    /// 引数で渡されたリポジトリ。なければ選ばせる。
    private func repositoryPath() -> String? {
        var args = Array(CommandLine.arguments.dropFirst())
        if let i = args.firstIndex(of: "--snapshot") { args.removeSubrange(i...min(i + 1, args.count - 1)) }
        args.removeAll { $0 == "--measure-scroll" }
        if let path = args.first { return path }
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.message = "リポジトリを選んでください"
        return panel.runModal() == .OK ? panel.url?.path : nil
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
