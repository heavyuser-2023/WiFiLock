import Foundation

/// 잠금 엔진이 매 초 판단한 상태
public enum EngineState: Equatable {
    /// 사용자가 자동 잠금을 껐음
    case disabled
    /// 일시 중지 중 (nil 이면 다시 켤 때까지)
    case paused(until: Date?)
    /// 현재 네트워크 규칙이 "잠그지 않음"
    case neverLock
    /// 다른 앱이 화면 꺼짐 방지를 요청해 보류 중
    case heldByAssertion(holders: [String])
    /// 화면이 이미 잠겨 있음
    case screenLocked
    /// 잠금까지 남은 시간을 세는 중
    case counting(remaining: TimeInterval, timeout: TimeInterval)
    /// 잠금 직전 경고 중
    case warning(remaining: TimeInterval)
    /// 방금 잠금 명령을 보냄
    case lockTriggered
    /// 이미 이번 자리비움 동안 잠갔으므로 사용자가 다시 움직일 때까지 기다림
    case waitingForActivity
}

/// 한 번의 판단에 필요한 입력값 (시스템 정보는 모두 바깥에서 넣어 주므로 테스트하기 쉽다)
public struct TickInput {
    public var now: Date
    /// 마지막 키보드·마우스 입력 후 지난 시간(초)
    public var idleSeconds: TimeInterval
    public var policy: LockPolicy
    public var enabled: Bool
    /// 일시 중지 상태: nil = 중지 아님, .some(nil) = 무기한, .some(date) = 해당 시각까지
    public var pause: Date??
    public var screenIsLocked: Bool
    /// 화면 꺼짐 방지를 요청한 앱 이름 (설정에서 존중하지 않기로 했으면 빈 배열)
    public var assertionHolders: [String]
    public var warnBeforeLock: Bool
    public var warningSeconds: TimeInterval

    public init(now: Date, idleSeconds: TimeInterval, policy: LockPolicy, enabled: Bool = true, pause: Date?? = nil,
                screenIsLocked: Bool = false, assertionHolders: [String] = [], warnBeforeLock: Bool = true, warningSeconds: TimeInterval = 10) {
        self.now = now
        self.idleSeconds = idleSeconds
        self.policy = policy
        self.enabled = enabled
        self.pause = pause
        self.screenIsLocked = screenIsLocked
        self.assertionHolders = assertionHolders
        self.warnBeforeLock = warnBeforeLock
        self.warningSeconds = warningSeconds
    }
}

public struct TickOutput: Equatable {
    public var state: EngineState
    /// true 면 지금 화면을 잠가야 한다
    public var shouldLock: Bool

    public var warningRemaining: TimeInterval? {
        if case .warning(let r) = state { return r }
        return nil
    }
}

/// 자리비움 시간과 현재 규칙으로 잠글 시점을 정하는 상태 기계.
///
/// - 같은 자리비움 동안에는 한 번만 잠근다. 사용자가 다시 입력하면(자리비움 시간이 줄어들면) 다시 무장한다.
/// - 경고를 켜 두었으면, 경고가 최소한 `warningSeconds` 동안 보인 뒤에 잠근다.
///   (예: 긴 시간 규칙의 네트워크에서 짧은 규칙으로 바뀌어 곧바로 시간이 넘었더라도 바로 잠그지 않고 경고부터 띄운다)
public final class LockScheduler {
    private var armed = true
    private var lastIdle: TimeInterval?
    private var warningDeadline: Date?

    public init() {}

    /// 실제로 쓰는 경고 시간. 잠금 시간의 절반을 넘지 않게 해서,
    /// 계속 사용 중인데도 경고 단계에 머물다 잠기는 일이 없게 한다.
    public static func effectiveWarning(timeout: TimeInterval, warningSeconds: TimeInterval) -> TimeInterval {
        max(0, min(warningSeconds, timeout / 2))
    }

    /// 사용자가 다시 움직였다고 알려 준다 (예: 화면 잠금이 풀림).
    public func noteUserActivity() {
        armed = true
        warningDeadline = nil
    }

    public func tick(_ input: TickInput) -> TickOutput {
        // 자리비움 시간이 줄었거나 1초 안쪽이면 그 사이 사용자가 입력한 것이다.
        if input.idleSeconds < 1.0 || (lastIdle.map { input.idleSeconds + 0.5 < $0 } ?? false) {
            armed = true
            warningDeadline = nil
        }
        lastIdle = input.idleSeconds

        if input.screenIsLocked {
            armed = false
            warningDeadline = nil
            return TickOutput(state: .screenLocked, shouldLock: false)
        }
        guard input.enabled else {
            warningDeadline = nil
            return TickOutput(state: .disabled, shouldLock: false)
        }
        if let pause = input.pause {
            if let until = pause {
                if until > input.now {
                    warningDeadline = nil
                    return TickOutput(state: .paused(until: until), shouldLock: false)
                }
            } else {
                warningDeadline = nil
                return TickOutput(state: .paused(until: nil), shouldLock: false)
            }
        }
        if input.policy.neverLocks {
            warningDeadline = nil
            return TickOutput(state: .neverLock, shouldLock: false)
        }
        if !input.assertionHolders.isEmpty {
            warningDeadline = nil
            return TickOutput(state: .heldByAssertion(holders: input.assertionHolders), shouldLock: false)
        }
        guard armed else {
            return TickOutput(state: .waitingForActivity, shouldLock: false)
        }

        let timeout = TimeInterval(input.policy.timeoutSeconds)
        let remaining = timeout - input.idleSeconds
        let warning = LockScheduler.effectiveWarning(timeout: timeout, warningSeconds: input.warningSeconds)

        if input.warnBeforeLock, warning > 0 {
            if remaining <= warning {
                if warningDeadline == nil {
                    // 경고는 최소 warning 초 동안 보이게 한다.
                    warningDeadline = input.now.addingTimeInterval(max(remaining, warning))
                }
                let left = warningDeadline!.timeIntervalSince(input.now)
                if left <= 0 {
                    armed = false
                    warningDeadline = nil
                    return TickOutput(state: .lockTriggered, shouldLock: true)
                }
                return TickOutput(state: .warning(remaining: left), shouldLock: false)
            }
            warningDeadline = nil
            return TickOutput(state: .counting(remaining: remaining, timeout: timeout), shouldLock: false)
        }

        if remaining <= 0 {
            armed = false
            return TickOutput(state: .lockTriggered, shouldLock: true)
        }
        return TickOutput(state: .counting(remaining: remaining, timeout: timeout), shouldLock: false)
    }
}
