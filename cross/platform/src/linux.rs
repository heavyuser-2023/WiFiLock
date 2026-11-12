//! 리눅스 구현 (systemd + 그놈·KDE 데스크톱 기준)
//!
//! - 자리비움 시간: 그놈 Mutter IdleMonitor → freedesktop ScreenSaver(KDE) → X11 XScreenSaver 확장 순서로 시도
//! - 잠김 여부: logind 세션의 `LockedHint`·`Active` → freedesktop ScreenSaver `GetActive`
//! - 화면 유지 요청: 그놈 세션 관리자 억제 목록 → logind 억제 목록 → KDE `HasInhibit`
//! - 잠그기: logind `Session.Lock` → freedesktop ScreenSaver `Lock` → `loginctl lock-session` → `xdg-screensaver lock`
//! - 와이파이: 네트워크매니저(D-Bus), 없으면 `/sys/class/net` 으로 연결 여부만. 공유기 주소는 `/proc/net/route`·`/proc/net/arp`

use std::fs;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use wifilock_core::{WiFiSnapshot, WiFiStatus};
use x11rb::connection::{Connection as _, RequestConnection};
use x11rb::protocol::screensaver::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use zbus::blocking::{connection, proxy::Builder, Connection, Proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;

use crate::parse;
use crate::{LocationStatus, Platform};

/// CLOCK_MONOTONIC 은 절전 동안 멈추고, CLOCK_BOOTTIME 은 계속 흐른다.
pub fn clocks() -> crate::Clocks {
    let read = |id: libc::clockid_t| {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        unsafe { libc::clock_gettime(id, &mut ts) };
        ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9
    };
    crate::Clocks { awake: read(libc::CLOCK_MONOTONIC), total: read(libc::CLOCK_BOOTTIME) }
}

const NM: &str = "org.freedesktop.NetworkManager";
const LOGIN1: &str = "org.freedesktop.login1";
const SESSION_AUTO: &str = "/org/freedesktop/login1/session/auto";
/// 디버스 상대가 응답하지 않을 때 기다리는 최대 시간 (기본은 무한정)
const DBUS_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, PartialEq, Debug)]
enum IdleMethod {
    Unknown,
    Mutter,
    FreedesktopScreenSaver,
    X11,
    None,
}

struct X11 {
    conn: RustConnection,
    root: u32,
}

pub struct LinuxPlatform {
    session: Option<Connection>,
    system: Option<Connection>,
    idle_method: Mutex<IdleMethod>,
    x11: Mutex<Option<X11>>,
    last_lock_method: Mutex<String>,
    /// (장치|게이트웨이 주소) → 공유기 하드웨어 주소. 잠깐 못 읽어도 규칙이 풀리지 않도록 보관한다.
    router_cache: Mutex<Option<(String, String)>>,
}

fn proxy<'a>(conn: &'a Connection, dest: &'a str, path: &'a str, iface: &'a str) -> zbus::Result<Proxy<'a>> {
    Builder::new(conn)
        .destination(dest)?
        .path(path)?
        .interface(iface)?
        .cache_properties(CacheProperties::No)
        .build()
}

fn is_kde() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").map(|v| v.to_uppercase().contains("KDE")).unwrap_or(false)
}

fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").map(|v| v == "wayland").unwrap_or(false)
}

impl LinuxPlatform {
    pub fn new() -> Self {
        let session = connection::Builder::session().ok().and_then(|b| b.method_timeout(DBUS_TIMEOUT).build().ok());
        let system = connection::Builder::system().ok().and_then(|b| b.method_timeout(DBUS_TIMEOUT).build().ok());
        LinuxPlatform {
            session,
            system,
            idle_method: Mutex::new(IdleMethod::Unknown),
            x11: Mutex::new(None),
            last_lock_method: Mutex::new(String::new()),
            router_cache: Mutex::new(None),
        }
    }

    fn mutter_idle(&self) -> Option<f64> {
        let c = self.session.as_ref()?;
        let p = proxy(c, "org.gnome.Mutter.IdleMonitor", "/org/gnome/Mutter/IdleMonitor/Core", "org.gnome.Mutter.IdleMonitor")
            .ok()?;
        let ms: u64 = p.call("GetIdletime", &()).ok()?;
        Some(ms as f64 / 1000.0)
    }

