import AppKit
import SwiftUI
import WiFiLockCore

private func l(_ ko: String, _ en: String) -> String {
    AppLang.pick(ko, en)
}

struct SettingsView: View {
    @ObservedObject var store: SettingsStore
    @ObservedObject var wifi: WiFiMonitor
    @ObservedObject var launchAtLogin: LaunchAtLogin
    @ObservedObject var controller: LockController

    @State private var showingAddSheet = false
    @State private var newSSID = ""
    @State private var newTimeout = AppSettings.defaultTrustedTimeout

    var body: some View {
        Form {
            generalSection
            currentNetworkSection
            rulesSection
            defaultsSection
            advancedSection
        }
        .formStyle(.grouped)
        .frame(minWidth: 560, idealWidth: 560, minHeight: 600, idealHeight: 720)
        .sheet(isPresented: $showingAddSheet) { addSheet }
    }

    // MARK: 일반

    private var generalSection: some View {
        Section {
            Toggle(isOn: $store.settings.enabled) {
                Text(l("자동 잠금 사용", "Enable Auto-Lock"))
                Text(l("키보드·마우스 입력이 없으면 아래 시간 뒤에 화면을 잠급니다.", "Automatically locks the screen after the idle time below."))
            }
            Toggle(l("로그인할 때 자동으로 실행", "Launch at Login"), isOn: Binding(
                get: { launchAtLogin.isEnabled || launchAtLogin.needsApproval },
                set: { launchAtLogin.set($0) }
            ))
            if launchAtLogin.needsApproval {
                HStack {
                    Label(l("시스템 설정의 로그인 항목에서 허용이 필요합니다", "Approval required in System Settings > Login Items"), systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.orange)
                    Spacer()
                    Button(l("열기", "Open")) { launchAtLogin.openLoginItemsSettings() }
                }
            }
            if let err = launchAtLogin.lastError {
                Text(err).font(.caption).foregroundStyle(.red)
            }
        } header: {
            Text(l("일반", "General"))
        }
    }

    // MARK: 현재 네트워크

