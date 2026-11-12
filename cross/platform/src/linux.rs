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

use wifilock_core::{WiFiSnapshot, WiFiStatus};
use x11rb::connection::{Connection as _, RequestConnection};
use x11rb::protocol::screensaver::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use zbus::blocking::{proxy::Builder, Connection, Proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;

use crate::parse;
use crate::{LocationStatus, Platform};

const NM: &str = "org.freedesktop.NetworkManager";
const LOGIN1: &str = "org.freedesktop.login1";
const SESSION_AUTO: &str = "/org/freedesktop/login1/session/auto";

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
}

fn proxy<'a>(conn: &'a Connection, dest: &'a str, path: &'a str, iface: &'a str) -> zbus::Result<Proxy<'a>> {
    Builder::new(conn)
        .destination(dest)?
        .path(path)?
        .interface(iface)?
        .cache_properties(CacheProperties::No)
        .build()
}

fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").map(|v| v == "wayland").unwrap_or(false)
}

impl LinuxPlatform {
    pub fn new() -> Self {
        LinuxPlatform {
            session: Connection::session().ok(),
            system: Connection::system().ok(),
            idle_method: Mutex::new(IdleMethod::Unknown),
            x11: Mutex::new(None),
            last_lock_method: Mutex::new(String::new()),
        }
    }

    fn mutter_idle(&self) -> Option<f64> {
        let c = self.session.as_ref()?;
        let p = proxy(c, "org.gnome.Mutter.IdleMonitor", "/org/gnome/Mutter/IdleMonitor/Core", "org.gnome.Mutter.IdleMonitor")
            .ok()?;
        let ms: u64 = p.call("GetIdletime", &()).ok()?;
        Some(ms as f64 / 1000.0)
    }

    fn freedesktop_idle(&self) -> Option<f64> {
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

    fn kde_has_inhibit(&self) -> Option<bool> {
        let c = self.session.as_ref()?;
        let p = proxy(c, "org.freedesktop.ScreenSaver", "/ScreenSaver", "org.freedesktop.ScreenSaver").ok()?;
        p.call("HasInhibit", &()).ok()
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
            return Some(WiFiSnapshot::new(WiFiStatus::PoweredOff, None, None, Some(&wifi_devices[0].1)));
        }
        // 100 = 연결 완료(activated)
        let Some((path, iface, _)) = wifi_devices.iter().find(|(_, _, s)| *s == 100) else {
            return Some(WiFiSnapshot::new(WiFiStatus::Disconnected, None, None, Some(&wifi_devices[0].1)));
        };
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
        let router = gateway_mac(iface);
        Some(WiFiSnapshot::new(WiFiStatus::Connected, ssid.as_deref(), router.as_deref(), Some(iface)))
    }

    /// 네트워크매니저가 없을 때: 무선 장치가 있는지, 연결되어 있는지만 본다.
    fn sysfs_snapshot() -> WiFiSnapshot {
        let mut first: Option<String> = None;
        if let Ok(entries) = fs::read_dir("/sys/class/net") {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !e.path().join("wireless").exists() && !e.path().join("phy80211").exists() {
                    continue;
                }
                let up = fs::read_to_string(e.path().join("operstate")).map(|s| s.trim() == "up").unwrap_or(false);
                if up {
                    let router = gateway_mac(&name);
                    return WiFiSnapshot::new(WiFiStatus::Connected, None, router.as_deref(), Some(&name));
                }
                first.get_or_insert(name);
            }
        }
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

fn gateway_mac(iface: &str) -> Option<String> {
    let route = fs::read_to_string("/proc/net/route").ok()?;
    let gw = parse::proc_route_gateway(&route, iface)?;
    let arp = fs::read_to_string("/proc/net/arp").ok()?;
    parse::proc_arp_mac(&arp, gw, iface)
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
        // 처음이거나 쓰던 방법이 실패하면 다시 고른다
        for method in [IdleMethod::Mutter, IdleMethod::FreedesktopScreenSaver, IdleMethod::X11] {
            if let Some(v) = try_method(method) {
                *m = method;
                return Some(v);
            }
        }
        *m = IdleMethod::None;
        None
    }

    fn screen_locked(&self) -> bool {
        if let Some(s) = self.session_proxy() {
            let locked = s.get_property::<bool>("LockedHint").ok();
            let active = s.get_property::<bool>("Active").ok();
            if locked.is_some() || active.is_some() {
                return locked.unwrap_or(false) || !active.unwrap_or(true);
            }
        }
        if let Some(c) = self.session.as_ref() {
            for path in ["/org/freedesktop/ScreenSaver", "/ScreenSaver"] {
                if let Ok(p) = proxy(c, "org.freedesktop.ScreenSaver", path, "org.freedesktop.ScreenSaver") {
                    if let Ok(v) = p.call::<_, _, bool>("GetActive", &()) {
                        return v;
                    }
                }
            }
        }
        false
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
        if self.kde_has_inhibit() == Some(true) {
            return vec!["다른 앱".into()];
        }
        Vec::new()
    }

    fn lock_screen(&self) -> Result<String, String> {
        let mut errors = Vec::new();
        if let Some(s) = self.session_proxy() {
            match s.call::<_, _, ()>("Lock", &()) {
                Ok(()) => {
                    *self.last_lock_method.lock().unwrap() = "logind".into();
                    return Ok("화면 잠금 (logind Session.Lock)".into());
                }
                Err(e) => errors.push(format!("logind: {e}")),
            }
        }
        if let Some(c) = self.session.as_ref() {
            for path in ["/org/freedesktop/ScreenSaver", "/ScreenSaver"] {
                if let Ok(p) = proxy(c, "org.freedesktop.ScreenSaver", path, "org.freedesktop.ScreenSaver") {
                    match p.call::<_, _, ()>("Lock", &()) {
                        Ok(()) => return Ok("화면 잠금 (org.freedesktop.ScreenSaver.Lock)".into()),
                        Err(e) => errors.push(format!("ScreenSaver: {e}")),
                    }
                }
            }
        }
        for (cmd, args) in [("loginctl", &["lock-session"][..]), ("xdg-screensaver", &["lock"][..])] {
            match Command::new(cmd).args(args).status() {
                Ok(s) if s.success() => return Ok(format!("화면 잠금 ({cmd} {})", args.join(" "))),
                Ok(s) => errors.push(format!("{cmd}: 종료 코드 {s}")),
                Err(e) => errors.push(format!("{cmd}: {e}")),
            }
        }
        Err(format!("화면을 잠글 방법을 찾지 못함 ({})", errors.join("; ")))
    }

    fn wifi_snapshot(&self) -> WiFiSnapshot {
        self.nm_snapshot().unwrap_or_else(Self::sysfs_snapshot)
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
            ("화면 유지 요청".into(), "그놈 세션 관리자 → logind → KDE HasInhibit".into()),
            ("와이파이".into(), if self.system.is_some() { "네트워크매니저 (D-Bus)" } else { "/sys/class/net" }.into()),
            ("세션 버스".into(), if self.session.is_some() { "연결됨" } else { "없음" }.into()),
            ("시스템 버스".into(), if self.system.is_some() { "연결됨" } else { "없음" }.into()),
        ]
    }
}
