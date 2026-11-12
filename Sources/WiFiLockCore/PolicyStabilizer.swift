import Foundation

/// 와이파이가 잠깐 끊겼다 붙는 경우 짧은 잠금 시간이 바로 적용되지 않도록 한다.
///
/// - 바뀐 상태가 지금보다 **더 짧은(엄격한)** 잠금 시간을 뜻하면 `downgradeDelay` 초 동안 그대로 유지될 때만 받아들인다.
/// - 더 길거나 같은 잠금 시간이면 바로 받아들인다.
/// - 설정 변경은 늘 바로 반영된다 (매번 현재 설정으로 다시 계산하므로).
public final class PolicyStabilizer {
    public var downgradeDelay: TimeInterval
    public private(set) var stable: WiFiSnapshot?
    private var pending: (policy: LockPolicy, snapshot: WiFiSnapshot, since: Date)?

    public init(downgradeDelay: TimeInterval = 15) {
        self.downgradeDelay = downgradeDelay
    }

    /// 확인 중인 변화가 있으면 그 시작 시각
    public var pendingSince: Date? { pending?.since }

    /// 잠금 시간을 정할 때 쓸 와이파이 상태를 돌려준다.
    public func update(_ snapshot: WiFiSnapshot, settings: AppSettings, now: Date) -> WiFiSnapshot {
        guard let current = stable else {
            stable = snapshot
            return snapshot
        }
        if snapshot == current {
            pending = nil
            return current
        }
        let newPolicy = PolicyResolver.resolve(snapshot: snapshot, settings: settings)
        let oldPolicy = PolicyResolver.resolve(snapshot: current, settings: settings)
        guard PolicyStabilizer.isStricter(newPolicy, than: oldPolicy) else {
            stable = snapshot
            pending = nil
            return snapshot
        }
        if let p = pending, p.policy == newPolicy {
            pending = (newPolicy, snapshot, p.since)
            if now.timeIntervalSince(p.since) >= downgradeDelay {
                stable = snapshot
                pending = nil
                return snapshot
            }
        } else {
            pending = (newPolicy, snapshot, now)
        }
        return current
    }

    /// a 가 b 보다 더 빨리 잠그는지
    public static func isStricter(_ a: LockPolicy, than b: LockPolicy) -> Bool {
        if a.neverLocks { return false }
        if b.neverLocks { return true }
        return a.timeoutSeconds < b.timeoutSeconds
    }
}
