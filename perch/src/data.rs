//! 数据层:HUD 只做客户端。数据全部来自 perch-core 写出的 toml/json 文件,操作全部调用 perch-core 命令。

use std::collections::HashMap;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use crate::live_theme::core_exe;

/// 资源目录字符串形式: 委托给 perch_data::paths::res()
pub fn res() -> &'static str {
    static RES: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    RES.get_or_init(|| perch_data::paths::res().to_string_lossy().to_string())
}
const NO_WINDOW: u32 = 0x0800_0000;

pub type Vars = HashMap<String, String>;

fn read_text(path: &PathBuf) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let u: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Some(String::from_utf16_lossy(&u))
    } else {
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}


pub fn core(args: &[&str]) {
    let _ = Command::new(core_exe()).args(args).creation_flags(NO_WINDOW).spawn();
}

pub fn open_target(target: &str, args: &str) {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "start", "", target]);
    if !args.is_empty() {
        cmd.raw_arg(args);
    }
    let _ = cmd.creation_flags(NO_WINDOW).spawn();
}

pub struct Hub {
    pub weather: Vars,
    pub battery: Vars,
    pub water: Vars,
    pub media: Vars,
    pub timeline: Vars,
    pub quotes: Vars,
    pub notify: Vars,
    pub todos: Vars,
    pub reminders: Vars,
    pub modes: Vars,
    pub ambient: String,
    last_files: Instant,
    // None = 还没跑过,第一次 tick 就执行(不能用"很久以前的 Instant":开机不久时减不动,会白等一个周期)
    last_battery: Option<Instant>,
    last_weather: Option<Instant>,
    last_wallpaper: Option<Instant>,
    theme_bootstrapped: bool,
    media_child: Option<Child>,
    clean_child: Option<Child>,
    clean_started: Instant,
    last_media: Instant,
    notify_sig: String,
    pub notify_until: Option<Instant>,
    pub todo_scroll: usize,
    pub todo_total: usize,
    pub todo_any_done: bool,
    /// Obsidian 今日日记里的任务(按文件修改时间缓存)
    obs_key: Option<(PathBuf, std::time::SystemTime)>,
    obs_tasks: Vec<crate::obsidian_tasks::Task>,
    pub rem_scroll: usize,
    pub history: Vec<(u64, String, String)>,
    pub history_scroll: usize,
    pub reminder_items: Vec<(u32, String, String, bool)>,
    notify_since: Option<Instant>,
}

impl Hub {
    pub fn new() -> Self {
        let long_ago = crate::log::ago(3600);
        let mut h = Self {
            weather: Vars::new(),
            battery: Vars::new(),
            water: Vars::new(),
            media: Vars::new(),
            timeline: Vars::new(),
            quotes: Vars::new(),
            notify: Vars::new(),
            todos: Vars::new(),
            reminders: Vars::new(),
            modes: Vars::new(),
            ambient: "low".to_string(),
            last_files: long_ago,
            last_battery: None,
            last_weather: None,
            last_wallpaper: None,
            theme_bootstrapped: false,
            media_child: None,
            clean_child: None,
            clean_started: long_ago,
            last_media: long_ago,
            notify_sig: String::new(),
            notify_until: None,
            todo_scroll: 0,
            todo_total: 0,
            todo_any_done: false,
            obs_key: None,
            obs_tasks: Vec::new(),
            rem_scroll: 0,
            history: Vec::new(),
            history_scroll: 0,
            reminder_items: Vec::new(),
            notify_since: None,
        };
        h.reload_files();
        h.notify_sig = h.notify_signature();
        h
    }

