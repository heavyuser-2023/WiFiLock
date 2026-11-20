# 개인정보 처리방침 (Privacy Policy)

최종 수정일: 2026-11-20

## 한국어

**와이파이 잠금(WiFiLock)** 은 사용자의 개인정보를 수집하지 않습니다.

1. **수집·저장·전송하는 개인정보 없음**
   - 이 프로그램은 이름, 이메일, 기기 식별자, 위치 좌표, 와이파이 연결 기록, 사용 통계(텔레메트리), 오류 보고서 등 어떠한 정보도 수집하거나 외부 서버로 전송하지 않습니다.
2. **위치 권한 및 와이파이(`wiFiControl`) 권한의 사용 목적**
   - 운영체제(Windows 11 24H2 이상, macOS)가 현재 연결된 와이파이 이름(SSID)을 읽기 위해 위치 권한을 요구합니다.
   - 와이파이 이름 및 공유기 주소는 오직 사용자가 직접 등록한 로컬 규칙(`settings.json`)과 비교하여 화면 자동 잠금 시간을 정하는 데만 실시간으로 사용되며, 디스크에 별도 이력으로 기록되거나 네트워크 밖으로 전송되지 않습니다.
3. **로컬 설정 저장**
   - 사용자가 직접 추가한 네트워크 규칙과 대기 시간 설정만 사용자 컴퓨터의 로컬 설정 폴더(`settings.json`)에 저장됩니다.
4. **문의**
   - 저장소: https://github.com/heavyuser-2023/WiFiLock

---

## English

**WiFiLock** does not collect any personal information.

1. **No Data Collection, Storage, or Transmission**
   - WiFiLock does not collect, store on external servers, or transmit any personal data, location coordinates, Wi-Fi connection history, usage telemetry, or crash reports.
2. **Use of Location and Wi-Fi (`wiFiControl`) Permissions**
   - Operating systems (such as Windows 11 24H2+ and macOS) require Location permission to read the currently connected Wi-Fi network name (SSID).
   - The Wi-Fi SSID and router MAC address are used solely in memory to match against local auto-lock rules configured by the user (`settings.json`). They are never logged as location history or sent off the device.
3. **Local Settings Only**
   - User-configured network rules and timeout preferences are stored exclusively in the local application configuration file (`settings.json`) on the user's device.
4. **Contact**
   - Repository: https://github.com/heavyuser-2023/WiFiLock
