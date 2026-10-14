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
            swiftSettings: [.defaultIsolation(MainActor.self)]
        ),
        .testTarget(name: "AmonhenKitTests", dependencies: ["AmonhenKit"]),
    ]
)
