import AppKit
import Combine
import CoreLocation
import CoreWLAN
import Foundation
import WiFiLockCore

/// 와이파이 연결 상태를 지켜본다.
///
/// macOS 14 부터는 위치 권한이 있어야 와이파이 이름(SSID)을 읽을 수 있다.
/// 권한이 없을 때를 대비해 공유기 하드웨어 주소도 함께 읽어 네트워크를 구분한다.
final class WiFiMonitor: NSObject, ObservableObject {
    @Published private(set) var snapshot: WiFiSnapshot = .unknown
    @Published private(set) var locationStatus: CLAuthorizationStatus = .notDetermined

    private let client = CWWiFiClient.shared()
    private let locationManager = CLLocationManager()
    private let routerQueue = DispatchQueue(label: "wifilock.router")
    private var pollTimer: Timer?

    // 공유기 주소 조회 캐시
    private var routerKey: String?
    private var routerMAC: String?
    private var routerCheckedAt: Date = .distantPast
    private var routerLookupInFlight = false
    /// 연속 실패 횟수 (macOS 가 주소를 가리는 환경에서 외부 명령을 계속 실행하지 않도록 간격을 늘린다)
    private var routerFailures = 0

    override init() {
        super.init()
        locationManager.delegate = self
        locationStatus = locationManager.authorizationStatus
    }

    func start() {
        client.delegate = self
        for event in [CWEventType.powerDidChange, .ssidDidChange, .bssidDidChange, .linkDidChange, .modeDidChange] {
            do { try client.startMonitoringEvent(with: event) } catch {
                AppLog.error("와이파이 이벤트 구독 실패(\(event.rawValue)): \(error.localizedDescription)")
            }
        }
        refresh()
        // 이벤트를 놓치는 경우를 대비해 주기적으로도 확인한다.
        pollTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in self?.refresh() }
        pollTimer?.tolerance = 1
    }

    func stop() {
        pollTimer?.invalidate()
        try? client.stopMonitoringAllEvents()
    }

    var hasLocationPermission: Bool {
        locationStatus == .authorizedAlways || locationStatus == .authorized
    }

    var locationServicesEnabled: Bool { CLLocationManager.locationServicesEnabled() }

    func requestLocationPermission() {
        if locationStatus == .notDetermined {
            locationManager.requestWhenInUseAuthorization()
        }
    }

    func openLocationSettings() {
        let urls = [
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_LocationServices",
            "x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices",
        ]
        for s in urls {
            if let url = URL(string: s), NSWorkspace.shared.open(url) { return }
        }
    }

    /// 자가 진단 전용: 감시를 시작하지 않은 인스턴스에 가짜 상태를 넣는다.
    func overrideSnapshotForTesting(_ s: WiFiSnapshot) {
        snapshot = s
    }

    /// 현재 상태를 다시 읽는다 (주 스레드에서 호출).
    func refresh() {
        let new = readSnapshot()
        if new != snapshot {
            AppLog.info("와이파이 상태: \(new.koreanDescription) / 공유기 \(new.routerMAC ?? "-")")
            snapshot = new
        }
    }

    private func readSnapshot() -> WiFiSnapshot {
        guard let iface = client.interface() else {
            resetRouterCache()
            return WiFiSnapshot(status: .unavailable)
        }
        let name = iface.interfaceName
        guard iface.powerOn() else {
            resetRouterCache()
            return WiFiSnapshot(status: .poweredOff, interfaceName: name)
        }
        let rssi = iface.rssiValue()
        let associated = iface.interfaceMode() != .none || rssi != 0
        guard associated else {
            resetRouterCache()
            return WiFiSnapshot(status: .disconnected, interfaceName: name)
        }
        let ssid = iface.ssid()
        updateRouterIfNeeded(interface: name, ssid: ssid)
        // 신호 세기는 매번 조금씩 바뀌어 불필요한 갱신을 일으키므로 상태에 넣지 않는다.
        return WiFiSnapshot(status: .connected, ssid: ssid, routerMAC: routerMAC, interfaceName: name)
    }

    private func resetRouterCache() {
        routerKey = nil
        routerMAC = nil
        routerCheckedAt = .distantPast
        routerFailures = 0
    }

    /// 공유기 주소는 외부 명령으로 읽으므로 네트워크가 바뀌었거나 일정 시간이 지났을 때만 다시 읽는다.
    private func updateRouterIfNeeded(interface: String?, ssid: String?) {
        guard let interface else { return }
        let key = "\(interface)|\(ssid ?? "")"
        if key != routerKey {
            routerKey = key
            routerMAC = nil
            routerCheckedAt = .distantPast
            routerFailures = 0
        }
        let age = Date().timeIntervalSince(routerCheckedAt)
        // 실패하면 4초, 8초, 16초 … 최대 5분 간격으로 다시 시도
        let retry = min(300, 4 * pow(2, Double(min(routerFailures, 7))))
        let due = routerMAC == nil ? age > retry : age > 60
        guard due, !routerLookupInFlight else { return }
        routerLookupInFlight = true
        routerCheckedAt = Date()
        routerQueue.async { [weak self] in
            let mac = RouterLookup.routerMAC(interface: interface)
            DispatchQueue.main.async {
                guard let self else { return }
                self.routerLookupInFlight = false
                guard self.routerKey == key else { return }
                self.routerFailures = mac == nil ? self.routerFailures + 1 : 0
                if mac != self.routerMAC {
                    self.routerMAC = mac
                    self.refresh()
                }
            }
        }
    }
}

