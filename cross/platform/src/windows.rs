//! 윈도우 구현
//!
//! - 자리비움 시간: `GetLastInputInfo`
//! - 잠김 여부: `WTSQuerySessionInformationW(WTSSessionInfoEx)` 의 잠금 플래그, 세션이 활성인지
//! - 화면 유지 요청: `CallNtPowerInformation(SystemExecutionState)` 의 화면 유지 표시 (앱 이름은 알 수 없음)
//! - 잠그기: `LockWorkStation`
//! - 와이파이: WLAN API (윈도우 11 24H2 부터 이름을 읽으려면 위치 권한 필요),
//!   공유기 주소는 기본 게이트웨이의 이웃 테이블(`GetIpNetTable2`)에서 찾는다

use std::ffi::c_void;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS, HANDLE, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetAdaptersAddresses, GetIpNetTable2, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST,
    GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH, MIB_IPNET_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::NetworkManagement::WiFi::{
    dot11_radio_state_off, wlan_interface_state_connected, wlan_intf_opcode_current_connection,
    wlan_intf_opcode_radio_state, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle,
    WlanQueryInterface, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST, WLAN_RADIO_STATE,
};
use windows::Win32::Networking::WinSock::{NlnsIncomplete, NlnsUnreachable, AF_INET, SOCKADDR_IN};
use windows::Win32::System::Power::{
    CallNtPowerInformation, SetThreadExecutionState, SystemExecutionState, ES_CONTINUOUS,
};
use windows::Win32::System::RemoteDesktop::{
    WTSActive, WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW, WTS_CURRENT_SERVER_HANDLE,
    WTS_CURRENT_SESSION,
};
use windows::Win32::System::Power::ES_DISPLAY_REQUIRED;
use windows::Win32::System::SystemInformation::{GetTickCount, GetTickCount64};
use windows::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::System::Shutdown::LockWorkStation;
use wifilock_core::{WiFiSnapshot, WiFiStatus};

use crate::parse;
use crate::{LocationStatus, Platform};

const IF_TYPE_IEEE80211: u32 = 71;

/// QueryUnbiasedInterruptTime 은 잠자기·최대 절전 동안 멈추고, GetTickCount64 는 계속 흐른다.
pub fn clocks() -> crate::Clocks {
    unsafe {
        let mut unbiased: u64 = 0;
        let ok = QueryUnbiasedInterruptTime(&mut unbiased).as_bool();
        let total = GetTickCount64() as f64 / 1000.0;
        let awake = if ok { unbiased as f64 / 1e7 } else { total };
        crate::Clocks { awake, total }
    }
}
const WTS_SESSIONSTATE_LOCK: i32 = 0;

pub struct WindowsPlatform {
    /// 와이파이 이름을 읽으려다 거부당했는지 (윈도우 11 24H2 위치 권한)
    ssid_denied: AtomicBool,
    /// (어댑터|게이트웨이 주소) → 공유기 하드웨어 주소. 깨어난 직후처럼 잠깐 못 읽어도 규칙이 풀리지 않도록 보관한다.
    router_cache: Mutex<Option<(String, String)>>,
}

impl WindowsPlatform {
    pub fn new() -> Self {
        WindowsPlatform { ssid_denied: AtomicBool::new(false), router_cache: Mutex::new(None) }
    }

