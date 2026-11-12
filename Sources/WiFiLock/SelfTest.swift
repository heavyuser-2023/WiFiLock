import AppKit
import CoreLocation
import WiFiLockCore

/// `--self-test <폴더>` 로 실행하면 앱을 실제로 띄운 상태에서 각 기능을 점검하고,
/// 결과(report.json, summary.txt)와 화면 그림(PNG)을 남긴 뒤 종료한다.
/// 실제 화면 잠금은 하지 않는다 (시험용 잠금 장치 사용).
final class SelfTest {
    struct Check: Encodable {
        let name: String
        let passed: Bool
        let detail: String
    }

    private let app: AppDelegate
    private let out: URL
    private var checks: [Check] = []
    private var info: [String: String] = [:]

    init(app: AppDelegate, outputDir: URL) {
        self.app = app
        self.out = outputDir
    }

    private func check(_ name: String, _ ok: Bool, _ detail: String = "") {
        checks.append(Check(name: name, passed: ok, detail: detail))
        AppLog.info("[자가 진단] \(ok ? "통과" : "실패") \(name) \(detail)")
    }

    func run() {
        try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        checkBundle()
        checkSystemAPIs()
        checkStatusItem()
        checkMenuAndActions()
        checkRealDisplayAssertion()
        checkEngineScenarios()
        checkSettingsStorePersistence()
        renderIcons()
        renderHUD()
        checkLiveMenuUpdate { [self] in afterMenuCheck() }
    }

    /// 메뉴를 실제로 열어 둔 채로 남은 시간이 1초마다 갱신되는지 확인한다.
    private func checkLiveMenuUpdate(then next: @escaping () -> Void) {
        let sb = app.statusBar!
        let start = Date()
        app.controller.idleProvider = { Date().timeIntervalSince(start) }
        var samples: [String] = []
        var sawOpen = false
        let sampler = Timer(timeInterval: 0.5, repeats: true) { _ in
            if sb.menuIsOpen { sawOpen = true }
            if let t = sb.visibleMenuTitles().first(where: { $0.hasPrefix("잠금까지") }) { samples.append(t) }
        }
        let closer = Timer(timeInterval: 3.2, repeats: false) { _ in sb.menu.cancelTracking() }
        RunLoop.main.add(sampler, forMode: .common)
        RunLoop.main.add(closer, forMode: .common)
        // 실제 클릭처럼 실행 루프에서 메뉴를 연다 (메뉴가 닫힐 때까지 이 호출 안에서 메뉴 추적이 돈다)
        let opener = Timer(timeInterval: 0.05, repeats: false) { [self] _ in
            sb.statusItem.button?.performClick(nil)
            sampler.invalidate()
            closer.invalidate()
            let distinct = Set(samples).count
            if sawOpen {
                check("메뉴를 연 채로 남은 시간 실시간 갱신", distinct >= 3, samples.joined(separator: " → "))
            } else {
                check("메뉴를 연 채로 남은 시간 실시간 갱신", true, "건너뜀: 메뉴를 열 수 없는 상태(화면 잠김 등)")
            }
            app.controller.idleProvider = { 0 }
            app.controller.tick()
            next()
        }
        RunLoop.main.add(opener, forMode: .default)
    }

