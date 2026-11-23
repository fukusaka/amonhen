import AppKit
import QuartzCore
import WebKit

/// 画面に出さない WebKit で、SVG をブラウザと同じに描いて画像にする（iOS のアセットと同じ系統の CoreSVG
/// （`NSImage(data:)`）の絵と並べて、崩れを見つけるため）。SVG の中のスクリプトは動かさず、外（http・https）
/// からは何も読まない。
@MainActor
final class WebRenderer: NSObject, WKNavigationDelegate {
    static let shared = WebRenderer()

    /// 描く大きさの上限（ポイント）。
    private static let maxSide: CGFloat = 2048

    private var webView: WKWebView?
    private var window: NSWindow?
    private var loaded: CheckedContinuation<Void, Never>?
    /// 外への読み込みを止める決まり（一度だけ作る）。
    private var blockRules: WKContentRuleList?

    /// `svg` をブラウザと同じに描いた画像（描けなければ nil）。大きさは SVG の幅と高さ（なければ viewBox、それもなければ
    /// 300 × 150）。
    func render(svg: Data) async -> NSImage? {
        let started = CACurrentMediaTime()
        let cold = webView == nil
        defer {
            let ms = (CACurrentMediaTime() - started) * 1000
            PerformanceLog.record(cold ? "SVG: WebKit render (first)" : "SVG: WebKit render", ms: ms)
        }
        let view = await makeWebView()
        let html = """
            <!doctype html><html><head><meta charset="utf-8"><style>html,body{margin:0;padding:0;background:transparent}
            img{display:block}</style></head><body><img id="svg" src="data:image/svg+xml;base64,\(svg.base64EncodedString())"></body></html>
            """
        await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
            loaded = done
            view.loadHTMLString(html, baseURL: nil)
        }
        // <img> の大きさが決まるのを待つ。
        let size = try? await view.callAsyncJavaScript(
            """
            const img = document.getElementById('svg');
            if (!img.complete) { await new Promise(r => { img.onload = r; img.onerror = r; }); }
            return [img.naturalWidth, img.naturalHeight];
            """, contentWorld: .defaultClient) as? [Double]
        guard let size, size.count == 2, size[0] > 0, size[1] > 0 else { return nil }
        let scale = min(1, Self.maxSide / CGFloat(max(size[0], size[1])))
        let rect = NSRect(x: 0, y: 0, width: CGFloat(size[0]) * scale, height: CGFloat(size[1]) * scale)
        view.frame = rect
        if scale < 1 {
            _ = try? await view.evaluateJavaScript(
                "document.getElementById('svg').style.width='\(rect.width)px';document.getElementById('svg').style.height='\(rect.height)px'")
        }
        let config = WKSnapshotConfiguration()
        config.rect = rect
        return try? await view.takeSnapshot(configuration: config)
    }

    /// 前もって WebKit を起こしておく（最初の 1 回は WebKit の裏のプロセスを起こすので 250〜300 ms かかる。
    /// 2 回目からは 7〜10 ms）。すべてのファイルを開いたときに呼ぶ。
    func prewarm() async {
        guard webView == nil else { return }
        let view = await makeWebView()
        await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
            loaded = done
            view.loadHTMLString("<!doctype html>", baseURL: nil)
        }
    }

    private func makeWebView() async -> WKWebView {
        if let webView { return webView }
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        config.websiteDataStore = .nonPersistent()
        if blockRules == nil {
            blockRules = try? await WKContentRuleListStore.default().compileContentRuleList(
                forIdentifier: "io.github.fukusaka.amonhen.no-network",
                encodedContentRuleList: #"[{"trigger":{"url-filter":"^(https?|ftp|wss?)://"},"action":{"type":"block"}}]"#)
        }
        if let blockRules { config.userContentController.add(blockRules) }
        let view = WKWebView(frame: NSRect(x: 0, y: 0, width: 300, height: 150), configuration: config)
        view.navigationDelegate = self
        view.setValue(false, forKey: "drawsBackground")
        // 描くには窓が要る。画面の外に置き、見せない。
        let window = NSWindow(
            contentRect: NSRect(x: -10_000, y: -10_000, width: Self.maxSide, height: Self.maxSide),
            styleMask: .borderless, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView?.addSubview(view)
        self.window = window
        webView = view
        return view
    }

    nonisolated func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        MainActor.assumeIsolated {
            loaded?.resume()
            loaded = nil
        }
    }

    nonisolated func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
        MainActor.assumeIsolated {
            loaded?.resume()
            loaded = nil
        }
    }
}