    fn clear_router_cache(&self) {
        *self.router_cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// `conn` 은 연결을 구분하는 값(장치 이름과 와이파이 이름). 연결이 끊기면 보관값을 비우므로
    /// 게이트웨이 주소가 같은 다른 네트워크로 옮겨도 이전 공유기 주소를 이어 쓰지 않는다.
    fn router_mac(&self, conn: &str) -> Option<String> {
        let mut cache = self.router_cache.lock().unwrap_or_else(|e| e.into_inner());
        let Some((gw_key, found)) = gateway_mac() else {
            *cache = None;
            return None;
        };
        let key = format!("{conn}|{gw_key}");
        match found {
            Some(mac) => {
                *cache = Some((key, mac.clone()));
                Some(mac)
            }
            None => cache.as_ref().filter(|(k, _)| *k == key).map(|(_, m)| m.clone()),
        }
    }
}

struct WlanHandle(HANDLE);

impl WlanHandle {
    fn open() -> Option<Self> {
        let mut version = 0u32;
        let mut handle = HANDLE::default();
        let rc = unsafe { WlanOpenHandle(2, None, &mut version, &mut handle) };
        (rc == ERROR_SUCCESS.0).then_some(WlanHandle(handle))
    }
}

impl Drop for WlanHandle {
    fn drop(&mut self) {
        unsafe {
            WlanCloseHandle(self.0, None);
        }
    }
}

fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|c| *c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

/// 와이파이 장치들
fn wlan_interfaces(h: &WlanHandle) -> Vec<WLAN_INTERFACE_INFO> {
    let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
    let rc = unsafe { WlanEnumInterfaces(h.0, None, &mut list) };
    if rc != ERROR_SUCCESS.0 || list.is_null() {
        return Vec::new();
    }
    let out = unsafe {
        let n = (*list).dwNumberOfItems as usize;
        let first = (*list).InterfaceInfo.as_ptr();
        (0..n).map(|i| *first.add(i)).collect()
    };
    unsafe { WlanFreeMemory(list as *const c_void) };
    out
}

/// 무선 장치의 소프트웨어·하드웨어 스위치가 모두 꺼져 있는지
fn radio_off(h: &WlanHandle, guid: &GUID) -> bool {
    let mut size = 0u32;
    let mut data: *mut c_void = std::ptr::null_mut();
    let rc = unsafe { WlanQueryInterface(h.0, guid, wlan_intf_opcode_radio_state, None, &mut size, &mut data, None) };
    if rc != ERROR_SUCCESS.0 || data.is_null() {
        return false;
    }
    let off = unsafe {
        let rs = &*(data as *const WLAN_RADIO_STATE);
        let n = (rs.dwNumberOfPhys as usize).min(rs.PhyRadioState.len());
        n > 0
            && rs.PhyRadioState[..n].iter().all(|p| {
                p.dot11SoftwareRadioState == dot11_radio_state_off || p.dot11HardwareRadioState == dot11_radio_state_off
            })
    };
    unsafe { WlanFreeMemory(data) };
    off
}

/// 연결된 와이파이 이름. Err(true) 면 권한 부족으로 거부됨.
fn current_ssid(h: &WlanHandle, guid: &GUID) -> Result<Option<String>, bool> {
    let mut size = 0u32;
    let mut data: *mut c_void = std::ptr::null_mut();
    let rc = unsafe {
        WlanQueryInterface(h.0, guid, wlan_intf_opcode_current_connection, None, &mut size, &mut data, None)
    };
    if rc == ERROR_ACCESS_DENIED.0 {
        return Err(true);
    }
    if rc != ERROR_SUCCESS.0 || data.is_null() {
        return Err(false);
    }
    let ssid = unsafe {
        let attrs = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
        let s = &attrs.wlanAssociationAttributes.dot11Ssid;
        let len = (s.uSSIDLength as usize).min(s.ucSSID.len());
        parse::ssid_from_bytes(&s.ucSSID[..len])
    };
    unsafe { WlanFreeMemory(data) };
    Ok(ssid)
}

/// 와이파이 어댑터의 기본 게이트웨이와 그 하드웨어 주소 (공유기 주소).
/// 게이트웨이가 없으면 None, 게이트웨이는 있는데 주소를 못 읽으면 Some((열쇠, None)).
fn gateway_mac() -> Option<(String, Option<String>)> {
    unsafe {
        let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let mut size: u32 = 16 * 1024;
        let mut buf: Vec<u8>;
        loop {
            buf = vec![0u8; size as usize];
            let rc = GetAdaptersAddresses(
                AF_INET.0 as u32,
                flags,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            );
            if rc == ERROR_BUFFER_OVERFLOW.0 {
                continue;
            }
            if rc != NO_ERROR.0 {
                return None;
            }
            break;
        }
        let mut cur = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        let mut target: Option<([u8; 4], u64)> = None;
        while !cur.is_null() {
            let a = &*cur;
            if a.IfType == IF_TYPE_IEEE80211 && a.OperStatus == IfOperStatusUp {
                let mut gw = a.FirstGatewayAddress;
                while !gw.is_null() {
                    let sa = (*gw).Address.lpSockaddr;
                    if !sa.is_null() && (*sa).sa_family == AF_INET {
                        let sin = &*(sa as *const SOCKADDR_IN);
                        let ip = sin.sin_addr.S_un.S_un_b;
                        target = Some(([ip.s_b1, ip.s_b2, ip.s_b3, ip.s_b4], a.Luid.Value));
                        break;
                    }
                    gw = (*gw).Next;
                }
            }
            if target.is_some() {
                break;
            }
            cur = a.Next;
        }
        let (ip, luid) = target?;
        let key = format!("{luid}|{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);

        let mut table: *mut MIB_IPNET_TABLE2 = std::ptr::null_mut();
        if GetIpNetTable2(AF_INET, &mut table) != NO_ERROR || table.is_null() {
            return Some((key, None));
        }
        let mut found = None;
        let n = (*table).NumEntries as usize;
        let rows = (*table).Table.as_ptr();
        for i in 0..n {
            let row = &*rows.add(i);
            if row.InterfaceLuid.Value != luid {
                continue;
            }
            if row.Address.si_family != AF_INET {
                continue;
            }
            let b = row.Address.Ipv4.sin_addr.S_un.S_un_b;
            if [b.s_b1, b.s_b2, b.s_b3, b.s_b4] != ip {
                continue;
            }
            if row.State == NlnsUnreachable || row.State == NlnsIncomplete {
                continue;
            }
            let len = (row.PhysicalAddressLength as usize).min(row.PhysicalAddress.len());
            found = parse::mac_from_bytes(&row.PhysicalAddress[..len]);
            break;
        }
        FreeMibTable(table as *const c_void);
        Some((key, found))
    }
}

impl Platform for WindowsPlatform {
    fn os(&self) -> &'static str {
        "windows"
    }

    fn idle_seconds(&self) -> Option<f64> {
        let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        unsafe {
            if !GetLastInputInfo(&mut info).as_bool() {
                return None;
            }
            // 둘 다 32비트 밀리초 값이라 49.7일마다 한 바퀴 돈다. 뺄셈을 감싸서 계산한다.
            let now = GetTickCount();
            Some(now.wrapping_sub(info.dwTime) as f64 / 1000.0)
        }
    }

    fn screen_locked(&self) -> bool {
        unsafe {
            let mut buf = PWSTR::null();
            let mut bytes = 0u32;
            if WTSQuerySessionInformationW(
                Some(WTS_CURRENT_SERVER_HANDLE),
                WTS_CURRENT_SESSION,
                WTSSessionInfoEx,
                &mut buf,
                &mut bytes,
            )
            .is_err()
                || buf.is_null()
            {
                return false;
            }
            let info = &*(buf.0 as *const WTSINFOEXW);
            let l1 = &info.Data.WTSInfoExLevel1;
            let locked = l1.SessionFlags == WTS_SESSIONSTATE_LOCK;
            let active = l1.SessionState == WTSActive;
            WTSFreeMemory(buf.0 as *mut c_void);
            locked || !active
        }
    }

    fn display_sleep_preventers(&self) -> Vec<String> {
        let mut state: u32 = 0;
        let status = unsafe {
            CallNtPowerInformation(
                SystemExecutionState,
                None,
                0,
                Some(&mut state as *mut u32 as *mut c_void),
                std::mem::size_of::<u32>() as u32,
            )
        };
        if status.is_ok() && state & ES_DISPLAY_REQUIRED.0 != 0 {
            vec!["다른 앱".to_string()]
        } else {
            Vec::new()
        }
    }

    fn lock_screen(&self) -> Result<String, String> {
        unsafe { LockWorkStation() }
            .map(|_| "화면 잠금 (LockWorkStation)".to_string())
            .map_err(|e| format!("화면 잠금 실패: {e}"))
    }

    fn wifi_snapshot(&self) -> WiFiSnapshot {
        let Some(h) = WlanHandle::open() else {
            self.clear_router_cache();
            return WiFiSnapshot::status(WiFiStatus::Unavailable);
        };
        let ifaces = wlan_interfaces(&h);
        if ifaces.is_empty() {
            self.clear_router_cache();
            return WiFiSnapshot::status(WiFiStatus::Unavailable);
        }
        // 연결된 장치를 먼저 본다
        let connected = ifaces.iter().find(|i| i.isState == wlan_interface_state_connected);
        let Some(iface) = connected else {
            self.clear_router_cache();
            let first = &ifaces[0];
            let name = wide_to_string(&first.strInterfaceDescription);
            let status = if radio_off(&h, &first.InterfaceGuid) { WiFiStatus::PoweredOff } else { WiFiStatus::Disconnected };
            return WiFiSnapshot::new(status, None, None, Some(&name));
        };
        let name = wide_to_string(&iface.strInterfaceDescription);
        let ssid = match current_ssid(&h, &iface.InterfaceGuid) {
            Ok(s) => {
                self.ssid_denied.store(false, Ordering::Relaxed);
                s
            }
            Err(denied) => {
                if denied {
                    self.ssid_denied.store(true, Ordering::Relaxed);
                }
                None
            }
        };
        let router = self.router_mac(&format!("{name}|{}", ssid.as_deref().unwrap_or("")));
        WiFiSnapshot::new(WiFiStatus::Connected, ssid.as_deref(), router.as_deref(), Some(&name))
    }

    fn location_status(&self) -> LocationStatus {
        if self.ssid_denied.load(Ordering::Relaxed) {
            LocationStatus::Denied
        } else {
            LocationStatus::NotRequired
        }
    }

    fn request_location_permission(&self) {
        self.open_location_settings();
    }

    fn open_location_settings(&self) {
        let _ = Command::new("explorer.exe").arg("ms-settings:privacy-location").spawn();
    }

    fn system_auto_locks(&self) -> Vec<crate::SystemAutoLock> {
        crate::autolock::windows(&autolock_inputs())
    }

    fn open_system_settings(&self, target: crate::SettingsTarget) {
        match target {
            crate::SettingsTarget::WindowsScreenSaver => {
                let _ = Command::new("control.exe").arg("desk.cpl,,@screensaver").spawn();
            }
            crate::SettingsTarget::WindowsPower => {
                let _ = Command::new("explorer.exe").arg("ms-settings:powersleep").spawn();
            }
            _ => {}
        }
    }

    fn diagnostics(&self) -> Vec<(String, String)> {
        vec![
            ("자리비움 시간".into(), "GetLastInputInfo".into()),
            ("화면 잠그기".into(), "LockWorkStation".into()),
            ("잠김 여부".into(), "WTSQuerySessionInformationW(WTSSessionInfoEx)".into()),
            ("화면 유지 요청".into(), "CallNtPowerInformation(SystemExecutionState) — 앱 이름은 알 수 없음".into()),
            ("와이파이".into(), "WLAN API + GetIpNetTable2".into()),
        ]
    }
}

/// 자가 진단용: 이 스레드가 화면 유지를 요청(on)하거나 해제한다.
pub fn test_display_hold(on: bool) {
    unsafe {
        if on {
            SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED);
        } else {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

// ---------- 윈도우가 스스로 잠그는 설정 ----------

mod syslock {
    use std::ffi::c_void;

    use windows::core::{GUID, HSTRING};
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::System::Power::{
        CallNtPowerInformation, GetSystemPowerStatus, PowerGetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex,
        SystemPowerCapabilities, SYSTEM_POWER_CAPABILITIES, SYSTEM_POWER_STATUS,
    };
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };

    const SUB_VIDEO: GUID = GUID::from_u128(0x7516b95f_f776_4464_8c53_06167f40cc99);
    const VIDEOIDLE: GUID = GUID::from_u128(0x3c0bc021_c8a8_4e07_a973_6b14cbcb2b7e);
    const SUB_SLEEP: GUID = GUID::from_u128(0x238c9fa8_0aad_41ed_83f4_97be242c8f20);
    const STANDBYIDLE: GUID = GUID::from_u128(0x29f6c1db_86da_48c5_9fdb_f2b67b1f44da);
    const SUB_NONE: GUID = GUID::from_u128(0xfea3413e_7e05_4911_9a71_700331f1c294);
    const CONSOLELOCK: GUID = GUID::from_u128(0x0e796bdb_100d_47d6_a2d5_f7d2daa51f51);

    const DESKTOP: &str = "Control Panel\\Desktop";
    const DESKTOP_POLICY: &str = "Software\\Policies\\Microsoft\\Windows\\Control Panel\\Desktop";

    pub fn reg_string(root: HKEY, key: &str, name: &str) -> Option<String> {
        let mut buf = [0u16; 512];
        let mut size = std::mem::size_of_val(&buf) as u32;
        let r = unsafe {
            RegGetValueW(
                root,
                &HSTRING::from(key),
                &HSTRING::from(name),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut c_void),
                Some(&mut size),
            )
        };
        if r.is_err() {
            return None;
        }
        let len = (size as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]);
        Some(s.trim_end_matches('\0').trim().to_string())
    }

