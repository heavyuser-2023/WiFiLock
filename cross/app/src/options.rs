//! 실행 인자

use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// 실제로 잠그지 않고 기록만 남긴다
    pub dry_run: bool,
    /// 자가 진단 결과를 이 폴더에 남기고 끝낸다
    pub self_test_dir: Option<PathBuf>,
    /// 설정 파일 경로 (기본: 앱 설정 폴더/settings.json)
    pub settings_file: Option<PathBuf>,
    /// 기록 파일 경로 (기본: 앱 기록 폴더/wifilock.log)
    pub log_file: Option<PathBuf>,
    /// 모든 네트워크의 잠금 시간을 강제로 지정 (초)
    pub override_timeout: Option<i64>,
    /// 시작하자마자 설정 창 열기
    pub open_settings: bool,
    /// 시작할 때 위치 권한을 묻지 않음
    pub no_location_prompt: bool,
    /// 경고 창 모양 확인용: 시작하자마자 경고 창을 띄운다 (개발용)
    pub preview_hud: bool,
}

impl Options {
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Self {
        let mut o = Options::default();
        let mut it = args.into_iter().skip(1);
        while let Some(a) = it.next() {
            match a.as_str() {
                "--dry-run" => o.dry_run = true,
                "--self-test" => o.self_test_dir = it.next().map(PathBuf::from),
                "--settings-file" => o.settings_file = it.next().map(PathBuf::from),
                "--log-file" => o.log_file = it.next().map(PathBuf::from),
                "--override-timeout" => o.override_timeout = it.next().and_then(|v| v.parse().ok()),
                "--open-settings" => o.open_settings = true,
                "--no-location-prompt" => o.no_location_prompt = true,
                "--preview-hud" => o.preview_hud = true,
                _ => {}
            }
        }
        if o.self_test_dir.is_some() {
            // 자가 진단은 절대 화면을 잠그지 않는다
            o.dry_run = true;
            o.no_location_prompt = true;
        }
        o
    }

    pub fn is_self_test(&self) -> bool {
        self.self_test_dir.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args() {
        let o = Options::parse(
            ["app", "--self-test", "/tmp/x", "--override-timeout", "30", "--unknown"].iter().map(|s| s.to_string()),
        );
        assert_eq!(o.self_test_dir, Some(PathBuf::from("/tmp/x")));
        assert_eq!(o.override_timeout, Some(30));
        assert!(o.dry_run && o.no_location_prompt, "자가 진단은 잠그지 않는다");
        let p = Options::parse(["app"].iter().map(|s| s.to_string()));
        assert!(!p.dry_run && !p.is_self_test());
    }
}