extension WiFiMonitor: CWEventDelegate {
    func powerStateDidChangeForWiFiInterface(withName interfaceName: String) { scheduleRefresh() }
    func ssidDidChangeForWiFiInterface(withName interfaceName: String) { scheduleRefresh() }
    func bssidDidChangeForWiFiInterface(withName interfaceName: String) { scheduleRefresh() }
    func linkDidChangeForWiFiInterface(withName interfaceName: String) { scheduleRefresh() }
    func modeDidChangeForWiFiInterface(withName interfaceName: String) { scheduleRefresh() }

    private func scheduleRefresh() {
        DispatchQueue.main.async { [weak self] in
            self?.refresh()
            // 연결 직후에는 주소 할당이 늦을 수 있어 한 번 더 확인
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) { self?.refresh() }
        }
    }
}

extension WiFiMonitor: CLLocationManagerDelegate {
    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.locationStatus = manager.authorizationStatus
            AppLog.info("위치 권한 상태: \(self.locationStatus.rawValue)")
            self.routerKey = nil // 이름을 새로 읽게 되면 캐시를 다시 만든다
            self.refresh()
        }
    }
}

/// 공유기(기본 게이트웨이) 하드웨어 주소 조회
enum RouterLookup {
    static func routerMAC(interface: String) -> String? {
        guard let ip = run("/usr/sbin/ipconfig", ["getoption", interface, "router"])?
            .trimmingCharacters(in: .whitespacesAndNewlines),
            !ip.isEmpty, ip.allSatisfy({ $0.isNumber || $0 == "." }) else { return nil }
        guard let arp = run("/usr/sbin/arp", ["-n", ip]) else { return nil }
        return parseARP(arp)
    }

    /// "? (192.168.0.1) at d8:7:b6:47:6b:8c on en0 ifscope [ethernet]" 에서 주소를 뽑는다.
    static func parseARP(_ output: String) -> String? {
        let words = output.split(whereSeparator: { $0 == " " || $0 == "\n" })
        guard let atIndex = words.firstIndex(of: "at"), atIndex + 1 < words.count else { return nil }
        return MACAddress.normalize(String(words[atIndex + 1]))
    }

    private static func run(_ path: String, _ args: [String]) -> String? {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: path)
        p.arguments = args
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        do { try p.run() } catch { return nil }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else { return nil }
        return String(data: data, encoding: .utf8)
    }
}