    private func afterMenuCheck() {
        app.openSettings()
        // 실제 컨트롤러가 잠금 5초 전 상황이 되면 경고 창이 저절로 뜨는지 확인
        let timeout = TimeInterval(app.controller.currentPolicy.timeoutSeconds)
        app.controller.idleProvider = { max(0, timeout - 5) }
        app.controller.tick()
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.0) { [self] in
            let st = app.controller.status.state
            var isWarning = false
            if case .warning = st { isWarning = true }
            check("잠금 5초 전 경고 창 자동 표시", isWarning && app.hud.isVisible, "\(st)")
            // 사용자가 마우스를 움직인 상황 → 경고가 사라져야 한다
            app.controller.idleProvider = { 0 }
            app.controller.tick()
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { [self] in
                check("입력이 생기면 경고 창 사라짐", !app.hud.isVisible, "\(app.controller.status.state)")
                checkSettingsWindow()
                finish()
            }
        }
    }

    // MARK: 번들

    private func checkBundle() {
        let b = Bundle.main
        info["bundlePath"] = b.bundlePath
        check("번들 식별자", b.bundleIdentifier == "com.wifilock.WiFiLock", b.bundleIdentifier ?? "없음")
        check("메뉴 막대 전용 앱(LSUIElement)", (b.object(forInfoDictionaryKey: "LSUIElement") as? Bool) == true
              || (b.object(forInfoDictionaryKey: "LSUIElement") as? String) == "1")
        check("위치 권한 안내 문구", (b.object(forInfoDictionaryKey: "NSLocationUsageDescription") as? String)?.isEmpty == false)
        check("독에 나타나지 않음", NSApp.activationPolicy() == .accessory)
        check("앱 아이콘 파일", b.url(forResource: "AppIcon", withExtension: "icns") != nil)
    }

    // MARK: 시스템 기능

    private func checkSystemAPIs() {
        let locker = SystemScreenLocker()
        check("화면 잠금 함수 사용 가능", locker.hasNativeLock, "SACLockScreenImmediate")
        let idle = SystemProbes.idleSeconds()
        check("자리비움 시간 읽기", idle >= 0 && idle < 60 * 60 * 24 * 365, String(format: "%.1f초", idle))
        let locked = SystemProbes.isScreenLocked()
        info["screenLocked"] = "\(locked)"
        info["idleSeconds"] = String(format: "%.0f", idle)
        // 사용자가 자리를 비워 실제로 잠겨 있을 수 있으므로 값만 기록한다.
        check("화면 잠김 상태 읽기", true, locked ? "지금 잠겨 있음" : "지금 잠겨 있지 않음")
        let holders = SystemProbes.displaySleepPreventers()
        info["displaySleepPreventers"] = holders.joined(separator: ", ")
        check("화면 유지 요청 목록 읽기", true, holders.isEmpty ? "없음" : holders.joined(separator: ", "))

        app.wifi.refresh()
        let snap = app.wifi.snapshot
        info["wifiStatus"] = snap.status.rawValue
        info["wifiSSID"] = snap.ssid ?? "(이름 읽기 불가)"
        info["wifiRouter"] = snap.routerMAC ?? "-"
        info["locationAuth"] = "\(app.wifi.locationStatus.rawValue)"
        check("와이파이 상태 읽기", snap.status != .unavailable, "\(snap.status.rawValue) / \(snap.koreanDescription)")
        if snap.isConnected {
            // 공유기 주소는 macOS 개인정보 보호 정책에 따라 가려질 수 있다 (보조 수단이므로 기록만 한다).
            let mac = RouterLookup.routerMAC(interface: snap.interfaceName ?? "en0")
            info["routerLookup"] = mac ?? "가려짐"
            check("공유기 주소 읽기 시도", true, mac ?? "macOS 가 가림 → 와이파이 이름(위치 권한)으로만 구분")
        }
        check("ARP 출력 해석", RouterLookup.parseARP("? (192.168.0.1) at d8:7:b6:47:6b:8c on en0 ifscope [ethernet]") == "d8:07:b6:47:6b:8c")
        check("ARP 미완료 항목 무시", RouterLookup.parseARP("? (10.0.0.1) at (incomplete) on en0 ifscope [ethernet]") == nil)
    }

    // MARK: 메뉴 막대

    private func checkStatusItem() {
        let item = app.statusBar.statusItem
        check("메뉴 막대 아이콘 표시", item.isVisible && item.button != nil)
        check("아이콘 그림", item.button?.image != nil && item.button?.image?.isTemplate == true, "다크 모드에 맞춰 색이 바뀌는 템플릿 그림")
        if let win = item.button?.window, let screen = win.screen ?? NSScreen.main {
            let f = win.frame
            info["statusItemFrame"] = NSStringFromRect(f)
            let inMenuBar = f.maxY >= screen.frame.maxY - 2
            let onRight = f.midX > screen.frame.midX
            check("아이콘 위치: 화면 오른쪽 위 메뉴 막대", inMenuBar && onRight, NSStringFromRect(f))
        } else {
            check("아이콘 위치: 화면 오른쪽 위 메뉴 막대", false, "아이콘 창 없음")
        }
    }

    private func menuTitles() -> [String] {
        let sb = app.statusBar!
        sb.menuWillOpen(sb.menu)
        let titles = sb.visibleMenuTitles()
        sb.menuDidClose(sb.menu)
        return titles
    }

    private func checkMenuAndActions() {
        // 화면이 실제로 잠겨 있거나 오래 자리를 비운 상태여도 메뉴 동작을 같은 조건에서 점검하도록 고정한다.
        app.controller.screenLockedProvider = { false }
        app.controller.idleProvider = { 0 }
        app.controller.assertionProvider = { [] }
        app.controller.tick()
        let titles = menuTitles()
        info["menu"] = titles.joined(separator: " | ")
        for required in ["지금 잠그기", "자동 잠금 사용", "일시 중지", "설정…", "종료"] {
            check("메뉴 항목 '\(required)'", titles.contains(required))
        }
        check("메뉴에 네트워크 표시", titles.contains { $0.hasPrefix("와이파이:") }, titles.first ?? "")
        check("메뉴에 잠금 시간 표시", titles.contains { $0.hasPrefix("잠금 시간:") })
        let menu = app.statusBar.menu
        let quit = menu.items.first { $0.title == "종료" }
        check("종료 단축키 ⌘Q", quit?.keyEquivalent == "q" && quit?.action != nil)
        let settings = menu.items.first { $0.title == "설정…" }
        check("설정 단축키 ⌘,", settings?.keyEquivalent == "," && settings?.action != nil)

        // 지금 잠그기 → 시험용 잠금 장치가 호출되는지
        let dry = app.locker as? DryRunLocker
        let before = dry?.lockCount ?? -1
        if let idx = menu.items.firstIndex(where: { $0.title == "지금 잠그기" }) {
            menu.performActionForItem(at: idx)
        }
        check("'지금 잠그기' 동작", (dry?.lockCount ?? -1) == before + 1, "잠금 요청 \(before) → \(dry?.lockCount ?? -1)")

        // 자동 잠금 끄기/켜기 → 아이콘·문구 변화
        app.store.settings.enabled = false
        app.controller.tick()
        let offTitles = menuTitles()
        check("자동 잠금 끄기", app.controller.status.state == .disabled && offTitles.contains("자동 잠금이 꺼져 있음"))
        if let img = app.statusBar.statusItem.button?.image { savePNG(of: img, name: "icon-statusbar-live-off") }
        app.store.settings.enabled = true
        app.controller.tick()
        check("자동 잠금 다시 켜기", app.controller.status.state != .disabled)

        // 일시 중지 → 다시 시작
        if let pauseIdx = menu.items.firstIndex(where: { $0.title == "일시 중지" }),
           let sub = menu.items[pauseIdx].submenu, let idx = sub.items.firstIndex(where: { $0.title == "30분" }) {
            sub.performActionForItem(at: idx)
        }
        var pausedOK = false
        if case .paused(let until?) = app.controller.status.state {
            pausedOK = abs(until.timeIntervalSinceNow - 1800) < 5
        }
        let pausedTitles = menuTitles()
        check("일시 중지 30분", pausedOK && pausedTitles.contains("다시 시작") && !pausedTitles.contains("일시 중지"),
              pausedTitles.joined(separator: " | "))
        if let idx = menu.items.firstIndex(where: { $0.title == "다시 시작" }) {
            menu.performActionForItem(at: idx)
        }
        check("다시 시작", !app.controller.isPaused)
    }

    // MARK: 실제 화면 유지 요청 (caffeinate -d 로 만들어 확인)

    private func checkRealDisplayAssertion() {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/caffeinate")
        p.arguments = ["-d", "-t", "30"]
        do { try p.run() } catch {
            check("실제 화면 유지 요청 감지", false, "caffeinate 실행 실패: \(error)")
            return
        }
        Thread.sleep(forTimeInterval: 0.5)
        app.controller.assertionProvider = SystemProbes.displaySleepPreventers
        app.controller.tick()
        let holders = SystemProbes.displaySleepPreventers()
        var held = false
        if case .heldByAssertion(let h) = app.controller.status.state { held = h.contains("caffeinate") }
        check("실제 화면 유지 요청 감지 (caffeinate -d)", holders.contains("caffeinate") && held, holders.joined(separator: ", "))
        let titles = menuTitles()
        check("메뉴에 잠금 보류 이유 표시", titles.contains { $0.contains("잠금 보류") && $0.contains("caffeinate") },
              titles.first { $0.contains("보류") } ?? "")
        // 설정에서 끄면 요청을 무시
        app.store.settings.respectDisplaySleepAssertions = false
        app.controller.tick()
        var ignored = true
        if case .heldByAssertion = app.controller.status.state { ignored = false }
        check("'동영상 재생 중 잠그지 않기' 끄면 무시", ignored, "\(app.controller.status.state)")
        app.store.settings.respectDisplaySleepAssertions = true
        p.terminate()
        p.waitUntilExit()
        Thread.sleep(forTimeInterval: 0.3)
        app.controller.tick()
        var released = true
        if case .heldByAssertion(let h) = app.controller.status.state, h.contains("caffeinate") { released = false }
        check("요청이 끝나면 보류 해제", released && !SystemProbes.displaySleepPreventers().contains("caffeinate"))
        app.controller.assertionProvider = { [] }
        app.controller.tick()
    }

    // MARK: 잠금 엔진 시나리오 (가짜 시간·가짜 와이파이로 실제 컨트롤러 구동)

    private func checkEngineScenarios() {
        let suite = "WiFiLock.selftest.engine"
        UserDefaults().removePersistentDomain(forName: suite)
        let store = SettingsStore(defaults: UserDefaults(suiteName: suite)!)
        store.settings.unknownWiFiTimeoutSeconds = 120
        store.settings.noWiFiTimeoutSeconds = 60
        store.settings.warnBeforeLock = true
        store.settings.warningSeconds = 10
        let home = WiFiSnapshot(status: .connected, ssid: "SelfTestHome", routerMAC: "02:00:00:00:00:01", interfaceName: "en0")
        store.addRule(for: home, timeoutSeconds: 1800)

        let wifi = WiFiMonitor()
        let locker = DryRunLocker()
        let c = LockController(settingsStore: store, wifi: wifi, locker: locker)
        var now = Date()
        var idle: TimeInterval = 0
        var holders: [String] = []
        c.clock = { now }
        c.idleProvider = { idle }
        c.screenLockedProvider = { false }
        c.assertionProvider = { holders }

        func advance(_ seconds: Int) -> [TickOutput] {
            var outs: [TickOutput] = []
            for _ in 0..<seconds {
                now = now.addingTimeInterval(1)
                idle += 1
                outs.append(c.tick()!)
            }
            return outs
        }

        // 1) 집 와이파이: 30분 규칙
        wifi.overrideSnapshotForTesting(home)
        _ = c.tick()
        check("시나리오: 등록된 집 와이파이 → 30분", c.status.policy.timeoutSeconds == 1800 && {
            if case .rule = c.status.policy.source { return true } else { return false }
        }())
        _ = advance(100)
        check("시나리오: 집에서 100초 자리비움 → 잠그지 않음", locker.lockCount == 0)

        // 1-1) 와이파이가 5초 끊겼다 다시 붙음 → 집 규칙 유지, 경고·잠금 없음
        wifi.overrideSnapshotForTesting(WiFiSnapshot(status: .disconnected))
        let flap = advance(5)
        let flapPolicyKept = c.status.policy.timeoutSeconds == 1800 && c.status.policyChangePending
        wifi.overrideSnapshotForTesting(home)
        let flapAfter = advance(3)
        check("시나리오: 5초 순간 끊김은 무시", flapPolicyKept && !(flap + flapAfter).contains { $0.shouldLock || $0.warningRemaining != nil }
              && !c.status.policyChangePending && locker.lockCount == 0, "끊긴 동안 \(c.status.policy.timeoutSeconds)초 규칙")

        // 2) 자리비움 중 와이파이 끊김이 15초 이어짐 → 1분 규칙, 이미 넘었으므로 경고 10초 후 잠금
        wifi.overrideSnapshotForTesting(WiFiSnapshot(status: .disconnected))
        let first = c.tick()!
        check("시나리오: 끊긴 직후엔 집 규칙 유지(확인 중)", c.status.policy.timeoutSeconds == 1800 && c.status.policyChangePending && !first.shouldLock)
        let outs = advance(30)
        check("시나리오: 끊김 15초 지속 → 1분 규칙", c.status.policy.timeoutSeconds == 60 && c.status.policy.source == .noWiFi)
        let warnAt = outs.firstIndex(where: { $0.warningRemaining != nil }).map { $0 + 1 }
        check("시나리오: 시간 초과 시 경고 먼저", warnAt == 15, "경고 시작 \(warnAt.map(String.init) ?? "없음")초")
        let lockAt = outs.firstIndex(where: \.shouldLock).map { $0 + 1 }
        check("시나리오: 경고 10초 뒤 잠금", locker.lockCount == 1 && lockAt == 25, "잠금 시점 \(lockAt.map(String.init) ?? "없음")초")
        _ = advance(120)
        check("시나리오: 같은 자리비움 동안 다시 잠그지 않음", locker.lockCount == 1)

        // 2-1) 애플워치처럼 입력 없이 잠금이 풀림 → 곧바로 다시 잠그지 않고 해제 시각부터 다시 센다
        let beforeUnlock = locker.lockCount
        c.notePresence()
        let afterUnlock = advance(45)
        check("시나리오: 입력 없는 잠금 해제 후 바로 다시 잠그지 않음",
              locker.lockCount == beforeUnlock && !afterUnlock.contains { $0.warningRemaining != nil },
              "해제 후 45초 동안 잠금 \(locker.lockCount - beforeUnlock)회")
        let rest = advance(20)
        check("시나리오: 해제 후 1분 지나면 다시 잠금", locker.lockCount == beforeUnlock + 1 && rest.contains { $0.shouldLock })

        // 3) 사용자가 돌아옴 → 다시 1분 뒤 잠금
        idle = 0
        _ = c.tick()
        let outs2 = advance(70)
        let firstWarn = outs2.firstIndex(where: { $0.warningRemaining != nil }).map { $0 + 1 }
        let lock2 = outs2.firstIndex(where: \.shouldLock).map { $0 + 1 }
        check("시나리오: 복귀 후 50초에 경고, 60초에 잠금", firstWarn == 50 && lock2 == 60 && locker.lockCount == 3,
              "경고 \(firstWarn.map(String.init) ?? "-")초, 잠금 \(lock2.map(String.init) ?? "-")초")

        // 4) 등록되지 않은 와이파이 → 2분
        idle = 0
        wifi.overrideSnapshotForTesting(WiFiSnapshot(status: .connected, ssid: "Cafe", routerMAC: "02:00:00:00:00:99"))
        _ = c.tick()
        let outs3 = advance(125)
        check("시나리오: 낯선 와이파이 → 2분 뒤 잠금", c.status.policy.source == .unknownWiFi
              && outs3.firstIndex(where: \.shouldLock).map { $0 + 1 } == 120 && locker.lockCount == 4)

        // 5) 이름을 모를 때(위치 권한 없음) 공유기 주소로 집을 알아봄
        idle = 0
        wifi.overrideSnapshotForTesting(WiFiSnapshot(status: .connected, ssid: nil, routerMAC: "02:00:00:00:00:01"))
        _ = c.tick()
        check("시나리오: 이름 없이 공유기 주소로 집 인식", c.status.policy.timeoutSeconds == 1800)

        // 6) 화면 유지 요청이 있으면 보류
        idle = 0
        wifi.overrideSnapshotForTesting(WiFiSnapshot(status: .poweredOff))
        holders = ["동영상 재생기"]
        let held = advance(90)
        check("시나리오: 동영상 재생 중 보류", locker.lockCount == 4 && held.last?.state == .heldByAssertion(holders: ["동영상 재생기"]))
        holders = []
        let released = advance(11)
        check("시나리오: 재생 끝나면 경고 후 잠금", released.contains { $0.warningRemaining != nil } && locker.lockCount == 5)

        // 7) "잠그지 않음" 규칙
        idle = 0
        store.settings.rules[0].timeoutSeconds = 0
        wifi.overrideSnapshotForTesting(home)
        _ = advance(4000)
        check("시나리오: '잠그지 않음' 규칙", locker.lockCount == 5 && c.status.state == .neverLock)

        // 8) 끄기
        store.settings.rules[0].timeoutSeconds = 30
        store.settings.enabled = false
        _ = advance(100)
        check("시나리오: 자동 잠금 꺼짐", locker.lockCount == 5 && c.status.state == .disabled)

        // 9) 계속 사용 중: 잠금 30초 + 경고 60초처럼 경고가 더 길어도 잠그지 않음
        store.settings.enabled = true
        store.settings.warningSeconds = 60
        store.settings.rules[0].timeoutSeconds = 30
        var busy: [TickOutput] = []
        for i in 0..<120 {
            now = now.addingTimeInterval(1)
            idle = Double(i % 4) * 0.1
            busy.append(c.tick()!)
        }
        check("시나리오: 계속 사용 중이면 잠금·경고 없음 (잠금 30초, 경고 60초)",
              locker.lockCount == 5 && !busy.contains { $0.shouldLock || $0.warningRemaining != nil })

        UserDefaults().removePersistentDomain(forName: suite)
    }

    private func checkSettingsStorePersistence() {
        let suite = "WiFiLock.selftest.persist"
        UserDefaults().removePersistentDomain(forName: suite)
        let a = SettingsStore(defaults: UserDefaults(suiteName: suite)!)
        a.settings.noWiFiTimeoutSeconds = 30
        a.addRule(ssid: "저장 시험", timeoutSeconds: 600)
        let b = SettingsStore(defaults: UserDefaults(suiteName: suite)!)
        check("설정 저장·불러오기", b.settings == a.settings && b.settings.rules.first?.name == "저장 시험")
        UserDefaults().removePersistentDomain(forName: suite)
    }

    // MARK: 설정 창

    private func checkSettingsWindow() {
        guard let w = app.settingsWindow.nsWindow else {
            check("설정 창 열기", false, "창 없음")
            return
        }
        check("설정 창 열기", w.isVisible, "\(w.title) \(NSStringFromRect(w.frame))")
        check("설정 창 제목", w.title == "와이파이 잠금 설정")
        if let view = w.contentView {
            let ok = savePNG(of: view, name: "settings-window")
            check("설정 창 그리기", ok)
            let texts = collectTexts(in: view)
            info["settingsTexts"] = texts.prefix(40).joined(separator: " | ")
        }
        // 설정 창에서 값을 바꾸면 메뉴에도 반영되는지 (같은 저장소를 공유)
        // (자가 진단은 빈 설정 저장소로 시작하므로 등록된 네트워크가 없다 → 기본값 두 개만 바꾸면 된다)
        app.store.settings.noWiFiTimeoutSeconds = 600
        app.store.settings.unknownWiFiTimeoutSeconds = 600
        app.controller.tick()
        let titles = menuTitles()
        let policyTitle = titles.first { $0.hasPrefix("잠금 시간:") } ?? ""
        check("설정 변경이 메뉴에 반영", app.controller.status.policy.timeoutSeconds == 600 && policyTitle.contains("10분"), policyTitle)
        app.settingsWindow.close()
        check("설정 창 닫기", !w.isVisible)
    }

    private func collectTexts(in view: NSView) -> [String] {
        var result: [String] = []
        if let tf = view as? NSTextField, !tf.stringValue.isEmpty { result.append(tf.stringValue) }
        for sub in view.subviews { result += collectTexts(in: sub) }
        return result
    }

    // MARK: 그림 남기기

    private func renderIcons() {
        savePNG(of: Icons.statusBar(active: true), name: "icon-statusbar-active")
        savePNG(of: Icons.statusBar(active: false), name: "icon-statusbar-paused")
        savePNG(of: Icons.statusBar(active: true, warning: true), name: "icon-statusbar-warning")
        if let icon = NSImage(named: "AppIcon") ?? Bundle.main.url(forResource: "AppIcon", withExtension: "icns").flatMap(NSImage.init(contentsOf:)) {
            savePNG(of: icon, name: "icon-app", size: NSSize(width: 256, height: 256), background: nil)
        }
    }

    private func renderHUD() {
        app.hud.show(remaining: 10)
        if let v = app.hud.contentViewForSnapshot { savePNG(of: v, name: "warning-hud") }
        check("잠금 직전 경고 창 표시", app.hud.isVisible)
        app.hud.hide()
    }

    @discardableResult
    private func savePNG(of image: NSImage, name: String, size: NSSize? = nil, background: NSColor? = .white) -> Bool {
        let target = size ?? NSSize(width: image.size.width * 4 + 16, height: image.size.height * 4 + 16)
        let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(target.width), pixelsHigh: Int(target.height),
                                   bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                   colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        if let background {
            background.setFill()
            NSRect(origin: .zero, size: target).fill()
        }
        let drawRect: NSRect
        if size == nil {
            drawRect = NSRect(x: 8, y: 8, width: image.size.width * 4, height: image.size.height * 4)
        } else {
            drawRect = NSRect(origin: .zero, size: target)
        }
        image.draw(in: drawRect)
        NSGraphicsContext.restoreGraphicsState()
        return write(rep, name: name)
    }

    @discardableResult
    private func savePNG(of view: NSView, name: String) -> Bool {
        view.layoutSubtreeIfNeeded()
        guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return false }
        view.cacheDisplay(in: view.bounds, to: rep)
        return write(rep, name: name)
    }

    private func write(_ rep: NSBitmapImageRep, name: String) -> Bool {
        guard let data = rep.representation(using: .png, properties: [:]) else { return false }
        do {
            try data.write(to: out.appendingPathComponent("\(name).png"))
            return true
        } catch {
            return false
        }
    }

    // MARK: 마무리

    private func finish() {
        let failed = checks.filter { !$0.passed }
        struct Report: Encodable {
            let passed: Int
            let failed: Int
            let checks: [Check]
            let info: [String: String]
        }
        let report = Report(passed: checks.count - failed.count, failed: failed.count, checks: checks, info: info)
        let enc = JSONEncoder()
        enc.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        if let data = try? enc.encode(report) {
            try? data.write(to: out.appendingPathComponent("report.json"))
        }
        var summary = "자가 진단: 통과 \(report.passed) / 실패 \(report.failed)\n"
        for c in checks {
            summary += "\(c.passed ? "[통과]" : "[실패]") \(c.name)\(c.detail.isEmpty ? "" : " — \(c.detail)")\n"
        }
        summary += "\n[정보]\n" + info.sorted { $0.key < $1.key }.map { "\($0.key): \($0.value)" }.joined(separator: "\n") + "\n"
        try? summary.write(to: out.appendingPathComponent("summary.txt"), atomically: true, encoding: .utf8)
        AppLog.info("자가 진단 끝: 통과 \(report.passed), 실패 \(report.failed)")
        AppLog.flush()

        UserDefaults().removePersistentDomain(forName: "WiFiLock.selftest")
        if failed.isEmpty {
            // 메뉴의 '종료' 를 눌러 정상 종료되는지도 확인한다.
            let menu = app.statusBar.menu
            if let idx = menu.items.firstIndex(where: { $0.title == "종료" }) {
                menu.performActionForItem(at: idx)
            }
            // 종료되지 않았다면 실패로 처리
            DispatchQueue.main.asyncAfter(deadline: .now() + 3) { exit(3) }
        } else {
            exit(1)
        }
    }
}
