// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "Unkvoid",
    platforms: [.macOS(.v14)],
    targets: [
        .systemLibrary(name: "UnkvoidCore", path: "Sources/UnkvoidCore"),
        .executableTarget(
            name: "Unkvoid",
            dependencies: ["UnkvoidCore"],
            path: "Sources/Unkvoid",
            linkerSettings: [
                .unsafeFlags(["-L../../target/release", "-L../../target/debug"])
            ]
        ),
        .testTarget(
            name: "UnkvoidTests",
            dependencies: ["Unkvoid"],
            path: "Tests/UnkvoidTests"
        ),
    ]
)