    pub fn v<'a>(m: &'a Vars, k: &str) -> &'a str {
        m.get(k).map(|s| s.as_str()).unwrap_or("")
    }

    fn notify_signature(&self) -> String {
        format!("{}|{}", Self::v(&self.notify, "NotifyTitle"), Self::v(&self.notify, "NotifyMsg"))
    }

    fn reload_files(&mut self) {
        use perch_data::paths;
        if let Ok(w) = perch_data::read::<perch_data::Weather>(paths::weather_json()) {
            let mut m = Vars::new();
            m.insert("WeatherTemp".into(), w.temp.to_string());
            m.insert("WeatherStatus".into(), w.status);
            m.insert("WeatherHigh".into(), w.high.to_string());
            m.insert("WeatherLow".into(), w.low.to_string());
            m.insert("WeatherRainMM".into(), w.rain_mm.to_string());
            m.insert("WeatherRainChance".into(), w.rain_chance);
            m.insert("WeatherHumidity".into(), w.humidity);
            m.insert("WeatherWind".into(), w.wind);
            m.insert("WeatherGlowFill".into(), w.glow_fill);
            m.insert("WeatherGlowStroke".into(), w.glow_stroke);
            m.insert("WeatherD1Date".into(), w.d1.date);
            m.insert("WeatherD1Status".into(), w.d1.status);
            m.insert("WeatherD1Range".into(), w.d1.range);
            m.insert("WeatherD2Date".into(), w.d2.date);
            m.insert("WeatherD2Status".into(), w.d2.status);
            m.insert("WeatherD2Range".into(), w.d2.range);
            m.insert("WeatherD3Date".into(), w.d3.date);
            m.insert("WeatherD3Status".into(), w.d3.status);
            m.insert("WeatherD3Range".into(), w.d3.range);
            self.weather = m;
        }
        if let Ok(b) = perch_data::read::<perch_data::Battery>(paths::battery_toml()) {
            crate::cards::set_battery_available(b.available);
            let mut m = Vars::new();
            m.insert("MouseBatteryIcon".into(), b.mouse_icon);
            m.insert("MouseBatteryText".into(), b.mouse_text);
            m.insert("MouseBatteryStatus".into(), b.mouse_status);
            m.insert("MouseBatteryColor".into(), b.mouse_color);
            self.battery = m;
        }
        if let Ok(w) = perch_data::read::<perch_data::Water>(paths::water_toml()) {
            let mut m = Vars::new();
            m.insert("WaterMl".into(), w.ml.to_string());
            m.insert("WaterGoal".into(), w.goal.to_string());
            m.insert("WaterPct".into(), w.pct.to_string());
            m.insert("WaterLast".into(), w.last);
            m.insert("WaterCupsN".into(), w.cups_n.to_string());
            m.insert("WaterDone".into(), w.done.to_string());
            self.water = m;
        }
        if let Ok(m) = perch_data::read::<perch_data::MediaInfo>(paths::media_json()) {
            let mut v = Vars::new();
            v.insert("MediaTitle".into(), m.title);
            v.insert("MediaArtist".into(), m.artist);
            v.insert("MediaStatus".into(), m.status);
            v.insert("MediaCoverFile".into(), m.cover_file);
            v.insert("MediaAppIcon".into(), m.app_icon);
            v.insert("MediaPlayIcon".into(), m.play_icon);
            self.media = v;
        }
        if let Ok(tl) = perch_data::read::<perch_data::MediaTimeline>(paths::media_timeline_toml()) {
            let mut v = Vars::new();
            v.insert("MediaPosMs".into(), tl.pos_ms.to_string());
            v.insert("MediaDurMs".into(), tl.dur_ms.to_string());
            v.insert("MediaStampMs".into(), tl.stamp_ms.to_string());
            v.insert("MediaPlaying".into(), tl.playing.to_string());
            self.timeline = v;
        }
        if let Ok(q) = perch_data::read::<perch_data::Quote>(paths::quote_toml()) {
            let mut m = Vars::new();
            m.insert("QuoteSpeaker".into(), q.speaker);
            m.insert("CurrentQuoteText".into(), q.text);
            self.quotes = m;
        }
        if let Ok(n) = perch_data::read::<perch_data::NotifyInfo>(paths::notify_info_json()) {
            let mut m = Vars::new();
            m.insert("NotifyTitle".into(), n.title);
            m.insert("NotifyMsg".into(), n.msg);
            self.notify = m;
        }
        self.build_todos();
        self.load_reminders();
        self.load_history();
        let modes_toml_path = paths::modes_toml();
        if let Ok(m) = perch_data::read::<perch_data::Modes>(&modes_toml_path) {
            let mut modes = Vars::new();
            modes.insert("WpMode".into(), m.wallpaper);
            modes.insert("BgmAuto".into(), m.bgm);
            modes.insert("AutoMode".into(), m.theme_auto);
            modes.insert("SysLight".into(), m.system_theme);
            self.modes = modes;
        }
        self.ambient = fs::read_to_string(paths::ambient_level_txt()).map(|s| s.trim().to_string()).unwrap_or_else(|_| "low".into());
    }

    /// 通知内容变化 -> 显示 6 秒
    fn check_notify(&mut self) {
        let sig = self.notify_signature();
        if sig != self.notify_sig {
            self.notify_sig = sig;
            self.notify_since = Some(Instant::now());
            self.notify_until = Some(Instant::now() + Duration::from_secs(6));
        }
        if let Some(t) = self.notify_until {
            if Instant::now() > t {
                self.notify_until = None;
            }
        }
    }

    /// 每帧调用(内部节流):文件 1 秒读一次,core 定时任务按各自周期触发
    pub fn tick(&mut self) {
        if self.last_files.elapsed() >= Duration::from_millis(1000) {
            self.last_files = Instant::now();
            self.reload_files();
            self.check_notify();
        }
        let intervals = crate::config::get().intervals;
        let due = |t: Option<Instant>, secs: u64| t.map_or(true, |t| t.elapsed() >= Duration::from_secs(secs.max(1)));
        // 全新安装:还没有 theme.toml / 语录 → 让 core 生成默认主题(之后由主题切换维护)
        if !self.theme_bootstrapped {
            self.theme_bootstrapped = true;
            if !perch_data::paths::theme_toml().exists() {
                core(&["theme", "default"]);
            }
        }
        if due(self.last_battery, intervals.battery_sec) {
            self.last_battery = Some(Instant::now());
            core(&["battery"]);
        }
        if due(self.last_weather, intervals.weather_sec) {
            self.last_weather = Some(Instant::now());
            core(&["weather"]);
        }
        // 壁纸由 Windows 桌面自己显示;昼夜/主题变化后由 core 换成对应图片
        if due(self.last_wallpaper, intervals.wallpaper_sec) {
            self.last_wallpaper = Some(Instant::now());
            core(&["wallpaper", "sync"]);
        }
        // media update 同时带动系统明暗探测 / 主题轮换 / 喝水提醒
        let busy = match &mut self.media_child {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        };
        if !busy && self.last_media.elapsed() >= Duration::from_millis(900) {
            self.last_media = Instant::now();
            self.media_child = Command::new(core_exe()).args(["media", "update"]).creation_flags(NO_WINDOW).spawn().ok();
        }
    }

    /// 把 todos.json 按当前滚动位置合成 4 个槽位变量(TodoId1..4 等,沿用早期槽位变量命名)
    fn build_todos(&mut self) {
        let text = read_text(&perch_data::paths::todos_json()).unwrap_or_default();
        let items: Vec<(u32, String, bool)> = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .map(|a| {
                a.iter()
                    .map(|i| (i["id"].as_u64().unwrap_or(0) as u32, i["text"].as_str().unwrap_or("").to_string(), i["done"].as_bool().unwrap_or(false)))
                    .collect()
            })
            .unwrap_or_default();
        // 并入 Obsidian 今日日记里的任务(id ≥ ID_BASE,文字前带 📓)
        let mut items = items;
        crate::obsidian_tasks::refresh(&mut self.obs_key, &mut self.obs_tasks);
        for t in &self.obs_tasks {
            items.push((crate::obsidian_tasks::ID_BASE + t.line as u32, format!("📓 {}", t.text), t.done));
        }
        self.todo_total = items.len();
        self.todo_any_done = items.iter().any(|i| i.2 && i.0 < crate::obsidian_tasks::ID_BASE);
        let max_scroll = items.len().saturating_sub(4);
        self.todo_scroll = self.todo_scroll.min(max_scroll);
        let mut m = Vars::new();
        for slot in 0..4 {
            let n = slot + 1;
            let item = if items.is_empty() { None } else { items.get(self.todo_scroll + slot) };
            match item {
                Some((id, text, done)) => {
                    m.insert(format!("TodoId{}", n), id.to_string());
                    m.insert(format!("TodoIcon{}", n), if *done { "✔" } else { "▪" }.to_string());
                    m.insert(format!("TodoIconColor{}", n), if *done { "100,116,139,180" } else { "#ThemeSub#" }.to_string());
                    m.insert(format!("TodoText{}", n), text.clone());
                    m.insert(format!("TodoColor{}", n), if *done { "148,163,184,160" } else { "#ColorWhite#" }.to_string());
                    m.insert(format!("TodoVisible{}", n), "1".to_string());
                }
                None if items.is_empty() && slot == 0 => {
                    m.insert("TodoId1".into(), "0".into());
                    m.insert("TodoIcon1".into(), "⚡".into());
                    m.insert("TodoIconColor1".into(), "#ThemeSub#".into());
                    m.insert("TodoText1".into(), perch_data::t("cards2.todo_all_done"));
                    m.insert("TodoColor1".into(), "160,174,192,200".into());
                    m.insert("TodoVisible1".into(), "1".into());
                }
                None => {
                    m.insert(format!("TodoId{}", n), "0".to_string());
                    m.insert(format!("TodoVisible{}", n), "0".to_string());
                }
            }
        }
        self.todos = m;
    }

    pub fn scroll_todos(&mut self, delta: i32) {
        let max = self.todo_total.saturating_sub(4);
        self.todo_scroll = (self.todo_scroll as i32 + delta).clamp(0, max as i32) as usize;
        self.build_todos();
    }

    fn load_reminders(&mut self) {
        let text = read_text(&perch_data::paths::reminders_json()).unwrap_or_default();
        let items = serde_json::from_str::<Vec<perch_data::ReminderItem>>(&text).unwrap_or_default();
        self.reminder_items = items.iter().map(|i| (i.id, i.time.clone(), i.text.clone(), i.fired)).collect();
        let max = (self.reminder_items.len() + crate::calendar::upcoming().len()).saturating_sub(5);
        self.rem_scroll = self.rem_scroll.min(max);

        // 自行计算卡片展示项(直接读 reminders.json)
        let pending: Vec<&perch_data::ReminderItem> = items.iter().filter(|i| !i.fired && i.due_ts.is_some()).collect();
        let next = pending
            .iter()
            .min_by_key(|i| i.due_ts.unwrap_or(u64::MAX))
            .copied()
            .or_else(|| items.iter().find(|i| i.due_ts.is_none()))
            .or_else(|| items.iter().filter(|i| i.fired).max_by_key(|i| i.due_ts.unwrap_or(0)));

        let mut rem_vars = Vars::new();
        if let Some(i) = next {
            rem_vars.insert("ReminderId".into(), i.id.to_string());
            rem_vars.insert("ReminderTime".into(), i.time.clone());
            rem_vars.insert("ReminderText".into(), i.text.clone());
            rem_vars.insert("ReminderFired".into(), if i.fired { "1" } else { "0" }.into());
        } else {
            rem_vars.insert("ReminderId".into(), "0".into());
            rem_vars.insert("ReminderTime".into(), String::new());
            rem_vars.insert("ReminderText".into(), String::new());
            rem_vars.insert("ReminderFired".into(), "0".into());
        }
        let count = pending.len() + items.iter().filter(|i| i.due_ts.is_none()).count();
        rem_vars.insert("ReminderCount".into(), count.to_string());
        self.reminders = rem_vars;
    }

    fn load_history(&mut self) {
        let text = read_text(&perch_data::paths::notify_history_json()).unwrap_or_default();
        let mut list: Vec<(u64, String, String)> = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .map(|a| a.iter().map(|i| (i["ts"].as_u64().unwrap_or(0), i["title"].as_str().unwrap_or("").to_string(), i["msg"].as_str().unwrap_or("").to_string())).collect())
            .unwrap_or_default();
        list.reverse(); // 最新在前
        self.history = list;
        let max = self.history.len().saturating_sub(8);
        self.history_scroll = self.history_scroll.min(max);
    }

    pub fn scroll_history(&mut self, delta: i32) {
        let max = self.history.len().saturating_sub(8);
        self.history_scroll = (self.history_scroll as i32 + delta).clamp(0, max as i32) as usize;
    }

    pub fn latest_history_ts(&self) -> u64 {
        self.history.first().map(|h| h.0).unwrap_or(0)
    }

    /// 点击通知卡片:立即关闭
    pub fn dismiss_notify(&mut self) {
        self.notify_until = None;
    }

    pub fn scroll_reminders(&mut self, delta: i32) {
        let max = (self.reminder_items.len() + crate::calendar::upcoming().len()).saturating_sub(5);
        self.rem_scroll = (self.rem_scroll as i32 + delta).clamp(0, max as i32) as usize;
    }

    /// 通知卡片透明度 0..1(淡入 0.25s / 淡出 0.5s)
    pub fn notify_alpha(&self) -> f32 {
        let (Some(since), Some(until)) = (self.notify_since, self.notify_until) else {
            return 0.0;
        };
        let now = Instant::now();
        let fin = (now.duration_since(since).as_secs_f32() / 0.25).clamp(0.0, 1.0);
        let fout = (until.saturating_duration_since(now).as_secs_f32() / 0.5).clamp(0.0, 1.0);
        fin.min(fout)
    }

    /// 桌面被盖住时的"后台心跳":只每 30 秒跑一次 core media update,
    /// 让日程提醒 / 喝水提醒 / 主题轮换 / 昼夜探测继续工作;不读文件、不更新界面数据。
    pub fn tick_background(&mut self) {
        let busy = match &mut self.media_child {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        };
        if !busy && self.last_media.elapsed() >= Duration::from_secs(30) {
            self.last_media = Instant::now();
            self.media_child = Command::new(core_exe()).args(["media", "update"]).creation_flags(NO_WINDOW).spawn().ok();
        }
    }

    /// 内存净化:后台跑 core power clean,界面显示 loading 直到进程结束
    pub fn start_clean(&mut self) {
        if self.cleaning() {
            return;
        }
        self.clean_started = Instant::now();
        self.clean_child = Command::new(core_exe()).args(["power", "clean"]).creation_flags(NO_WINDOW).spawn().ok();
    }

    pub fn cleaning(&mut self) -> bool {
        let busy = match &mut self.clean_child {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        };
        // 太快结束时也至少显示 0.9 秒,让用户看到反馈
        busy || self.clean_started.elapsed() < Duration::from_millis(900)
    }

    /// 操作之后马上重读一次文件
    /// 当前播放位置/总长(毫秒):按写入时间戳外推
    pub fn media_progress(&self) -> (i64, i64) {
        let num = |k: &str| Self::v(&self.timeline, k).parse::<i64>().unwrap_or(0);
        let (pos, dur, stamp, playing) = (num("MediaPosMs"), num("MediaDurMs"), num("MediaStampMs"), num("MediaPlaying"));
        if dur <= 0 {
            return (0, 0);
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        let p = if playing == 1 && stamp > 0 { pos + (now - stamp).max(0) } else { pos };
        (p.clamp(0, dur), dur)
    }

    pub fn refresh_soon(&mut self) {
        self.last_wallpaper = None;
        self.last_files = crate::log::ago(10);
    }
}
