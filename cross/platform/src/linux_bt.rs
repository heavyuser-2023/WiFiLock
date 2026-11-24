//! 리눅스 블루투스 구현: BlueZ D-Bus (`org.bluez`)

use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use wifilock_core::{
    infer_device_kind, mac_normalize, parse_mibeacon_mac, BluetoothObservation,
};
use zbus::blocking::{connection, proxy::Builder, Connection, Proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::bt_common::merge_observations;
use crate::{BluetoothScanMode, BluetoothStatus};

const BLUEZ: &str = "org.bluez";
const DBUS_TIMEOUT: Duration = Duration::from_millis(1500);
const STATUS_CACHE_SECS: u64 = 5;

type ManagedObjects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

struct ScanControl {
    mode: BluetoothScanMode,
    wake_flag: bool,
}

pub struct LinuxBluetooth {
    ctrl: Arc<(Mutex<ScanControl>, Condvar)>,
    status_cache: Mutex<Option<(Instant, BluetoothStatus)>>,
}

impl LinuxBluetooth {
    pub fn new() -> Self {
        let ctrl = Arc::new((
            Mutex::new(ScanControl {
                mode: BluetoothScanMode::Off,
                wake_flag: false,
            }),
            Condvar::new(),
        ));
        let ctrl_w = ctrl.clone();
        let _ = std::thread::Builder::new()
            .name("wifilock-linux-bt".into())
            .spawn(move || worker_loop(ctrl_w));
        LinuxBluetooth {
            ctrl,
            status_cache: Mutex::new(None),
        }
    }

    pub fn status(&self, system_conn: Option<&Connection>) -> BluetoothStatus {
        {
            let g = self.status_cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((at, st)) = *g {
                if at.elapsed() < Duration::from_secs(STATUS_CACHE_SECS) {
                    return st;
                }
            }
        }
        let st = match system_conn.and_then(fetch_managed_objects) {
            Some(objs) => adapter_from_objects(&objs).map_or(BluetoothStatus::Unavailable, |(_, powered)| {
                if powered {
                    BluetoothStatus::Ready
                } else {
                    BluetoothStatus::PoweredOff
                }
            }),
            None => BluetoothStatus::Unavailable,
        };
        *self.status_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), st));
        st
    }

    pub fn set_mode(&self, mode: BluetoothScanMode) {
        let (m, cv) = &*self.ctrl;
        let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
        if g.mode != mode {
            g.mode = mode;
            g.wake_flag = true;
            cv.notify_all();
        }
    }

    pub fn open_settings(&self) {
        for (cmd, args) in [
            ("gnome-control-center", &["bluetooth"][..]),
            ("systemsettings", &["kcm_bluetooth"][..]),
            ("systemsettings5", &["kcm_bluetooth"][..]),
            ("blueman-manager", &[][..]),
        ] {
            if Command::new(cmd).args(args).spawn().is_ok() {
                return;
            }
        }
    }

    pub fn observations(&self, system_conn: Option<&Connection>) -> Vec<BluetoothObservation> {
        let Some(conn) = system_conn else {
            return Vec::new();
        };
        let Some(objs) = fetch_managed_objects(conn) else {
            return Vec::new();
        };
        let now = crate::linux::clocks().total;
        let mut paired = Vec::new();
        let mut discovered = Vec::new();

        for (path, ifaces) in &objs {
            let Some(props) = ifaces.get("org.bluez.Device1") else {
                continue;
            };
            let Some(obs) = parse_device1(path.as_str(), props, now) else {
                continue;
            };
            if obs.paired {
                paired.push(obs);
            } else {
                discovered.push(obs);
            }
        }
        merge_observations(discovered, paired)
    }
}

fn bluez_proxy<'a>(conn: &'a Connection, path: &'a str, iface: &'a str) -> zbus::Result<Proxy<'a>> {
    Builder::new(conn)
        .destination(BLUEZ)?
        .path(path)?
        .interface(iface)?
        .cache_properties(CacheProperties::No)
        .build()
}

