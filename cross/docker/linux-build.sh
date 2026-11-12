#!/usr/bin/env bash
# 도커 안에서: 테스트 → 리눅스 설치 파일 빌드 → 가상 화면에서 자가 진단
set -euo pipefail
cd /src/cross
export CARGO_TARGET_DIR=/build/target
cargo test -q -p wifilock-core -p wifilock-platform 2>&1 | grep -E "test result|FAILED|panicked" || true
npm ci --no-audit --no-fund
npx tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}' -- --target-dir /build/target 2>&1 | tail -25
mkdir -p /out/bundle /out/selftest
cp -r /build/target/release/bundle/deb /build/target/release/bundle/rpm /build/target/release/bundle/appimage /out/bundle/ 2>/dev/null || true
# 자가 진단: 가상 화면(Xvfb) + 세션 버스
set +e
dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" timeout 240 /build/target/release/wifilock --self-test /out/selftest > /out/selftest/stdout.txt 2>&1
echo "자가 진단 종료 코드: $?" | tee -a /out/selftest/stdout.txt
