//! 设置与数据的备份 / 恢复:把配置、布局、选项、番茄钟统计、收集箱、待办、日程等复制到 <安装目录>\backups\<时间戳>\ 下。
//! 恢复前会先自动再备份一次当前状态("pre-restore"),最多保留 12 份。

use std::path::{Path, PathBuf};

use crate::config;
use crate::data;

const KEEP: usize = 12;

fn appdata() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("perch")
}

/// (备份内的相对路径, 实际文件)
fn files() -> Vec<(String, PathBuf)> {
    let d = perch_data::paths::data_dir();
    let a = appdata();
    vec![
        ("config.toml".into(), config::path()),
        ("appdata/layout.json".into(), a.join("layout.json")),
        ("appdata/options.txt".into(), a.join("options.txt")),
        ("appdata/pomodoro.json".into(), a.join("pomodoro.json")),
        ("appdata/notes.md".into(), a.join("notes.md")),
        ("data/todos.json".into(), d.join("todos.json")),
        ("data/reminders.json".into(), d.join("reminders.json")),
        ("data/water_cfg.json".into(), d.join("water_cfg.json")),
        ("data/weather_cfg.json".into(), d.join("weather_cfg.json")),
        ("data/bgm.json".into(), d.join("bgm.json")),
    ]
}

pub fn dir() -> PathBuf {
    config::root().join("backups")
}

fn make(label: &str) -> Result<PathBuf, String> {
    let name = format!("{}{}", chrono::Local::now().format("%Y%m%d-%H%M%S"), label);
    let target = dir().join(&name);
    let mut n = 0;
    for (rel, src) in files() {
        if !src.is_file() {
            continue;
        }
        let dst = target.join(&rel);
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::copy(&src, &dst).map_err(|e| format!("{}: {}", rel, e))?;
        n += 1;
    }
    if n == 0 {
        return Err(perch_data::t("misc.backup_none_files").into());
    }
    prune();
    Ok(target)
}

pub fn backup() -> Result<PathBuf, String> {
    make("")
}

fn list() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir()).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    v.sort();
    v
}

pub fn latest() -> Option<PathBuf> {
    // 恢复时跳过 pre-restore 这种自动备份,取用户手动做的最近一份
    list().into_iter().rev().find(|p| !p.file_name().map_or(false, |n| n.to_string_lossy().contains("pre-restore")))
}

fn prune() {
    let all = list();
    if all.len() > KEEP {
        for old in &all[..all.len() - KEEP] {
            let _ = std::fs::remove_dir_all(old);
        }
    }
}

/// 恢复:先备份当前状态,再把备份里的文件复制回去
pub fn restore(from: &Path) -> Result<usize, String> {
    make("-pre-restore")?;
    let mut n = 0;
    for (rel, dst) in files() {
        let src = from.join(&rel);
        if !src.is_file() {
            continue;
        }
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::copy(&src, &dst).map_err(|e| format!("{}: {}", rel, e))?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_list_has_config() {
        assert!(super::files().iter().any(|(r, _)| r == "config.toml"));
    }

    /// 用真实文件做一次"备份 → 立刻恢复"(状态相同,所以是幂等的),确认内容不变并清理产生的备份
    /// cargo test backup -- --ignored --nocapture
    #[test]
    #[ignore]
    fn roundtrip_real_files() {
        let before = std::fs::read(crate::config::path()).unwrap();
        let b = super::backup().expect("backup");
        let n_files = walk(&b);
        let n = super::restore(&b).expect("restore");
        let after = std::fs::read(crate::config::path()).unwrap();
        println!("backed up {} files, restored {}, config identical = {}", n_files, n, before == after);
        assert_eq!(before, after);
        let made: Vec<_> = std::fs::read_dir(super::dir()).unwrap().flatten().map(|e| e.path()).collect();
        for p in made {
            let _ = std::fs::remove_dir_all(p);
        }
    }

    fn walk(d: &std::path::Path) -> usize {
        std::fs::read_dir(d).map(|rd| rd.flatten().map(|e| if e.path().is_dir() { walk(&e.path()) } else { 1 }).sum()).unwrap_or(0)
    }
}