fn fetch_managed_objects(conn: &Connection) -> Option<ManagedObjects> {
    let p = bluez_proxy(conn, "/", "org.freedesktop.DBus.ObjectManager").ok()?;
    p.call("GetManagedObjects", &()).ok()
}

fn prop_bool(props: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    props.get(key)?.downcast_ref::<bool>().ok()
}

fn prop_i16(props: &HashMap<String, OwnedValue>, key: &str) -> Option<i16> {
    props.get(key)?.downcast_ref::<i16>().ok()
}

fn prop_u32(props: &HashMap<String, OwnedValue>, key: &str) -> Option<u32> {
    props.get(key)?.downcast_ref::<u32>().ok()
}

fn prop_string(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props
        .get(key)
        .and_then(|v| v.downcast_ref::<&str>().ok())
        .map(str::to_owned)
        .filter(|s| !s.trim().is_empty())
}

fn adapter_from_objects(objs: &ManagedObjects) -> Option<(String, bool)> {
    let mut list: Vec<(&str, bool)> = objs
        .iter()
        .filter_map(|(path, ifaces)| {
            let props = ifaces.get("org.bluez.Adapter1")?;
            let powered = prop_bool(props, "Powered").unwrap_or(false);
            Some((path.as_str(), powered))
        })
        .collect();
    list.sort_by(|a, b| a.0.cmp(b.0));
    list.into_iter().next().map(|(p, pw)| (p.to_owned(), pw))
}

fn parse_device1(_path: &str, props: &HashMap<String, OwnedValue>, now: f64) -> Option<BluetoothObservation> {
    let raw_addr = prop_string(props, "Address")?;
    let addr = mac_normalize(&raw_addr)?;
    let addr_type = prop_string(props, "AddressType").unwrap_or_default().to_ascii_lowercase();
    let paired = prop_bool(props, "Paired").unwrap_or(false);
    let connected = prop_bool(props, "Connected").unwrap_or(false);
    let rssi = prop_i16(props, "RSSI").and_then(BluetoothObservation::normalize_rssi);
    let class = prop_u32(props, "Class");
    let major = class.map(|c| (c >> 8) & 0x1f);
    let minor = class.map(|c| (c >> 2) & 0x3f);
    let icon = prop_string(props, "Icon");

    let name = prop_string(props, "Name").or_else(|| {
        let alias = prop_string(props, "Alias")?;
        // BlueZ는 이름이 없을 때 Alias에 "AA-BB-CC-DD-EE-FF"를 채우므로 주소와 같으면 무시
        if mac_normalize(&alias).as_deref() == Some(addr.as_str()) {
            None
        } else {
            Some(alias)
        }
    });

    let mut is_mibeacon = false;
    let mut mibeacon_mac = None;
    if let Some(sd_val) = props.get("ServiceData") {
        if let Ok(sd_map) = HashMap::<String, OwnedValue>::try_from(sd_val.clone()) {
            for (uuid, val) in sd_map {
                if uuid.to_ascii_lowercase().starts_with("0000fe95") {
                    is_mibeacon = true;
                    if let Ok(bytes) = Vec::<u8>::try_from(val) {
                        mibeacon_mac = parse_mibeacon_mac(&bytes);
                    }
                }
            }
        }
    }

    let mac_address = mibeacon_mac.or_else(|| {
        if addr_type == "public" {
            Some(addr.clone())
        } else {
            None
        }
    });
    let classic_address = if paired { Some(addr.clone()) } else { None };
    let kind = infer_device_kind(name.as_deref(), major, minor, is_mibeacon, icon.as_deref());

    Some(BluetoothObservation {
        key: format!("bluez:{addr}"),
        name,
        kind,
        ble_id: Some(format!("bluez:{addr}")),
        mac_address,
        classic_address,
        rssi,
        connected,
        paired,
        seen_at: if connected || rssi.is_some() { now } else { 0.0 },
    })
}