    pub fn reg_dword(root: HKEY, key: &str, name: &str) -> Option<u32> {
        let mut v: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let r = unsafe {
            RegGetValueW(
                root,
                &HSTRING::from(key),
                &HSTRING::from(name),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut v as *mut u32 as *mut c_void),
                Some(&mut size),
            )
        };
        r.is_ok().then_some(v)
    }

    /// 관리 정책 값이 있으면 그것을, 없으면 사용자 값을 쓴다.
    fn desktop_value(name: &str) -> Option<String> {
        reg_string(HKEY_CURRENT_USER, DESKTOP_POLICY, name).or_else(|| reg_string(HKEY_CURRENT_USER, DESKTOP, name))
    }

    /// 화면 보호기가 정해져 있고 켜져 있으면 (시작까지 초, 다시 시작할 때 로그온 화면 표시)
    pub fn screensaver() -> Option<(i64, bool)> {
        let exe = desktop_value("SCRNSAVE.EXE").unwrap_or_default();
        let active = desktop_value("ScreenSaveActive").as_deref() == Some("1");
        if exe.is_empty() || !active {
            return None;
        }
        let timeout = desktop_value("ScreenSaveTimeOut").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let secure = desktop_value("ScreenSaverIsSecure").as_deref() == Some("1");
        Some((timeout, secure))
    }

    pub fn inactivity_limit() -> Option<i64> {
        reg_dword(
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\System",
            "InactivityTimeoutSecs",
        )
        .map(i64::from)
    }

    /// 모던 스탠바이에서 화면이 꺼진 뒤 로그인을 요구하기까지 더 기다리는 초 (u32 최댓값 = 요구 안 함)
    pub fn delay_lock_interval() -> Option<u32> {
        reg_dword(HKEY_CURRENT_USER, DESKTOP, "DelayLockInterval")
    }

    pub fn on_battery() -> bool {
        let mut s = SYSTEM_POWER_STATUS::default();
        unsafe { GetSystemPowerStatus(&mut s) }.is_ok() && s.ACLineStatus == 0
    }

    pub fn modern_standby() -> bool {
        let mut caps = SYSTEM_POWER_CAPABILITIES::default();
        let status = unsafe {
            CallNtPowerInformation(
                SystemPowerCapabilities,
                None,
                0,
                Some(&mut caps as *mut _ as *mut c_void),
                std::mem::size_of::<SYSTEM_POWER_CAPABILITIES>() as u32,
            )
        };
        status.is_ok() && caps.AoAc
    }

    /// 지금 전원 계획의 값 (전원 연결 / 배터리)
    pub fn power_value(sub: &GUID, setting: &GUID, battery: bool) -> Option<u32> {
        unsafe {
            let mut scheme: *mut GUID = std::ptr::null_mut();
            if PowerGetActiveScheme(None, &mut scheme).is_err() || scheme.is_null() {
                return None;
            }
            let mut v: u32 = 0;
            // 두 함수의 반환 형식이 달라(u32 / WIN32_ERROR) 성공 여부만 맞춰 본다.
            let ok = if battery {
                PowerReadDCValueIndex(None, Some(scheme), Some(sub), Some(setting), &mut v) == 0
            } else {
                PowerReadACValueIndex(None, Some(scheme), Some(sub), Some(setting), &mut v).is_ok()
            };
            let _ = LocalFree(Some(HLOCAL(scheme as *mut c_void)));
            ok.then_some(v)
        }
    }

    pub fn video_idle(battery: bool) -> Option<u32> {
        power_value(&SUB_VIDEO, &VIDEOIDLE, battery)
    }

    pub fn standby_idle(battery: bool) -> Option<u32> {
        power_value(&SUB_SLEEP, &STANDBYIDLE, battery)
    }

    pub fn console_lock(battery: bool) -> Option<u32> {
        power_value(&SUB_NONE, &CONSOLELOCK, battery)
    }
}

/// 윈도우가 스스로 잠그는 데 관계된 값들을 읽는다.
fn autolock_inputs() -> crate::autolock::WindowsInputs {
    let battery = syslock::on_battery();
    let modern_standby = syslock::modern_standby();
    // 깨어날 때 로그인 요구 (전원 계획 값이 없으면 윈도우 기본값인 "요구함"으로 본다)
    let console_lock = syslock::console_lock(battery).unwrap_or(1) != 0;
    let wake_lock_delay = if !console_lock {
        None
    } else if modern_standby {
        match syslock::delay_lock_interval() {
            Some(u32::MAX) => None,
            Some(v) => Some(i64::from(v)),
            None => Some(0),
        }
    } else {
        Some(0)
    };
    crate::autolock::WindowsInputs {
        on_battery: battery,
        screensaver: syslock::screensaver(),
        inactivity_limit: syslock::inactivity_limit(),
        display_off: syslock::video_idle(battery).map(i64::from),
        sleep_after: syslock::standby_idle(battery).map(i64::from),
        wake_lock_delay,
        modern_standby,
    }
}
