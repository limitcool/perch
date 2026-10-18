use std::os::windows::process::CommandExt;
use std::time::{Instant, SystemTime};

use crate::live_theme::{self, ThemeStyle};
use crate::quotes_data::get_theme_quotes;
use crate::theme::{get_theme, ThemeConfig};

/// 主题 id 是 `&'static str`:同一个 id 只泄漏一次(主题数量很少,可以忽略)
fn intern_theme_id(id: &str) -> &'static str {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SET: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut g = SET.lock().unwrap_or_else(|e| e.into_inner());
    let set = g.get_or_insert_with(HashSet::new);
    if let Some(s) = set.get(id) {
        return s;
    }
    let leaked: &'static str = Box::leak(id.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerState {
    Idle,
    Running,
    Paused,
}

pub struct AppState {
    pub current_theme_name: &'static str,
    pub theme: ThemeConfig,
    /// 主题包声明的装饰:战术顶栏(chrome = "tactical")及其文字
    pub chrome_tactical: bool,
    pub chrome_title: String,
    pub chrome_sub: String,
    pub chrome_badges: Vec<String>,
    pub style: ThemeStyle,
    pub selector_open: bool,
    pub settings_open: bool,
    pub tools_open: bool,
    pub procs_open: bool,
    pub tools_tab: usize,
    pub settings_tab: usize,
    pub settings_scroll: usize,
    pub cal_open: bool,
    pub cal_tab: u8,
    pub notif_open: bool,
    pub notif_seen: u64,
    pub theme_fonts: bool,
    pub float_mode: bool,
    pub edit_layout: bool,
    pub drag: Option<(usize, f32, f32, f32, f32)>,
    pub seek_preview: Option<f32>,
    pub seek_hold: Option<(Instant, f32)>,
    pub theme_mtime: Option<SystemTime>,

    // 番茄钟
    pub timer_state: TimerState,
    pub timer_preset_mins: u32, // 25, 45, 15, 5
    pub timer_remaining_secs: u32,
    /// None = 专注;Some(false) = 短休息;Some(true) = 长休息
    pub timer_break: Option<bool>,
    pub pomo_count: u32,
    pub pomo_minutes: u32,
    pub pomo_streak: u32,
    pomo_date: String,
    pub last_timer_tick: Instant,
    pub last_timer_persist: Instant,

    // 饮水卡片
    pub water_ml: u32,
    pub water_target_ml: u32,

    // 待办卡片
    pub todos: Vec<(&'static str, bool)>,

    // 语录卡片
    pub quote_idx: usize,
    pub last_quote_switch: Instant,

    // 媒体播放器
    pub is_playing: bool,
    pub track_title: &'static str,
    pub track_artist: &'static str,
    pub volume: f32, // 0.0 ~ 1.0

    // 天气模拟/缓存
    pub weather_temp: i32,
    pub weather_desc: &'static str,
    pub weather_humidity: u32,
    pub weather_wind: &'static str,
}

impl AppState {
    pub fn new(default_theme: &'static str) -> Self {
        let theme = get_theme(default_theme);
        let mut app = Self {
            current_theme_name: default_theme,
            theme,
            chrome_tactical: false,
            chrome_title: String::new(),
            chrome_sub: String::new(),
            chrome_badges: Vec::new(),

            timer_state: TimerState::Idle,
            timer_preset_mins: crate::config::get().pomodoro.presets.first().copied().unwrap_or(25),
            timer_remaining_secs: crate::config::get().pomodoro.presets.first().copied().unwrap_or(25) * 60,
            timer_break: None,
            pomo_count: 0,
            pomo_minutes: 0,
            pomo_streak: 0,
            pomo_date: String::new(),
            last_timer_tick: Instant::now(),
            last_timer_persist: Instant::now(),

            water_ml: 750,
            water_target_ml: 2000,

            todos: vec![
                ("部署原生 Direct2D HUD 引擎", true),
                ("验证 8 大战术主题色盘与字体", true),
                ("测试 Win32 硬件级鼠标穿透与交互", false),
                ("优化后台空载 CPU 占用至 0.1%", false),
            ],

            quote_idx: 0,
            last_quote_switch: Instant::now(),

            is_playing: true,
            track_title: "Starry Sky (Instrumental)",
            track_artist: "Yuki Kajiura // FictionJunction",
            volume: 0.72,

            weather_temp: 24,
            weather_desc: "晴朗适战",
            weather_humidity: 48,
            weather_wind: "3.2m/s 东北风",
            style: ThemeStyle::default(),
            selector_open: false,
            settings_open: false,
            tools_open: false,
            procs_open: false,
            tools_tab: 0,
            settings_tab: 0,
            settings_scroll: 0,
            cal_open: false,
            cal_tab: 0,
            notif_open: false,
            notif_seen: 0,
            theme_fonts: load_options().0,
            float_mode: load_options().1,
            edit_layout: false,
            drag: None,
            seek_preview: None,
            seek_hold: None,
            theme_mtime: None,
        };
        app.reload_theme();
        app.load_pomo_stats();
        app.persist_timer();
        app
    }

