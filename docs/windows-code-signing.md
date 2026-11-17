# Windows 무료 오픈소스 코드 서명 (SignPath Foundation) 설정 안내

이 저장소(`.github/workflows/cross-build.yml`, `.signpath/*.xml`)는 비영리 재단 **SignPath Foundation**의 무료 오픈소스 코드 서명(`signpath/github-action-submit-signing-request@v3`)을 사용해 Windows 실행 파일과 설치 파일을 자동 서명하도록 구성되어 있습니다.

## 1. 동작 구조

윈도우용 Tauri NSIS 설치 파일(`-setup.exe`)과 WiX 설치 파일(`.msi`)은 내부에 `wifilock.exe`를 포함하므로 2단계로 서명합니다.

1. `npx tauri build --no-bundle`로 `wifilock.exe`를 빌드하고 `--self-test`를 실행합니다.
2. **1차 서명** (`.signpath/artifact-configuration-binary.xml`, slug: `binary`):
   - `cross/target/release/wifilock.exe`를 업로드해 SignPath로 서명받은 뒤 원본 경로에 덮어씁니다.
3. `npx tauri bundle --bundles nsis,msi --no-binary-patching`으로 서명된 `wifilock.exe`의 서명을 유지한 채 `-setup.exe`와 `.msi`를 생성합니다.
4. **2차 서명** (`.signpath/artifact-configuration-installers.xml`, slug: `installers`):
   - 생성된 `WiFiLock_*_x64-setup.exe`(Authenticode 서명)와 `WiFiLock_*_x64_en-US.msi`(내부 `wifilock.exe` 서명 검증 후 MSI 컨테이너 서명)를 SignPath로 서명받아 최종 결과물로 내보냅니다.
5. PowerShell `Get-AuthenticodeSignature`로 서명 상태(`Valid`)를 검증합니다.

> 저장소에 `SIGNPATH_ORGANIZATION_ID` 변수가 아직 등록되지 않은 동안에는 서명 단계를 건너뛰고 미서명 빌드만 정상 수행합니다.

## 2. SignPath Foundation 신청 및 대시보드 설정

1. **오픈소스 프로그램 신청**:
   - [SignPath Foundation](https://signpath.org/)의 Open Source Community 프로그램에서 GitHub 저장소(`https://github.com/heavyuser-2023/WiFiLock`)로 무료 코드 서명을 신청합니다.
   - 필수 요건(MIT 라이선스, 비공개 코드 없음, 릴리스 배포 이력, `README.md`의 Code Signing Policy 및 Privacy Policy)은 이미 저장소에 포함되어 있습니다.
2. **GitHub App 연동 및 프로젝트 생성**:
   - SignPath 조직 승인 후 대시보드에서 **SignPath GitHub App**을 저장소에 설치하고 **GitHub Actions**를 신뢰할 수 있는 빌드 시스템으로 연결합니다.
   - 프로젝트(권장 slug: `wifilock`)를 생성합니다.
3. **Artifact Configurations 등록**:
   - Slug `binary`: 저장소의 [`.signpath/artifact-configuration-binary.xml`](../.signpath/artifact-configuration-binary.xml) 내용을 등록합니다.
   - Slug `installers`: 저장소의 [`.signpath/artifact-configuration-installers.xml`](../.signpath/artifact-configuration-installers.xml) 내용을 등록합니다.
4. **Signing Policy 생성**:
   - Slug `release-signing`으로 서명 정책을 만들고, 신뢰할 수 있는 빌드 원본(`.github/workflows/cross-build.yml`, 브랜치 `rust-tauri` / `main`)과 승인자를 지정합니다.

## 3. GitHub 저장소 Variables 및 Secrets 설정

GitHub 저장소 **Settings > Secrets and variables > Actions**에서 아래 항목을 등록하면 다음 Windows 빌드부터 자동으로 서명됩니다.

| 구분 | 이름 | 값 |
| --- | --- | --- |
| **Secret** | `SIGNPATH_API_TOKEN` | SignPath 사용자 프로필에서 발급한 REST API 토큰 |
| **Variable** | `SIGNPATH_ORGANIZATION_ID` | SignPath 대시보드의 Organization ID (UUID) |
| **Variable** *(선택)* | `SIGNPATH_PROJECT_SLUG` | 기본값 `wifilock` |
| **Variable** *(선택)* | `SIGNPATH_POLICY_SLUG` | 기본값 `release-signing` |
| **Variable** *(선택)* | `SIGNPATH_BINARY_CONFIG_SLUG` | 기본값 `binary` |
| **Variable** *(선택)* | `SIGNPATH_INSTALLERS_CONFIG_SLUG` | 기본값 `installers` |
