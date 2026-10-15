import AppKit
import CryptoKit

/// Gravatar のアバター。メールアドレスの MD5 で引き、読み終えたら `onLoad` を呼ぶ（描き直すため）。
/// 読んだ画像はアプリが動いている間だけ覚える。Gravatar に送るのはメールアドレスのハッシュだけ。
@MainActor
final class Avatars {
    static let shared = Avatars()

    /// 画像を読み終えたとき（どのメールアドレスか）。
    var onLoad: [(String) -> Void] = []

    private var images: [String: NSImage] = [:]
    private var loading: Set<String> = []
    /// 取れなかったもの（ネットワークがないなど）。何度も取りに行かない。
    private var failed: Set<String> = []

    /// 覚えていれば画像を返す。まだなら取りに行き（`Settings.showAvatars` のときだけ）、nil を返す。
    func image(for email: String) -> NSImage? {
        let key = email.trimmingCharacters(in: .whitespaces).lowercased()
        if let image = images[key] { return image }
        guard Settings.showAvatars, !key.isEmpty, !loading.contains(key), !failed.contains(key) else { return nil }
        loading.insert(key)
        let hash = Insecure.MD5.hash(data: Data(key.utf8)).map { String(format: "%02x", $0) }.joined()
        // 登録していない人は、ハッシュから作る模様（identicon）にする。高解像度の画面のため 2 倍の大きさで取る。
        guard let url = URL(string: "https://www.gravatar.com/avatar/\(hash)?s=96&d=identicon") else { return nil }
        Task {
            let data = try? await URLSession.shared.data(from: url).0
            loading.remove(key)
            guard let data, let image = NSImage(data: data) else {
                failed.insert(key)
                return
            }
            images[key] = image
            for f in onLoad { f(key) }
        }
        return nil
    }

    /// 丸く切り抜いた `size` の大きさの画像。まだ読んでいなければ、名前の頭文字の丸。
    func circle(for email: String, name: String, size: CGFloat) -> NSImage {
        let source = image(for: email)
        return NSImage(size: NSSize(width: size, height: size), flipped: false) { rect in
            let path = NSBezierPath(ovalIn: rect)
            path.addClip()
            if let source {
                source.draw(in: rect)
            } else {
                NSColor.systemGray.withAlphaComponent(0.35).setFill()
                path.fill()
                let initial = String(name.trimmingCharacters(in: .whitespaces).prefix(1)).uppercased()
                let attrs: [NSAttributedString.Key: Any] = [
                    .font: NSFont.systemFont(ofSize: size * 0.5, weight: .semibold), .foregroundColor: NSColor.white,
                ]
                let text = NSAttributedString(string: initial, attributes: attrs)
                let bounds = text.size()
                text.draw(at: NSPoint(x: rect.midX - bounds.width / 2, y: rect.midY - bounds.height / 2))
            }
            return true
        }
    }
}