    /// 从 perch-core 写出的 theme.toml 载入配色和样式包
    pub fn reload_theme(&mut self) {
        self.theme_mtime = live_theme::theme_mtime();
        if let Some((cfg, style, name)) = live_theme::load(get_theme(&self.current_theme_name)) {
            let key: &'static str = intern_theme_id(&name);
            if key != self.current_theme_name {
                self.quote_idx = 0;
            }
            self.current_theme_name = key;
            let pack = perch_data::theme_pack::load(key);
            self.chrome_tactical = pack.style.chrome == "tactical";
            self.chrome_title = pack.style.chrome_title.clone();
            self.chrome_sub = pack.style.chrome_sub.clone();
            self.chrome_badges = pack.style.chrome_badges.clone();
            self.theme = cfg;
            self.style = style;
            // 默认所有主题统一字体(和旧皮肤一致);主题专属字体可在托盘菜单里打开
            if !self.theme_fonts {
                self.style.font_main = "Microsoft YaHei UI".into();
                self.style.font_num = "Segoe UI Variable Display".into();
            }
            crate::window::THEME_FONTS_ON.store(self.theme_fonts, std::sync::atomic::Ordering::Relaxed);
            crate::window::FLOAT_ON.store(self.float_mode, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn toggle_theme_fonts(&mut self) {
        self.theme_fonts = !self.theme_fonts;
        save_options(self.theme_fonts, self.float_mode);
        self.reload_theme();
    }

    /// 窗口层级:true = 浮窗(始终置顶),false = 桌面小部件(置底)
    pub fn set_float(&mut self, on: bool) {
        self.float_mode = on;
        save_options(self.theme_fonts, self.float_mode);
        crate::window::FLOAT_ON.store(on, std::sync::atomic::Ordering::Relaxed);
    }

    /// 进度条显示用的比例:拖动中 / 刚松手等待播放器更新时用目标位置
    pub fn seek_display_frac(&self) -> Option<f32> {
        if let Some(f) = self.seek_preview {
            return Some(f);
        }
        match self.seek_hold {
            Some((until, f)) if Instant::now() < until => Some(f),
            _ => None,
        }
    }

    pub fn reload_theme_if_changed(&mut self) {
        let m = live_theme::theme_mtime();
        if m.is_some() && m != self.theme_mtime {
            self.reload_theme();
        }
    }

    pub fn set_theme(&mut self, theme_name: &'static str) {
        self.current_theme_name = theme_name;
        self.theme = get_theme(theme_name);
        self.quote_idx = 0;
    }

    pub fn cycle_theme(&mut self) {
        let themes: Vec<String> = perch_data::theme_pack::list().into_iter().map(|p| p.id).collect();
        if themes.is_empty() {
            return;
        }
        let next_idx = themes
            .iter()
            .position(|t| t == self.current_theme_name)
            .map(|i| (i + 1) % themes.len())
            .unwrap_or(0);
        // 由 perch-core 完成真正的换装(壁纸/BGM/theme.toml),HUD 随后热重载
        let _ = std::process::Command::new(live_theme::core_exe())
            .args(["theme", themes[next_idx].as_str()])
            .creation_flags(0x0800_0000)
            .spawn();
    }

    pub fn set_preset(&mut self, mins: u32) {
        self.timer_break = None;
        self.timer_preset_mins = mins;
        self.timer_remaining_secs = mins * 60;
        self.timer_state = TimerState::Idle;
        self.persist_timer();
    }

    pub fn toggle_timer(&mut self) {
        self.timer_state = match self.timer_state {
            TimerState::Running => TimerState::Paused,
            _ => TimerState::Running,
        };
        self.persist_timer();
    }

    pub fn reset_timer(&mut self) {
        self.timer_break = None;
        self.timer_state = TimerState::Idle;
        self.timer_remaining_secs = self.timer_preset_mins * 60;
        self.persist_timer();
    }

    /// 镜像番茄钟状态给 perch-core(自动换主题时不打断进行中的专注),与旧皮肤格式一致
    pub fn persist_timer(&mut self) {
        let v = match self.timer_state {
            TimerState::Idle => 0,
            TimerState::Running => 1,
            TimerState::Paused => 2,
        };
        let timer = perch_data::Timer { v: 1, state: v };
        let toml_path = perch_data::paths::timer_toml();
        let _ = perch_data::write_atomic(&toml_path, &timer);
        self.last_timer_persist = Instant::now();
    }

    /// 当前阶段的总秒数(进度环用)
    pub fn timer_total_secs(&self) -> u32 {
        match self.timer_break {
            None => self.timer_preset_mins * 60,
            Some(true) => crate::config::get().pomodoro.long_break * 60,
            Some(false) => crate::config::get().pomodoro.short_break * 60,
        }
    }

    /// 一个阶段结束:专注 → 记一个番茄并自动进入休息(每 N 个番茄后是长休息);休息 → 回到专注待命
    fn timer_finished(&mut self) {
        let cfg = crate::config::get().pomodoro;
        self.play_sound(&cfg.sound);
        match self.timer_break {
            None => {
                self.roll_pomo_date();
                self.pomo_count += 1;
                self.pomo_minutes += self.timer_preset_mins;
                self.pomo_streak += 1;
                let long = cfg.cycles_before_long > 0 && self.pomo_streak % cfg.cycles_before_long == 0;
                let mins = if long { cfg.long_break } else { cfg.short_break };
                self.timer_break = Some(long);
                self.timer_remaining_secs = mins * 60;
                self.timer_state = TimerState::Running; // 休息自动开始计时
                self.save_pomo_stats();
                let msg = perch_data::t_fmt(if long { "misc.pomo_done_long" } else { "misc.pomo_done_short" }, &[("n", &self.pomo_count.to_string()), ("mins", &mins.to_string())]);
                crate::data::core(&["notify", &perch_data::t("misc.pomo_done_title"), &msg]);
            }
            Some(_) => {
                self.timer_break = None;
                self.timer_state = TimerState::Idle;
                self.timer_remaining_secs = self.timer_preset_mins * 60;
                crate::data::core(&["notify", &perch_data::t("misc.break_over_title"), &perch_data::t("misc.break_over_body")]);
            }
        }
        self.persist_timer();
    }

    fn play_sound(&self, sound: &str) {
        unsafe {
            match sound {
                "off" => {}
                "system" | "" => {
                    let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(windows::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK);
                }
                path => {
                    use windows::core::PCWSTR;
                    let wide: Vec<u16> = crate::config::resolve_target(path).encode_utf16().chain(Some(0)).collect();
                    let _ = windows::Win32::Media::Audio::PlaySoundW(PCWSTR(wide.as_ptr()), None, windows::Win32::Media::Audio::SND_FILENAME | windows::Win32::Media::Audio::SND_ASYNC);
                }
            }
        }
    }

    // ---- 今日番茄统计(%APPDATA%\perch\pomodoro.json,跨天自动清零) ----
    fn pomo_path() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join("perch").join("pomodoro.json")
    }

    fn today() -> String {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    }

    fn roll_pomo_date(&mut self) {
        let t = Self::today();
        if self.pomo_date != t {
            self.pomo_date = t;
            self.pomo_count = 0;
            self.pomo_minutes = 0;
            self.pomo_streak = 0;
        }
    }

    fn load_pomo_stats(&mut self) {
        if let Ok(text) = std::fs::read_to_string(Self::pomo_path()) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                self.pomo_date = v["date"].as_str().unwrap_or("").to_string();
                self.pomo_count = v["count"].as_u64().unwrap_or(0) as u32;
                self.pomo_minutes = v["minutes"].as_u64().unwrap_or(0) as u32;
                self.pomo_streak = v["streak"].as_u64().unwrap_or(0) as u32;
            }
        }
        self.roll_pomo_date();
    }

