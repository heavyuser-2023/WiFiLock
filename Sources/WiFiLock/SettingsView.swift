import AppKit
import SwiftUI
import WiFiLockCore

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
                Text("자동 잠금 사용")
                Text("키보드·마우스 입력이 없으면 아래 시간 뒤에 화면을 잠급니다.")
            }
            Toggle("로그인할 때 자동으로 실행", isOn: Binding(
                get: { launchAtLogin.isEnabled || launchAtLogin.needsApproval },
                set: { launchAtLogin.set($0) }
            ))
            if launchAtLogin.needsApproval {
                HStack {
                    Label("시스템 설정의 로그인 항목에서 허용이 필요합니다", systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.orange)
                    Spacer()
                    Button("열기") { launchAtLogin.openLoginItemsSettings() }
                }
            }
            if let err = launchAtLogin.lastError {
                Text(err).font(.caption).foregroundStyle(.red)
            }
        } header: {
            Text("일반")
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
                        Text("공유기 주소 \(mac)").font(.caption).foregroundStyle(.secondary)
                    }
                    Text("지금 적용: \(KoreanFormat.timeout(controller.status.policy.timeoutSeconds)) · \(controller.status.policy.source.koreanDescription)")
                        .font(.caption).foregroundStyle(.secondary)
                    Text(StatusBarController.stateText(controller.status.state))
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                if canAddCurrent {
                    Button("등록") { store.addRule(for: wifi.snapshot) }
                }
            }
            .padding(.vertical, 2)

            if !wifi.hasLocationPermission {
                VStack(alignment: .leading, spacing: 6) {
                    Label("위치 권한이 없어 와이파이 이름을 읽을 수 없습니다", systemImage: "location.slash")
                        .foregroundStyle(.orange)
                    Text(permissionExplanation)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Button(wifi.locationStatus == .notDetermined ? "위치 권한 요청" : "시스템 설정에서 허용하기") {
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
            Text("현재 네트워크")
        }
    }

    private var permissionExplanation: String {
        let head = "macOS 는 위치 권한을 받은 앱에만 와이파이 이름을 알려 줍니다. "
        let tail = " 위치 정보는 와이파이 이름 확인에만 쓰며 저장하거나 밖으로 보내지 않습니다."
        if wifi.snapshot.isConnected, wifi.snapshot.routerMAC != nil {
            return head + "지금은 공유기 주소로 네트워크를 구분하고 있지만, 와이파이 이름으로 구분하려면 권한을 허용해 주세요." + tail
        }
        return head + "권한이 없으면 '와이파이 연결됨'과 '연결 안 됨'만 구분해 기본 잠금 시간을 적용합니다. 집·사무실처럼 네트워크마다 시간을 다르게 하려면 권한을 허용해 주세요." + tail
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
                Text("등록된 네트워크가 없습니다. 집이나 사무실처럼 믿을 수 있는 네트워크를 등록하면 그곳에서는 잠금 시간을 길게 둘 수 있습니다.")
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
                    Label("현재 네트워크 추가", systemImage: "plus")
                }
                .disabled(!canAddCurrent)
                Button {
                    newSSID = ""
                    newTimeout = AppSettings.defaultTrustedTimeout
                    showingAddSheet = true
                } label: {
                    Label("이름으로 추가…", systemImage: "keyboard")
                }
                Spacer()
            }
        } header: {
            Text("네트워크별 잠금 시간")
        } footer: {
            Text("위에 있는 규칙부터 적용됩니다. 공유기 주소까지 기록된 규칙은 이름이 같아도 공유기가 다르면 적용하지 않습니다. 이름으로만 추가한 규칙은 같은 이름의 모든 와이파이에 적용됩니다.")
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
            TimeoutPicker(title: "등록되지 않은 와이파이", systemImage: "wifi", selection: $store.settings.unknownWiFiTimeoutSeconds)
            TimeoutPicker(title: "와이파이 꺼짐 · 연결 안 됨", systemImage: "wifi.slash", selection: $store.settings.noWiFiTimeoutSeconds)
        } header: {
            Text("기본 잠금 시간")
        } footer: {
            Text("등록되지 않은 곳에서는 짧게 두면 노트북을 두고 자리를 비웠을 때 더 안전합니다.")
                .font(.caption).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    // MARK: 고급

    private var advancedSection: some View {
        Section {
            Toggle("잠그기 전에 화면에 경고 표시", isOn: $store.settings.warnBeforeLock)
            Picker("경고 표시 시간", selection: $store.settings.warningSeconds) {
                ForEach(warningOptions, id: \.self) { s in
                    Text(KoreanFormat.duration(s)).tag(s)
                }
            }
            .disabled(!store.settings.warnBeforeLock)
            if store.settings.warnBeforeLock {
                Text("잠금 시간이 짧으면 경고는 잠금 시간의 절반까지만 표시됩니다. 경고 중에 마우스나 키보드를 움직이면 취소됩니다.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Toggle(isOn: $store.settings.respectDisplaySleepAssertions) {
                Text("동영상 재생 중에는 잠그지 않기")
                Text("앱이 화면을 켜 두도록 요청한 동안(동영상, 발표, 화상 회의 등)은 잠금을 미룹니다.")
            }
            Toggle("메뉴 막대에 남은 시간 표시", isOn: $store.settings.showCountdownInMenuBar)
        } header: {
            Text("고급")
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
            Text("와이파이 이름으로 추가").font(.headline)
            Text("대소문자까지 정확히 입력해 주세요. 이름으로 구분하려면 위치 권한이 필요합니다.")
                .font(.caption).foregroundStyle(.secondary)
            TextField("와이파이 이름", text: $newSSID)
                .textFieldStyle(.roundedBorder)
                .onSubmit(commitAdd)
            TimeoutPicker(title: "잠금 시간", systemImage: "timer", selection: $newTimeout)
            HStack {
                Spacer()
                Button("취소", role: .cancel) { showingAddSheet = false }
                    .keyboardShortcut(.cancelAction)
                Button("추가", action: commitAdd)
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
                .help(isCurrent ? "지금 연결된 네트워크" : "")
            VStack(alignment: .leading, spacing: 2) {
                TextField("이름", text: $rule.name)
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
            .help("이 네트워크 삭제")
        }
    }

    private var identifierText: String {
        var parts: [String] = []
        if let ssid = rule.ssid { parts.append("와이파이 이름: \(ssid)") }
        if let mac = rule.routerMAC { parts.append("공유기: \(mac)") }
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
            w.title = "\(AppInfo.displayName) 설정"
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
