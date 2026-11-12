import AppKit
import Combine
import Foundation
import WiFiLockCore

/// 화면에 보여 줄 현재 상황
struct LockStatus: Equatable {
    var state: EngineState = .counting(remaining: 0, timeout: 0)
    var policy: LockPolicy = LockPolicy(timeoutSeconds: 0, source: .noWiFi)
    /// 실제 와이파이 상태 (메뉴 표시용)
    var snapshot: WiFiSnapshot = .unknown
    /// 와이파이 변화가 잠금 시간을 줄이는 쪽이라 확인 중 (순간 끊김 대비)
    var policyChangePending = false
    var lastLockAt: Date?
}

/// 매 초 시스템 상태를 읽어 잠금 여부를 판단하고 실행한다.
final class LockController: ObservableObject {
    @Published private(set) var status = LockStatus()
    /// nil = 일시 중지 아님, .some(nil) = 다시 켤 때까지, .some(date) = 그 시각까지
    @Published private(set) var pause: Date?? = nil

    let settingsStore: SettingsStore
    let wifi: WiFiMonitor
    private let locker: ScreenLocking
    private let scheduler = LockScheduler()
    let stabilizer = PolicyStabilizer(downgradeDelay: 15)
    private let overrideTimeout: Int?
    private var timer: Timer?
    private var cancellables = Set<AnyCancellable>()
    private var observers: [NSObjectProtocol] = []
    /// 잠금 해제·잠자기에서 깨어난 시각. 입력 없이(애플워치 등) 풀린 경우에도 이 시각부터 자리비움을 센다.
    private var lastPresenceAt: Date?
    private var isTicking = false

    /// 시험용 주입 지점 (자가 진단에서 사용)
    var idleProvider: () -> TimeInterval = SystemProbes.idleSeconds
    var screenLockedProvider: () -> Bool = { SystemProbes.isScreenLocked() || !SystemProbes.isSessionOnConsole() }
    var assertionProvider: () -> [String] = SystemProbes.displaySleepPreventers
    var clock: () -> Date = Date.init

    init(settingsStore: SettingsStore, wifi: WiFiMonitor, locker: ScreenLocking, overrideTimeout: Int? = nil) {
        self.settingsStore = settingsStore
        self.wifi = wifi
        self.locker = locker
        self.overrideTimeout = overrideTimeout
    }

    func start() {
        // 설정이나 와이파이가 바뀌면 바로 다시 판단한다. (@Published 는 바뀌기 직전에 알리므로 한 박자 뒤에 판단)
        settingsStore.$settings.dropFirst().sink { [weak self] _ in
            DispatchQueue.main.async { self?.tick() }
        }.store(in: &cancellables)
        wifi.$snapshot.dropFirst().sink { [weak self] _ in
            DispatchQueue.main.async { self?.tick() }
        }.store(in: &cancellables)

        let dnc = DistributedNotificationCenter.default()
        observers.append(dnc.addObserver(forName: NSNotification.Name("com.apple.screenIsUnlocked"), object: nil, queue: .main) { [weak self] _ in
            AppLog.info("화면 잠금 해제됨")
            self?.notePresence()
        })
        observers.append(dnc.addObserver(forName: NSNotification.Name("com.apple.screenIsLocked"), object: nil, queue: .main) { [weak self] _ in
            AppLog.info("화면 잠김")
            self?.tick()
        })
        let wnc = NSWorkspace.shared.notificationCenter
        observers.append(wnc.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in
            AppLog.info("잠자기에서 깨어남")
            self?.wifi.refresh()
            self?.notePresence()
        })
        observers.append(wnc.addObserver(forName: NSWorkspace.sessionDidBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
            AppLog.info("이 사용자 세션으로 돌아옴")
            self?.notePresence()
        })

        let t = Timer(timeInterval: 1, repeats: true) { [weak self] _ in self?.tick() }
        t.tolerance = 0.2
        RunLoop.main.add(t, forMode: .common) // 메뉴가 열려 있어도 계속 동작
        timer = t
        tick()
    }

    func stop() {
        timer?.invalidate()
        timer = nil
        for o in observers {
            DistributedNotificationCenter.default().removeObserver(o)
            NSWorkspace.shared.notificationCenter.removeObserver(o)
        }
        observers.removeAll()
    }

    /// 사용자가 자리에 있다는 신호 (잠금 해제, 깨어남, 세션 복귀)
    func notePresence() {
        lastPresenceAt = clock()
        scheduler.noteUserActivity()
        tick()
    }

    /// 잠금 시간을 정할 때 쓰는 와이파이 상태 (순간 끊김 대비를 거친 값)
    private var policySnapshot: WiFiSnapshot { stabilizer.stable ?? wifi.snapshot }

    var currentPolicy: LockPolicy {
        var p = PolicyResolver.resolve(snapshot: policySnapshot, settings: settingsStore.settings)
        if let o = overrideTimeout { p.timeoutSeconds = o }
        return p
    }

    @discardableResult
    func tick() -> TickOutput? {
        // 잠금 명령 처리 중 다시 들어오는 것을 막는다
        guard !isTicking else { return nil }
        isTicking = true
        defer { isTicking = false }

        let now = clock()
        if let p = pause, let until = p, until <= now {
            pause = nil
            AppLog.info("일시 중지 끝")
        }
        let settings = settingsStore.settings
        _ = stabilizer.update(wifi.snapshot, settings: settings, now: now)
        let policy = currentPolicy
        var idle = idleProvider()
        if let presence = lastPresenceAt {
            idle = min(idle, max(0, now.timeIntervalSince(presence)))
        }
        let holders = settings.respectDisplaySleepAssertions ? assertionProvider() : []
        let input = TickInput(
            now: now,
            idleSeconds: idle,
            policy: policy,
            enabled: settings.enabled,
            pause: pause,
            screenIsLocked: screenLockedProvider(),
            assertionHolders: holders,
            warnBeforeLock: settings.warnBeforeLock,
            warningSeconds: TimeInterval(settings.warningSeconds)
        )
        let out = scheduler.tick(input)
        var newStatus = LockStatus(state: out.state, policy: policy, snapshot: wifi.snapshot,
                                   policyChangePending: stabilizer.pendingSince != nil, lastLockAt: status.lastLockAt)
        if out.shouldLock {
            let reason = "\(policy.source.koreanDescription), 자리비움 \(Int(idle))초 ≥ \(KoreanFormat.timeout(policy.timeoutSeconds))"
            if locker.lock(reason: reason) {
                newStatus.lastLockAt = now
            }
        }
        if newStatus != status { status = newStatus }
        return out
    }

    // MARK: - 사용자 동작

    func lockNow() {
        if locker.lock(reason: "사용자가 '지금 잠그기' 선택") {
            status.lastLockAt = clock()
        }
    }

    func pause(for seconds: TimeInterval?) {
        if let seconds {
            pause = .some(clock().addingTimeInterval(seconds))
            AppLog.info("일시 중지: \(Int(seconds))초")
        } else {
            pause = .some(nil)
            AppLog.info("일시 중지: 다시 켤 때까지")
        }
        tick()
    }

    func resume() {
        pause = nil
        AppLog.info("다시 시작")
        tick()
    }

    var isPaused: Bool { pause != nil }
}