    /// KDE 의 화면 보호기 서비스 (밀리초를 돌려준다). 다른 데스크톱의 같은 이름 서비스는 단위가 달라 쓰지 않는다.
    fn freedesktop_idle(&self) -> Option<f64> {
        if !is_kde() {
            return None;
        }
        let c = self.session.as_ref()?;
        for path in ["/org/freedesktop/ScreenSaver", "/ScreenSaver"] {
            if let Ok(p) = proxy(c, "org.freedesktop.ScreenSaver", path, "org.freedesktop.ScreenSaver") {
                if let Ok(ms) = p.call::<_, _, u32>("GetSessionIdleTime", &()) {
                    return Some(ms as f64 / 1000.0);
                }
            }
        }
        None
    }

    fn x11_idle(&self) -> Option<f64> {
        if is_wayland() || std::env::var_os("DISPLAY").is_none() {
            return None;
        }
        let mut slot = self.x11.lock().unwrap();
        if slot.is_none() {
            let (conn, screen) = x11rb::connect(None).ok()?;
            conn.extension_information(x11rb::protocol::screensaver::X11_EXTENSION_NAME).ok()??;
            let root = conn.setup().roots.get(screen)?.root;
            *slot = Some(X11 { conn, root });
        }
        let x = slot.as_ref()?;
        match x.conn.screensaver_query_info(x.root).ok().and_then(|c| c.reply().ok()) {
            Some(info) => Some(info.ms_since_user_input as f64 / 1000.0),
            None => {
                *slot = None;
                None
            }
        }
    }

