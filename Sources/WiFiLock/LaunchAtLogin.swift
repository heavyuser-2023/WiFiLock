import Combine
import Foundation
import ServiceManagement

/// 로그인 시 자동 실행 (macOS 13 이상의 로그인 항목)
final class LaunchAtLogin: ObservableObject {
    @Published private(set) var isEnabled = false
    @Published private(set) var needsApproval = false
    @Published var lastError: String?

    init() { refresh() }

    func refresh() {
        let status = SMAppService.mainApp.status
        isEnabled = status == .enabled
        needsApproval = status == .requiresApproval
    }

    func set(_ enabled: Bool) {
        do {
            if enabled {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            lastError = nil
            AppLog.info("로그인 시 자동 실행: \(enabled)")
        } catch {
            lastError = "로그인 항목을 바꾸지 못했습니다: \(error.localizedDescription)"
            AppLog.error(lastError!)
        }
        refresh()
    }

    func openLoginItemsSettings() {
        SMAppService.openSystemSettingsLoginItems()
    }
}
