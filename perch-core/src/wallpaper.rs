//! perch-core::wallpaper: 动态壁纸管理、昼夜交替、Auto Dark Mode 跟随与壁纸池切换

use std::fs;
use std::path::PathBuf;
use windows::core::w;
use crate::util::{epoch_secs, set_windows_wallpaper};
use crate::theme::current_theme_name;

fn wp_dir() -> PathBuf {
    perch_data::paths::wallpapers_dir()
}

pub fn load_modes() -> perch_data::Modes {
    let p = perch_data::paths::modes_toml();
    perch_data::read::<perch_data::Modes>(&p).unwrap_or(perch_data::Modes {
        v: 1,
        wallpaper: "auto".into(),
        bgm: "0".into(),
        theme_auto: "weekday".into(),
        system_theme: "0".into(),
    })
}

pub fn save_modes(m: &perch_data::Modes) {
    let p = perch_data::paths::modes_toml();
    let _ = perch_data::write_atomic(&p, m);
}

pub fn wallpaper_mode() -> String {
    let m = load_modes();
    match m.wallpaper.as_str() {
        "day" => "day".to_string(),
        "night" => "night".to_string(),
        _ => "auto".to_string(),
    }
}

fn sun_minutes() -> (u32, u32) {
    let t = fs::read_to_string(perch_data::paths::sun_times_txt()).unwrap_or_default();
    let mut it = t.split_whitespace().filter_map(|x| x.parse::<u32>().ok());
    match (it.next(), it.next()) {
        (Some(a), Some(b)) if a < b && b <= 1440 => (a, b),
        _ => (360, 1080),
    }
}

pub fn system_uses_light_theme() -> Option<bool> {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    // Auto Dark Mode 切换时会改 AppsUseLightTheme / SystemUsesLightTheme
    for name in [w!("AppsUseLightTheme"), w!("SystemUsesLightTheme")] {
        let mut data: u32 = 0;
        let mut size: u32 = 4;
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
                name,
                RRF_RT_REG_DWORD,
                None,
                Some(&mut data as *mut u32 as *mut core::ffi::c_void),
                Some(&mut size),
            )
        };
        if r.0 == 0 {
            return Some(data != 0);
        }
    }
    None
}

// 把系统浅/深色写入 modes.toml
pub fn probe_system_theme() {
    let mut m = load_modes();
    let modes_file = perch_data::paths::modes_toml();
    let need_save = !modes_file.exists();

    if let Some(light) = system_uses_light_theme() {
        let want = if light { "1" } else { "0" };
        if m.system_theme != want {
            m.system_theme = want.to_string();
            save_modes(&m);
            return;
        }
    }
    if need_save {
        save_modes(&m);
    }
}

fn pool_size(theme: &str, period: &str) -> usize {
    let mut k = 0;
    while wp_dir().join(format!("{}_{}_{}.png", theme, period, k + 1)).exists() {
        k += 1;
    }
    k
}

/// "换一张壁纸":当天内在壁纸池里顺延的偏移(跨天自动清零)
fn wallpaper_offset() -> usize {
    let t = fs::read_to_string(perch_data::paths::data_dir().join("wallpaper_offset.txt")).unwrap_or_default();
    let mut it = t.split_whitespace();
    let (day, off) = (it.next().and_then(|s| s.parse::<u64>().ok()), it.next().and_then(|s| s.parse::<usize>().ok()));
    if day == Some(epoch_secs() / 86400) { off.unwrap_or(0) } else { 0 }
}

pub fn wallpaper_next() {
    let off = wallpaper_offset() + 1;
    let _ = fs::write(perch_data::paths::data_dir().join("wallpaper_offset.txt"), format!("{} {}", epoch_secs() / 86400, off));
    sync_wallpaper();
}

pub fn current_wallpaper_path(theme: &str) -> Option<PathBuf> {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let st = unsafe { GetLocalTime() };
    let minutes = st.wHour as u32 * 60 + st.wMinute as u32;
    let (rise, set) = sun_minutes();
    let is_day = match wallpaper_mode().as_str() {
        "day" => true,
        "night" => false,
        // 跟随系统:Auto Dark Mode 切换时会改 AppsUseLightTheme;读不到再按日出日落
        _ => system_uses_light_theme().unwrap_or(minutes >= rise && minutes < set),
    };
    let wday_mon0 = (st.wDayOfWeek as usize + 6) % 7;
    for period in if is_day { ["day", "night"] } else { ["night", "day"] } {
        let k = pool_size(theme, period);
        if k > 0 {
            return Some(wp_dir().join(format!("{}_{}_{}.png", theme, period, ((wday_mon0 + wallpaper_offset()) % k) + 1)));
        }
    }
    let legacy = wp_dir().join(format!("{}.png", theme));
    if legacy.exists() {
        Some(legacy)
    } else {
        None
    }
}

pub fn sync_wallpaper() {
    if let Some(p) = current_wallpaper_path(&current_theme_name()) {
        set_windows_wallpaper(&p.to_string_lossy());
    }
}

pub fn set_wallpaper_mode(mode: &str) {
    if !["auto", "day", "night"].contains(&mode) {
        return;
    }
    let mut m = load_modes();
    m.wallpaper = mode.to_string();
    save_modes(&m);
    sync_wallpaper();
}
