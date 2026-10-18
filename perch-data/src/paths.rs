//! perch-data::paths: 统一的运行根目录与数据文件路径解析
//! 消除散落在 perch-core 与 perch 中的硬编码路径与 `@Resources\` 字符串。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 根据传入参数计算 root 路径（纯函数，方便单元测试）
pub fn resolve_root_with(
    exe: Option<&Path>,
    perch_home: Option<&str>,
    local_app_data: Option<&str>,
) -> PathBuf {
    // 1. exe 相对位置探测
    if let Some(exe_path) = exe {
        // 1a. core 结构: <root>/@Resources/bin/perch-core.exe -> 向上三级
        if let Some(root) = exe_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        {
            if root.join("@Resources").is_dir() {
                return root.to_path_buf();
            }
        }
        // 1b. hud 结构: <root>/perch.exe -> 向上一级
        if let Some(root) = exe_path.parent() {
            if root.join("@Resources").is_dir() {
                return root.to_path_buf();
            }
        }
    }

    // 2. 环境变量 PERCH_HOME
    if let Some(home) = perch_home {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    // 3. %LOCALAPPDATA%\perch
    if let Some(lad) = local_app_data {
        let trimmed = lad.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("perch");
        }
    }

    // 4. 兜底:当前工作目录下的 perch
    PathBuf::from("perch")
}

/// 运行时安装根目录
pub fn root() -> &'static Path {
    static ROOT_CELL: OnceLock<PathBuf> = OnceLock::new();
    ROOT_CELL.get_or_init(|| {
        let exe = std::env::current_exe().ok();
        let home = std::env::var("PERCH_HOME").ok();
        let lad = std::env::var("LOCALAPPDATA").ok();
        resolve_root_with(
            exe.as_deref(),
            home.as_deref(),
            lad.as_deref(),
        )
    })
    .as_path()
}

/// 资源目录: <root>/@Resources
pub fn res() -> PathBuf {
    root().join("@Resources")
}

// -----------------------------------------------------------------------------
// 子目录
// -----------------------------------------------------------------------------

pub fn data_dir() -> PathBuf {
    res().join("data")
}

pub fn weather_dir() -> PathBuf {
    res().join("weather")
}

pub fn battery_dir() -> PathBuf {
    res().join("battery")
}

/// 主题目录：优先使用 <root>/themes（阶段 4 新规范），若不存在则回退 <root>/@Resources/themes
pub fn themes_dir() -> PathBuf {
    let new_dir = root().join("themes");
    if new_dir.is_dir() {
        new_dir
    } else {
        res().join("themes")
    }
}

pub fn app_icons_dir() -> PathBuf {
    res().join("app_icons")
}

pub fn dock_icons_dir() -> PathBuf {
    res().join("dock_icons")
}

pub fn wallpapers_dir() -> PathBuf {
    res().join("wallpapers")
}

// -----------------------------------------------------------------------------
// 具体文件路径
// -----------------------------------------------------------------------------

pub fn config_toml() -> PathBuf {
    root().join("config.toml")
}

pub fn theme_toml() -> PathBuf {
    res().join("theme.toml")
}

pub fn battery_toml() -> PathBuf {
    battery_dir().join("battery.toml")
}

pub fn last_battery_txt() -> PathBuf {
    battery_dir().join("last_battery.txt")
}

pub fn quote_toml() -> PathBuf {
    data_dir().join("quote.toml")
}

pub fn quotes_state_json() -> PathBuf {
    data_dir().join("quotes_state.json")
}

pub fn water_toml() -> PathBuf {
    data_dir().join("water.toml")
}

pub fn water_cfg_json() -> PathBuf {
    data_dir().join("water_cfg.json")
}

pub fn water_state_json() -> PathBuf {
    data_dir().join("water_state.json")
}

pub fn ambient_level_txt() -> PathBuf {
    data_dir().join("ambient_level.txt")
}

pub fn modes_toml() -> PathBuf {
    data_dir().join("modes.toml")
}

pub fn timer_toml() -> PathBuf {
    data_dir().join("timer.toml")
}

pub fn todos_json() -> PathBuf {
    data_dir().join("todos.json")
}

pub fn reminders_json() -> PathBuf {
    data_dir().join("reminders.json")
}

pub fn notify_history_json() -> PathBuf {
    data_dir().join("notify_history.json")
}

pub fn notify_info_json() -> PathBuf {
    data_dir().join("notify_info.json")
}

pub fn sun_times_txt() -> PathBuf {
    data_dir().join("sun_times.txt")
}

