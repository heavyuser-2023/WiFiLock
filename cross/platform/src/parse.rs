//! 명령 출력·시스템 파일 해석 (운영체제와 무관하게 시험할 수 있도록 따로 둔다)

use wifilock_core::mac_normalize;

/// 맥 `arp -n <ip>` 출력에서 하드웨어 주소를 꺼낸다.
/// 예: "? (192.168.0.1) at d8:7:b6:47:6b:8c on en0 ifscope [ethernet]"
pub fn arp_output_mac(output: &str) -> Option<String> {
    for line in output.lines() {
        if let Some(rest) = line.split(" at ").nth(1) {
            let token = rest.split_whitespace().next()?;
            if let Some(mac) = mac_normalize(token) {
                return Some(mac);
            }
        }
    }
    None
}

/// 리눅스 `/proc/net/route` 에서 해당 장치의 기본 게이트웨이 주소를 찾는다.
pub fn proc_route_gateway(content: &str, iface: &str) -> Option<[u8; 4]> {
    for line in content.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 4 || cols[0] != iface || cols[1] != "00000000" {
            continue;
        }
        let flags = u32::from_str_radix(cols[3], 16).ok()?;
        // RTF_UP(1) | RTF_GATEWAY(2)
        if flags & 0x3 != 0x3 {
            continue;
        }
        let gw = u32::from_str_radix(cols[2], 16).ok()?;
        if gw == 0 {
            continue;
        }
        // 리틀 엔디언으로 적혀 있다
        return Some(gw.to_le_bytes());
    }
    None
}

/// 리눅스 `/proc/net/arp` 에서 해당 주소·장치의 하드웨어 주소를 찾는다.
pub fn proc_arp_mac(content: &str, ip: [u8; 4], iface: &str) -> Option<String> {
    let want = format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
    for line in content.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 6 || cols[0] != want || cols[5] != iface {
            continue;
        }
        let flags = u32::from_str_radix(cols[2].trim_start_matches("0x"), 16).unwrap_or(0);
        // ATF_COM(2): 주소 확인이 끝난 항목
        if flags & 0x2 == 0 {
            continue;
        }
        if let Some(mac) = mac_normalize(cols[3]) {
            return Some(mac);
        }
    }
    None
}

/// 6바이트 하드웨어 주소를 문자열로
pub fn mac_from_bytes(b: &[u8]) -> Option<String> {
    if b.len() < 6 {
        return None;
    }
    mac_normalize(&format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", b[0], b[1], b[2], b[3], b[4], b[5]))
}

/// 와이파이 이름 바이트를 문자열로 (UTF-8 이 아니면 깨진 글자는 대체)
pub fn ssid_from_bytes(b: &[u8]) -> Option<String> {
    if b.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(b).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arp() {
        assert_eq!(
            arp_output_mac("? (192.168.0.1) at d8:7:b6:47:6b:8c on en0 ifscope [ethernet]").as_deref(),
            Some("d8:07:b6:47:6b:8c")
        );
        assert_eq!(arp_output_mac("? (10.0.0.1) at (incomplete) on en0 ifscope [ethernet]"), None);
        assert_eq!(arp_output_mac("192.168.0.1 (192.168.0.1) -- no entry"), None);
    }

    #[test]
    fn route_and_arp() {
        let route = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                     eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
                     wlan0\t0000A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0\n\
                     wlan0\t00000000\t0100A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n";
        assert_eq!(proc_route_gateway(route, "wlan0"), Some([192, 168, 0, 1]));
        assert_eq!(proc_route_gateway(route, "eth0"), Some([192, 168, 1, 1]));
        assert_eq!(proc_route_gateway(route, "wlp2s0"), None);

        let arp = "IP address       HW type     Flags       HW address            Mask     Device\n\
                   192.168.0.1      0x1         0x2         d8:07:b6:47:6b:8c     *        wlan0\n\
                   192.168.1.1      0x1         0x0         00:00:00:00:00:00     *        eth0\n";
        assert_eq!(proc_arp_mac(arp, [192, 168, 0, 1], "wlan0").as_deref(), Some("d8:07:b6:47:6b:8c"));
        assert_eq!(proc_arp_mac(arp, [192, 168, 1, 1], "eth0"), None, "확인되지 않은 항목은 무시");
        assert_eq!(proc_arp_mac(arp, [192, 168, 0, 1], "eth0"), None, "장치가 다르면 무시");
    }

    #[test]
    fn bytes() {
        assert_eq!(mac_from_bytes(&[0xd8, 7, 0xb6, 0x47, 0x6b, 0x8c]).as_deref(), Some("d8:07:b6:47:6b:8c"));
        assert_eq!(mac_from_bytes(&[0; 6]), None);
        assert_eq!(mac_from_bytes(&[1, 2]), None);
        assert_eq!(ssid_from_bytes(b"Home").as_deref(), Some("Home"));
        assert_eq!(ssid_from_bytes("우리집".as_bytes()).as_deref(), Some("우리집"));
        assert_eq!(ssid_from_bytes(b""), None);
    }
}
