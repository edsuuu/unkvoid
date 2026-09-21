// swift-tools-version: 6.0
import Foundation
import PackageDescription

/// De que build do núcleo a biblioteca vem. Com os dois caminhos na linha do linker valeria
/// sempre o primeiro que existisse, e o app de desenvolvimento passaria a ligar um núcleo de
/// release parado no tempo. O `bundle.sh` pede `release`; o resto usa o `debug`.
let core = ProcessInfo.processInfo.environment["UNKVOID_CORE"] ?? "debug"

let package = Package(
    name: "Unkvoid",
    platforms: [.macOS(.v14)],
    targets: [
        .systemLibrary(name: "UnkvoidCore", path: "Sources/UnkvoidCore"),
        .executableTarget(
            name: "Unkvoid",
            dependencies: ["UnkvoidCore"],
            path: "Sources/Unkvoid",
            exclude: ["Info.plist"],
            linkerSettings: [
                .unsafeFlags(["-L../../target/\(core)"]),
                // Executável solto não tem pacote, e sem o texto de uso embutido o sistema
                // mata o processo na primeira vez que ele pede o microfone.
                .unsafeFlags(["-Xlinker", "-sectcreate", "-Xlinker", "__TEXT", "-Xlinker", "__info_plist", "-Xlinker", "Sources/Unkvoid/Info.plist"]),
            ]
        ),
        .testTarget(
            name: "UnkvoidTests",
            dependencies: ["Unkvoid"],
            path: "Tests/UnkvoidTests"
        ),
    ]
)