    fn session_proxy(&self) -> Option<Proxy<'_>> {
        proxy(self.system.as_ref()?, LOGIN1, SESSION_AUTO, "org.freedesktop.login1.Session").ok()
    }

    fn gnome_inhibitors(&self) -> Option<Vec<String>> {
        let c = self.session.as_ref()?;
        let sm = proxy(c, "org.gnome.SessionManager", "/org/gnome/SessionManager", "org.gnome.SessionManager").ok()?;
        // 8 = 자리비움(화면 보호기·잠금) 억제
        let inhibited: bool = sm.call("IsInhibited", &(8u32,)).ok()?;
        if !inhibited {
            return Some(Vec::new());
        }
        let list: Vec<OwnedObjectPath> = sm.call("GetInhibitors", &()).unwrap_or_default();
        let mut names = Vec::new();
        for path in list {
            let Ok(ip) = proxy(c, "org.gnome.SessionManager", path.as_str(), "org.gnome.SessionManager.Inhibitor") else {
                continue;
            };
            let flags: u32 = ip.call("GetFlags", &()).unwrap_or(0);
            if flags & 8 == 0 {
                continue;
            }
            let app: String = ip.call("GetAppId", &()).unwrap_or_default();
            let name = if app.is_empty() { "다른 앱".to_string() } else { app.trim_end_matches(".desktop").to_string() };
            if !names.contains(&name) {
                names.push(name);
            }
        }
        if names.is_empty() {
            names.push("다른 앱".into());
        }
        Some(names)
    }

    fn logind_inhibitors(&self) -> Option<Vec<String>> {
        let c = self.system.as_ref()?;
        let m = proxy(c, LOGIN1, "/org/freedesktop/login1", "org.freedesktop.login1.Manager").ok()?;
        let list: Vec<(String, String, String, String, u32, u32)> = m.call("ListInhibitors", &()).ok()?;
        let me = std::process::id();
        let mut names = Vec::new();
        for (what, who, _why, mode, _uid, pid) in list {
            if pid == me || mode != "block" || !what.split(':').any(|w| w == "idle") {
                continue;
            }
            if !names.contains(&who) {
                names.push(who);
            }
        }
        Some(names)
    }

    /// KDE 전원 관리(파워데빌)가 받은 "화면 설정 변경 억제"(4) 요청. 브라우저의 동영상 재생 등이 여기로 온다.
    fn kde_inhibitors(&self) -> Option<Vec<String>> {
        let c = self.session.as_ref()?;
        let p = proxy(
            c,
            "org.kde.Solid.PowerManagement.PolicyAgent",
            "/org/kde/Solid/PowerManagement/PolicyAgent",
            "org.kde.Solid.PowerManagement.PolicyAgent",
        )
        .ok()?;
        let has: bool = p.call("HasInhibition", &(4u32,)).ok()?;
        if !has {
            return Some(Vec::new());
        }
        let list: Vec<(String, String)> = p.call("ListInhibitions", &()).unwrap_or_default();
        let mut names: Vec<String> = Vec::new();
        for (app, _reason) in list {
            if !app.is_empty() && !names.contains(&app) {
                names.push(app);
            }
        }
        if names.is_empty() {
            names.push("다른 앱".into());
        }
        Some(names)
    }

    fn clear_router_cache(&self) {
        *self.router_cache.lock().unwrap() = None;
    }

    /// 공유기 주소를 읽고, 잠깐 못 읽으면 같은 연결·장치·게이트웨이의 마지막 값을 쓴다.
    /// `conn` 은 연결마다 달라지는 값(네트워크매니저의 활성 연결 경로)이라, 게이트웨이 주소가 흔한 값이어도
    /// 다른 네트워크의 공유기 주소를 잘못 이어 쓰지 않는다.
    fn router_mac(&self, iface: &str, conn: &str) -> Option<String> {
        let route = fs::read_to_string("/proc/net/route").ok();
        let gw = route.as_deref().and_then(|r| parse::proc_route_gateway(r, iface));
        let mut cache = self.router_cache.lock().unwrap();
        let Some(gw) = gw else {
            *cache = None;
            return None;
        };
        let key = format!("{conn}|{iface}|{}.{}.{}.{}", gw[0], gw[1], gw[2], gw[3]);
        let found = fs::read_to_string("/proc/net/arp").ok().and_then(|a| parse::proc_arp_mac(&a, gw, iface));
        match found {
            Some(mac) => {
                *cache = Some((key, mac.clone()));
                Some(mac)
            }
            None => cache.as_ref().filter(|(k, _)| *k == key).map(|(_, m)| m.clone()),
        }
    }

    fn screensaver_active(&self) -> Option<bool> {
        let c = self.session.as_ref()?;
        for path in ["/org/freedesktop/ScreenSaver", "/ScreenSaver"] {
            if let Ok(p) = proxy(c, "org.freedesktop.ScreenSaver", path, "org.freedesktop.ScreenSaver") {
                if let Ok(v) = p.call::<_, _, bool>("GetActive", &()) {
                    return Some(v);
                }
            }
        }
        None
    }

    /// logind 가 알려 주는 잠김 여부. 읽을 수 없으면 None.
    fn logind_locked(&self) -> Option<bool> {
        let s = self.session_proxy()?;
        let locked = s.get_property::<bool>("LockedHint").ok();
        let active = s.get_property::<bool>("Active").ok();
        if locked.is_none() && active.is_none() {
            return None;
        }
        Some(locked.unwrap_or(false) || !active.unwrap_or(true))
    }

    /// 잠금 명령 뒤 확인용: 어느 쪽이든 잠김을 가리키면 잠긴 것으로 본다.
    fn locked_any(&self) -> bool {
        self.logind_locked().unwrap_or(false) || self.screensaver_active().unwrap_or(false)
    }

    /// 잠금 명령을 보낸 뒤 실제로 잠겼는지 최대 2초 확인한다.
    fn wait_locked(&self) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            if self.locked_any() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        self.locked_any()
    }

    fn nm_snapshot(&self) -> Option<WiFiSnapshot> {
        let c = self.system.as_ref()?;
        let nm = proxy(c, NM, "/org/freedesktop/NetworkManager", NM).ok()?;
        let devices: Vec<OwnedObjectPath> = nm.call("GetDevices", &()).ok()?;
        let mut wifi_devices = Vec::new();
        for d in &devices {
            let Ok(dev) = proxy(c, NM, d.as_str(), "org.freedesktop.NetworkManager.Device") else { continue };
            if dev.get_property::<u32>("DeviceType").ok() == Some(2) {
                let iface: String = dev.get_property("Interface").unwrap_or_default();
                let state: u32 = dev.get_property("State").unwrap_or(0);
                wifi_devices.push((d.clone(), iface, state));
            }
        }
        if wifi_devices.is_empty() {
            return Some(WiFiSnapshot::status(WiFiStatus::Unavailable));
        }
        let enabled: bool = nm.get_property("WirelessEnabled").unwrap_or(true);
        if !enabled {
            self.clear_router_cache();
            return Some(WiFiSnapshot::new(WiFiStatus::PoweredOff, None, None, Some(&wifi_devices[0].1)));
        }
        // 100 = 연결 완료(activated)
        let Some((path, iface, _)) = wifi_devices.iter().find(|(_, _, s)| *s == 100) else {
            self.clear_router_cache();
            return Some(WiFiSnapshot::new(WiFiStatus::Disconnected, None, None, Some(&wifi_devices[0].1)));
        };
        let active_conn = proxy(c, NM, path.as_str(), "org.freedesktop.NetworkManager.Device")
            .ok()
            .and_then(|d| d.get_property::<OwnedObjectPath>("ActiveConnection").ok())
            .map(|p| p.as_str().to_string())
            .unwrap_or_default();
        let ssid = proxy(c, NM, path.as_str(), "org.freedesktop.NetworkManager.Device.Wireless")
            .ok()
            .and_then(|w| w.get_property::<OwnedObjectPath>("ActiveAccessPoint").ok())
            .filter(|ap| ap.as_str() != "/")
            .and_then(|ap| {
                proxy(c, NM, ap.as_str(), "org.freedesktop.NetworkManager.AccessPoint")
                    .ok()?
                    .get_property::<Vec<u8>>("Ssid")
                    .ok()
            })
            .and_then(|b| parse::ssid_from_bytes(&b));
        let router = self.router_mac(iface, &format!("{active_conn}|{}", ssid.as_deref().unwrap_or("")));
        Some(WiFiSnapshot::new(WiFiStatus::Connected, ssid.as_deref(), router.as_deref(), Some(iface)))
    }

    /// 네트워크매니저가 없을 때: 무선 장치가 있는지, 연결되어 있는지만 본다.
    fn sysfs_snapshot(&self) -> WiFiSnapshot {
        let mut first: Option<String> = None;
        if let Ok(entries) = fs::read_dir("/sys/class/net") {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !e.path().join("wireless").exists() && !e.path().join("phy80211").exists() {
                    continue;
                }
                let up = fs::read_to_string(e.path().join("operstate")).map(|s| s.trim() == "up").unwrap_or(false);
                if up {
                    let router = self.router_mac(&name, "sysfs");
                    return WiFiSnapshot::new(WiFiStatus::Connected, None, router.as_deref(), Some(&name));
                }
                first.get_or_insert(name);
            }
        }
        *self.router_cache.lock().unwrap() = None;
        let Some(name) = first else { return WiFiSnapshot::status(WiFiStatus::Unavailable) };
        let status = if rfkill_wlan_blocked() { WiFiStatus::PoweredOff } else { WiFiStatus::Disconnected };
        WiFiSnapshot::new(status, None, None, Some(&name))
    }
}

