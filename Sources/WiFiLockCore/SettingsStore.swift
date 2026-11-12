import Combine
import Foundation

/// 설정을 UserDefaults 에 JSON 으로 저장·불러온다.
public final class SettingsStore: ObservableObject {
    public static let storageKey = "settings.v1"

    @Published public var settings: AppSettings {
        didSet {
            if settings != oldValue { save() }
        }
    }

    private let defaults: UserDefaults

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        self.settings = SettingsStore.load(from: defaults)
    }

    public static func load(from defaults: UserDefaults) -> AppSettings {
        guard let data = defaults.data(forKey: storageKey) else { return .default }
        return (try? JSONDecoder().decode(AppSettings.self, from: data)) ?? .default
    }

    public func save() {
        if let data = try? JSONEncoder().encode(settings) {
            defaults.set(data, forKey: SettingsStore.storageKey)
        }
    }

    // MARK: - 규칙 편집 도우미

    /// 현재 네트워크를 규칙에 추가한다. 이미 같은 네트워크 규칙이 있으면 그 규칙을 돌려준다.
    @discardableResult
    public func addRule(for snapshot: WiFiSnapshot, timeoutSeconds: Int = AppSettings.defaultTrustedTimeout) -> NetworkRule? {
        if let existing = PolicyResolver.matchingRule(for: snapshot, in: settings.rules) {
            // 이름을 몰랐던 규칙에 이제 이름을 알게 됐다면 채워 넣는다.
            if existing.ssid == nil, let ssid = snapshot.ssid, existing.routerMAC == snapshot.routerMAC,
               let idx = settings.rules.firstIndex(where: { $0.id == existing.id }) {
                settings.rules[idx].ssid = ssid
                return settings.rules[idx]
            }
            return existing
        }
        guard var rule = PolicyResolver.makeRule(for: snapshot, timeoutSeconds: timeoutSeconds) else { return nil }
        // 이름이 같은 다른 공유기의 네트워크가 이미 있으면 구분할 수 있게 공유기 주소 끝자리를 붙인다.
        if settings.rules.contains(where: { $0.name == rule.name }), let mac = rule.routerMAC {
            rule.name += " (공유기 …\(mac.suffix(5)))"
        }
        settings.rules.append(rule)
        return rule
    }

    /// 와이파이 이름을 직접 입력해 규칙을 추가한다. 빈 이름이거나 이미 있으면 nil.
    @discardableResult
    public func addRule(ssid rawSSID: String, timeoutSeconds: Int) -> NetworkRule? {
        let ssid = rawSSID.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !ssid.isEmpty, !settings.rules.contains(where: { $0.ssid == ssid }) else { return nil }
        let rule = NetworkRule(name: ssid, ssid: ssid, routerMAC: nil, timeoutSeconds: timeoutSeconds)
        settings.rules.append(rule)
        return rule
    }

    public func removeRule(id: UUID) {
        settings.rules.removeAll { $0.id == id }
    }
}
