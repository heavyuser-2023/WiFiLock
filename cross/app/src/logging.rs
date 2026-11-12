//! 간단한 기록 파일 (1MB 를 넘으면 .old 로 넘기고 새로 쓴다)

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

static FILE: Mutex<Option<File>> = Mutex::new(None);

pub fn init(path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if fs::metadata(path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = fs::rename(path, path.with_extension("log.old"));
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(path) {
        *FILE.lock().unwrap() = Some(f);
    }
}

fn write(level: &str, msg: &str) {
    let line = format!("{} [{level}] {msg}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
    if let Ok(mut g) = FILE.lock() {
        if let Some(f) = g.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

pub fn info(msg: impl AsRef<str>) {
    write("정보", msg.as_ref());
}

pub fn error(msg: impl AsRef<str>) {
    write("오류", msg.as_ref());
}
