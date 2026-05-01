// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "MabelCompanion",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "MabelCompanion", targets: ["MabelCompanion"])
    ],
    targets: [
        .target(
            name: "MabelCompanion",
            resources: [.process("../../Resources")]
        )
    ]
)
