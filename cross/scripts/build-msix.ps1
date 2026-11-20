param(
    [string]$Arch = "x64",
    [string]$BinaryPath = "",
    [string]$OutDir = "",
    [string]$IdentityName = "",
    [string]$Publisher = "",
    [string]$PublisherDisplayName = "",
    [string]$DisplayName = "",
    [string]$ShortName = "",
    [switch]$SelfSign
)

$ErrorActionPreference = "Stop"

$crossDir = Split-Path -Parent $PSScriptRoot
$tauriConfPath = Join-Path $crossDir "app/tauri.conf.json"
$identityPath = Join-Path $crossDir "msix/store-identity.json"
$templatePath = Join-Path $crossDir "msix/AppxManifest.template.xml"

if (-not $BinaryPath) {
    $BinaryPath = Join-Path $crossDir "target/release/wifilock.exe"
}
if (-not (Test-Path $BinaryPath)) {
    throw "실행 파일을 찾지 못했습니다: $BinaryPath"
}

if (-not $OutDir) {
    $OutDir = Join-Path $crossDir "target/release/bundle/msix"
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$tauriConf = Get-Content -Raw -Encoding UTF8 $tauriConfPath | ConvertFrom-Json
$rawVersion = [string]$tauriConf.version
if ($rawVersion -match '^(\d+)\.(\d+)\.(\d+)$') {
    $msixVersion = "$($matches[1]).$($matches[2]).$($matches[3]).0"
} elseif ($rawVersion -match '^(\d+)\.(\d+)\.(\d+)\.(\d+)$') {
    $msixVersion = $rawVersion
} else {
    throw "지원하지 않는 버전 형식입니다: $rawVersion"
}

$identity = Get-Content -Raw -Encoding UTF8 $identityPath | ConvertFrom-Json

function Resolve-Setting {
    param([string]$CliValue, [string]$EnvName, [string]$DefaultValue)
    if (-not [string]::IsNullOrWhiteSpace($CliValue)) { return $CliValue.Trim() }
    $envVal = [Environment]::GetEnvironmentVariable($EnvName)
    if (-not [string]::IsNullOrWhiteSpace($envVal)) { return $envVal.Trim() }
    return $DefaultValue.Trim()
}

$resolvedIdentityName = Resolve-Setting $IdentityName "MSIX_IDENTITY_NAME" $identity.identityName
$resolvedPublisher = Resolve-Setting $Publisher "MSIX_PUBLISHER" $identity.publisher
$resolvedPublisherDisplayName = Resolve-Setting $PublisherDisplayName "MSIX_PUBLISHER_DISPLAY_NAME" $identity.publisherDisplayName
$resolvedDisplayName = Resolve-Setting $DisplayName "MSIX_DISPLAY_NAME" $identity.displayName
$resolvedShortName = Resolve-Setting $ShortName "MSIX_SHORT_NAME" $identity.shortName

$placeholderPublisher = "CN=00000000-0000-0000-0000-000000000000"
if ($resolvedPublisher -eq $placeholderPublisher) {
    Write-Warning "테스트용 기본 Identity(CN=00000000-...)로 MSIX를 빌드합니다. Microsoft Store 업로드 전 Partner Center 제품 ID의 Identity/Publisher 값으로 설정하세요."
}

# Windows Kits 10 SDK에서 makeappx.exe, makepri.exe, signtool.exe 위치 탐색
$kitsBinRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
if (-not (Test-Path $kitsBinRoot)) {
    throw "Windows 10/11 SDK(bin) 경로를 찾지 못했습니다: $kitsBinRoot"
}

$sdkVersionDirs = Get-ChildItem -Path $kitsBinRoot -Directory | Where-Object {
    $_.Name -match '^\d+\.\d+\.\d+\.\d+$' -and
    (Test-Path (Join-Path $_.FullName "x64\makeappx.exe")) -and
    (Test-Path (Join-Path $_.FullName "x64\makepri.exe"))
} | Sort-Object { [version]$_.Name } -Descending

if (-not $sdkVersionDirs -or $sdkVersionDirs.Count -eq 0) {
    throw "Windows SDK에서 x64 makeappx.exe 및 makepri.exe를 찾지 못했습니다: $kitsBinRoot"
}

$sdkBinDir = Join-Path $sdkVersionDirs[0].FullName "x64"
$makeAppx = Join-Path $sdkBinDir "makeappx.exe"
$makePri = Join-Path $sdkBinDir "makepri.exe"
$signTool = Join-Path $sdkBinDir "signtool.exe"
Write-Host "Windows SDK 도구 사용: $sdkBinDir"

$stageDir = Join-Path $crossDir "target/msix-stage"
$workDir = Join-Path $crossDir "target/msix-work"
if (Test-Path $stageDir) { Remove-Item -Recurse -Force $stageDir }
if (Test-Path $workDir) { Remove-Item -Recurse -Force $workDir }
New-Item -ItemType Directory -Force -Path $stageDir | Out-Null
New-Item -ItemType Directory -Force -Path $workDir | Out-Null

# 실행 파일 복사
Copy-Item -Force $BinaryPath (Join-Path $stageDir "wifilock.exe")

# 아이콘 에셋 복사
$assetsDir = Join-Path $stageDir "Assets"
New-Item -ItemType Directory -Force -Path $assetsDir | Out-Null

$requiredIcons = @(
    "StoreLogo.png",
    "Square44x44Logo.png",
    "Square71x71Logo.png",
    "Square150x150Logo.png",
    "Square310x310Logo.png"
)
foreach ($icon in $requiredIcons) {
    $src = Join-Path $crossDir "app/icons/$icon"
    if (-not (Test-Path $src)) {
        throw "필수 아이콘을 찾지 못했습니다: $src"
    }
    Copy-Item -Force $src (Join-Path $assetsDir $icon)
}

$wideLogoSrc = Join-Path $crossDir "msix/Assets/Wide310x150Logo.png"
if (-not (Test-Path $wideLogoSrc)) {
    throw "와이드 로고를 찾지 못했습니다: $wideLogoSrc"
}
Copy-Item -Force $wideLogoSrc (Join-Path $assetsDir "Wide310x150Logo.png")

# 다국어 리소스(.resw) 복사
$stringsSrc = Join-Path $crossDir "msix/Strings"
$stringsStage = Join-Path $stageDir "Strings"
Copy-Item -Recurse -Force $stringsSrc $stringsStage

# AppxManifest.xml 생성 (XML 이스케이프 + BOM 없는 UTF-8)
$manifestTemplate = Get-Content -Raw -Encoding UTF8 $templatePath
$replacements = @{
    "{{IDENTITY_NAME}}"          = [System.Security.SecurityElement]::Escape($resolvedIdentityName)
    "{{PUBLISHER}}"              = [System.Security.SecurityElement]::Escape($resolvedPublisher)
    "{{PUBLISHER_DISPLAY_NAME}}" = [System.Security.SecurityElement]::Escape($resolvedPublisherDisplayName)
    "{{DISPLAY_NAME}}"           = [System.Security.SecurityElement]::Escape($resolvedDisplayName)
    "{{SHORT_NAME}}"             = [System.Security.SecurityElement]::Escape($resolvedShortName)
    "{{VERSION}}"                = [System.Security.SecurityElement]::Escape($msixVersion)
    "{{ARCH}}"                   = [System.Security.SecurityElement]::Escape($Arch)
}
$manifestContent = $manifestTemplate
foreach ($kv in $replacements.GetEnumerator()) {
    $manifestContent = $manifestContent.Replace($kv.Key, $kv.Value)
}
$manifestPath = Join-Path $stageDir "AppxManifest.xml"
[System.IO.File]::WriteAllText($manifestPath, $manifestContent, [System.Text.UTF8Encoding]::new($false))

# resources.pri 생성 (단일 PRI 파일에 en-US/ko-KR 모두 병합되도록 <packaging> 노드 제거)
$priConfig = Join-Path $workDir "priconfig.xml"
& $makePri createconfig /cf "$priConfig" /dq en-US /o
if ($LASTEXITCODE -ne 0) {
    throw "makepri createconfig 실패 (종료 코드 $LASTEXITCODE)"
}

[xml]$priXml = Get-Content -Raw -Encoding UTF8 $priConfig
$packagingNode = $priXml.SelectSingleNode("//packaging")
if ($packagingNode -and $packagingNode.ParentNode) {
    [void]$packagingNode.ParentNode.RemoveChild($packagingNode)
}
$priXml.Save($priConfig)

$priPath = Join-Path $stageDir "resources.pri"
& $makePri new /pr "$stageDir" /cf "$priConfig" /mn "$manifestPath" /of "$priPath" /o
if ($LASTEXITCODE -ne 0) {
    throw "makepri new 실패 (종료 코드 $LASTEXITCODE)"
}

# 컴파일된 resources.pri만 패키지에 포함하도록 원본 Strings 폴더 제거
Remove-Item -Recurse -Force $stringsStage

$outMsix = Join-Path $OutDir "WiFiLock_${rawVersion}_${Arch}.msix"
& $makeAppx pack /d "$stageDir" /p "$outMsix" /o
if ($LASTEXITCODE -ne 0) {
    throw "makeappx pack 실패 (종료 코드 $LASTEXITCODE)"
}

if ($SelfSign) {
    if (-not (Test-Path $signTool)) {
        throw "signtool.exe를 찾지 못했습니다: $signTool"
    }
    $cert = New-SelfSignedCertificate `
        -Type Custom `
        -Subject $resolvedPublisher `
        -KeyUsage DigitalSignature `
        -FriendlyName "WiFiLock Local Test Cert" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
    $pfxPath = Join-Path $workDir "wifilock-test.pfx"
    $pfxPwd = ConvertTo-SecureString -String "wifilock-test" -Force -AsPlainText
    Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $pfxPwd | Out-Null
    & $signTool sign /fd SHA256 /f "$pfxPath" /p "wifilock-test" "$outMsix"
    if ($LASTEXITCODE -ne 0) {
        throw "signtool sign 실패 (종료 코드 $LASTEXITCODE)"
    }
    Write-Host "로컬 테스트용 자체 서명 완료 (설치 전 인증서 신뢰 저장소 등록 필요): $($cert.Thumbprint)"
}

Write-Host "MSIX 패키지 생성 완료: $outMsix"
