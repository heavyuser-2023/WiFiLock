import Foundation

public enum AppLang: String, Sendable {
    case ko
    case en

    /// 단위 테스트가 영문 환경에서도 흔들리지 않도록 기본값은 `.ko` 로 두고, 앱 시작 시 시스템 언어로 맞춘다.
    public static var current: AppLang = .ko

    public static var isKo: Bool { current == .ko }

    public static func pick<T>(_ ko: T, _ en: T) -> T {
        isKo ? ko : en
    }

    public static func parse(_ tag: String) -> AppLang? {
        let clean = tag.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty else { return nil }
        let withoutEncoding = clean.split(whereSeparator: { $0 == "." || $0 == "@" }).first.map(String.init) ?? ""
        let primary = withoutEncoding.split(whereSeparator: { $0 == "_" || $0 == "-" }).first?.lowercased() ?? ""
        guard !primary.isEmpty else { return nil }
        if primary == "ko" || primary == "kor" || primary == "korean" {
            return .ko
        }
        return .en
    }

    public static func detectSystem() -> AppLang {
        if let env = ProcessInfo.processInfo.environment["WIFILOCK_LANG"], let parsed = parse(env) {
            return parsed
        }
        if let preferred = Locale.preferredLanguages.first, let parsed = parse(preferred) {
            return parsed
        }
        return .en
    }
}

public enum KoreanFormat {
    /// 잠금 시간 표시: 0 → "잠그지 않음"/"Never", 90 → "1분 30초"/"1 min 30 sec", 3600 → "1시간"/"1 hr"
    public static func timeout(_ seconds: Int, lang: AppLang = AppLang.current) -> String {
        if seconds <= 0 { return lang == .ko ? "잠그지 않음" : "Never" }
        return duration(seconds, lang: lang)
    }

    /// 기간 표시: 45 → "45초"/"45 sec", 120 → "2분"/"2 min", 5400 → "1시간 30분"/"1 hr 30 min"
    public static func duration(_ seconds: Int, lang: AppLang = AppLang.current) -> String {
        let s = max(0, seconds)
        let h = s / 3600
        let m = (s % 3600) / 60
        let sec = s % 60
        var parts: [String] = []
        if lang == .ko {
            if h > 0 { parts.append("\(h)시간") }
            if m > 0 { parts.append("\(m)분") }
            if sec > 0 || parts.isEmpty { parts.append("\(sec)초") }
        } else {
            if h > 0 { parts.append("\(h) hr") }
            if m > 0 { parts.append("\(m) min") }
            if sec > 0 || parts.isEmpty { parts.append("\(sec) sec") }
        }
        return parts.joined(separator: " ")
    }

    /// 남은 시간 표시 (올림): 59.2 → "1:00", 3725 → "1:02:05"
    public static func countdown(_ remaining: TimeInterval) -> String {
        let total = max(0, Int(remaining.rounded(.up)))
        let h = total / 3600
        let m = (total % 3600) / 60
        let s = total % 60
        if h > 0 { return String(format: "%d:%02d:%02d", h, m, s) }
        return String(format: "%d:%02d", m, s)
    }

    /// 오후 3:05 / 3:05 PM 같은 시각 표시
    public static func clock(
        _ date: Date,
        lang: AppLang = AppLang.current,
        locale: Locale? = nil,
        timeZone: TimeZone = .current
    ) -> String {
        let f = DateFormatter()
        f.locale = locale ?? Locale(identifier: lang == .ko ? "ko_KR" : "en_US_POSIX")
        f.timeZone = timeZone
        f.dateFormat = lang == .ko ? "a h:mm" : "h:mm a"
        return f.string(from: date)
    }
}

public extension WiFiSnapshot {
    /// 메뉴에 보여 줄 연결 상태 문구
    var koreanDescription: String {
        description(in: AppLang.current)
    }

    func description(in lang: AppLang) -> String {
        switch (lang, status) {
        case (.ko, .unavailable): return "와이파이 장치 없음"
        case (.ko, .poweredOff): return "와이파이 꺼짐"
        case (.ko, .disconnected): return "와이파이 연결 안 됨"
        case (.ko, .connected):
            if let ssid { return ssid }
            return "이름 확인 불가 (위치 권한 필요)"
        case (.en, .unavailable): return "No Wi-Fi hardware"
        case (.en, .poweredOff): return "Wi-Fi Off"
        case (.en, .disconnected): return "Not Connected"
        case (.en, .connected):
            if let ssid { return ssid }
            return "Name unavailable (Location permission required)"
        }
    }
}

public extension PolicySource {
    var koreanDescription: String {
        description(in: AppLang.current)
    }

    func description(in lang: AppLang) -> String {
        switch (lang, self) {
        case (.ko, .rule(let r)): return "등록된 네트워크 '\(r.name)'"
        case (.ko, .unknownWiFi): return "등록되지 않은 와이파이"
        case (.ko, .noWiFi): return "와이파이 연결 없음"
        case (.en, .rule(let r)): return "Registered network '\(r.name)'"
        case (.en, .unknownWiFi): return "Unregistered Wi-Fi"
        case (.en, .noWiFi): return "No Wi-Fi connection"
        }
    }
}