fn rfkill_wlan_blocked() -> bool {
    let Ok(entries) = fs::read_dir("/sys/class/rfkill") else { return false };
    let mut any = false;
    for e in entries.flatten() {
        let p = e.path();
        if fs::read_to_string(p.join("type")).map(|t| t.trim() == "wlan").unwrap_or(false) {
            any = true;
            let soft = fs::read_to_string(p.join("soft")).map(|v| v.trim() == "1").unwrap_or(false);
            let hard = fs::read_to_string(p.join("hard")).map(|v| v.trim() == "1").unwrap_or(false);
            if !(soft || hard) {
                return false;
            }
        }
    }
    any
}

impl Platform for LinuxPlatform {
    fn os(&self) -> &'static str {
        "linux"
    }

    fn idle_seconds(&self) -> Option<f64> {
        let mut m = self.idle_method.lock().unwrap();
        let try_method = |method: IdleMethod| match method {
            IdleMethod::Mutter => self.mutter_idle(),
            IdleMethod::FreedesktopScreenSaver => self.freedesktop_idle(),
            IdleMethod::X11 => self.x11_idle(),
            _ => None,
        };
        if !matches!(*m, IdleMethod::Unknown | IdleMethod::None) {
            if let Some(v) = try_method(*m) {
                return Some(v);
            }
        }
        // 처음이거나 쓰던 방법이 실패하면 다시 고른다.
        // X11 세션이면 단위가 확실한 X11 확장을 먼저, 웨이랜드면 그놈 → KDE 순서.
        let order = if is_wayland() {
            [IdleMethod::Mutter, IdleMethod::FreedesktopScreenSaver, IdleMethod::X11]
        } else {
            [IdleMethod::X11, IdleMethod::Mutter, IdleMethod::FreedesktopScreenSaver]
        };
        for method in order {
            if let Some(v) = try_method(method) {
                *m = method;
                return Some(v);
            }
        }
        *m = IdleMethod::None;
        None
    }

    /// 평소 판단: logind 의 잠금 표시·세션 활성 여부를 먼저 본다.
    /// 화면 보호기 서비스의 GetActive 는 데스크톱마다 뜻이 달라서(XFCE 등은 "잠금 없이 화면 보호기만 도는 중"도 참)
    /// KDE 이거나 logind 를 읽을 수 없을 때만 쓴다. 그러지 않으면 화면 보호기만 돈 것을 잠김으로 보고 끝내 잠그지 않는다.
    fn screen_locked(&self) -> bool {
        match self.logind_locked() {
            Some(true) => true,
            Some(false) => is_kde() && self.screensaver_active().unwrap_or(false),
            None => self.screensaver_active().unwrap_or(false),
        }
    }

    fn display_sleep_preventers(&self) -> Vec<String> {
        if let Some(v) = self.gnome_inhibitors() {
            return v;
        }
        if let Some(v) = self.logind_inhibitors() {
            if !v.is_empty() {
                return v;
            }
        }
        if let Some(v) = self.kde_inhibitors() {
            return v;
        }
        Vec::new()
    }

    /// logind 의 Lock 은 신호만 보내므로, 받아 줄 잠금 프로그램이 없어도 성공을 돌려준다.
    /// 그래서 방법마다 실제로 잠겼는지 확인하고, 안 잠겼으면 다음 방법을 시도한다.
    fn lock_screen(&self) -> Result<String, String> {
        let mut errors = Vec::new();
        if let Some(s) = self.session_proxy() {
            match s.call::<_, _, ()>("Lock", &()) {
                Ok(()) if self.wait_locked() => {
                    *self.last_lock_method.lock().unwrap() = "logind".into();
                    return Ok("화면 잠금 (logind Session.Lock)".into());
                }
                Ok(()) => errors.push("logind: 명령은 성공했지만 잠기지 않음".into()),
                Err(e) => errors.push(format!("logind: {e}")),
            }
        }
        if let Some(c) = self.session.as_ref() {
            for path in ["/org/freedesktop/ScreenSaver", "/ScreenSaver"] {
                if let Ok(p) = proxy(c, "org.freedesktop.ScreenSaver", path, "org.freedesktop.ScreenSaver") {
                    match p.call::<_, _, ()>("Lock", &()) {
                        Ok(()) if self.wait_locked() => return Ok("화면 잠금 (org.freedesktop.ScreenSaver.Lock)".into()),
                        Ok(()) => errors.push("ScreenSaver: 명령은 성공했지만 잠기지 않음".into()),
                        Err(e) => errors.push(format!("ScreenSaver: {e}")),
                    }
                }
            }
        }
        for (cmd, args) in [("loginctl", &["lock-session"][..]), ("xdg-screensaver", &["lock"][..])] {
            match Command::new(cmd).args(args).status() {
                Ok(s) if s.success() && self.wait_locked() => return Ok(format!("화면 잠금 ({cmd} {})", args.join(" "))),
                Ok(s) if s.success() => errors.push(format!("{cmd}: 명령은 성공했지만 잠기지 않음")),
                Ok(s) => errors.push(format!("{cmd}: 종료 코드 {s}")),
                Err(e) => errors.push(format!("{cmd}: {e}")),
            }
        }
        Err(format!("화면을 잠그지 못함 ({})", errors.join("; ")))
    }

    fn wifi_snapshot(&self) -> WiFiSnapshot {
        self.nm_snapshot().unwrap_or_else(|| self.sysfs_snapshot())
    }

    fn location_status(&self) -> LocationStatus {
        LocationStatus::NotRequired
    }

    fn request_location_permission(&self) {}

    fn open_location_settings(&self) {}

    fn diagnostics(&self) -> Vec<(String, String)> {
        let idle = match *self.idle_method.lock().unwrap() {
            IdleMethod::Mutter => "그놈 Mutter IdleMonitor",
            IdleMethod::FreedesktopScreenSaver => "org.freedesktop.ScreenSaver",
            IdleMethod::X11 => "X11 XScreenSaver 확장",
            IdleMethod::None => "읽을 수 없음",
            IdleMethod::Unknown => "아직 확인 전",
        };
        vec![
            ("자리비움 시간".into(), idle.into()),
            ("화면 잠그기".into(), "logind → ScreenSaver → loginctl → xdg-screensaver".into()),
            ("잠김 여부".into(), "logind LockedHint/Active → ScreenSaver.GetActive".into()),
            ("화면 유지 요청".into(), "그놈 세션 관리자 → logind → KDE 전원 관리".into()),
            ("와이파이".into(), if self.system.is_some() { "네트워크매니저 (D-Bus)" } else { "/sys/class/net" }.into()),
            ("세션 버스".into(), if self.session.is_some() { "연결됨" } else { "없음" }.into()),
            ("시스템 버스".into(), if self.system.is_some() { "연결됨" } else { "없음" }.into()),
        ]
    }
}
