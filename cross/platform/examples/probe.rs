//! 이 컴퓨터에서 시스템 기능을 한 번씩 읽어 출력한다 (화면을 잠그지는 않는다).
fn main() {
    let p = wifilock_platform::create();
    println!("운영체제: {}", p.os());
    println!("자리비움: {:?}", p.idle_seconds());
    println!("잠김: {}", p.screen_locked());
    println!("화면 유지 요청: {:?}", p.display_sleep_preventers());
    println!("와이파이: {:?}", p.wifi_snapshot());
    println!("위치 권한: {:?}", p.location_status());
    for (k, v) in p.diagnostics() {
        println!("  {k}: {v}");
    }
}
