// swift-tools-version: 6.2
import PackageDescription

// Rust のコア（crates/ffi-c）を静的ライブラリとしてリンクする。先に scripts/build-core.sh を実行すること。
let coreLibDir = Context.packageDirectory + "/../../target/release"

let package = Package(
    name: "AmonHen",
    platforms: [.macOS("14.4")],
    products: [
        .library(name: "AmonhenKit", targets: ["AmonhenKit"]),
        .executable(name: "AmonHen", targets: ["AmonHen"]),
    ],
    targets: [
        .systemLibrary(name: "CAmonhen", path: "Sources/CAmonhen"),
        .target(
            name: "AmonhenKit",
            dependencies: ["CAmonhen"],
            linkerSettings: [
                .unsafeFlags(["-L", coreLibDir]),
                .linkedLibrary("amonhen_ffi_c"),
                .linkedLibrary("iconv"),
            ]
        ),
        .executableTarget(
            name: "AmonHen",
            dependencies: ["AmonhenKit"],
            swiftSettings: [.defaultIsolation(MainActor.self)],
            linkerSettings: [
                // 実行ファイルに「macOS 26 の SDK でリンクした」と記録する（動く最低の macOS は 14.4 のまま）。macOS 26
                // 以降はこの記録で新しい見た目（ツールバー・タブ・アイコン）を使うかを決める。SwiftPM は手元の Xcode だと
                // デプロイ対象の 14.4 を記録してしまい、配布版（CI）と見た目が変わるので、明示する。
                .unsafeFlags(["-Xlinker", "-platform_version", "-Xlinker", "macos", "-Xlinker", "14.4", "-Xlinker", "26.0"]),
            ]
        ),
        .testTarget(name: "AmonhenKitTests", dependencies: ["AmonhenKit"]),
    ]
)
