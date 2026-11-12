#!/bin/bash
# 와이파이 잠금 앱 빌드 → 앱 번들 → 서명 → DMG·PKG 설치 파일 생성
#
# 환경 변수
#   VERSION        버전 (기본 1.0.0)
#   BUILD_NUMBER   빌드 번호 (기본: 빌드 날짜·시각, 예 20261112.0130)
#   SIGN_IDENTITY  서명 인증서 이름 또는 해시. 비우면 자동 선택, "-" 는 임시(ad-hoc) 서명
#   SKIP_TESTS=1   단위 테스트 건너뛰기
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

APP_NAME="WiFiLock"
VERSION="${VERSION:-1.0.0}"
BUILD_NUMBER="${BUILD_NUMBER:-$(date +%Y%m%d.%H%M)}"
BUILD="$ROOT/build"
DIST="$ROOT/dist"
APP="$BUILD/$APP_NAME.app"

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }
with_timeout() { local secs=$1; shift; perl -e 'alarm shift; exec @ARGV' "$secs" "$@"; }

mkdir -p "$BUILD" "$DIST"

if [[ "${SKIP_TESTS:-0}" != "1" ]]; then
    step "1/7 단위 테스트"
    if ! swift test >"$BUILD/test.log" 2>&1; then
        grep -E "error|failed" "$BUILD/test.log" | head -30
        echo "단위 테스트 실패 (전체 기록: $BUILD/test.log)"; exit 1
    fi
    grep -E "Executed [0-9]+ tests" "$BUILD/test.log" | tail -1
fi

step "2/7 릴리스 빌드 (애플 실리콘 + 인텔)"
if ! swift build -c release --arch arm64 --arch x86_64 >"$BUILD/build.log" 2>&1; then
    grep -E "error" "$BUILD/build.log" | head -30
    echo "빌드 실패 (전체 기록: $BUILD/build.log)"; exit 1
fi
grep -E "warning:" "$BUILD/build.log" | sort -u | head -20 || true
BIN_DIR=$(swift build -c release --arch arm64 --arch x86_64 --show-bin-path)
BIN="$BIN_DIR/$APP_NAME"
[[ -x "$BIN" ]] || { echo "실행 파일이 없습니다: $BIN"; exit 1; }
echo "아키텍처: $(lipo -archs "$BIN")"

step "3/7 앱 아이콘"
swift scripts/make_icon.swift "$BUILD/icon_1024.png"
ICONSET="$BUILD/AppIcon.iconset"
rm -rf "$ICONSET"; mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z $size $size "$BUILD/icon_1024.png" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$BUILD/icon_1024.png" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$BUILD/AppIcon.icns"

step "4/7 앱 번들 조립"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/$APP_NAME"
cp Resources/Info.plist "$APP/Contents/Info.plist"
plutil -replace CFBundleShortVersionString -string "$VERSION" "$APP/Contents/Info.plist"
plutil -replace CFBundleVersion -string "$BUILD_NUMBER" "$APP/Contents/Info.plist"
cp "$BUILD/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
cp -R Resources/ko.lproj "$APP/Contents/Resources/"
printf 'APPL????' > "$APP/Contents/PkgInfo"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

step "5/7 코드 서명"
if [[ -z "${SIGN_IDENTITY:-}" ]]; then
    # Developer ID → Apple Development → 임시 서명 순으로 고른다
    SIGN_IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null | awk -F'"' '/Developer ID Application/ {print $0}' | head -1 | awk '{print $2}')
    if [[ -z "$SIGN_IDENTITY" ]]; then
        SIGN_IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null | awk '/Apple Development/ {print $2; exit}')
    fi
    SIGN_IDENTITY="${SIGN_IDENTITY:--}"
