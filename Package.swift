// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "WiFiLock",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "WiFiLock", targets: ["WiFiLock"]),
    ],
    targets: [
        // 시스템에 의존하지 않는 순수 로직 (단위 테스트 대상)
        .target(name: "WiFiLockCore"),
        // 메뉴 막대 앱 본체
        .executableTarget(
            name: "WiFiLock",
            dependencies: ["WiFiLockCore"],
            linkerSettings: [
                .linkedFramework("AppKit"),
                .linkedFramework("SwiftUI"),
                .linkedFramework("CoreWLAN"),
                .linkedFramework("CoreLocation"),
                .linkedFramework("ServiceManagement"),
                .linkedFramework("IOKit"),
            ]
        ),
        .testTarget(name: "WiFiLockCoreTests", dependencies: ["WiFiLockCore"]),
    ]
)
