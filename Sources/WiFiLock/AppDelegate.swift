import AppKit
import Combine
import SwiftUI
import WiFiLockCore

final class AppDelegate: NSObject, NSApplicationDelegate {
    let options: LaunchOptions

    private(set) var store: SettingsStore!
    private(set) var wifi: WiFiMonitor!
    private(set) var controller: LockController!
    private(set) var statusBar: StatusBarController!
    private(set) var settingsWindow: SettingsWindowController!
    private(set) var launchAtLogin: LaunchAtLogin!
    private(set) var hud = WarningHUD()
    private(set) var locker: ScreenLocking!
    private var cancellables = Set<AnyCancellable>()
    private var selfTest: SelfTest?
    private var napActivity: NSObjectProtocol?

    init(options: LaunchOptions) {
        self.options = options
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        if let path = options.logFile { AppLog.fileURL = URL(fileURLWithPath: path) }
        AppLog.info("시작: 버전 \(AppInfo.version), 시험 모드 \(options.dryRun), 자가 진단 \(options.selfTestDir != nil)")

        if options.selfTestDir == nil, isAnotherInstanceRunning() {
            AppLog.info("이미 실행 중이라 종료")
            NSApp.terminate(nil)
            return
        }

        setupMainMenu()

        let suite = options.settingsSuite ?? (options.selfTestDir != nil ? "WiFiLock.selftest" : nil)
        if options.selfTestDir != nil, let suite { UserDefaults().removePersistentDomain(forName: suite) }
        let defaults = suite.flatMap { UserDefaults(suiteName: $0) } ?? .standard
        store = SettingsStore(defaults: defaults)
        wifi = WiFiMonitor()
        locker = options.dryRun || options.selfTestDir != nil ? DryRunLocker() : SystemScreenLocker()
        controller = LockController(settingsStore: store, wifi: wifi, locker: locker, overrideTimeout: options.overrideTimeout)
        launchAtLogin = LaunchAtLogin()

        settingsWindow = SettingsWindowController { [unowned self] in
            SettingsView(store: self.store, wifi: self.wifi, launchAtLogin: self.launchAtLogin, controller: self.controller)
        }
        statusBar = StatusBarController(
            controller: controller,
            onOpenSettings: { [weak self] in self?.openSettings() },
            onAbout: { [weak self] in self?.showAbout() },
            onQuit: { NSApp.terminate(nil) }
        )

        // 잠금 직전 경고
        controller.$status.sink { [weak self] st in
            guard let self else { return }
            if case .warning(let remaining) = st.state {
                self.hud.show(remaining: remaining)
            } else {
                self.hud.hide()
            }
        }.store(in: &cancellables)

        // 메뉴 막대 앱은 화면에 창이 없으면 "앱 낮잠" 대상이 되어 1초 타이머가 크게 늦어질 수 있다.
        // 잠자기는 막지 않고 앱 낮잠만 막는다.
        napActivity = ProcessInfo.processInfo.beginActivity(
            options: [.userInitiatedAllowingIdleSystemSleep],
            reason: "자리비움 시간을 재서 제때 화면을 잠그기 위함"
        )

        wifi.start()
        controller.start()

        if let dir = options.selfTestDir {
            let test = SelfTest(app: self, outputDir: URL(fileURLWithPath: dir))
            selfTest = test
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { test.run() }
            return
        }

        // 처음 실행이면 설정 창을 보여 주고 위치 권한을 요청한다.
        let firstLaunchKey = "didLaunchBefore"
        let isFirstLaunch = !defaults.bool(forKey: firstLaunchKey)
        defaults.set(true, forKey: firstLaunchKey)
        if !options.noLocationPrompt, wifi.locationStatus == .notDetermined {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in
                NSApp.activate(ignoringOtherApps: true)
                self?.wifi.requestLocationPermission()
            }
        }
        if isFirstLaunch || options.openSettings {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in self?.openSettings() }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        controller?.stop()
        wifi?.stop()
        AppLog.info("종료")
        AppLog.flush()
    }

    /// 이미 실행 중인데 다시 열면 (Finder 에서 더블 클릭 등) 설정 창을 띄운다.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        openSettings()
        return false
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func openSettings() {
        launchAtLogin.refresh()
        wifi.refresh()
        settingsWindow.show()
    }

    func showAbout() {
        NSApp.activate(ignoringOtherApps: true)
        let credits = NSAttributedString(
            string: "와이파이 연결 상태에 따라 자동 잠금 시간을 바꿔 주는 메뉴 막대 앱입니다.\n등록한 네트워크에서는 길게, 낯선 곳에서는 짧게 잠급니다.",
            attributes: [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.secondaryLabelColor]
        )
        NSApp.orderFrontStandardAboutPanel(options: [
            .applicationName: AppInfo.displayName,
            .credits: credits,
        ])
    }

    private func isAnotherInstanceRunning() -> Bool {
        guard let id = Bundle.main.bundleIdentifier else { return false }
        let me = NSRunningApplication.current.processIdentifier
        return NSRunningApplication.runningApplications(withBundleIdentifier: id).contains { $0.processIdentifier != me }
    }

    /// 메뉴 막대 앱이라도 입력 칸에서 ⌘C·⌘V 등이 동작하려면 편집 메뉴가 있어야 한다.
    private func setupMainMenu() {
        let main = NSMenu()

        let appItem = NSMenuItem()
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "\(AppInfo.displayName) 정보", action: #selector(aboutFromMenu), keyEquivalent: "").target = self
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "설정…", action: #selector(settingsFromMenu), keyEquivalent: ",").target = self
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "종료", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        main.addItem(appItem)

        let editItem = NSMenuItem()
        let edit = NSMenu(title: "편집")
        edit.addItem(withTitle: "실행 취소", action: Selector(("undo:")), keyEquivalent: "z")
        edit.addItem(withTitle: "실행 복귀", action: Selector(("redo:")), keyEquivalent: "Z")
        edit.addItem(.separator())
        edit.addItem(withTitle: "오려두기", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "복사하기", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "붙여넣기", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "전체 선택", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = edit
        main.addItem(editItem)

        let windowItem = NSMenuItem()
        let windowMenu = NSMenu(title: "윈도우")
        windowMenu.addItem(withTitle: "닫기", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        windowMenu.addItem(withTitle: "최소화", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowItem.submenu = windowMenu
        main.addItem(windowItem)

        NSApp.mainMenu = main
    }

    @objc private func aboutFromMenu() { showAbout() }
    @objc private func settingsFromMenu() { openSettings() }
}