fi
if [[ "$SIGN_IDENTITY" != "-" ]] && with_timeout 60 codesign --force --sign "$SIGN_IDENTITY" --timestamp=none "$APP" 2>"$BUILD/codesign.err"; then
    echo "서명: $(codesign -dvv "$APP" 2>&1 | awk -F= '/^Authority/ {print $2; exit}')"
else
    [[ "$SIGN_IDENTITY" != "-" ]] && { echo "인증서 서명 실패 → 임시 서명으로 대체"; cat "$BUILD/codesign.err" || true; }
    codesign --force --sign - "$APP"
    echo "서명: 임시(ad-hoc)"
fi
codesign --verify --strict --verbose=2 "$APP"

step "6/7 DMG 만들기"
DMG="$DIST/$APP_NAME-$VERSION.dmg"
STAGE="$BUILD/dmg"
rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/$APP_NAME.app"
ln -s /Applications "$STAGE/Applications"
cat > "$STAGE/설치 방법.txt" <<'TXT'
와이파이 잠금 설치 방법

1. WiFiLock 아이콘을 옆의 Applications(응용 프로그램) 폴더로 끌어다 놓습니다.
2. 응용 프로그램 폴더에서 '와이파이 잠금'을 실행합니다.
   - 다른 맥에서 "확인되지 않은 개발자" 또는 "열 수 없음" 경고가 나오면:
     macOS 15 이상: 경고 창을 닫고 시스템 설정 > 개인정보 보호 및 보안 맨 아래의
                    '그래도 열기'를 누른 뒤 암호를 입력하세요.
     macOS 13~14 : 앱을 Control-클릭(오른쪽 클릭) → '열기'를 누르세요.
3. 화면 오른쪽 위 메뉴 막대에 자물쇠가 달린 화면 아이콘이 나타납니다.
4. 위치 권한 요청이 나오면 '허용'을 누르세요. 와이파이 이름을 읽는 데만 씁니다.
5. 설정 창에서 집·사무실 네트워크를 등록하고 잠금 시간을 정하세요.
TXT
hdiutil create -volname "와이파이 잠금 $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$DMG" >/dev/null
if [[ "$SIGN_IDENTITY" != "-" ]]; then with_timeout 60 codesign --force --sign "$SIGN_IDENTITY" --timestamp=none "$DMG" 2>/dev/null || true; fi
echo "DMG: $DMG ($(du -h "$DMG" | cut -f1))"

step "7/7 PKG 설치 프로그램 만들기"
PKGROOT="$BUILD/pkgroot"
rm -rf "$PKGROOT"
mkdir -p "$PKGROOT/Applications"
ditto "$APP" "$PKGROOT/Applications/$APP_NAME.app"
pkgbuild --analyze --root "$PKGROOT" "$BUILD/component.plist" >/dev/null
# 같은 식별자의 앱이 다른 곳에 있어도 그쪽을 덮어쓰지 않고 반드시 /Applications 에 설치되게 한다
plutil -replace 0.BundleIsRelocatable -bool NO "$BUILD/component.plist"
# 같은 버전이나 이전 버전으로 다시 설치하는 것도 허용한다
plutil -replace 0.BundleIsVersionChecked -bool NO "$BUILD/component.plist"
pkgbuild --root "$PKGROOT" \
    --component-plist "$BUILD/component.plist" \
    --identifier com.wifilock.WiFiLock.pkg \
    --version "$VERSION" \
    --install-location / \
    --scripts packaging/pkg-scripts \
    "$BUILD/$APP_NAME-component.pkg" >/dev/null
sed "s/__VERSION__/$VERSION/" packaging/distribution.xml > "$BUILD/distribution.xml"
PKG="$DIST/$APP_NAME-$VERSION.pkg"
rm -f "$PKG"
productbuild --distribution "$BUILD/distribution.xml" \
    --resources packaging/pkg-resources \
    --package-path "$BUILD" \
    "$PKG" >/dev/null
echo "PKG: $PKG ($(du -h "$PKG" | cut -f1))"

step "완료"
ls -lh "$DIST"
