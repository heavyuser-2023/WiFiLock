import Foundation

/// 특정 와이파이 네트워크에 적용할 잠금 시간 규칙.
/// 와이파이 이름(SSID)으로 찾고, 이름을 읽을 수 없을 때는 공유기(라우터) 하드웨어 주소로 찾는다.
public struct NetworkRule: Codable, Identifiable, Equatable, Hashable {
    public var id: UUID
    /// 목록에 보여 줄 이름 (사용자가 바꿀 수 있음)
    public var name: String
    /// 와이파이 이름. 위치 권한이 없어 이름을 몰랐을 때 추가한 규칙은 nil.
    public var ssid: String?
    /// 공유기 하드웨어 주소 (정규화된 소문자 "aa:bb:cc:dd:ee:ff")
    public var routerMAC: String?
    /// 마지막 입력 후 잠글 때까지의 시간(초). 0 이면 이 네트워크에서는 잠그지 않는다.
    public var timeoutSeconds: Int

    public init(id: UUID = UUID(), name: String, ssid: String?, routerMAC: String?, timeoutSeconds: Int) {
        self.id = id
        self.name = name
        self.ssid = ssid.flatMap { $0.isEmpty ? nil : $0 }
        self.routerMAC = routerMAC.flatMap { MACAddress.normalize($0) }
        self.timeoutSeconds = max(0, timeoutSeconds)
    }

    private enum CodingKeys: String, CodingKey { case id, name, ssid, routerMAC, timeoutSeconds }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = (try? c.decode(UUID.self, forKey: .id)) ?? UUID()
        name = (try? c.decode(String.self, forKey: .name)) ?? ""
        let rawSSID = try? c.decodeIfPresent(String.self, forKey: .ssid)
        ssid = rawSSID.flatMap { $0.isEmpty ? nil : $0 }
        let rawMAC = try? c.decodeIfPresent(String.self, forKey: .routerMAC)
        routerMAC = rawMAC.flatMap { MACAddress.normalize($0) }
        timeoutSeconds = max(0, (try? c.decode(Int.self, forKey: .timeoutSeconds)) ?? AppSettings.defaultTrustedTimeout)
        if name.isEmpty { name = ssid ?? routerMAC ?? "네트워크" }
    }
}

/// 앱 전체 설정.
public struct AppSettings: Codable, Equatable {
    public static let defaultTrustedTimeout = 1800
    public static let warningPresets: [Int] = [5, 10, 15, 30, 60]

    /// 자동 잠금 사용 여부
    public var enabled: Bool
    /// 네트워크별 규칙 (위에서부터 우선)
    public var rules: [NetworkRule]
    /// 등록되지 않은 와이파이에 연결되었을 때의 잠금 시간(초, 0 = 잠그지 않음)
    public var unknownWiFiTimeoutSeconds: Int
    /// 와이파이가 꺼져 있거나 연결되지 않았을 때의 잠금 시간(초, 0 = 잠그지 않음)
    public var noWiFiTimeoutSeconds: Int
    /// 잠그기 직전에 화면에 경고를 띄울지 여부
    public var warnBeforeLock: Bool
    /// 경고를 띄우는 시간(초)
    public var warningSeconds: Int
    /// 동영상 재생처럼 앱이 "화면 꺼짐 방지"를 요청한 동안은 잠그지 않기
    public var respectDisplaySleepAssertions: Bool
    /// 메뉴 막대 아이콘 옆에 남은 시간 표시
    public var showCountdownInMenuBar: Bool

    public init(
        enabled: Bool = true,
        rules: [NetworkRule] = [],
        unknownWiFiTimeoutSeconds: Int = 300,
        noWiFiTimeoutSeconds: Int = 180,
        warnBeforeLock: Bool = true,
        warningSeconds: Int = 10,
        respectDisplaySleepAssertions: Bool = true,
        showCountdownInMenuBar: Bool = false
    ) {
        self.enabled = enabled
        self.rules = rules
        self.unknownWiFiTimeoutSeconds = max(0, unknownWiFiTimeoutSeconds)
        self.noWiFiTimeoutSeconds = max(0, noWiFiTimeoutSeconds)
        self.warnBeforeLock = warnBeforeLock
        self.warningSeconds = min(max(1, warningSeconds), 300)
        self.respectDisplaySleepAssertions = respectDisplaySleepAssertions
        self.showCountdownInMenuBar = showCountdownInMenuBar
    }

    public static let `default` = AppSettings()

    private enum CodingKeys: String, CodingKey {
        case enabled, rules, unknownWiFiTimeoutSeconds, noWiFiTimeoutSeconds
        case warnBeforeLock, warningSeconds, respectDisplaySleepAssertions, showCountdownInMenuBar
    }

    /// 저장된 값 일부가 없거나 깨져 있어도 기본값으로 채워 읽는다 (버전이 바뀌어도 설정이 날아가지 않도록).
    public init(from decoder: Decoder) throws {
        let d = AppSettings.default
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            enabled: (try? c.decode(Bool.self, forKey: .enabled)) ?? d.enabled,
            rules: (try? c.decode([NetworkRule].self, forKey: .rules)) ?? d.rules,
            unknownWiFiTimeoutSeconds: (try? c.decode(Int.self, forKey: .unknownWiFiTimeoutSeconds)) ?? d.unknownWiFiTimeoutSeconds,
            noWiFiTimeoutSeconds: (try? c.decode(Int.self, forKey: .noWiFiTimeoutSeconds)) ?? d.noWiFiTimeoutSeconds,
            warnBeforeLock: (try? c.decode(Bool.self, forKey: .warnBeforeLock)) ?? d.warnBeforeLock,
            warningSeconds: (try? c.decode(Int.self, forKey: .warningSeconds)) ?? d.warningSeconds,
            respectDisplaySleepAssertions: (try? c.decode(Bool.self, forKey: .respectDisplaySleepAssertions)) ?? d.respectDisplaySleepAssertions,
            showCountdownInMenuBar: (try? c.decode(Bool.self, forKey: .showCountdownInMenuBar)) ?? d.showCountdownInMenuBar
        )
    }
}

/// 설정 화면에서 고를 수 있는 잠금 시간 목록 (초). 0 은 "잠그지 않음".
public enum TimeoutPresets {
    public static let all: [Int] = [30, 60, 120, 180, 300, 600, 900, 1200, 1800, 2700, 3600, 7200, 0]

    /// 현재 값이 목록에 없으면 (예: 예전 버전 값) 목록에 끼워 넣어 선택 상자가 비지 않게 한다.
    public static func including(_ value: Int) -> [Int] {
        if all.contains(value) { return all }
        var list = all.filter { $0 != 0 } + [value]
        list.sort()
        return list.filter { $0 != 0 } + [0]
    }
}

public enum MACAddress {
    /// "d8:7:b6:47:6b:8c" 같은 표기를 "d8:07:b6:47:6b:8c" 로 맞춘다. 형식이 틀리면 nil.
    public static func normalize(_ raw: String) -> String? {
        let parts = raw.trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
            .split(separator: ":", omittingEmptySubsequences: false)
        guard parts.count == 6 else { return nil }
        var out: [String] = []
        for p in parts {
            guard (1...2).contains(p.count), let v = UInt8(p, radix: 16) else { return nil }
            out.append(String(format: "%02x", v))
        }
        let joined = out.joined(separator: ":")
        // 모두 0 이거나 브로드캐스트 주소는 의미 없는 값
        if joined == "00:00:00:00:00:00" || joined == "ff:ff:ff:ff:ff:ff" { return nil }
        return joined
    }
}
