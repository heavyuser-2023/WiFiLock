import XCTest
@testable import WiFiLockCore

final class SchedulerTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_000_000)
    let fiveMin = LockPolicy(timeoutSeconds: 300, source: .unknownWiFi)

    /// 자리비움 시간이 1초씩 늘어나는 상황을 흉내 낸다.
    private func run(_ s: LockScheduler, from idle0: Int, to idle1: Int, policy: LockPolicy, warn: Bool, warningSeconds: Double = 10,
                     startAt: Date? = nil) -> [TickOutput] {
        var out: [TickOutput] = []
        let base = startAt ?? t0
        for (i, idle) in (idle0...idle1).enumerated() {
            out.append(s.tick(TickInput(now: base.addingTimeInterval(Double(i)), idleSeconds: Double(idle), policy: policy,
                                        warnBeforeLock: warn, warningSeconds: warningSeconds)))
        }
        return out
    }

    func testCountsDownAndLocksExactlyOnceWithoutWarning() {
        let s = LockScheduler()
        let outs = run(s, from: 0, to: 400, policy: fiveMin, warn: false)
        XCTAssertEqual(outs.filter(\.shouldLock).count, 1)
        let lockIndex = outs.firstIndex(where: \.shouldLock)!
        XCTAssertEqual(lockIndex, 300, "자리비움 300초에 잠가야 한다")
        XCTAssertEqual(outs[10].state, .counting(remaining: 290, timeout: 300))
        XCTAssertEqual(outs[350].state, .waitingForActivity)
    }

    func testWarningPhaseThenLock() {
        let s = LockScheduler()
        let outs = run(s, from: 0, to: 330, policy: fiveMin, warn: true, warningSeconds: 10)
        XCTAssertEqual(outs.filter(\.shouldLock).count, 1)
        // 289초까지는 세는 중, 290초부터 경고
        XCTAssertEqual(outs[289].state, .counting(remaining: 11, timeout: 300))
        XCTAssertEqual(outs[290].warningRemaining, 10)
        XCTAssertEqual(outs[295].warningRemaining, 5)
        let lockIndex = outs.firstIndex(where: \.shouldLock)!
        XCTAssertEqual(lockIndex, 300)
    }

    func testActivityDuringWarningCancelsIt() {
        let s = LockScheduler()
        _ = run(s, from: 0, to: 295, policy: fiveMin, warn: true)
        // 사용자가 마우스를 움직여 자리비움 시간이 0 으로 돌아감
        let after = s.tick(TickInput(now: t0.addingTimeInterval(296), idleSeconds: 0.2, policy: fiveMin))
        XCTAssertEqual(after.state, .counting(remaining: 299.8, timeout: 300))
        XCTAssertFalse(after.shouldLock)
    }

    func testRearmsAfterUserReturns() {
        let s = LockScheduler()
        let first = run(s, from: 0, to: 310, policy: fiveMin, warn: false)
        XCTAssertEqual(first.filter(\.shouldLock).count, 1)
        // 사용자가 돌아와 입력 → 다시 300초 자리비움 → 다시 잠금
        let second = run(s, from: 0, to: 310, policy: fiveMin, warn: false, startAt: t0.addingTimeInterval(1000))
        XCTAssertEqual(second.filter(\.shouldLock).count, 1)
    }

    func testPolicyShrinkWhileIdleWarnsFirst() {
        // 30분 규칙 네트워크에서 10분째 자리비움 중 와이파이가 끊겨 1분 규칙으로 바뀐 경우
        let s = LockScheduler()
        let long = LockPolicy(timeoutSeconds: 1800, source: .unknownWiFi)
        let short = LockPolicy(timeoutSeconds: 60, source: .noWiFi)
        _ = run(s, from: 590, to: 600, policy: long, warn: true)
        let a = s.tick(TickInput(now: t0.addingTimeInterval(11), idleSeconds: 601, policy: short, warningSeconds: 10))
        XCTAssertEqual(a.warningRemaining, 10, "바로 잠그지 않고 경고를 먼저 띄운다")
        XCTAssertFalse(a.shouldLock)
        var locked = false
        for i in 1...10 {
            let o = s.tick(TickInput(now: t0.addingTimeInterval(11 + Double(i)), idleSeconds: 601 + Double(i), policy: short, warningSeconds: 10))
            if o.shouldLock { locked = true; XCTAssertEqual(i, 10) }
        }
        XCTAssertTrue(locked)
    }

    func testPolicyShrinkWithoutWarningLocksImmediately() {
        let s = LockScheduler()
        let short = LockPolicy(timeoutSeconds: 60, source: .noWiFi)
        let o = s.tick(TickInput(now: t0, idleSeconds: 601, policy: short, warnBeforeLock: false))
        XCTAssertTrue(o.shouldLock)
    }

    func testNeverLockPolicy() {
        let s = LockScheduler()
        let never = LockPolicy(timeoutSeconds: 0, source: .unknownWiFi)
        let outs = run(s, from: 0, to: 5000, policy: never, warn: true)
        XCTAssertTrue(outs.allSatisfy { !$0.shouldLock && $0.state == .neverLock })
    }

    func testDisabled() {
        let s = LockScheduler()
        let o = s.tick(TickInput(now: t0, idleSeconds: 9999, policy: fiveMin, enabled: false))
        XCTAssertEqual(o.state, .disabled)
        XCTAssertFalse(o.shouldLock)
    }

    func testPausedUntilAndExpiry() {
        let s = LockScheduler()
        let until = t0.addingTimeInterval(60)
        let p = s.tick(TickInput(now: t0, idleSeconds: 9999, policy: fiveMin, pause: .some(until), warnBeforeLock: false))
        XCTAssertEqual(p.state, .paused(until: until))
        XCTAssertFalse(p.shouldLock)
        // 일시 중지가 끝나면 (여전히 자리비움이 길면) 잠근다
        let q = s.tick(TickInput(now: t0.addingTimeInterval(61), idleSeconds: 10060, policy: fiveMin, pause: .some(until), warnBeforeLock: false))
        XCTAssertTrue(q.shouldLock)
    }

    func testPausedIndefinitely() {
        let s = LockScheduler()
        let p = s.tick(TickInput(now: t0, idleSeconds: 9999, policy: fiveMin, pause: .some(nil)))
        XCTAssertEqual(p.state, .paused(until: nil))
        XCTAssertFalse(p.shouldLock)
    }

    func testAssertionHoldsLock() {
        let s = LockScheduler()
        let o = s.tick(TickInput(now: t0, idleSeconds: 9999, policy: fiveMin, assertionHolders: ["QuickTime Player"]))
        XCTAssertEqual(o.state, .heldByAssertion(holders: ["QuickTime Player"]))
        XCTAssertFalse(o.shouldLock)
        // 재생이 끝나 요청이 사라지면 경고 후 잠금
        let w = s.tick(TickInput(now: t0.addingTimeInterval(1), idleSeconds: 10000, policy: fiveMin))
        XCTAssertNotNil(w.warningRemaining)
    }

    func testScreenAlreadyLockedDoesNotLockAgain() {
        let s = LockScheduler()
        for i in 0..<100 {
            let o = s.tick(TickInput(now: t0.addingTimeInterval(Double(i)), idleSeconds: 500 + Double(i), policy: fiveMin, screenIsLocked: true))
            XCTAssertEqual(o.state, .screenLocked)
            XCTAssertFalse(o.shouldLock)
        }
        // 잠금이 풀렸지만 (예: 애플워치로 풀어) 아직 입력이 없으면 다시 잠그지 않는다
        let o = s.tick(TickInput(now: t0.addingTimeInterval(100), idleSeconds: 600, policy: fiveMin))
        XCTAssertEqual(o.state, .waitingForActivity)
        // 입력이 생기면 다시 무장
        let r = s.tick(TickInput(now: t0.addingTimeInterval(101), idleSeconds: 0.1, policy: fiveMin))
        if case .counting = r.state {} else { XCTFail("다시 세기 시작해야 한다: \(r.state)") }
    }

    func testNoteUserActivityRearms() {
        let s = LockScheduler()
        _ = run(s, from: 0, to: 310, policy: fiveMin, warn: false)
        s.noteUserActivity()
        let o = s.tick(TickInput(now: t0.addingTimeInterval(400), idleSeconds: 311, policy: fiveMin, warnBeforeLock: false))
        XCTAssertTrue(o.shouldLock)
    }

    func testTimeoutShorterThanWarning() {
        let s = LockScheduler()
        let p = LockPolicy(timeoutSeconds: 5, source: .noWiFi)
        let outs = run(s, from: 0, to: 20, policy: p, warn: true, warningSeconds: 10)
        // 경고는 잠금 시간의 절반(2.5초)으로 줄어든다: 3초에 경고 시작, 6초쯤 잠금
        XCTAssertNil(outs[0].warningRemaining, "자리비움 0초에서는 경고하지 않는다")
        XCTAssertEqual(outs.firstIndex(where: { $0.warningRemaining != nil }), 3)
        XCTAssertEqual(outs.firstIndex(where: \.shouldLock), 6)
        XCTAssertEqual(outs.filter(\.shouldLock).count, 1)
    }

    /// 검토에서 찾은 결함: 경고 시간이 잠금 시간 이상이면 계속 입력 중인데도 잠갔다.
    func testContinuousActivityNeverLocksEvenIfWarningExceedsTimeout() {
        for (timeout, warning) in [(30, 30.0), (30, 60.0), (60, 60.0), (5, 10.0)] {
            let s = LockScheduler()
            let p = LockPolicy(timeoutSeconds: timeout, source: .unknownWiFi)
            for i in 0..<300 {
                // 계속 마우스를 움직이는 사용자: 매 초 자리비움 0.0 ~ 0.3초
                let idle = Double(i % 4) * 0.1
                let o = s.tick(TickInput(now: t0.addingTimeInterval(Double(i)), idleSeconds: idle, policy: p, warningSeconds: warning))
                XCTAssertFalse(o.shouldLock, "잠금 \(timeout)초 / 경고 \(warning)초, \(i)초째")
                XCTAssertNil(o.warningRemaining, "잠금 \(timeout)초 / 경고 \(warning)초, \(i)초째")
            }
        }
    }

    func testSlowTypingJustUnderOneSecondDoesNotLock() {
        // 0.9초 간격으로 입력이 이어지는 경우 (자리비움이 0.5초 넘게 줄지 않을 수 있음)
        let s = LockScheduler()
        let p = LockPolicy(timeoutSeconds: 30, source: .unknownWiFi)
        for i in 0..<200 {
            let idle = [0.6, 0.9, 0.7, 0.95][i % 4]
            let o = s.tick(TickInput(now: t0.addingTimeInterval(Double(i)), idleSeconds: idle, policy: p, warningSeconds: 30))
            XCTAssertFalse(o.shouldLock)
        }
    }

    func testEffectiveWarning() {
        XCTAssertEqual(LockScheduler.effectiveWarning(timeout: 300, warningSeconds: 10), 10)
        XCTAssertEqual(LockScheduler.effectiveWarning(timeout: 30, warningSeconds: 30), 15)
        XCTAssertEqual(LockScheduler.effectiveWarning(timeout: 30, warningSeconds: 60), 15)
        XCTAssertEqual(LockScheduler.effectiveWarning(timeout: 0, warningSeconds: 10), 0)
    }
}
