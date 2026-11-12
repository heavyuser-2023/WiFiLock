import AppKit
import Combine
import WiFiLockCore

/// 오른쪽 위 메뉴 막대 아이콘과 메뉴
final class StatusBarController: NSObject, NSMenuDelegate {
    let statusItem: NSStatusItem
    let menu = NSMenu()
    private let controller: LockController
    private let onOpenSettings: () -> Void
    private let onAbout: () -> Void
    private let onQuit: () -> Void
    private var cancellables = Set<AnyCancellable>()
    private(set) var menuIsOpen = false
    private var lastIconKey = ""

    // 상태 표시 항목 (메뉴가 열려 있는 동안 계속 갱신)
    private let networkItem = NSMenuItem()
    private let policyItem = NSMenuItem()
    private let stateItem = NSMenuItem()
    private let permissionItem = NSMenuItem()
    private let addNetworkItem = NSMenuItem()
    private let lockNowItem = NSMenuItem()
    private let enabledItem = NSMenuItem()
    private let pauseItem = NSMenuItem()
    private let resumeItem = NSMenuItem()

    init(controller: LockController, onOpenSettings: @escaping () -> Void, onAbout: @escaping () -> Void, onQuit: @escaping () -> Void) {
        self.controller = controller
        self.onOpenSettings = onOpenSettings
        self.onAbout = onAbout
        self.onQuit = onQuit
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        super.init()
        statusItem.autosaveName = "WiFiLockStatusItem"
        statusItem.button?.imagePosition = .imageLeading
        statusItem.button?.toolTip = AppInfo.displayName
        buildMenu()
        statusItem.menu = menu
        updateButton()

        // 상태는 주 스레드의 1초 타이머에서만 바뀐다. 메뉴가 열려 있는 동안에도 갱신되도록
        // 작업 대기열을 거치지 않고 바로 반영한다. (@Published 는 바뀌기 직전에 알리므로 넘겨받은 새 값을 쓴다)
        controller.$status.sink { [weak self] st in
            self?.updateButton(status: st)
            if self?.menuIsOpen == true { self?.updateMenuItems(status: st) }
        }.store(in: &cancellables)
        controller.settingsStore.$settings.sink { [weak self] s in
            self?.updateButton(settings: s)
        }.store(in: &cancellables)
        controller.wifi.$locationStatus.sink { [weak self] _ in
            DispatchQueue.main.async { if self?.menuIsOpen == true { self?.updateMenuItems() } }
        }.store(in: &cancellables)
    }

    // MARK: - 메뉴 구성

