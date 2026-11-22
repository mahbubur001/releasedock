// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "ReleaseDockKit",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "ReleaseDockCore", targets: ["ReleaseDockCore"])
    ],
    targets: [
        .target(
            name: "ReleaseDockCore",
            linkerSettings: [.linkedLibrary("sqlite3")]
        ),
        .testTarget(
            name: "ReleaseDockCoreTests",
            dependencies: ["ReleaseDockCore"]
        )
    ],
    swiftLanguageModes: [.v6]
)