fn start_discovery(conn: &Connection, adapter_path: &str) -> bool {
    let Ok(proxy) = bluez_proxy(conn, adapter_path, "org.bluez.Adapter1") else {
        return false;
    };
    let mut filter: HashMap<&str, Value<'_>> = HashMap::new();
    filter.insert("DuplicateData", Value::from(true));
    let _ = proxy.call::<_, _, ()>("SetDiscoveryFilter", &(filter,));
    proxy.call::<_, _, ()>("StartDiscovery", &()).is_ok()
}

fn stop_discovery(conn: &Connection, adapter_path: &str) {
    if let Ok(proxy) = bluez_proxy(conn, adapter_path, "org.bluez.Adapter1") {
        let _ = proxy.call::<_, _, ()>("StopDiscovery", &());
    }
}

fn wait_for_change_or_timeout(
    ctrl: &(Mutex<ScanControl>, Condvar),
    timeout: Option<Duration>,
) -> BluetoothScanMode {
    let (m, cv) = ctrl;
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    if !g.wake_flag {
        match timeout {
            None => {
                while !g.wake_flag {
                    g = cv.wait(g).unwrap_or_else(|e| e.into_inner());
                }
            }
            Some(d) => {
                let deadline = Instant::now() + d;
                while !g.wake_flag {
                    let rem = deadline.saturating_duration_since(Instant::now());
                    if rem.is_zero() {
                        break;
                    }
                    let (ng, res) = cv.wait_timeout(g, rem).unwrap_or_else(|e| e.into_inner());
                    g = ng;
                    if res.timed_out() {
                        break;
                    }
                }
            }
        }
    }
    g.wake_flag = false;
    g.mode
}

fn worker_loop(ctrl: Arc<(Mutex<ScanControl>, Condvar)>) {
    let mut conn: Option<Connection> = None;
    let mut started_adapter: Option<String> = None;

    loop {
        let mode = ctrl.0.lock().unwrap_or_else(|e| e.into_inner()).mode;
        if mode == BluetoothScanMode::Off {
            if let (Some(c), Some(path)) = (conn.as_ref(), started_adapter.take()) {
                stop_discovery(c, &path);
            }
            let _ = wait_for_change_or_timeout(&ctrl, None);
            continue;
        }

        if conn.is_none() {
            conn = connection::Builder::system()
                .ok()
                .and_then(|b| b.method_timeout(DBUS_TIMEOUT).build().ok());
        }
        let Some(c) = conn.as_ref() else {
            let _ = wait_for_change_or_timeout(&ctrl, Some(Duration::from_secs(10)));
            continue;
        };

        let adapter = fetch_managed_objects(c)
            .and_then(|objs| adapter_from_objects(&objs))
            .filter(|(_, powered)| *powered)
            .map(|(path, _)| path);
        let Some(adapter_path) = adapter else {
            started_adapter = None;
            let _ = wait_for_change_or_timeout(&ctrl, Some(Duration::from_secs(8)));
            continue;
        };

        match mode {
            BluetoothScanMode::Off => {}
            BluetoothScanMode::Radar => {
                if started_adapter.as_deref() != Some(adapter_path.as_str())
                    && start_discovery(c, &adapter_path)
                {
                    started_adapter = Some(adapter_path);
                }
                let _ = wait_for_change_or_timeout(&ctrl, Some(Duration::from_secs(15)));
            }
            BluetoothScanMode::Background => {
                if start_discovery(c, &adapter_path) {
                    started_adapter = Some(adapter_path.clone());
                }
                let next_mode = wait_for_change_or_timeout(&ctrl, Some(Duration::from_secs(3)));
                if next_mode != BluetoothScanMode::Background {
                    continue;
                }
                if let Some(path) = started_adapter.take() {
                    stop_discovery(c, &path);
                }
                let _ = wait_for_change_or_timeout(&ctrl, Some(Duration::from_secs(4)));
            }
        }
    }
}
