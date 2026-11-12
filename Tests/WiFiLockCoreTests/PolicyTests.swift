import XCTest
@testable import WiFiLockCore

final class PolicyTests: XCTestCase {
    let home = NetworkRule(name: "우리집", ssid: "Home_5G", routerMAC: "d8:7:b6:47:6b:8c", timeoutSeconds: 1800)
    let office = NetworkRule(name: "사무실", ssid: "Office", routerMAC: nil, timeoutSeconds: 600)
    let cafeByMAC = NetworkRule(name: "카페", ssid: nil, routerMAC: "aa:bb:cc:dd:ee:ff", timeoutSeconds: 0)

    var settings: AppSettings {
        AppSettings(rules: [home, office, cafeByMAC], unknownWiFiTimeoutSeconds: 120, noWiFiTimeoutSeconds: 60)
    }

    func testMACNormalization() {
        XCTAssertEqual(MACAddress.normalize("d8:7:b6:47:6b:8c"), "d8:07:b6:47:6b:8c")
        XCTAssertEqual(MACAddress.normalize(" AA:BB:CC:DD:EE:FF\n"), "aa:bb:cc:dd:ee:ff")
        XCTAssertNil(MACAddress.normalize("(incomplete)"))
        XCTAssertNil(MACAddress.normalize("aa:bb:cc:dd:ee"))
        XCTAssertNil(MACAddress.normalize("aa:bb:cc:dd:ee:zz"))
        XCTAssertNil(MACAddress.normalize("0:0:0:0:0:0"))
        XCTAssertNil(MACAddress.normalize("ff:ff:ff:ff:ff:ff"))
        XCTAssertNil(MACAddress.normalize("aaa:bb:cc:dd:ee:ff"))
    }

    func testRuleStoresNormalizedMAC() {
        XCTAssertEqual(home.routerMAC, "d8:07:b6:47:6b:8c")
    }

    func testNoWiFiUsesNoWiFiTimeout() {
        for status in [WiFiStatus.poweredOff, .disconnected, .unavailable] {
            let p = PolicyResolver.resolve(snapshot: WiFiSnapshot(status: status), settings: settings)
            XCTAssertEqual(p, LockPolicy(timeoutSeconds: 60, source: .noWiFi), "\(status)")
        }
    }

    func testMatchBySSID() {
        let p = PolicyResolver.resolve(snapshot: WiFiSnapshot(status: .connected, ssid: "Office"), settings: settings)
        XCTAssertEqual(p.timeoutSeconds, 600)
        XCTAssertEqual(p.source, .rule(office))
    }

    func testSSIDIsCaseSensitiveAndExact() {
        let p = PolicyResolver.resolve(snapshot: WiFiSnapshot(status: .connected, ssid: "office"), settings: settings)
        XCTAssertEqual(p.source, .unknownWiFi)
        XCTAssertEqual(p.timeoutSeconds, 120)
    }

    func testUnknownSSIDFallsBackToUnknownWiFi() {
        let p = PolicyResolver.resolve(snapshot: WiFiSnapshot(status: .connected, ssid: "Starbucks", routerMAC: "11:22:33:44:55:66"), settings: settings)
        XCTAssertEqual(p, LockPolicy(timeoutSeconds: 120, source: .unknownWiFi))
    }

    func testMatchByRouterWhenSSIDUnavailable() {
        // 위치 권한이 없어 이름을 모를 때는 공유기 주소로 찾는다.
        let p = PolicyResolver.resolve(snapshot: WiFiSnapshot(status: .connected, ssid: nil, routerMAC: "D8:07:B6:47:6B:8C"), settings: settings)
        XCTAssertEqual(p.source, .rule(home))
        XCTAssertEqual(p.timeoutSeconds, 1800)
    }

    func testRouterMatchDoesNotOverrideDifferentKnownSSID() {
        // 같은 공유기의 손님용 네트워크는 집 규칙에 걸리면 안 된다.
        let p = PolicyResolver.resolve(snapshot: WiFiSnapshot(status: .connected, ssid: "Home_Guest", routerMAC: "d8:07:b6:47:6b:8c"), settings: settings)
        XCTAssertEqual(p.source, .unknownWiFi)
    }

    /// 검토에서 찾은 결함: 이름만 같은 다른 공유기의 네트워크를 집으로 착각했다.
    func testSameSSIDDifferentRouterDoesNotMatch() {
        let p = PolicyResolver.resolve(snapshot: WiFiSnapshot(status: .connected, ssid: "Home_5G", routerMAC: "99:99:99:99:99:99"), settings: settings)
        XCTAssertEqual(p.source, .unknownWiFi)
    }

    func testSameSSIDWithUnknownRouterStillMatches() {
        // 연결 직후 공유기 주소를 아직 못 읽었거나, macOS 가 주소를 가린 경우
        let p = PolicyResolver.resolve(snapshot: WiFiSnapshot(status: .connected, ssid: "Home_5G", routerMAC: nil), settings: settings)
        XCTAssertEqual(p.source, .rule(home))
        // 이름으로만 등록한 규칙은 어느 공유기든 맞는다
        let q = PolicyResolver.resolve(snapshot: WiFiSnapshot(status: .connected, ssid: "Office", routerMAC: "99:99:99:99:99:99"), settings: settings)
        XCTAssertEqual(q.source, .rule(office))
    }

    func testMACOnlyRuleMatchesEvenWhenSSIDKnown() {
        let p = PolicyResolver.resolve(snapshot: WiFiSnapshot(status: .connected, ssid: "CafeFree", routerMAC: "aa:bb:cc:dd:ee:ff"), settings: settings)
        XCTAssertEqual(p.source, .rule(cafeByMAC))
        XCTAssertTrue(p.neverLocks)
    }

    func testConnectedWithoutAnyIdentifierIsUnknown() {
        let p = PolicyResolver.resolve(snapshot: WiFiSnapshot(status: .connected), settings: settings)
        XCTAssertEqual(p.source, .unknownWiFi)
    }

    func testFirstMatchingRuleWins() {
        let dup = NetworkRule(name: "중복", ssid: "Office", routerMAC: nil, timeoutSeconds: 30)
        var s = settings
        s.rules.append(dup)
        XCTAssertEqual(PolicyResolver.resolve(snapshot: WiFiSnapshot(status: .connected, ssid: "Office"), settings: s).timeoutSeconds, 600)
    }

    func testMakeRule() {
        XCTAssertNil(PolicyResolver.makeRule(for: WiFiSnapshot(status: .disconnected)))
        XCTAssertNil(PolicyResolver.makeRule(for: WiFiSnapshot(status: .connected)))
        let byName = PolicyResolver.makeRule(for: WiFiSnapshot(status: .connected, ssid: "A", routerMAC: "1:2:3:4:5:6"))
        XCTAssertEqual(byName?.name, "A")
        XCTAssertEqual(byName?.routerMAC, "01:02:03:04:05:06")
        let byMAC = PolicyResolver.makeRule(for: WiFiSnapshot(status: .connected, routerMAC: "1:2:3:4:5:6"))
        XCTAssertEqual(byMAC?.name, "공유기 01:02:03:04:05:06")
        XCTAssertNil(byMAC?.ssid)
    }
}