    fn save_pomo_stats(&self) {
        let p = Self::pomo_path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, serde_json::json!({ "date": self.pomo_date, "count": self.pomo_count, "minutes": self.pomo_minutes, "streak": self.pomo_streak }).to_string());
    }

    pub fn add_water(&mut self, ml: u32) {
        self.water_ml = (self.water_ml + ml) % (self.water_target_ml + 250);
    }

    pub fn toggle_todo(&mut self, idx: usize) {
        if idx < self.todos.len() {
            self.todos[idx].1 = !self.todos[idx].1;
        }
    }

    pub fn next_quote(&mut self) {
        let (_, quotes) = get_theme_quotes(self.current_theme_name);
        if !quotes.is_empty() {
            self.quote_idx = (self.quote_idx + 1) % quotes.len();
            self.last_quote_switch = Instant::now();
        }
    }

    pub fn tick_1s(&mut self) {
        // 番茄钟倒计时
        if self.timer_state == TimerState::Running {
            if self.timer_remaining_secs > 0 {
                self.timer_remaining_secs -= 1;
            } else {
                self.timer_finished();
            }
            if self.last_timer_persist.elapsed().as_secs() >= 300 {
                self.persist_timer();
            }
        }

        // 语录 15 秒轮播
        if self.last_quote_switch.elapsed().as_secs() >= 15 {
            self.next_quote();
        }
    }
}

fn options_path() -> std::path::PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("perch").join("options.txt")
}

/// (主题专属字体, 浮窗置顶)
fn load_options() -> (bool, bool) {
    let t = std::fs::read_to_string(options_path()).unwrap_or_default();
    let on = |k: &str| t.lines().any(|l| l.trim() == format!("{}=1", k));
    (on("theme_fonts"), on("float"))
}

fn save_options(theme_fonts: bool, float: bool) {
    let p = options_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, format!("theme_fonts={}\nfloat={}\n", theme_fonts as i32, float as i32));
}
