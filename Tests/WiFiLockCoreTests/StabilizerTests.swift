import XCTest
@testable import WiFiLockCore

/// 검토에서 찾은 결함: 와이파이가 2~3초 끊겼다 붙어도 짧은 잠금 시간이 바로 적용됐다.
final class StabilizerTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 2_000_000)
    let home = WiFiSnapshot(status: .connected, ssid: "Home", routerMAC: "02:00:00:00:00:01")
    let cafe = WiFiSnapshot(status: .connected, ssid: "Cafe", routerMAC: "02:00:00:00:00:02")
    let off = WiFiSnapshot(status: .disconnected)

    var settings: AppSettings {
        AppSettings(rules: [NetworkRule(name: "Home", ssid: "Home", routerMAC: "02:00:00:00:00:01", timeoutSeconds: 1800)],
                    unknownWiFiTimeoutSeconds: 300, noWiFiTimeoutSeconds: 60)
    }

    func testBriefDropIsIgnored() {
        let s = PolicyStabilizer(downgradeDelay: 15)
        XCTAssertEqual(s.update(home, settings: settings, now: t0), home)
        for i in 1...5 {
            XCTAssertEqual(s.update(off, settings: settings, now: t0.addingTimeInterval(Double(i))), home, "\(i)초째에는 아직 집 규칙")
        }
        XCTAssertNotNil(s.pendingSince)
        XCTAssertEqual(s.update(home, settings: settings, now: t0.addingTimeInterval(6)), home)
        XCTAssertNil(s.pendingSince)
    }

    func testSustainedDropIsAppliedAfterDelay() {
        let s = PolicyStabilizer(downgradeDelay: 15)
        _ = s.update(home, settings: settings, now: t0)
        XCTAssertEqual(s.update(off, settings: settings, now: t0.addingTimeInterval(1)), home)
        XCTAssertEqual(s.update(off, settings: settings, now: t0.addingTimeInterval(15)), home)
        XCTAssertEqual(s.update(off, settings: settings, now: t0.addingTimeInterval(16)), off)
    }

    func testFlappingBetweenTwoStricterStatesKeepsTimerIfSamePolicy() {
        // 꺼짐 ↔ 연결 안 됨 처럼 상태는 달라도 같은 잠금 시간이면 확인 시간을 이어서 센다
        let s = PolicyStabilizer(downgradeDelay: 15)
        _ = s.update(home, settings: settings, now: t0)
        _ = s.update(off, settings: settings, now: t0.addingTimeInterval(1))
        let powered = WiFiSnapshot(status: .poweredOff)
        _ = s.update(powered, settings: settings, now: t0.addingTimeInterval(8))
        XCTAssertEqual(s.update(powered, settings: settings, now: t0.addingTimeInterval(16)), powered)
    }

    func testLongerTimeoutIsAppliedImmediately() {
        let s = PolicyStabilizer(downgradeDelay: 15)
        _ = s.update(off, settings: settings, now: t0)
        XCTAssertEqual(s.update(cafe, settings: settings, now: t0.addingTimeInterval(1)), cafe, "60초 → 300초: 바로")
        XCTAssertEqual(s.update(home, settings: settings, now: t0.addingTimeInterval(2)), home, "300초 → 1800초: 바로")
    }

    func testSwitchToStricterNetworkWaits() {
        let s = PolicyStabilizer(downgradeDelay: 15)
        _ = s.update(home, settings: settings, now: t0)
        XCTAssertEqual(s.update(cafe, settings: settings, now: t0.addingTimeInterval(1)), home)
        XCTAssertEqual(s.update(cafe, settings: settings, now: t0.addingTimeInterval(17)), cafe)
    }

    func testNeverLockOrdering() {
        let never = LockPolicy(timeoutSeconds: 0, source: .unknownWiFi)
        let five = LockPolicy(timeoutSeconds: 300, source: .unknownWiFi)
        XCTAssertTrue(PolicyStabilizer.isStricter(five, than: never))
        XCTAssertFalse(PolicyStabilizer.isStricter(never, than: five))
        XCTAssertFalse(PolicyStabilizer.isStricter(five, than: five))
    }
}
