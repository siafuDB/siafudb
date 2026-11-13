// swift-tools-version:5.9
// Produced by scripts/ffi/build-ios.sh, which creates SiafuDBFFI.xcframework
// next to this file (it is not committed).
import PackageDescription

let package = Package(
    name: "SiafuDB",
    platforms: [.iOS(.v13), .macOS(.v11)],
    products: [
        .library(name: "SiafuDB", targets: ["SiafuDB"]),
    ],
    targets: [
        .binaryTarget(name: "SiafuDBFFI", path: "SiafuDBFFI.xcframework"),
        .target(
            name: "SiafuDB",
            dependencies: ["SiafuDBFFI"],
            path: "Sources/SiafuDB"
        ),
    ]
)
