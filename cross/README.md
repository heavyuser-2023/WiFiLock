# 와이파이 잠금 2 (러스트·타우리판: 맥·윈도우·리눅스)

와이파이 연결 상태에 따라 **자동 화면 잠금 시간**을 바꿔 주는 앱의 여러 운영체제판입니다.
기존 맥 전용판(저장소 최상위의 스위프트 코드)과 같은 동작을 러스트와 타우리 2로 다시 만들었습니다.

- 등록한 네트워크(집·사무실)는 길게, 등록되지 않은 와이파이는 짧게, 와이파이가 없으면 더 짧게 잠급니다.
- 맥은 화면 오른쪽 위 메뉴 막대, 윈도우는 오른쪽 아래 알림 영역, 리눅스는 데스크톱의 상태 아이콘 영역에 자물쇠 달린 모니터 아이콘이 나타납니다.
- 메뉴: 현재 네트워크·잠금 시간·남은 시간, 지금 잠그기, 자동 잠금 사용, 일시 중지(30분~4시간·다시 시작할 때까지), 현재 네트워크 등록, 설정, 정보, 종료
- 잠기기 전 경고 창(기본 10초 전, 마우스나 키보드를 움직이면 취소), 동영상 재생 중 잠금 보류, 로그인할 때 자동 실행

## 설치 파일

| 운영체제 | 파일 | 비고 |
| --- | --- | --- |
| 맥 11 이상 (애플 실리콘·인텔) | `WiFiLock_2.0.0_universal.dmg` | 열어서 응용 프로그램 폴더로 끌어다 놓기 |
| 윈도우 10·11 (x64) | `WiFiLock_2.0.0_x64-setup.exe` | 현재 사용자에게 설치 (관리자 권한 필요 없음), 한국어 설치 화면 |
| 윈도우 10·11 (x64) | `WiFiLock_2.0.0_x64_en-US.msi` | 회사 배포 도구용 |
| 우분투·데비안 (x64) | `WiFiLock_2.0.0_amd64.deb` | `sudo apt install ./WiFiLock_2.0.0_amd64.deb` |
| 페도라 등 (x64) | `WiFiLock-2.0.0-1.x86_64.rpm` | `sudo dnf install ./WiFiLock-2.0.0-1.x86_64.rpm` |
| 그 밖의 리눅스 (x64) | `WiFiLock_2.0.0_amd64.AppImage` | 실행 권한을 주고 바로 실행 |

설치 파일은 애플 공증·윈도우 코드 서명을 받지 않았습니다.
- 맥: 처음 열 때 경고가 나오면 시스템 설정 > 개인정보 보호 및 보안 맨 아래의 **그래도 열기**를 누릅니다.
- 윈도우: "Windows의 PC 보호" 창이 나오면 **추가 정보** → **실행**을 누릅니다.

## 운영체제별 차이

| 기능 | 맥 | 윈도우 | 리눅스 (그놈·KDE + systemd) |
| --- | --- | --- | --- |
| 와이파이 이름 | 위치 권한 필요 | 윈도우 11 24H2부터 위치 권한 필요 | 네트워크매니저에서 바로 읽음 |
| 공유기 주소로 구분 | macOS 가 가리는 경우가 많음 | 기본 게이트웨이 주소로 구분 | 기본 게이트웨이 주소로 구분 |
| 자리비움 시간 | 항상 | 항상 | 그놈·KDE·X11 에서 읽음. 못 읽는 데스크톱이면 메뉴에 알리고 잠그지 않음 |
| 화면 잠그기 | 제어 센터의 화면 잠금과 같은 동작 | 윈도우 키+L 과 같은 동작 | logind → 화면 보호기 순서로 시도 |
| 동영상 재생 중 보류 | 어떤 앱인지 이름까지 표시 | "다른 앱"으로 표시 (이름은 알 수 없음) | 그놈·logind·KDE 가 알려 주는 요청 |
| 메뉴 아이콘 옆 남은 시간 | 지원 | 지원하지 않음 | 지원 (데스크톱에 따라 다름) |
| 아이콘 | 다크 모드에 맞춰 색이 바뀌는 그림 | 색 바탕 그림 | 색 바탕 그림. 그놈은 "AppIndicator" 확장이 있어야 보임 |

