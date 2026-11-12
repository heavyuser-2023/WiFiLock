import AppKit
import Foundation
import os

enum AppInfo {
    static let bundleID = Bundle.main.bundleIdentifier ?? "com.wifilock.WiFiLock"
    static let displayName = "와이파이 잠금"
    static var version: String {
        let v = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "개발판"
        let b = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "0"
        return "\(v) (\(b))"
    }
}

/// 실행 인자. 검증·개발용 옵션은 일반 사용 시 쓰이지 않는다.
struct LaunchOptions {
    /// 실제로 잠그지 않고 기록만 남김
    var dryRun = false
    /// 자가 진단을 실행하고 결과를 이 폴더에 쓴 뒤 종료
    var selfTestDir: String?
    /// 설정을 별도 저장소에 보관 (검증할 때 사용자 설정을 건드리지 않기 위함)
    var settingsSuite: String?
    /// 기록을 파일로도 남김
    var logFile: String?
    /// 모든 네트워크의 잠금 시간을 이 값(초)으로 강제
    var overrideTimeout: Int?
    /// 시작할 때 위치 권한을 묻지 않음
    var noLocationPrompt = false
    /// 시작하자마자 설정 창 열기
    var openSettings = false

    static func parse(_ args: [String]) -> LaunchOptions {
        var o = LaunchOptions()
        var it = args.dropFirst().makeIterator()
        while let a = it.next() {
            switch a {
            case "--dry-run": o.dryRun = true
            case "--self-test": o.selfTestDir = it.next()
            case "--settings-suite": o.settingsSuite = it.next()
            case "--log-file": o.logFile = it.next()
            case "--override-timeout": o.overrideTimeout = it.next().flatMap(Int.init)
            case "--no-location-prompt": o.noLocationPrompt = true
            case "--open-settings": o.openSettings = true
            default: break // -NSDocumentRevisionsDebugMode 같은 시스템 인자는 무시
            }
        }
        return o
    }
}

/// 통합 기록 (시스템 로그 + 선택적으로 파일)
enum AppLog {
    private static let logger = Logger(subsystem: AppInfo.bundleID, category: "app")
    private static let queue = DispatchQueue(label: "wifilock.log")
    static var fileURL: URL?

    static func info(_ message: String) {
        logger.info("\(message, privacy: .public)")
        write("INFO", message)
    }

    static func error(_ message: String) {
        logger.error("\(message, privacy: .public)")
        write("ERROR", message)
    }

    private static func write(_ level: String, _ message: String) {
        guard let url = fileURL else { return }
        let line = "\(ISO8601DateFormatter().string(from: Date())) [\(level)] \(message)\n"
        queue.async {
            if let h = try? FileHandle(forWritingTo: url) {
                h.seekToEndOfFile()
                h.write(Data(line.utf8))
                try? h.close()
            } else {
                try? Data(line.utf8).write(to: url)
            }
        }
    }

    static func flush() { queue.sync {} }
}

/// 메뉴 막대·앱 아이콘에 쓰는 그림
enum Icons {
    /// 메뉴 막대 아이콘: 잠금 동작 중이면 자물쇠 달린 화면, 쉬는 중이면 열린 자물쇠 화면
    static func statusBar(active: Bool, warning: Bool = false) -> NSImage {
        let names = active ? ["lock.display", "lock.fill"] : ["lock.open.display", "lock.open.fill"]
        let config = NSImage.SymbolConfiguration(pointSize: 15, weight: warning ? .bold : .regular)
        for name in names {
            if let img = NSImage(systemSymbolName: name, accessibilityDescription: active ? "자동 잠금 동작 중" : "자동 잠금 쉬는 중")?
                .withSymbolConfiguration(config) {
                img.isTemplate = true
                return img
            }
        }
        return drawnFallback(active: active)
    }

    /// 시스템 기호를 쓸 수 없는 경우를 위한 직접 그린 아이콘
    private static func drawnFallback(active: Bool) -> NSImage {
        let img = NSImage(size: NSSize(width: 18, height: 16), flipped: false) { _ in
            NSColor.black.setStroke()
            let screen = NSBezierPath(roundedRect: NSRect(x: 1, y: 4, width: 16, height: 11), xRadius: 2, yRadius: 2)
            screen.lineWidth = 1.4
            screen.stroke()
            NSBezierPath.fill(NSRect(x: 7, y: 1, width: 4, height: 2))
            NSColor.black.setFill()
            NSBezierPath(roundedRect: NSRect(x: 6.5, y: 6, width: 5, height: 4), xRadius: 0.8, yRadius: 0.8).fill()
            let shackle = NSBezierPath()
            shackle.lineWidth = 1.2
            shackle.move(to: NSPoint(x: 7.6, y: 10))
            shackle.appendArc(withCenter: NSPoint(x: 9, y: 11), radius: 1.4, startAngle: 180, endAngle: active ? 0 : 60, clockwise: true)
            shackle.stroke()
            return true
        }
        img.isTemplate = true
        return img
    }
}
