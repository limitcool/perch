//! 简单的文件日志:%APPDATA%\perch\hud.log(超过 512 KB 自动轮转为 hud.old.log)。

use std::io::Write;
use std::path::PathBuf;

fn dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("perch")
}

pub fn log(msg: &str) {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let path = d.join("hud.log");
    if std::fs::metadata(&path).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&path, d.join("hud.old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), msg);
    }
}

/// panic 也写进日志(退出码非零,计划任务会据此重启)
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        log(&format!("[PANIC] {}", info));
    }));
}

/// "很久以前"的时间点。Instant 在开机不久时减去较长的时间会溢出 panic,所以用 checked_sub;
/// 减不动时退回现在(只是少等一个间隔,不会崩)
pub fn ago(secs: u64) -> std::time::Instant {
    let now = std::time::Instant::now();
    now.checked_sub(std::time::Duration::from_secs(secs)).unwrap_or(now)
}
