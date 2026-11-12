import XCTest
@testable import WiFiLockCore

final class SettingsTests: XCTestCase {
    private var suiteName: String!
    private var defaults: UserDefaults!

    override func setUp() {
        suiteName = "WiFiLockTests.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)
    }

    override func tearDown() {
        defaults.removePersistentDomain(forName: suiteName)
    }

    func testDefaults() {
        let s = SettingsStore(defaults: defaults).settings
        XCTAssertEqual(s, .default)
        XCTAssertTrue(s.enabled)
        XCTAssertEqual(s.unknownWiFiTimeoutSeconds, 300)
        XCTAssertEqual(s.noWiFiTimeoutSeconds, 180)
        XCTAssertTrue(s.rules.isEmpty)
    }

    func testPersistsAcrossInstances() {
        let a = SettingsStore(defaults: defaults)
        a.settings.noWiFiTimeoutSeconds = 60
        a.settings.warnBeforeLock = false
        a.addRule(for: WiFiSnapshot(status: .connected, ssid: "Home", routerMAC: "1:2:3:4:5:6"), timeoutSeconds: 3600)
        let b = SettingsStore(defaults: defaults)
        XCTAssertEqual(b.settings, a.settings)
        XCTAssertEqual(b.settings.rules.first?.ssid, "Home")
        XCTAssertEqual(b.settings.rules.first?.timeoutSeconds, 3600)
    }

    func testCorruptDataFallsBackToDefaults() {
        defaults.set(Data("not json".utf8), forKey: SettingsStore.storageKey)
        XCTAssertEqual(SettingsStore(defaults: defaults).settings, .default)
    }

    func testPartialDataKeepsKnownFields() {
        let json = #"{"noWiFiTimeoutSeconds": 45, "rules": [{"ssid": "X", "timeoutSeconds": 99}], "futureField": 1}"#
        defaults.set(Data(json.utf8), forKey: SettingsStore.storageKey)
        let s = SettingsStore(defaults: defaults).settings
        XCTAssertEqual(s.noWiFiTimeoutSeconds, 45)
        XCTAssertEqual(s.unknownWiFiTimeoutSeconds, 300)
        XCTAssertEqual(s.rules.count, 1)
        XCTAssertEqual(s.rules[0].name, "X", "이름이 없으면 와이파이 이름을 쓴다")
        XCTAssertEqual(s.rules[0].timeoutSeconds, 99)
    }

    func testAddRuleIsIdempotentAndFillsMissingSSID() {
        let store = SettingsStore(defaults: defaults)
        // 처음엔 위치 권한이 없어 공유기 주소로만 등록
        let r1 = store.addRule(for: WiFiSnapshot(status: .connected, routerMAC: "aa:bb:cc:dd:ee:ff"))
        XCTAssertNotNil(r1)
        XCTAssertNil(r1?.ssid)
        // 같은 공유기로 다시 추가 → 새로 만들지 않음
        store.addRule(for: WiFiSnapshot(status: .connected, routerMAC: "AA:BB:CC:DD:EE:FF"))
        XCTAssertEqual(store.settings.rules.count, 1)
        // 나중에 이름을 알게 되면 기존 규칙에 이름을 채움
        let r2 = store.addRule(for: WiFiSnapshot(status: .connected, ssid: "Cafe", routerMAC: "aa:bb:cc:dd:ee:ff"))
        XCTAssertEqual(store.settings.rules.count, 1)
        XCTAssertEqual(r2?.ssid, "Cafe")
        XCTAssertEqual(r2?.id, r1?.id)
    }

    func testAddRuleBySSID() {
        let store = SettingsStore(defaults: defaults)
        XCTAssertNotNil(store.addRule(ssid: "  Office  ", timeoutSeconds: 600))
        XCTAssertEqual(store.settings.rules.first?.ssid, "Office")
        XCTAssertNil(store.addRule(ssid: "Office", timeoutSeconds: 60), "중복은 추가하지 않는다")
        XCTAssertNil(store.addRule(ssid: "   ", timeoutSeconds: 60))
        XCTAssertEqual(store.settings.rules.count, 1)
        store.removeRule(id: store.settings.rules[0].id)
        XCTAssertTrue(store.settings.rules.isEmpty)
        XCTAssertTrue(SettingsStore(defaults: defaults).settings.rules.isEmpty)
    }

    func testAddRuleWhenDisconnectedFails() {
        let store = SettingsStore(defaults: defaults)
        XCTAssertNil(store.addRule(for: WiFiSnapshot(status: .disconnected)))
        XCTAssertNil(store.addRule(for: WiFiSnapshot(status: .connected)))
        XCTAssertTrue(store.settings.rules.isEmpty)
    }

    func testValueClamping() {
        let s = AppSettings(unknownWiFiTimeoutSeconds: -5, noWiFiTimeoutSeconds: -1, warningSeconds: 0)
        XCTAssertEqual(s.unknownWiFiTimeoutSeconds, 0)
        XCTAssertEqual(s.noWiFiTimeoutSeconds, 0)
        XCTAssertEqual(s.warningSeconds, 1)
    }

    func testTimeoutPresetsIncludeCustomValue() {
        XCTAssertEqual(TimeoutPresets.including(300), TimeoutPresets.all)
        let custom = TimeoutPresets.including(45)
        XCTAssertTrue(custom.contains(45))
        XCTAssertEqual(custom.last, 0, "'잠그지 않음' 은 항상 마지막")
        XCTAssertEqual(custom.dropLast(), custom.dropLast().sorted())
    }

    func testKoreanFormatting() {
        XCTAssertEqual(KoreanFormat.timeout(0), "잠그지 않음")
        XCTAssertEqual(KoreanFormat.timeout(30), "30초")
        XCTAssertEqual(KoreanFormat.timeout(90), "1분 30초")
        XCTAssertEqual(KoreanFormat.timeout(300), "5분")
        XCTAssertEqual(KoreanFormat.timeout(3600), "1시간")
        XCTAssertEqual(KoreanFormat.timeout(5400), "1시간 30분")
        XCTAssertEqual(KoreanFormat.duration(0), "0초")
        XCTAssertEqual(KoreanFormat.countdown(59.2), "1:00")
        XCTAssertEqual(KoreanFormat.countdown(0), "0:00")
        XCTAssertEqual(KoreanFormat.countdown(-3), "0:00")
        XCTAssertEqual(KoreanFormat.countdown(3725), "1:02:05")
        let date = Date(timeIntervalSince1970: 0) // 1970-01-01 09:00 KST
        XCTAssertEqual(KoreanFormat.clock(date, timeZone: TimeZone(identifier: "Asia/Seoul")!), "오전 9:00")
    }

    func testSnapshotDescriptions() {
        XCTAssertEqual(WiFiSnapshot(status: .poweredOff).koreanDescription, "와이파이 꺼짐")
        XCTAssertEqual(WiFiSnapshot(status: .connected, ssid: "Home").koreanDescription, "Home")
        XCTAssertEqual(WiFiSnapshot(status: .connected).koreanDescription, "이름 확인 불가 (위치 권한 필요)")
    }
}