pub fn weather_cfg_json() -> PathBuf {
    data_dir().join("weather_cfg.json")
}

pub fn media_prefer_txt() -> PathBuf {
    data_dir().join("media_prefer.txt")
}

pub fn media_sessions_txt() -> PathBuf {
    data_dir().join("media_sessions.txt")
}

pub fn media_seek_last_txt() -> PathBuf {
    data_dir().join("media_seek_last.txt")
}

pub fn bgm_json() -> PathBuf {
    data_dir().join("bgm.json")
}

pub fn weather_json() -> PathBuf {
    weather_dir().join("weather.json")
}

pub fn media_json() -> PathBuf {
    weather_dir().join("media.json")
}

pub fn media_timeline_toml() -> PathBuf {
    weather_dir().join("media_timeline.toml")
}

pub fn cover_png() -> PathBuf {
    weather_dir().join("cover.png")
}

pub fn default_cover_png() -> PathBuf {
    dock_icons_dir().join("default_album.png")
}

/// 解析用户配置中的目标路径（支持 `@Resources/...` 相对写法）
pub fn resolve_resource(target: &str) -> PathBuf {
    let trimmed = target.trim();
    if let Some(rest) = trimmed.strip_prefix("@Resources") {
        let sub = rest.trim_start_matches(['\\', '/']);
        res().join(sub)
    } else {
        let p = Path::new(trimmed);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            root().join(p)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_root_priorities() {
        // 1. Fallback 兜底
        let root = resolve_root_with(None, None, None);
        assert_eq!(root, PathBuf::from("perch"));

        // 2. LOCALAPPDATA 兜底
        let root = resolve_root_with(None, None, Some(r"C:\TestUser\AppData\Local"));
        assert_eq!(root, PathBuf::from(r"C:\TestUser\AppData\Local\perch"));

        // 3. PERCH_HOME 优先于 LOCALAPPDATA
        let root = resolve_root_with(
            None,
            Some(r"E:\custom\perch"),
            Some(r"C:\TestUser\AppData\Local"),
        );
        assert_eq!(root, PathBuf::from(r"E:\custom\perch"));

        // 4. exe 存在 @Resources 优先于环境变量
        let temp = tempdir().unwrap();
        let res_dir = temp.path().join("@Resources");
        std::fs::create_dir_all(&res_dir).unwrap();

        // 4a. core: <root>/@Resources/bin/perch-core.exe
        let bin_dir = res_dir.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let core_exe = bin_dir.join("perch-core.exe");

        let root = resolve_root_with(
            Some(&core_exe),
            Some(r"E:\custom\perch"),
            Some(r"C:\TestUser\AppData\Local"),
        );
        assert_eq!(root, temp.path());

        // 4b. hud: <root>/perch.exe
        let hud_exe = temp.path().join("perch.exe");
        let root = resolve_root_with(
            Some(&hud_exe),
            Some(r"E:\custom\perch"),
            Some(r"C:\TestUser\AppData\Local"),
        );
        assert_eq!(root, temp.path());
    }

    #[test]
    fn test_resolve_resource() {
        let r1 = resolve_resource(r"@Resources\dock_icons\app.png");
        assert!(r1.ends_with(r"@Resources\dock_icons\app.png"));

        let r2 = resolve_resource(r"@Resources/wallpapers/demo.png");
        assert!(r2.ends_with(r"wallpapers\demo.png") || r2.ends_with(r"wallpapers/demo.png"));

        let r3 = resolve_resource(r"C:\Windows\System32\notepad.exe");
        assert_eq!(r3, PathBuf::from(r"C:\Windows\System32\notepad.exe"));
    }

    #[test]
    fn test_paths_under_res_and_root() {
        let res_dir = res();
        assert_eq!(theme_toml(), res_dir.join("theme.toml"));
        assert_eq!(battery_toml(), res_dir.join(r"battery\battery.toml"));
        assert_eq!(water_toml(), res_dir.join(r"data\water.toml"));
        assert_eq!(quote_toml(), res_dir.join(r"data\quote.toml"));
        assert_eq!(modes_toml(), res_dir.join(r"data\modes.toml"));
        assert_eq!(todos_json(), res_dir.join(r"data\todos.json"));
        assert_eq!(reminders_json(), res_dir.join(r"data\reminders.json"));
        assert_eq!(weather_json(), res_dir.join(r"weather\weather.json"));
        assert_eq!(media_json(), res_dir.join(r"weather\media.json"));
    }
}
