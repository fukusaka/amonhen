import Foundation

/// 画面の文字の訳。コードには英語で書き（それが鍵）、日本語は `apps/macos/Localization/ja.lproj/Localizable.strings`。
/// git の操作と画面の名前（Commit・Push・Pull・Fetch・Working Copy・History など）は、日本語でも英語のまま
/// （docs/ux.md の「Language」）。
enum L10n {
    /// 使う言語の `.lproj`。`.app` なら Contents/Resources（`scripts/package-app.sh` が入れる）、開発中の実行ファイル
    /// ならソースの中。言語は利用者の設定の順で選ぶ（素の実行ファイルでは Bundle が英語しか選ばないので、自分で選ぶ）。
    nonisolated static let bundle: Bundle = {
        let root: Bundle
        if Bundle.main.url(forResource: "Localizable", withExtension: "strings", subdirectory: nil, localization: "ja") != nil {
            root = .main
        } else {
            let source = URL(fileURLWithPath: #filePath)
                .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
                .appendingPathComponent("Localization")
            root = Bundle(url: source) ?? .main
        }
        let language = Bundle.preferredLocalizations(from: root.localizations, forPreferences: Locale.preferredLanguages).first
        return language.flatMap { root.url(forResource: $0, withExtension: "lproj") }.flatMap(Bundle.init(url:)) ?? root
    }()
}

/// `key`（英語）の訳。
nonisolated func L(_ key: String) -> String {
    L10n.bundle.localizedString(forKey: key, value: key, table: nil)
}

/// `format`（英語。`%@`・`%d`、順番を変えるなら `%1$@`）の訳に、`args` を入れる。
nonisolated func L(_ format: String, _ args: CVarArg...) -> String {
    String(format: L(format), locale: Locale.current, arguments: args)
}
