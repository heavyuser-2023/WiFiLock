import Foundation

public enum KoreanFormat {
    /// 잠금 시간 표시: 0 → "잠그지 않음", 90 → "1분 30초", 3600 → "1시간"
    public static func timeout(_ seconds: Int) -> String {
        if seconds <= 0 { return "잠그지 않음" }
        return duration(seconds)
    }

    /// 기간 표시: 45 → "45초", 120 → "2분", 5400 → "1시간 30분"
    public static func duration(_ seconds: Int) -> String {
        let s = max(0, seconds)
        let h = s / 3600
        let m = (s % 3600) / 60
        let sec = s % 60
        var parts: [String] = []
        if h > 0 { parts.append("\(h)시간") }
        if m > 0 { parts.append("\(m)분") }
        if sec > 0 || parts.isEmpty { parts.append("\(sec)초") }
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

    /// 오후 3:05 같은 시각 표시
    public static func clock(_ date: Date, locale: Locale = Locale(identifier: "ko_KR"), timeZone: TimeZone = .current) -> String {
        let f = DateFormatter()
        f.locale = locale
        f.timeZone = timeZone
        f.dateFormat = "a h:mm"
        return f.string(from: date)
    }
}

public extension WiFiSnapshot {
    /// 메뉴에 보여 줄 연결 상태 문구
    var koreanDescription: String {
        switch status {
        case .unavailable: return "와이파이 장치 없음"
        case .poweredOff: return "와이파이 꺼짐"
        case .disconnected: return "와이파이 연결 안 됨"
        case .connected:
            if let ssid { return ssid }
            return "이름 확인 불가 (위치 권한 필요)"
        }
    }
}

public extension PolicySource {
    var koreanDescription: String {
        switch self {
        case .rule(let r): return "등록된 네트워크 '\(r.name)'"
        case .unknownWiFi: return "등록되지 않은 와이파이"
        case .noWiFi: return "와이파이 연결 없음"
        }
    }
}
