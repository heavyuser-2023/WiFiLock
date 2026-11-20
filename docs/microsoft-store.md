# Microsoft Store (MSIX) 배포 안내

WiFiLock을 비용 없이 코드 서명(Microsoft Store 자동 서명)하여 Windows SmartScreen 및 Windows 11 스마트 앱 컨트롤(SAC) 경고 없이 배포하기 위한 **MSIX 패키징 및 Partner Center 제출 절차**입니다.

---

## 1. 앱 이름 예약 및 제품 ID 확인 (Partner Center)

1. [Microsoft Partner Center](https://partner.microsoft.com/dashboard) (또는 [storedeveloper.microsoft.com](https://storedeveloper.microsoft.com/ko-KR/home))에 개발자 계정(`heavyuser@naver.com`)으로 로그인합니다.
2. **Windows 및 Xbox > 개요 > 새 앱 만들기**를 선택하고 앱 이름(예: `WiFiLock` 또는 가용한 이름)을 예약합니다.
3. 앱이 생성되면 왼쪽 메뉴의 **제품 관리 > 제품 ID**로 이동하여 아래 세 값을 확인합니다.
   - `Package/Identity/Name` (예: `12345heavyuser.WiFiLock`)
   - `Package/Identity/Publisher` (예: `CN=XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX`)
   - `Package/Properties/PublisherDisplayName` (개발자 계정 표시 이름)

---

## 2. 패키지 신원(Identity) 값 반영

아래 두 가지 방법 중 하나를 선택해 위 3개 값을 설정합니다.

### 방법 A: GitHub Actions Variables에 등록 (권장 — 저장소에 ID 노출 안 함)
GitHub 저장소 **Settings > Secrets and variables > Actions > Variables** 탭에서 다음 변수를 추가합니다.

| Variable 이름 | Partner Center 제품 ID 항목 |
| --- | --- |
| `MSIX_IDENTITY_NAME` | `Package/Identity/Name` |
| `MSIX_PUBLISHER` | `Package/Identity/Publisher` |
| `MSIX_PUBLISHER_DISPLAY_NAME` | `Package/Properties/PublisherDisplayName` |
| `MSIX_DISPLAY_NAME` *(선택)* | 예약한 앱 표시 이름 (기본값 `WiFiLock`) |
| `MSIX_SHORT_NAME` *(선택)* | 시작 메뉴 짧은 이름 (기본값 `WiFiLock`) |

### 방법 B: 저장소 설정 파일에 직접 반영
[`cross/msix/store-identity.json`](../cross/msix/store-identity.json) 파일의 각 필드를 위 값으로 수정해 커밋합니다. (단, 환경변수/Actions Variable이 설정되어 있으면 그 값이 우선 적용됩니다.)

---

## 3. `.msix` 패키지 빌드 및 다운로드

1. `rust-tauri` 브랜치에 푸시하거나 GitHub Actions에서 **러스트·타우리판 빌드 (맥·윈도우·리눅스)** 워크플로를 수동 실행(`workflow_dispatch`)합니다.
2. 윈도우(`windows-latest`) 잡이 끝나면 아티팩트 `wifilock-windows`를 내려받습니다.
3. 압축을 풀면 `WiFiLock_<버전>_x64.msix` 파일이 포함되어 있습니다.

> **서명 안내**: Microsoft Store에 올리는 `.msix`는 **직접 서명하지 않은 미서명 상태 그대로 업로드**해야 합니다. 스토어 심사 통과 후 Microsoft가 공식 Store 인증서로 자동 서명하여 배포합니다.

---

## 4. Partner Center 제출 양식 작성 팁

1. **패키지 업로드**:
   - 내려받은 `WiFiLock_<버전>_x64.msix`를 Partner Center의 **패키지** 단계에 업로드합니다.
   - `runFullTrust` 제한된 기능이 포함되어 있으므로 업로드 후 경고가 뜰 수 있습니다(오류가 아니므로 계속 진행 가능).
2. **제출 옵션 — `runFullTrust` 제한된 기능 소명**:
   - **제출 옵션** 또는 추가 정보 입력란에 아래와 같이 입력합니다.
   > **영문 예시:**  
   > WiFiLock is a Win32/Tauri desktop system-tray utility packaged as MSIX. It requires the `runFullTrust` capability to monitor user idle time (`GetLastInputInfo`), check display sleep assertions (`CallNtPowerInformation`), query the current Wi-Fi connection via the native Windows WLAN API (`WlanQueryInterface`), inspect OS power/screensaver lock policies, and lock the workstation (`LockWorkStation`) on behalf of the user.
3. **개인정보 처리방침 URL**:
   - 아래 공개 URL을 입력합니다 (제출 전 `main` 브랜치에도 반영되어 있어야 접근 가능합니다).
   - `https://github.com/heavyuser-2023/WiFiLock/blob/main/docs/privacy-policy.md`
4. **인증 참고 사항 (Certification Notes)**:
   - 앱 실행 시 작업 표시줄 트레이(알림 영역)에 아이콘으로 상주하며, 트레이 아이콘 클릭 또는 앱 재실행 시 설정 창이 열린다는 점을 간단히 적어 두면 심사가 수월합니다.
5. **로컬 PC에서 설치 테스트가 필요한 경우**:
   - 스토어 업로드 전 Windows PC에서 직접 설치해 보려면 PowerShell에서 `-SelfSign` 옵션으로 빌드할 수 있습니다.
     ```powershell
     cd cross
     npx tauri build --no-bundle
     ./scripts/build-msix.ps1 -SelfSign
     ```
   - 생성된 인증서를 현재 사용자 신뢰할 수 있는 루트 인증 기관(`Cert:\CurrentUser\Root`)에 일시 추가한 뒤 `.msix`를 더블 클릭하여 설치·실행(트레이, 위치 권한, `StartupTask` 토글)을 확인합니다.

---

## 5. MSIX 패키지 동작 특이사항

- **로그인 시 자동 실행**: 일반 설치(`.exe`/`.msi`)는 레지스트리(`tauri-plugin-autostart`)를 사용하지만, MSIX 패키지에서는 레지스트리가 가상화되므로 `AppxManifest.xml`에 선언된 `windows.startupTask`(`TaskId="WiFiLockStartup"`) 및 WinRT `Windows.ApplicationModel.StartupTask` API로 동작합니다.
- **와이파이 이름 읽기 (Windows 11 24H2 이상)**: 매니페스트에 `<DeviceCapability Name="location" />`과 `<DeviceCapability Name="wiFiControl" />`이 선언되어 있으며, 앱은 **설정 > 개인 정보 및 보안 > 위치**의 일반 앱 허용 흐름에 맞게 안내합니다.