    private var currentNetworkSection: some View {
        Section {
            HStack(alignment: .center, spacing: 12) {
                Image(systemName: wifiSymbol)
                    .font(.system(size: 22))
                    .foregroundStyle(wifi.snapshot.isConnected ? Color.accentColor : Color.secondary)
                    .frame(width: 30)
                VStack(alignment: .leading, spacing: 3) {
                    Text(wifi.snapshot.koreanDescription).font(.headline)
                    if wifi.snapshot.isConnected, let mac = wifi.snapshot.routerMAC {
                        Text(AppLang.isKo ? "공유기 주소 \(mac)" : "Router MAC \(mac)")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Text(
                        AppLang.isKo
                            ? "지금 적용: \(KoreanFormat.timeout(controller.status.policy.timeoutSeconds)) · \(controller.status.policy.source.koreanDescription)"
                            : "Current policy: \(KoreanFormat.timeout(controller.status.policy.timeoutSeconds)) · \(controller.status.policy.source.koreanDescription)"
                    )
                    .font(.caption).foregroundStyle(.secondary)
                    Text(StatusBarController.stateText(controller.status.state))
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                if canAddCurrent {
                    Button(l("등록", "Add")) { store.addRule(for: wifi.snapshot) }
                }
            }
            .padding(.vertical, 2)

            if !wifi.hasLocationPermission {
                VStack(alignment: .leading, spacing: 6) {
                    Label(l("위치 권한이 없어 와이파이 이름을 읽을 수 없습니다", "Location permission required to read Wi-Fi name"), systemImage: "location.slash")
                        .foregroundStyle(.orange)
                    Text(permissionExplanation)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Button(wifi.locationStatus == .notDetermined
                        ? l("위치 권한 요청", "Request Location Permission")
                        : l("시스템 설정에서 허용하기", "Allow in System Settings")) {
                        if wifi.locationStatus == .notDetermined {
                            NSApp.activate(ignoringOtherApps: true)
                            wifi.requestLocationPermission()
                        } else {
                            wifi.openLocationSettings()
                        }
                    }
                }
                .padding(.vertical, 2)
            }
        } header: {
            Text(l("현재 네트워크", "Current Network"))
        }
    }

    private var permissionExplanation: String {
        if AppLang.isKo {
            let head = "macOS 는 위치 권한을 받은 앱에만 와이파이 이름을 알려 줍니다. "
            let tail = " 위치 정보는 와이파이 이름 확인에만 쓰며 저장하거나 밖으로 보내지 않습니다."
            if wifi.snapshot.isConnected, wifi.snapshot.routerMAC != nil {
                return head + "지금은 공유기 주소로 네트워크를 구분하고 있지만, 와이파이 이름으로 구분하려면 권한을 허용해 주세요." + tail
            }
            return head + "권한이 없으면 '와이파이 연결됨'과 '연결 안 됨'만 구분해 기본 잠금 시간을 적용합니다. 집·사무실처럼 네트워크마다 시간을 다르게 하려면 권한을 허용해 주세요." + tail
        }
        let head = "macOS only shares Wi-Fi network names with apps that have Location permission. "
        let tail = " Location data is used solely to check the Wi-Fi name and is never stored or sent anywhere."
        if wifi.snapshot.isConnected, wifi.snapshot.routerMAC != nil {
            return head + "Currently distinguishing networks by router MAC address; grant permission to identify networks by Wi-Fi name." + tail
        }
        return head + "Without permission, only 'Wi-Fi connected' and 'Not connected' are distinguished using default timeouts. Grant permission to set per-network timeouts." + tail
    }

    private var wifiSymbol: String {
        switch wifi.snapshot.status {
        case .connected: return "wifi"
        case .disconnected: return "wifi.exclamationmark"
        case .poweredOff, .unavailable: return "wifi.slash"
        }
    }

    private var canAddCurrent: Bool {
        let s = wifi.snapshot
        guard s.isConnected, s.ssid != nil || s.routerMAC != nil else { return false }
        return PolicyResolver.matchingRule(for: s, in: store.settings.rules) == nil
            || (s.ssid != nil && PolicyResolver.matchingRule(for: s, in: store.settings.rules)?.ssid == nil)
    }

    // MARK: 네트워크별 규칙

    private var rulesSection: some View {
        Section {
            if store.settings.rules.isEmpty {
                Text(l(
                    "등록된 네트워크가 없습니다. 집이나 사무실처럼 믿을 수 있는 네트워크를 등록하면 그곳에서는 잠금 시간을 길게 둘 수 있습니다.",
                    "No registered networks. Add trusted networks like home or office to use longer lock times there."
                ))
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            }
            ForEach(store.settings.rules) { rule in
                RuleRow(
                    rule: binding(for: rule.id),
                    isCurrent: PolicyResolver.matchingRule(for: wifi.snapshot, in: store.settings.rules)?.id == rule.id,
                    onDelete: { store.removeRule(id: rule.id) }
                )
            }
            HStack {
                Button {
                    store.addRule(for: wifi.snapshot)
                } label: {
                    Label(l("현재 네트워크 추가", "Add Current Network"), systemImage: "plus")
                }
                .disabled(!canAddCurrent)
                Button {
                    newSSID = ""
                    newTimeout = AppSettings.defaultTrustedTimeout
                    showingAddSheet = true
                } label: {
                    Label(l("이름으로 추가…", "Add by Name…"), systemImage: "keyboard")
                }
                Spacer()
            }
        } header: {
            Text(l("네트워크별 잠금 시간", "Per-Network Lock Times"))
        } footer: {
            Text(l(
                "위에 있는 규칙부터 적용됩니다. 공유기 주소까지 기록된 규칙은 이름이 같아도 공유기가 다르면 적용하지 않습니다. 이름으로만 추가한 규칙은 같은 이름의 모든 와이파이에 적용됩니다.",
                "Rules are applied from top to bottom. Rules with a router MAC address only apply to that specific router, even if another Wi-Fi has the same name. Rules added by name only apply to any Wi-Fi with that name."
            ))
            .font(.caption).foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
            .multilineTextAlignment(.leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func binding(for id: UUID) -> Binding<NetworkRule> {
        Binding(
            get: { store.settings.rules.first { $0.id == id } ?? NetworkRule(id: id, name: "", ssid: nil, routerMAC: nil, timeoutSeconds: 0) },
            set: { newValue in
                if let i = store.settings.rules.firstIndex(where: { $0.id == id }) {
                    store.settings.rules[i] = newValue
                }
            }
        )
    }

    // MARK: 기본값

    private var defaultsSection: some View {
        Section {
            TimeoutPicker(title: l("등록되지 않은 와이파이", "Unregistered Wi-Fi"), systemImage: "wifi", selection: $store.settings.unknownWiFiTimeoutSeconds)
            TimeoutPicker(title: l("와이파이 꺼짐 · 연결 안 됨", "Wi-Fi Off · Not Connected"), systemImage: "wifi.slash", selection: $store.settings.noWiFiTimeoutSeconds)
        } header: {
            Text(l("기본 잠금 시간", "Default Lock Times"))
        } footer: {
            Text(l(
                "등록되지 않은 곳에서는 짧게 두면 노트북을 두고 자리를 비웠을 때 더 안전합니다.",
                "Using a shorter timeout for unregistered networks helps keep your Mac secure away from home."
            ))
            .font(.caption).foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    // MARK: 고급

    private var advancedSection: some View {
        Section {
            Toggle(l("잠그기 전에 화면에 경고 표시", "Show warning before locking"), isOn: $store.settings.warnBeforeLock)
            Picker(l("경고 표시 시간", "Warning duration"), selection: $store.settings.warningSeconds) {
                ForEach(warningOptions, id: \.self) { s in
                    Text(KoreanFormat.duration(s)).tag(s)
                }
            }
            .disabled(!store.settings.warnBeforeLock)
            if store.settings.warnBeforeLock {
                Text(l(
                    "잠금 시간이 짧으면 경고는 잠금 시간의 절반까지만 표시됩니다. 경고 중에 마우스나 키보드를 움직이면 취소됩니다.",
                    "For short lock times, the warning is capped at half the lock time. Moving the mouse or pressing a key cancels it."
                ))
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            }
            Toggle(isOn: $store.settings.respectDisplaySleepAssertions) {
                Text(l("동영상 재생 중에는 잠그지 않기", "Do not lock during video playback"))
                Text(l(
                    "앱이 화면을 켜 두도록 요청한 동안(동영상, 발표, 화상 회의 등)은 잠금을 미룹니다.",
                    "Delays locking while apps keep the screen awake (videos, presentations, meetings, etc.)."
                ))
            }
            Toggle(l("메뉴 막대에 남은 시간 표시", "Show countdown in menu bar"), isOn: $store.settings.showCountdownInMenuBar)
        } header: {
            Text(l("고급", "Advanced"))
        }
    }

    private var warningOptions: [Int] {
        var list = AppSettings.warningPresets
        if !list.contains(store.settings.warningSeconds) { list.append(store.settings.warningSeconds); list.sort() }
        return list
    }

    // MARK: 이름으로 추가

    private var addSheet: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(l("와이파이 이름으로 추가", "Add by Wi-Fi Name")).font(.headline)
            Text(l(
                "대소문자까지 정확히 입력해 주세요. 이름으로 구분하려면 위치 권한이 필요합니다.",
                "Enter the exact case-sensitive Wi-Fi name. Location permission is required to match by name."
            ))
            .font(.caption).foregroundStyle(.secondary)
            TextField(l("와이파이 이름", "Wi-Fi name"), text: $newSSID)
                .textFieldStyle(.roundedBorder)
                .onSubmit(commitAdd)
            TimeoutPicker(title: l("잠금 시간", "Lock Time"), systemImage: "timer", selection: $newTimeout)
            HStack {
                Spacer()
                Button(l("취소", "Cancel"), role: .cancel) { showingAddSheet = false }
                    .keyboardShortcut(.cancelAction)
                Button(l("추가", "Add"), action: commitAdd)
                    .keyboardShortcut(.defaultAction)
                    .disabled(newSSID.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 380)
    }

    private func commitAdd() {
        guard !newSSID.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        store.addRule(ssid: newSSID, timeoutSeconds: newTimeout)
        showingAddSheet = false
    }
}

/// 규칙 한 줄
struct RuleRow: View {
    @Binding var rule: NetworkRule
    var isCurrent: Bool
    var onDelete: () -> Void

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            Image(systemName: isCurrent ? "checkmark.circle.fill" : "wifi")
                .foregroundStyle(isCurrent ? Color.green : Color.secondary)
                .frame(width: 18)
                .help(isCurrent ? l("지금 연결된 네트워크", "Currently connected network") : "")
            VStack(alignment: .leading, spacing: 2) {
                TextField(l("이름", "Name"), text: $rule.name)
                    .textFieldStyle(.plain)
                    .font(.body.weight(.medium))
                Text(identifierText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            Picker("", selection: $rule.timeoutSeconds) {
                ForEach(TimeoutPresets.including(rule.timeoutSeconds), id: \.self) { s in
                    Text(KoreanFormat.timeout(s)).tag(s)
                }
            }
            .labelsHidden()
            .frame(width: 130)
            Button(role: .destructive, action: onDelete) {
                Image(systemName: "trash")
            }
            .buttonStyle(.borderless)
            .help(l("이 네트워크 삭제", "Delete this network"))
        }
    }

    private var identifierText: String {
        var parts: [String] = []
        if let ssid = rule.ssid { parts.append(AppLang.isKo ? "와이파이 이름: \(ssid)" : "Wi-Fi Name: \(ssid)") }
        if let mac = rule.routerMAC { parts.append(AppLang.isKo ? "공유기: \(mac)" : "Router: \(mac)") }
        return parts.joined(separator: " · ")
    }
}

/// 잠금 시간 선택 상자
struct TimeoutPicker: View {
    var title: String
    var systemImage: String
    @Binding var selection: Int

    var body: some View {
        Picker(selection: $selection) {
            ForEach(TimeoutPresets.including(selection), id: \.self) { s in
                Text(KoreanFormat.timeout(s)).tag(s)
            }
        } label: {
            Label(title, systemImage: systemImage)
        }
    }
}

/// 설정 창
final class SettingsWindowController: NSObject, NSWindowDelegate {
    private var window: NSWindow?
    private let makeView: () -> SettingsView

    init(makeView: @escaping () -> SettingsView) {
        self.makeView = makeView
    }

    var nsWindow: NSWindow? { window }

    func show() {
        if window == nil {
            let hosting = NSHostingController(rootView: makeView())
            let w = NSWindow(contentViewController: hosting)
            w.title = AppLang.isKo ? "\(AppInfo.displayName) 설정" : "\(AppInfo.displayName) Settings"
            w.styleMask = [.titled, .closable, .miniaturizable, .resizable]
            w.setContentSize(NSSize(width: 560, height: 720))
            w.isReleasedWhenClosed = false
            w.delegate = self
            w.center()
            window = w
        }
        NSApp.activate(ignoringOtherApps: true)
        window?.makeKeyAndOrderFront(nil)
    }

    func close() { window?.close() }

    /// 닫으면 창을 버려서, 보이지 않는 설정 화면이 매 초 다시 그려지지 않게 한다.
    func windowWillClose(_ notification: Notification) {
        guard let w = notification.object as? NSWindow, w === window else { return }
        DispatchQueue.main.async { [weak self] in
            if self?.window === w, w.isVisible == false {
                w.contentViewController = nil
                self?.window = nil
            }
        }
    }
}