    private func buildMenu() {
        menu.delegate = self
        menu.autoenablesItems = false

        for item in [networkItem, policyItem, stateItem] {
            item.isEnabled = false
            menu.addItem(item)
        }
        permissionItem.title = "와이파이 이름을 읽으려면 위치 권한이 필요합니다…"
        permissionItem.image = NSImage(systemSymbolName: "location.slash", accessibilityDescription: nil)
        permissionItem.target = self
        permissionItem.action = #selector(requestPermission)
        menu.addItem(permissionItem)
        menu.addItem(.separator())

        lockNowItem.title = "지금 잠그기"
        lockNowItem.keyEquivalent = "l"
        lockNowItem.keyEquivalentModifierMask = [.command]
        lockNowItem.image = NSImage(systemSymbolName: "lock.fill", accessibilityDescription: nil)
        lockNowItem.target = self
        lockNowItem.action = #selector(lockNow)
        menu.addItem(lockNowItem)

        enabledItem.title = "자동 잠금 사용"
        enabledItem.target = self
        enabledItem.action = #selector(toggleEnabled)
        menu.addItem(enabledItem)

        pauseItem.title = "일시 중지"
        pauseItem.image = NSImage(systemSymbolName: "pause.circle", accessibilityDescription: nil)
        let pauseMenu = NSMenu()
        for (title, secs) in [("30분", 1800.0), ("1시간", 3600.0), ("2시간", 7200.0), ("4시간", 14400.0)] {
            let i = NSMenuItem(title: title, action: #selector(pauseFor(_:)), keyEquivalent: "")
            i.target = self
            i.representedObject = secs
            pauseMenu.addItem(i)
        }
        pauseMenu.addItem(.separator())
        let indefinite = NSMenuItem(title: "다시 시작할 때까지", action: #selector(pauseFor(_:)), keyEquivalent: "")
        indefinite.target = self
        pauseMenu.addItem(indefinite)
        pauseItem.submenu = pauseMenu
        menu.addItem(pauseItem)

        resumeItem.title = "다시 시작"
        resumeItem.image = NSImage(systemSymbolName: "play.circle", accessibilityDescription: nil)
        resumeItem.target = self
        resumeItem.action = #selector(resume)
        menu.addItem(resumeItem)

        menu.addItem(.separator())

        addNetworkItem.title = "현재 네트워크를 등록"
        addNetworkItem.image = NSImage(systemSymbolName: "plus.circle", accessibilityDescription: nil)
        addNetworkItem.target = self
        addNetworkItem.action = #selector(addCurrentNetwork)
        menu.addItem(addNetworkItem)

        let settingsItem = NSMenuItem(title: "설정…", action: #selector(openSettings), keyEquivalent: ",")
        settingsItem.image = NSImage(systemSymbolName: "gearshape", accessibilityDescription: nil)
        settingsItem.target = self
        menu.addItem(settingsItem)

        menu.addItem(.separator())

        let aboutItem = NSMenuItem(title: "\(AppInfo.displayName) 정보", action: #selector(about), keyEquivalent: "")
        aboutItem.target = self
        menu.addItem(aboutItem)

        let quitItem = NSMenuItem(title: "종료", action: #selector(quit), keyEquivalent: "q")
        quitItem.image = NSImage(systemSymbolName: "power", accessibilityDescription: nil)
        quitItem.target = self
        menu.addItem(quitItem)

        updateMenuItems()
    }

    func menuWillOpen(_ menu: NSMenu) {
        menuIsOpen = true
        controller.wifi.refresh()
        controller.tick()
        updateMenuItems()
    }

    func menuDidClose(_ menu: NSMenu) {
        menuIsOpen = false
    }

    /// 메뉴 항목 문구를 현재 상태로 맞춘다.
    func updateMenuItems(status: LockStatus? = nil) {
        let st = status ?? controller.status
        let settings = controller.settingsStore.settings
        let snap = st.snapshot

        // 1) 네트워크
        let wifiSymbol: String
        switch snap.status {
        case .connected: wifiSymbol = "wifi"
        case .poweredOff, .unavailable: wifiSymbol = "wifi.slash"
        case .disconnected: wifiSymbol = "wifi.exclamationmark"
        }
        networkItem.image = NSImage(systemSymbolName: wifiSymbol, accessibilityDescription: nil)
        var net = "와이파이: \(snap.koreanDescription)"
        if snap.isConnected, snap.ssid == nil, let mac = snap.routerMAC { net += " · 공유기 \(mac)" }
        networkItem.attributedTitle = Self.styled(net, bold: true)

        // 2) 적용 규칙
        let p = st.policy
        let source: String
        switch p.source {
        case .rule(let r): source = "등록된 네트워크 '\(r.name)'"
        case .unknownWiFi: source = "등록되지 않은 네트워크"
        case .noWiFi: source = "와이파이 연결 없음"
        }
        policyItem.image = NSImage(systemSymbolName: "timer", accessibilityDescription: nil)
        let pendingNote = st.policyChangePending ? " · 연결 변화 확인 중" : ""
        policyItem.attributedTitle = Self.styled("잠금 시간: \(KoreanFormat.timeout(p.timeoutSeconds)) (\(source))\(pendingNote)")

        // 3) 지금 상태
        stateItem.image = NSImage(systemSymbolName: Self.stateSymbol(st.state), accessibilityDescription: nil)
        stateItem.attributedTitle = Self.styled(Self.stateText(st.state))

        // 위치 권한 안내
        permissionItem.isHidden = controller.wifi.hasLocationPermission || !snap.isConnected

        // 현재 네트워크 등록
        if snap.isConnected, snap.ssid != nil || snap.routerMAC != nil {
            if case .rule = p.source {
                addNetworkItem.isHidden = true
            } else {
                addNetworkItem.isHidden = false
                addNetworkItem.title = "현재 네트워크(\(snap.ssid ?? "공유기 \(snap.routerMAC!)"))를 등록"
            }
        } else {
            addNetworkItem.isHidden = true
        }

        enabledItem.state = settings.enabled ? .on : .off
        pauseItem.isHidden = controller.isPaused || !settings.enabled
        resumeItem.isHidden = !controller.isPaused
    }

    static func stateText(_ state: EngineState) -> String {
        switch state {
        case .disabled: return "자동 잠금이 꺼져 있음"
        case .paused(let until):
            if let until { return "일시 중지됨 (\(KoreanFormat.clock(until))까지)" }
            return "일시 중지됨 (다시 시작할 때까지)"
        case .neverLock: return "이 네트워크에서는 자동으로 잠그지 않음"
        case .heldByAssertion(let holders):
            return "잠금 보류: \(holders.prefix(2).joined(separator: ", ")) 이(가) 화면 유지 중"
        case .screenLocked: return "화면 잠김"
        case .counting(let remaining, _): return "잠금까지 \(KoreanFormat.countdown(remaining)) 남음"
        case .warning(let remaining): return "곧 잠김: \(Int(remaining.rounded(.up)))초"
        case .lockTriggered: return "화면을 잠갔습니다"
        case .waitingForActivity: return "잠금 완료 · 다시 사용하면 시간을 새로 셉니다"
        }
    }

    static func stateSymbol(_ state: EngineState) -> String {
        switch state {
        case .disabled: return "lock.slash"
        case .paused: return "pause.circle"
        case .neverLock: return "lock.open"
        case .heldByAssertion: return "play.display"
        case .screenLocked, .lockTriggered, .waitingForActivity: return "lock.fill"
        case .counting: return "hourglass"
        case .warning: return "exclamationmark.triangle"
        }
    }

    private static func styled(_ text: String, bold: Bool = false) -> NSAttributedString {
        NSAttributedString(string: text, attributes: [
            .font: bold ? NSFont.menuFont(ofSize: 13).bold() : NSFont.menuFont(ofSize: 13),
            .foregroundColor: bold ? NSColor.labelColor : NSColor.secondaryLabelColor,
        ])
    }

    // MARK: - 아이콘

    func updateButton(status: LockStatus? = nil, settings: AppSettings? = nil) {
        guard let button = statusItem.button else { return }
        let st = status ?? controller.status
        let settings = settings ?? controller.settingsStore.settings
        let active: Bool
        switch st.state {
        case .disabled, .paused, .neverLock: active = false
        default: active = true
        }
        var warning = false
        if case .warning = st.state { warning = true }
        let key = "\(active)-\(warning)"
        if key != lastIconKey {
            button.image = Icons.statusBar(active: active, warning: warning)
            lastIconKey = key
        }
        var title = ""
        if settings.showCountdownInMenuBar {
            switch st.state {
            case .counting(let r, _): title = " " + KoreanFormat.countdown(r)
            case .warning(let r): title = " " + KoreanFormat.countdown(r)
            default: break
            }
        }
        if button.title != title { button.title = title }
        button.toolTip = "\(AppInfo.displayName) — \(Self.stateText(st.state))"
    }

    // MARK: - 동작

    @objc private func lockNow() { controller.lockNow() }

    @objc private func toggleEnabled() {
        controller.settingsStore.settings.enabled.toggle()
        updateMenuItems()
    }

    @objc private func pauseFor(_ sender: NSMenuItem) {
        controller.pause(for: sender.representedObject as? TimeInterval)
        updateMenuItems()
    }

    @objc private func resume() {
        controller.resume()
        updateMenuItems()
    }

    @objc private func addCurrentNetwork() {
        if let rule = controller.settingsStore.addRule(for: controller.wifi.snapshot) {
            AppLog.info("네트워크 등록: \(rule.name)")
        }
        controller.tick()
        onOpenSettings()
    }

    @objc private func requestPermission() {
        if controller.wifi.locationStatus == .notDetermined {
            NSApp.activate(ignoringOtherApps: true)
            controller.wifi.requestLocationPermission()
        } else {
            controller.wifi.openLocationSettings()
        }
    }

    @objc private func openSettings() { onOpenSettings() }
    @objc private func about() { onAbout() }
    @objc private func quit() { onQuit() }

    /// 자가 진단용: 지금 메뉴에 보이는 항목 제목들
    func visibleMenuTitles() -> [String] {
        menu.items.filter { !$0.isHidden }.map { $0.isSeparatorItem ? "---" : ($0.attributedTitle?.string ?? $0.title) }
    }
}

private extension NSFont {
    func bold() -> NSFont {
        NSFontManager.shared.convert(self, toHaveTrait: .boldFontMask)
    }
}
