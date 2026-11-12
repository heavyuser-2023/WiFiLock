#!/bin/bash
# 빌드 결과 검증: 앱 번들 구조 → 서명 → 앱 자가 진단(실제로 띄워 기능 점검) → DMG → PKG
# 화면을 실제로 잠그지는 않는다.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
APP_NAME="WiFiLock"
VERSION="${VERSION:-1.0.0}"
APP="$ROOT/build/$APP_NAME.app"
DMG="$ROOT/dist/$APP_NAME-$VERSION.dmg"
PKG="$ROOT/dist/$APP_NAME-$VERSION.pkg"
OUT="$ROOT/build/verify"
PASS=0
FAIL=0

ok()   { PASS=$((PASS + 1)); printf '  \033[32m[통과]\033[0m %s\n' "$*"; }
bad()  { FAIL=$((FAIL + 1)); printf '  \033[31m[실패]\033[0m %s\n' "$*"; }
chk()  { local name=$1; shift; if "$@" >/dev/null 2>&1; then ok "$name"; else bad "$name"; fi; }
step() { printf '\n\033[1;34m== %s\033[0m\n' "$*"; }
with_timeout() { local secs=$1; shift; perl -e 'alarm shift; exec @ARGV' "$secs" "$@"; }

rm -rf "$OUT"; mkdir -p "$OUT"

step "1. 앱 번들 구조"
chk "앱 번들 존재" test -d "$APP"
chk "실행 파일" test -x "$APP/Contents/MacOS/$APP_NAME"
chk "Info.plist 형식" plutil -lint "$APP/Contents/Info.plist"
chk "앱 아이콘(icns)" test -s "$APP/Contents/Resources/AppIcon.icns"
chk "한글 이름 파일" test -s "$APP/Contents/Resources/ko.lproj/InfoPlist.strings"
ARCHS=$(lipo -archs "$APP/Contents/MacOS/$APP_NAME")
[[ "$ARCHS" == *arm64* && "$ARCHS" == *x86_64* ]] && ok "범용 실행 파일 ($ARCHS)" || bad "범용 실행 파일 ($ARCHS)"
[[ $(plutil -extract LSUIElement raw "$APP/Contents/Info.plist") == "true" ]] && ok "메뉴 막대 전용(LSUIElement)" || bad "LSUIElement"
[[ $(plutil -extract CFBundleShortVersionString raw "$APP/Contents/Info.plist") == "$VERSION" ]] && ok "버전 $VERSION" || bad "버전"
MINOS=$(otool -l "$APP/Contents/MacOS/$APP_NAME" | awk '/LC_BUILD_VERSION/{f=1} f&&/minos/{print $2; exit}')
[[ "$MINOS" == "13.0" ]] && ok "최소 macOS 버전 $MINOS" || bad "최소 macOS 버전 ($MINOS)"

step "2. 코드 서명"
chk "서명 유효성 (strict)" codesign --verify --strict --deep "$APP"
echo "  서명 정보: $(codesign -dvv "$APP" 2>&1 | grep -E '^Authority' | head -1)"
echo "  게이트키퍼 평가: $(spctl --assess --type execute "$APP" 2>&1 | head -1)"

step "3. 앱 자가 진단 (실제 실행, 시험용 잠금 장치)"
LOG="$OUT/selftest-log.txt"
with_timeout 90 "$APP/Contents/MacOS/$APP_NAME" --self-test "$OUT/selftest" --log-file "$LOG" --no-location-prompt
RC=$?
if [[ -f "$OUT/selftest/summary.txt" ]]; then
    sed 's/^/  /' "$OUT/selftest/summary.txt" | grep -E "^\s+\[(통과|실패)\]|자가 진단:" | sed -n '1,200p'
fi
[[ $RC -eq 0 ]] && ok "자가 진단 종료 코드 0 (메뉴의 '종료'로 정상 종료)" || bad "자가 진단 종료 코드 $RC"
grep -q '"failed" : 0' "$OUT/selftest/report.json" 2>/dev/null && ok "자가 진단 실패 항목 없음" || bad "자가 진단에 실패 항목 있음"

