//! perch-data: 共享数据层
//! 定义 perch-core 与 perch 通信的数据结构，以及原子写入与安全读取。

pub mod i18n;
pub mod paths;
pub mod theme_pack;
pub mod weather_map;

pub use i18n::{t, t_fmt};

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// f32 按最短十进制写出(避免 1.2 被写成 1.2000000476837158)
fn ser_f32<S: serde::Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(v.to_string().parse::<f64>().unwrap_or(*v as f64))
}

pub fn default_v() -> u32 {
    1
}

pub fn default_water_goal() -> u64 {
    2000
}

pub fn default_dash() -> String {
    "--:--".to_string()
}

pub fn default_danger() -> String {
    "239,68,68,255".to_string()
}

pub fn default_warn() -> String {
    "249,115,22,255".to_string()
}

pub fn default_ok() -> String {
    "16,185,129,255".to_string()
}

// -----------------------------------------------------------------------------
// 1. 扁平状态量 (TOML)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Battery {
    #[serde(default = "default_v")]
    pub v: u32,
    #[serde(default)]
    pub mouse_icon: String,
    #[serde(default)]
    pub mouse_text: String,
    #[serde(default)]
    pub mouse_status: String,
    #[serde(default)]
    pub mouse_color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Quote {
    #[serde(default = "default_v")]
    pub v: u32,
    #[serde(default)]
    pub speaker: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Water {
    #[serde(default = "default_v")]
    pub v: u32,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub ml: u64,
    #[serde(default = "default_water_goal")]
    pub goal: u64,
    #[serde(default)]
    pub pct: u64,
    #[serde(default = "default_dash")]
    pub last: String,
    #[serde(default)]
    pub cups_n: usize,
    #[serde(default)]
    pub done: u8,
    #[serde(default)]
    pub last_ts: u64,
    #[serde(default)]
    pub last_remind_ts: u64,
    #[serde(default)]
    pub log: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Modes {
    #[serde(default = "default_v")]
    pub v: u32,
    #[serde(default)]
    pub wallpaper: String,
    #[serde(default)]
    pub bgm: String,
    #[serde(default)]
    pub theme_auto: String,
    #[serde(default)]
    pub system_theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Timer {
    #[serde(default = "default_v")]
    pub v: u32,
    #[serde(default)]
    pub state: u8, // 0: Idle, 1: Running, 2: Paused
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MediaTimeline {
    #[serde(default = "default_v")]
    pub v: u32,
    #[serde(default)]
    pub pos_ms: u64,
    #[serde(default)]
    pub dur_ms: u64,
    #[serde(default)]
    pub stamp_ms: u64,
    #[serde(default)]
    pub playing: u8,
}

// -----------------------------------------------------------------------------
// 2. 结构化数据 (JSON)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TodoItem {
    pub id: usize,
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ReminderItem {
    pub id: u32,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub due_ts: Option<u64>,
    #[serde(default)]
    pub fired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct NotifyInfo {
    #[serde(default = "default_v")]
    pub v: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub msg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DailyForecast {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub range: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Weather {
    #[serde(default = "default_v")]
    pub v: u32,
    #[serde(default)]
    pub temp: i32,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub high: i32,
    #[serde(default)]
    pub low: i32,
    #[serde(default)]
    pub rain_mm: f64,
    #[serde(default)]
    pub rain_chance: String,
    #[serde(default)]
    pub humidity: String,
    #[serde(default)]
    pub wind: String,
    #[serde(default)]
    pub glow_fill: String,
    #[serde(default)]
    pub glow_stroke: String,
    #[serde(default)]
    pub d1: DailyForecast,
    #[serde(default)]
    pub d2: DailyForecast,
    #[serde(default)]
    pub d3: DailyForecast,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MediaInfo {
    #[serde(default = "default_v")]
    pub v: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub cover_file: String,
    #[serde(default)]
    pub app_icon: String,
    #[serde(default)]
    pub play_icon: String,
}

// -----------------------------------------------------------------------------
// 3. 主题数据 (TOML)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemeToml {
    #[serde(default = "default_v")]
    pub v: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub sub: String,
    #[serde(default)]
    pub bg: String,
    #[serde(default)]
    pub dock_bg: String,
    #[serde(default)]
    pub stroke: String,
    #[serde(default)]
    pub item_bg: String,
    #[serde(default)]
    pub item_hover: String,
    #[serde(default)]
    pub pill_bg: String,
    #[serde(default)]
    pub dock_title: String,
    #[serde(default)]
    pub color_white: String,
    #[serde(default)]
    pub color_muted: String,
    #[serde(default)]
    pub color_on_accent: String,
    #[serde(default)]
    pub color_on_sub: String,
    #[serde(default = "default_danger")]
    pub danger: String,
    #[serde(default = "default_warn")]
    pub warn: String,
    #[serde(default = "default_ok")]
    pub ok: String,
    #[serde(default)]
    pub field_name: String,
    #[serde(default)]
    pub unit_name: String,
    #[serde(default)]
    pub font_main: String,
    #[serde(default)]
    pub font_num: String,
    #[serde(default, serialize_with = "ser_f32")]
    pub radius: f32,
    #[serde(default)]
    pub frame_stroke: String,
    #[serde(default, serialize_with = "ser_f32")]
    pub frame_width: f32,
    #[serde(default)]
    pub shape2: String,
    #[serde(default)]
    pub corner: String,
    #[serde(default, serialize_with = "ser_f32")]
    pub bar_h: f32,
    #[serde(default)]
    pub seg_color: String,
    #[serde(default)]
    pub title_timer: String,
    #[serde(default)]
    pub sub_timer: String,
    #[serde(default)]
    pub title_events: String,
    #[serde(default)]
    pub sub_events: String,
    #[serde(default)]
    pub title_todo: String,
    #[serde(default)]
    pub sub_todo: String,
    #[serde(default)]
    pub title_media: String,
    #[serde(default)]
    pub sub_media: String,
    #[serde(default)]
    pub title_weather: String,
    #[serde(default)]
    pub title_water: String,
    #[serde(default)]
    pub sub_water: String,
    #[serde(default)]
    pub lbl_cpu: String,
    #[serde(default)]
    pub lbl_ram: String,
    #[serde(default)]
    pub lbl_c: String,
    #[serde(default)]
    pub lbl_d: String,
    #[serde(default)]
    pub lbl_net: String,
}

// -----------------------------------------------------------------------------
// 4. 原子写入与安全读取公共函数
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Toml,
    Json,
}

impl Format {
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|s| s.to_str()) {
            Some("json") => Format::Json,
            _ => Format::Toml,
        }
    }
}

/// 将数据原子写入目标文件:写临时文件 -> sync -> rename
pub fn write_atomic<T: Serialize>(path: impl AsRef<Path>, value: &T) -> io::Result<()> {
    let path = path.as_ref();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let format = Format::from_path(path);
    let bytes = match format {
        Format::Toml => toml::to_string_pretty(value)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
            .into_bytes(),
        Format::Json => serde_json::to_string_pretty(value)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
            .into_bytes(),
    };

    // 在同目录下创建临时文件
    let mut temp = tempfile::Builder::new()
        .prefix(".tmp_")
        .tempfile_in(parent)?;

    temp.write_all(&bytes)?;
    temp.flush()?;
    temp.as_file().sync_all()?;

    // persist: 将 tempfile 重命名为目标路径并保持权限
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// 从文件中安全读取并解析。若不存在或格式错误返回 Err，调用方据此保留上一次的内存旧值。
pub fn read<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T, Box<dyn std::error::Error + Send + Sync>> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)?;
    let format = Format::from_path(path);
    match format {
        Format::Toml => {
            let val = toml::from_str::<T>(&content)?;
            Ok(val)
        }
        Format::Json => {
            let val = serde_json::from_str::<T>(&content)?;
            Ok(val)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_f32_written_short() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.toml");
        let t = ThemeToml { radius: 14.0, frame_width: 1.2, bar_h: 0.3, ..Default::default() };
        write_atomic(&path, &t).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("frame_width = 1.2\n") || text.contains("frame_width = 1.2\r\n"), "{text}");
        let back: ThemeToml = read(&path).unwrap();
        assert_eq!(back.frame_width, 1.2f32);
    }

    #[test]
    fn test_battery_roundtrip_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("battery.toml");
        let b = Battery {
            v: 1,
            mouse_icon: "🖱️".into(),
            mouse_text: "85%".into(),
            mouse_status: "Mouse: 85%".into(),
            mouse_color: "239,68,68,255".into(),
        };
        write_atomic(&path, &b).unwrap();
        assert!(path.exists());

        // 验证读取
        let read_b: Battery = read(&path).unwrap();
        assert_eq!(b, read_b);

        // 验证无残留 .tmp
        let files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files, vec!["battery.toml"]);
    }

    #[test]
    fn test_weather_roundtrip_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weather.json");
        let w = Weather {
            v: 1,
            temp: 20,
            status: "🌦️ 小雨".into(),
            high: 22,
            low: 18,
            rain_mm: 1.5,
            rain_chance: "30%".into(),
            humidity: "80%".into(),
            wind: "12 km/h".into(),
            glow_fill: "56,189,248,220".into(),
            glow_stroke: "14,165,233,255".into(),
            d1: DailyForecast {
                date: "明天".into(),
                status: "☀️ 晴".into(),
                range: "18°~25°".into(),
            },
            d2: Default::default(),
            d3: Default::default(),
        };
        write_atomic(&path, &w).unwrap();
        let read_w: Weather = read(&path).unwrap();
        assert_eq!(w, read_w);
    }

    #[test]
    fn test_corrupt_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.toml");
        fs::write(&path, "invalid toml {[[[[").unwrap();
        let res: Result<Battery, _> = read(&path);
        assert!(res.is_err());
    }

    #[test]
    fn test_water_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("water.toml");
        let w = Water {
            v: 1,
            date: "2026-10-17".into(),
            ml: 750,
            goal: 2000,
            pct: 37,
            last: "14:20".into(),
            cups_n: 3,
            done: 0,
            last_ts: 1792216256,
            last_remind_ts: 1792216256,
            log: vec![serde_json::json!(["14:20", 250])],
        };
        write_atomic(&path, &w).unwrap();
        let read_w: Water = read(&path).unwrap();
        assert_eq!(w, read_w);
    }
}
