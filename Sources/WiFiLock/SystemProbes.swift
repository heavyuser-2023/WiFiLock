import AppKit
import CoreGraphics
import Foundation
import IOKit.pwr_mgt

/// 시스템 상태 읽기 (자리비움 시간, 화면 잠김 여부, 화면 꺼짐 방지 요청)
enum SystemProbes {
    /// 마지막 키보드·마우스·트랙패드 입력 이후 지난 초
    static func idleSeconds() -> TimeInterval {
        let anyInput = CGEventType(rawValue: ~0)!
        return CGEventSource.secondsSinceLastEventType(.hidSystemState, eventType: anyInput)
    }

    /// 화면이 잠겨 있는지 (로그인 창이 떠 있는지)
    static func isScreenLocked() -> Bool {
        guard let dict = CGSessionCopyCurrentDictionary() as? [String: Any] else { return false }
        if let locked = dict["CGSSessionScreenIsLocked"] as? Bool { return locked }
        if let locked = dict["CGSSessionScreenIsLocked"] as? Int { return locked != 0 }
        return false
    }

    /// 이 사용자 세션이 지금 화면 앞(콘솔)에 있는지. 빠른 사용자 전환으로 뒤로 가 있으면 false.
    static func isSessionOnConsole() -> Bool {
        guard let dict = CGSessionCopyCurrentDictionary() as? [String: Any] else { return true }
        if let on = dict[kCGSessionOnConsoleKey as String] as? Bool { return on }
        if let on = dict[kCGSessionOnConsoleKey as String] as? Int { return on != 0 }
        return true
    }

    /// 화면이 꺼지지 않도록 요청한 앱 이름들 (동영상 재생, 발표 등). 이 앱 자신은 제외.
    static func displaySleepPreventers() -> [String] {
        var unmanaged: Unmanaged<CFDictionary>?
        guard IOPMCopyAssertionsByProcess(&unmanaged) == kIOReturnSuccess,
              let byPID = unmanaged?.takeRetainedValue() as? [NSNumber: [[String: Any]]] else { return [] }
        let types: Set<String> = ["PreventUserIdleDisplaySleep", "NoDisplaySleepAssertion"]
        let myPID = ProcessInfo.processInfo.processIdentifier
        var names: [String] = []
        for (pidNumber, assertions) in byPID {
            let pid = pid_t(truncating: pidNumber)
            guard pid != myPID else { continue }
            let active = assertions.contains { a in
                guard let type = a["AssertType"] as? String, types.contains(type) else { return false }
                // 0 이면 해제된 요청
                if let level = a["AssertLevel"] as? Int, level == 0 { return false }
                return true
            }
            guard active else { continue }
            let name = NSRunningApplication(processIdentifier: pid)?.localizedName
                ?? assertions.first?["Process Name"] as? String
                ?? "PID \(pid)"
            if !names.contains(name) { names.append(name) }
        }
        return names.sorted()
    }
}

protocol ScreenLocking {
    /// 화면을 잠근다. 성공적으로 명령을 보냈으면 true.
    func lock(reason: String) -> Bool
}

/// macOS 의 "화면 잠금" (제어 센터·⌃⌘Q 와 같은 동작)
final class SystemScreenLocker: ScreenLocking {
    private typealias LockFn = @convention(c) () -> Int32
    private let lockFn: LockFn?

    init() {
        let path = "/System/Library/PrivateFrameworks/login.framework/Versions/Current/login"
        if let handle = dlopen(path, RTLD_LAZY), let sym = dlsym(handle, "SACLockScreenImmediate") {
            lockFn = unsafeBitCast(sym, to: LockFn.self)
        } else {
            lockFn = nil
        }
    }

    var hasNativeLock: Bool { lockFn != nil }

    func lock(reason: String) -> Bool {
        AppLog.info("화면 잠금 실행: \(reason)")
        if let lockFn {
            let rc = lockFn()
            if rc == 0 {
                // 2초 뒤에도 잠기지 않았다면 대체 방법을 쓴다.
                DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [weak self] in
                    guard let self, !SystemProbes.isScreenLocked() else { return }
                    AppLog.error("화면이 잠기지 않아 디스플레이 끄기로 대체")
                    _ = self.sleepDisplay()
                }
                return true
            }
            AppLog.error("화면 잠금 함수가 오류 \(rc) 를 돌려줌 → 디스플레이 끄기로 대체")
        }
        return sleepDisplay()
    }

    /// 대체 방법: 디스플레이를 끈다 (시스템 설정의 "디스플레이가 꺼지면 암호 요구" 가 즉시이면 잠긴다).
    /// 주 스레드를 막지 않도록 끝나기를 기다리지 않는다.
    private func sleepDisplay() -> Bool {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/pmset")
        p.arguments = ["displaysleepnow"]
        p.terminationHandler = { proc in
            if proc.terminationStatus != 0 { AppLog.error("디스플레이 끄기 명령 실패: \(proc.terminationStatus)") }
        }
        do {
            try p.run()
            return true
        } catch {
            AppLog.error("디스플레이 끄기 실패: \(error)")
            return false
        }
    }
}

/// 검증용: 실제로 잠그지 않고 기록만 남긴다.
final class DryRunLocker: ScreenLocking {
    private(set) var lockCount = 0
    func lock(reason: String) -> Bool {
        lockCount += 1
        AppLog.info("[시험 모드] 화면 잠금 요청 #\(lockCount): \(reason)")
        return true
    }
}
