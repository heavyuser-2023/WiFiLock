import Foundation

/// 와이파이 연결 상태
public enum WiFiStatus: String, Equatable {
    /// 와이파이 장치가 없음
    case unavailable
    /// 와이파이가 꺼져 있음
    case poweredOff
    /// 켜져 있지만 어떤 네트워크에도 연결되지 않음
    case disconnected
    /// 네트워크에 연결됨
    case connected
}

/// 특정 시점의 와이파이 상태
public struct WiFiSnapshot: Equatable {
    public var status: WiFiStatus
    /// 와이파이 이름. 위치 권한이 없으면 연결되어 있어도 nil 이다.
    public var ssid: String?
    /// 공유기 하드웨어 주소 (정규화된 값)
    public var routerMAC: String?
    public var interfaceName: String?
    public var rssi: Int?

    public init(status: WiFiStatus, ssid: String? = nil, routerMAC: String? = nil, interfaceName: String? = nil, rssi: Int? = nil) {
        self.status = status
        self.ssid = ssid.flatMap { $0.isEmpty ? nil : $0 }
        self.routerMAC = routerMAC.flatMap { MACAddress.normalize($0) }
        self.interfaceName = interfaceName
        self.rssi = rssi
    }

    public var isConnected: Bool { status == .connected }

    public static let unknown = WiFiSnapshot(status: .disconnected)
}

/// 현재 적용되는 잠금 시간의 근거
public enum PolicySource: Equatable {
    case rule(NetworkRule)
    case unknownWiFi
    case noWiFi
}

public struct LockPolicy: Equatable {
    /// 잠금까지 시간(초). 0 이하면 잠그지 않는다.
    public var timeoutSeconds: Int
    public var source: PolicySource

    public init(timeoutSeconds: Int, source: PolicySource) {
        self.timeoutSeconds = timeoutSeconds
        self.source = source
    }

    public var neverLocks: Bool { timeoutSeconds <= 0 }
}

public enum PolicyResolver {
    /// 현재 와이파이 상태에 맞는 규칙을 찾는다.
    ///
    /// - 와이파이 이름을 알면 이름이 같은 규칙을 먼저 찾고, 없으면 "이름 없이 공유기 주소로만 등록된" 규칙을 찾는다.
    ///   (같은 공유기가 손님용·내부용 네트워크를 함께 내보내는 경우 서로 섞이지 않게 하려는 것)
    /// - 이름을 모르면 공유기 주소가 같은 규칙을 찾는다.
    /// - 이름이 같아도 규칙과 현재 상태 모두 공유기 주소를 알고 있는데 서로 다르면 다른 네트워크로 본다.
    ///   (예: 집의 "iptime" 을 "잠그지 않음" 으로 등록했을 때 카페의 다른 "iptime" 에서 잠금이 꺼지지 않도록)
    public static func matchingRule(for snapshot: WiFiSnapshot, in rules: [NetworkRule]) -> NetworkRule? {
        guard snapshot.isConnected else { return nil }
        if let ssid = snapshot.ssid {
            let sameRouter: (NetworkRule) -> Bool = { rule in
                guard let a = rule.routerMAC, let b = snapshot.routerMAC else { return true }
                return a == b
            }
            if let byName = rules.first(where: { $0.ssid == ssid && sameRouter($0) }) { return byName }
            if let mac = snapshot.routerMAC {
                return rules.first(where: { $0.ssid == nil && $0.routerMAC == mac })
            }
            return nil
        }
        if let mac = snapshot.routerMAC {
            return rules.first(where: { $0.routerMAC == mac })
        }
        return nil
    }

    public static func resolve(snapshot: WiFiSnapshot, settings: AppSettings) -> LockPolicy {
        guard snapshot.isConnected else {
            return LockPolicy(timeoutSeconds: settings.noWiFiTimeoutSeconds, source: .noWiFi)
        }
        if let rule = matchingRule(for: snapshot, in: settings.rules) {
            return LockPolicy(timeoutSeconds: rule.timeoutSeconds, source: .rule(rule))
        }
        return LockPolicy(timeoutSeconds: settings.unknownWiFiTimeoutSeconds, source: .unknownWiFi)
    }

    /// 현재 네트워크를 규칙으로 만든다. 연결되어 있지 않거나 식별할 정보가 없으면 nil.
    public static func makeRule(for snapshot: WiFiSnapshot, timeoutSeconds: Int = AppSettings.defaultTrustedTimeout) -> NetworkRule? {
        guard snapshot.isConnected, snapshot.ssid != nil || snapshot.routerMAC != nil else { return nil }
        let name = snapshot.ssid ?? "공유기 \(snapshot.routerMAC!)"
        return NetworkRule(name: name, ssid: snapshot.ssid, routerMAC: snapshot.routerMAC, timeoutSeconds: timeoutSeconds)
    }
}
