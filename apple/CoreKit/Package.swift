// swift-tools-version: 6.0
// The Swift face of the shared Rust core. scripts/build-apple-core.sh builds
// Frameworks/PaperloomCoreFFI.xcframework and the generated bindings in
// Sources/CoreKit/Generated/; both are build products, not committed.
import PackageDescription

let package = Package(
    name: "CoreKit",
    platforms: [.iOS("26.0"), .macOS("26.0")],
    products: [
        .library(name: "CoreKit", targets: ["CoreKit"]),
    ],
    targets: [
        .binaryTarget(name: "PaperloomCoreFFI", path: "Frameworks/PaperloomCoreFFI.xcframework"),
        .target(
            name: "CoreKit",
            dependencies: ["PaperloomCoreFFI"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .testTarget(
            name: "CoreKitTests",
            dependencies: ["CoreKit"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