## 구조

```
core/        운영체제와 무관한 핵심 로직 (세 운영체제가 그대로 함께 씀) + 단위 테스트
  settings.rs    설정·네트워크 규칙 (기존 맥 앱과 같은 저장 형식)
  policy.rs      와이파이 상태 → 잠금 시간 결정
  stabilizer.rs  와이파이 순간 끊김 대비 (짧아지는 변화는 15초 지속될 때만 적용)
  scheduler.rs   자리비움 시간 → 경고·잠금 시점 (같은 자리비움 동안 한 번만 잠금)
  controller.rs  매 초 판단 흐름 (잠금 해제·잠자기에서 깨어남 감지, 일시 중지)
  store.rs       설정 파일 저장 (임시 파일에 쓴 뒤 바꿔치기)
  format.rs      한글 시간 표기와 상태 문구
platform/    운영체제별 시스템 기능 (Platform 하나로 감쌈)
  macos.rs     CoreGraphics·IOKit·CoreWLAN·login.framework
  windows.rs   GetLastInputInfo·WTS·WLAN API·IP 도우미·LockWorkStation
  linux.rs     D-Bus(logind·네트워크매니저·그놈·KDE)·X11
app/         타우리 앱 (메뉴 아이콘, 메뉴, 설정 창, 경고 창, 자가 진단)
ui/          설정 창·경고 창 화면 (HTML·CSS·자바스크립트, 세 운영체제 공통)
scripts/     아이콘 생성 (맥에서 실행)
docker/      리눅스 빌드·자가 진단용 도커 환경
```

## 빌드

필요한 것: 러스트(이 폴더의 `rust-toolchain.toml` 이 1.99.1 을 자동으로 설치), 노드 22

```bash
npm ci
```

```bash
cargo test --workspace
```

```bash
npx tauri build
```

- 맥 범용(애플 실리콘·인텔): `npx tauri build --target universal-apple-darwin --bundles app,dmg`
- 리눅스는 `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf rpm` 이 필요합니다. 도커로 빌드하려면 `docker/linux-build.Dockerfile` 을 쓰세요.
- 윈도우·리눅스 설치 파일은 깃허브 자동 빌드(`.github/workflows/cross-build.yml`)가 각 운영체제 서버에서 만들고, 자가 진단까지 돌린 뒤 결과물로 올립니다.

## 자가 진단

```bash
./target/release/wifilock --self-test ./selftest
```

실제 앱을 띄운 채 시스템 기능, 메뉴 아이콘과 메뉴, 메뉴 동작, 잠금 판단 시나리오(가짜 시간·와이파이), 경고 창, 설정 창, 설정 저장을 점검하고
`selftest/summary.txt`·`report.json` 을 남긴 뒤 끝납니다. 시험 모드로 돌기 때문에 화면을 실제로 잠그지 않습니다.

개발용 실행 인자:

| 인자 | 설명 |
| --- | --- |
| `--dry-run` | 실제로 잠그지 않고 기록만 남김 |
| `--self-test <폴더>` | 자가 진단 후 종료 (종료 코드 0 = 모두 통과) |
| `--settings-file <경로>` | 다른 설정 파일 사용 |
| `--log-file <경로>` | 기록 파일 위치 |
| `--override-timeout <초>` | 모든 네트워크의 잠금 시간을 강제로 지정 |
| `--open-settings` | 시작하자마자 설정 창 열기 |
| `--no-location-prompt` | 시작할 때 위치 권한을 묻지 않음 |
| `--preview-hud` | 경고 창 모양 확인 (판단을 멈추고 경고 창만 띄움) |

설정 파일 위치: 맥 `~/Library/Application Support/com.wifilock.app/settings.json`, 윈도우 `%APPDATA%\com.wifilock.app\settings.json`, 리눅스 `~/.config/com.wifilock.app/settings.json`