step "4. DMG"
chk "DMG 파일" test -s "$DMG"
chk "DMG 무결성 (hdiutil verify)" hdiutil verify "$DMG"
MNT=$(mktemp -d "$OUT/mnt.XXXX")
if hdiutil attach "$DMG" -nobrowse -readonly -mountpoint "$MNT" >/dev/null 2>&1; then
    chk "DMG 안 앱" test -x "$MNT/$APP_NAME.app/Contents/MacOS/$APP_NAME"
    chk "DMG 안 응용 프로그램 폴더 바로가기" test -L "$MNT/Applications"
    chk "DMG 안 설치 방법 안내" test -s "$MNT/설치 방법.txt"
    chk "DMG 안 앱 서명" codesign --verify --strict "$MNT/$APP_NAME.app"
    if cmp -s "$MNT/$APP_NAME.app/Contents/MacOS/$APP_NAME" "$APP/Contents/MacOS/$APP_NAME"; then ok "DMG 안 앱이 최신 빌드와 같음"; else bad "DMG 안 앱이 최신 빌드와 다름"; fi
    hdiutil detach "$MNT" -quiet || hdiutil detach "$MNT" -force -quiet
else
    bad "DMG 연결"
fi
rmdir "$MNT" 2>/dev/null || true

step "5. PKG"
chk "PKG 파일" test -s "$PKG"
FILES=$(pkgutil --payload-files "$PKG" 2>/dev/null)
echo "$FILES" | grep -q "^./Applications/$APP_NAME.app/Contents/MacOS/$APP_NAME$" && ok "설치 위치 /Applications/$APP_NAME.app" || bad "설치 내용에 앱 실행 파일 없음"
EXP="$OUT/pkg-expanded"
rm -rf "$EXP"
if pkgutil --expand "$PKG" "$EXP" >/dev/null 2>&1; then
    chk "설치 전 스크립트" test -x "$EXP/$APP_NAME-component.pkg/Scripts/preinstall"
    chk "설치 후 스크립트" test -x "$EXP/$APP_NAME-component.pkg/Scripts/postinstall"
    chk "설치 스크립트 문법" sh -n "$EXP/$APP_NAME-component.pkg/Scripts/postinstall"
    if grep -q "<relocate>" "$EXP/$APP_NAME-component.pkg/PackageInfo"; then bad "설치 위치 고정 (다른 위치로 옮겨 설치될 수 있음)"; else ok "설치 위치 고정 (다른 곳의 같은 앱을 덮어쓰지 않음)"; fi
    chk "설치 화면 제목" grep -q "<title>와이파이 잠금</title>" "$EXP/Distribution"
    chk "설치 안내 화면" test -s "$EXP/Resources/welcome.html"
else
    bad "PKG 펼치기"
fi
FULL="$OUT/pkg-full"
rm -rf "$FULL"
if pkgutil --expand-full "$PKG" "$FULL" >/dev/null 2>&1; then
    PAYAPP="$FULL/$APP_NAME-component.pkg/Payload/Applications/$APP_NAME.app"
    chk "PKG 안 앱 서명" codesign --verify --strict "$PAYAPP"
    if cmp -s "$PAYAPP/Contents/MacOS/$APP_NAME" "$APP/Contents/MacOS/$APP_NAME"; then ok "PKG 안 앱이 최신 빌드와 같음"; else bad "PKG 안 앱이 최신 빌드와 다름"; fi
else
    bad "PKG 전체 펼치기"
fi
echo "  설치 프로그램 서명: $(pkgutil --check-signature "$PKG" 2>&1 | sed -n '2p' | sed 's/^ *//')"

step "결과"
echo "  통과 $PASS / 실패 $FAIL"
echo "  자가 진단 그림·보고서: $OUT/selftest"
[[ $FAIL -eq 0 ]]
