//! 用户配置:安装目录下的 config.toml(缺失时按当前习惯自动生成)。
//! 运行时根目录取自 exe 所在位置,不再写死盘符/路径。

use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

/// 运行时根目录:委托给 perch_data::paths::root()。
pub fn root() -> &'static std::path::Path {
    perch_data::paths::root()
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct DockEntry {
    pub name: String,
    /// dock_icons\<icon>.png 的名字,或一个图片路径;留空则从 target 的 exe 图标提取
    pub icon: String,
    pub target: String,
    pub args: String,
    /// 单击的默认动作:"auto"(默认)/ "single"(已运行就切到窗口)/ "multi"(总是新开)。Shift+单击或中键走另一种
    pub mode: String,
    /// 判断"是否在运行"用的进程名(留空取 target 的文件名),例如 wt.exe 对应 WindowsTerminal.exe
    #[serde(rename = "match")]
    pub match_: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Pomodoro {
    pub presets: Vec<u32>,
    pub short_break: u32,
    pub long_break: u32,
    pub cycles_before_long: u32,
    /// "system"(系统提示音)/ "off" / 一个 .wav 路径
    pub sound: String,
}

impl Default for Pomodoro {
    fn default() -> Self {
        Self { presets: vec![25, 45, 15, 5], short_break: 5, long_break: 15, cycles_before_long: 4, sound: "system".into() }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct SystemCfg {
    pub disks: Vec<String>,
}

impl Default for SystemCfg {
    fn default() -> Self {
        Self { disks: vec![r"C:\".into()] }
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Display {
    /// 0 = 主显示器;1、2… = 按系统枚举顺序选择其他显示器
    pub monitor: u32,
}

/// 全局热键,写法如 "Win+Alt+S"、"Ctrl+Shift+Space"(修饰键 Win Ctrl Alt Shift + 字母 / 数字 / F1-F24 / Space / Enter / Tab / Esc)
#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Hotkeys {
    pub toggle: String,
    pub float: String,
    pub search: String,
    pub tools: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self { toggle: "Win+Alt+H".into(), float: "Win+Alt+L".into(), search: "Win+Alt+S".into(), tools: "Win+Alt+V".into() }
    }
}

/// 日历订阅:ics 可以是本地文件路径或 https 网址(Google 日历的"私人 iCal 地址"、Outlook 发布的 ICS 等)
#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct CalendarCfg {
    pub ics: Vec<String>,
    /// 事件开始前多少分钟提醒(0 = 不提醒)
    pub notify_min: u32,
}

impl Default for CalendarCfg {
    fn default() -> Self {
        Self { ics: Vec::new(), notify_min: 10 }
    }
}

/// 笔记 / Obsidian:收集箱文件、库路径、日记目录覆盖、是否把今日日记里的任务并进待办
#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct NotesCfg {
    pub file: String,
    pub vault: String,
    /// 相对库根目录的日记文件夹;留空取库里 daily-notes.json 的设置(那个设置可能过期,如文件夹被改名)
    pub daily_folder: String,
    pub sync_tasks: bool,
}

impl Default for NotesCfg {
    fn default() -> Self {
        Self { file: String::new(), vault: String::new(), daily_folder: String::new(), sync_tasks: true }
    }
}

/// 音频:快捷页的输出设备列表里要隐藏的设备(名字包含这些文字就隐藏,不区分大小写)
#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AudioCfg {
    pub hide: Vec<String>,
}

impl Default for AudioCfg {
    fn default() -> Self {
        Self { hide: vec!["Steam Streaming".into()] }
    }
}

/// 温度提醒:GPU 温度(NVML)达到阈值时发通知,冷却时间内不重复
#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Alert {
    /// 0 = 关闭
    pub gpu_temp: u32,
    pub cooldown_min: u32,
}

impl Default for Alert {
    fn default() -> Self {
        Self { gpu_temp: 85, cooldown_min: 10 }
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Widgets {
    /// 隐藏的卡片名:battery timer remind todo media water clock weather system controls dock quotes
    pub hidden: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Thresholds {
    pub battery_low: u8,
    pub battery_warn: u8,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            battery_low: 20,
            battery_warn: 40,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Intervals {
    pub battery_sec: u64,
    pub weather_sec: u64,
    pub wallpaper_sec: u64,
    pub app_index_sec: u64,
    pub ics_min: u64,
    pub clash_sec: u64,
}

impl Default for Intervals {
    fn default() -> Self {
        Self {
            battery_sec: 120,
            weather_sec: 900,
            wallpaper_sec: 60,
            app_index_sec: 300,
            ics_min: 30,
            clash_sec: 10,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct DockSettings {
    pub multi_apps: Vec<String>,
}

impl Default for DockSettings {
    fn default() -> Self {
        Self {
            multi_apps: vec![
                "explorer.exe".into(),
                "windowsterminal.exe".into(),
                "cmd.exe".into(),
                "powershell.exe".into(),
                "pwsh.exe".into(),
            ],
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct UiConfig {
    pub language: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        // 旧版本的 config.toml 没有 [ui] 段:保持原来的中文界面;新生成的默认配置写的是 "auto"(跟随系统)
        Self { language: "zh-CN".into() }
    }
}

/// 首次生成 config.toml 时用:Dock 默认条目的名字按系统语言本地化
fn default_toml() -> String {
    perch_data::i18n::init_from_config("auto");
    if perch_data::i18n::current_language() == "en" {
        return DEFAULT_TOML_EN.to_string();
    }
    DEFAULT_TOML
        .replace("name = \"终端\"", &format!("name = \"{}\"", perch_data::t("misc.dock_terminal")))
        .replace("name = \"文件资源管理器\"", &format!("name = \"{}\"", perch_data::t("misc.dock_explorer")))
        .replace("name = \"记事本\"", &format!("name = \"{}\"", perch_data::t("misc.dock_notepad")))
        .replace("name = \"计算器\"", &format!("name = \"{}\"", perch_data::t("misc.dock_calc")))
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Config {
    pub calendar: CalendarCfg,
    pub notes: NotesCfg,
    pub audio: AudioCfg,
    pub hotkeys: Hotkeys,
    pub alert: Alert,
    pub widgets: Widgets,
    pub pomodoro: Pomodoro,
    pub system: SystemCfg,
    pub display: Display,
    pub thresholds: Thresholds,
    pub intervals: Intervals,
    pub ui: UiConfig,
    #[serde(alias = "dock_settings", alias = "dock_config")]
    pub dock_settings: DockSettings,
    pub dock: Vec<DockEntry>,
}

const DEFAULT_TOML: &str = r##"# Perch 配置(改完在托盘菜单点"重新载入配置",或重启 HUD)
# 路径用单引号可以免去反斜杠转义;每个 [[dock]] 是 Dock 上的一个应用,最多 24 个。

[ui]
language = "auto"             # "auto"(跟随系统) / "zh-CN"(简体中文) / "en"(English)

[thresholds]
battery_low = 20              # 鼠标/设备电量过低警告阈值(%)
battery_warn = 40             # 鼠标/设备电量提醒阈值(%)

[intervals]
battery_sec = 120             # 电量采集刷新间隔(秒)
weather_sec = 900             # 天气采集刷新间隔(秒, 15 分钟)
wallpaper_sec = 60            # 昼夜/壁纸同步检测间隔(秒)
app_index_sec = 300           # 启动器搜索索引重建间隔(秒)
ics_min = 30                  # 日历 ICS 订阅拉取间隔(分钟)
clash_sec = 10                # Clash 状态轮询间隔(秒)

[dock_settings]
multi_apps = ["explorer.exe", "windowsterminal.exe", "cmd.exe", "powershell.exe", "pwsh.exe"] # 默认总是新开窗口的进程

[pomodoro]
presets = [25, 45, 15, 5]   # 最多 4 个专注时长(分钟)
short_break = 5             # 每个番茄之后的短休息
long_break = 15             # 每满 cycles_before_long 个番茄之后的长休息
cycles_before_long = 4
sound = "system"            # "system" 系统提示音 / "off" 静音 / 'C:\path\to\sound.wav'

[system]
disks = ['C:\']             # 系统卡片里显示的磁盘(最多两个,如 ['C:\', 'D:\'])

[display]
monitor = 0                 # 0 = 主显示器,1/2... = 其他显示器

# 启动条:name 显示名;target 可执行文件/文件夹/URL(系统自带程序直接写名字即可);
# icon 留空则从 target 提取图标;支持绝对路径或安装目录下的相对路径
[[dock]]
name = "Edge"
target = "msedge.exe"

[[dock]]
name = "终端"
target = "wt.exe"

[[dock]]
name = "文件资源管理器"
target = "explorer.exe"

[[dock]]
name = "记事本"
target = "notepad.exe"

[[dock]]
name = "计算器"
target = "calc.exe"

# 启动器自定义 AI 引擎示例 (默认已内置 ChatGPT / Google AI / Claude / Gemini)
# [[launcher.ai]]
# name = "ChatGPT"
# url = "https://chatgpt.com/?q=%s"
# copy = false
#
# [[launcher.ai]]
# name = "Claude"
# url = "https://claude.ai/new?q=%s"
# copy = false
"##;

/// 与 DEFAULT_TOML 内容一致,注释和 Dock 名称是英文(英文界面首次生成配置时用)
const DEFAULT_TOML_EN: &str = r##"# Perch configuration (edit, then use "Reload Configuration" in the tray menu, or restart the HUD)
# Use single-quoted paths to avoid escaping backslashes; each [[dock]] is one app on the Dock (max 24).

[ui]
language = "auto"             # "auto" (follow system) / "zh-CN" (Simplified Chinese) / "en" (English)

[thresholds]
battery_low = 20              # low-battery warning threshold for the mouse/device (%)
battery_warn = 40             # battery reminder threshold (%)

[intervals]
battery_sec = 120             # battery refresh interval (seconds)
weather_sec = 900             # weather refresh interval (seconds, 15 minutes)
wallpaper_sec = 60            # day/night wallpaper sync check interval (seconds)
app_index_sec = 300           # launcher search index rebuild interval (seconds)
ics_min = 30                  # calendar ICS subscription fetch interval (minutes)
clash_sec = 10                # Clash status polling interval (seconds)

[dock_settings]
multi_apps = ["explorer.exe", "windowsterminal.exe", "cmd.exe", "powershell.exe", "pwsh.exe"] # processes that always open a new window

[pomodoro]
presets = [25, 45, 15, 5]   # up to 4 focus lengths (minutes)
short_break = 5             # short break after each pomodoro
long_break = 15             # long break after every cycles_before_long pomodoros
cycles_before_long = 4
sound = "system"            # "system" = system sound / "off" = mute / 'C:\path\to\sound.wav'

[system]
disks = ['C:\']             # disks shown on the system card (at most two, e.g. ['C:\', 'D:\'])

[display]
monitor = 0                 # 0 = primary monitor, 1/2... = other monitors

# Dock: name = label; target = executable / folder / URL (system programs can be written by name);
# leave icon empty to extract it from target; absolute paths or paths relative to the install folder work
[[dock]]
name = "Edge"
target = "msedge.exe"

[[dock]]
name = "Terminal"
target = "wt.exe"

[[dock]]
name = "File Explorer"
target = "explorer.exe"

[[dock]]
name = "Notepad"
target = "notepad.exe"

[[dock]]
name = "Calculator"
target = "calc.exe"

# Custom AI engines for the launcher (ChatGPT / Google AI / Claude / Gemini are built in)
# [[launcher.ai]]
# name = "ChatGPT"
# url = "https://chatgpt.com/?q=%s"
# copy = false
#
# [[launcher.ai]]
# name = "Claude"
# url = "https://claude.ai/new?q=%s"
# copy = false
"##;

static CFG: RwLock<Option<Config>> = RwLock::new(None);

pub fn path() -> PathBuf {
    root().join("config.toml")
}

fn parse(text: &str) -> Config {
    match toml::from_str::<Config>(text) {
        Ok(c) => c,
        Err(e) => {
            crate::log::log(&format!("config.toml parse error, using defaults: {}", e));
            toml::from_str::<Config>(DEFAULT_TOML).unwrap_or_default()
        }
    }
}

/// 读取(不存在则先写入默认文件)并缓存
pub fn reload() {
    let p = path();
    if !p.exists() {
        let _ = std::fs::write(&p, default_toml());
    }
    let text = std::fs::read_to_string(&p).unwrap_or_else(|_| DEFAULT_TOML.to_string());
    let mut cfg = parse(&text);
    perch_data::i18n::init_from_config(&cfg.ui.language);
    cfg.dock.truncate(24);
    cfg.pomodoro.presets.truncate(4);
    if cfg.pomodoro.presets.is_empty() {
        cfg.pomodoro.presets = Pomodoro::default().presets;
    }
    if let Ok(mut g) = CFG.write() {
        *g = Some(cfg);
    }
}

pub fn get() -> Config {
    if let Ok(g) = CFG.read() {
        if let Some(c) = g.as_ref() {
            return c.clone();
        }
    }
    reload();
    CFG.read().ok().and_then(|g| g.clone()).unwrap_or_default()
}

/// 解析 target: 支持相对安装目录或绝对路径
pub fn resolve_target(t: &str) -> String {
    perch_data::paths::resolve_resource(t).to_string_lossy().to_string()
}

/// 取图标用的真实路径:裸程序名(msedge.exe / notepad.exe)依次在 App Paths 注册表和 PATH 里找;
/// 找不到就原样返回(图标取不到时 Dock 画占位)
pub fn icon_source(t: &str) -> String {
    let local = resolve_target(t);
    // 带路径 / 盘符 / @Resources 前缀的,或安装目录下确实存在的:直接用
    if t.contains('\\') || t.contains('/') || t.contains(':') || t.starts_with('@') || std::path::Path::new(&local).exists() {
        return local;
    }
    // 裸程序名:注册表 App Paths → PATH 搜索
    app_paths_lookup(t).or_else(|| search_path(t)).unwrap_or(local)
}

fn app_paths_lookup(name: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};
    let sub: Vec<u16> = format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{}", name).encode_utf16().chain(Some(0)).collect();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut buf = vec![0u16; 520];
        let mut size = (buf.len() * 2) as u32;
        let r = unsafe { RegGetValueW(hive, PCWSTR(sub.as_ptr()), PCWSTR::null(), RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)) };
        if r.is_ok() {
            let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            let p = String::from_utf16_lossy(&buf[..len]).trim_matches('"').to_string();
            if std::path::Path::new(&p).exists() {
                return Some(p);
            }
        }
    }
    None
}

fn search_path(name: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::SearchPathW;
    let wname: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut buf = vec![0u16; 520];
    let n = unsafe { SearchPathW(PCWSTR::null(), PCWSTR(wname.as_ptr()), PCWSTR::null(), Some(&mut buf), None) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..n]))
}

pub fn dock_count() -> usize {
    get().dock.len().max(1)
}

// ---------------- 面板写回(toml_edit:保留注释和未识别的段) ----------------
use toml_edit::{DocumentMut, Item, Table, Value};

/// 读-改-写 config.toml 并重新载入。文件解析失败时不写,避免覆盖用户手写内容。
pub fn edit(f: impl FnOnce(&mut DocumentMut)) {
    let p = path();
    let text = std::fs::read_to_string(&p).unwrap_or_else(|_| DEFAULT_TOML.to_string());
    let mut doc = match text.parse::<DocumentMut>() {
        Ok(d) => d,
        Err(e) => {
            crate::log::log(&format!("config edit skipped, config.toml invalid: {}", e));
            return;
        }
    };
    f(&mut doc);
    // 每次面板写回前留一份上一版,改坏了能手动找回
    let _ = std::fs::copy(&p, p.with_extension("toml.bak"));
    let tmp = p.with_extension("toml.tmp");
    if std::fs::write(&tmp, doc.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
    reload();
}

/// 设置 [section].key,沿用原值后面的行内注释
pub fn set_value(doc: &mut DocumentMut, section: &str, key: &str, new: Value) {
    let root = doc.as_table_mut();
    if !root.contains_key(section) {
        root.insert(section, Item::Table(Table::new()));
    }
    let Some(t) = root.get_mut(section).and_then(|i| i.as_table_mut()) else { return };
    if let Some(v) = t.get_mut(key).and_then(|i| i.as_value_mut()) {
        let decor = v.decor().clone();
        *v = new;
        *v.decor_mut() = decor;
    } else {
        t.insert(key, Item::Value(new));
    }
}

/// 读取 [section].key 的字符串值(没有则空串)
pub fn raw_str(section: &str, key: &str) -> String {
    let text = std::fs::read_to_string(path()).unwrap_or_default();
    text.parse::<toml::Table>()
        .ok()
        .and_then(|t| t.get(section).and_then(|s| s.get(key)).and_then(|v| v.as_str().map(|s| s.to_string())))
        .unwrap_or_default()
}

pub fn dock_add(doc: &mut DocumentMut, name: &str, target: &str) {
    let root = doc.as_table_mut();
    if !root.contains_key("dock") {
        root.insert("dock", Item::ArrayOfTables(Default::default()));
    }
    if let Some(arr) = root.get_mut("dock").and_then(|i| i.as_array_of_tables_mut()) {
        if arr.len() >= 24 {
            return;
        }
        let mut t = Table::new();
        t.insert("name", toml_edit::value(name));
        t.insert("icon", toml_edit::value(""));
        // 单引号字符串免转义反斜杠(路径里不会有单引号)
        let literal = if !target.contains('\'') && !target.contains('\n') { format!("'{}'", target).parse::<Value>().ok() } else { None };
        t.insert("target", Item::Value(literal.unwrap_or_else(|| Value::from(target))));
        arr.push(t);
    }
}

pub fn dock_set(doc: &mut DocumentMut, idx: usize, key: &str, new: Value) {
    if let Some(arr) = doc.as_table_mut().get_mut("dock").and_then(|i| i.as_array_of_tables_mut()) {
        if let Some(t) = arr.get_mut(idx) {
            if let Some(v) = t.get_mut(key).and_then(|i| i.as_value_mut()) {
                let decor = v.decor().clone();
                *v = new;
                *v.decor_mut() = decor;
            } else {
                t.insert(key, Item::Value(new));
            }
        }
    }
}

pub fn dock_remove(doc: &mut DocumentMut, idx: usize) {
    if let Some(arr) = doc.as_table_mut().get_mut("dock").and_then(|i| i.as_array_of_tables_mut()) {
        if idx < arr.len() {
            arr.remove(idx);
        }
    }
}

/// 把第 idx 个与相邻项交换(dir = -1 上移 / +1 下移)
pub fn dock_move(doc: &mut DocumentMut, idx: usize, dir: i32) {
    if let Some(arr) = doc.as_table_mut().get_mut("dock").and_then(|i| i.as_array_of_tables_mut()) {
        let j = idx as i32 + dir;
        if j < 0 || j as usize >= arr.len() || idx >= arr.len() {
            return;
        }
        let mut tables: Vec<Table> = arr.iter().cloned().collect();
        tables.swap(idx, j as usize);
        arr.clear();
        for t in tables {
            arr.push(t);
        }
    }
}

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn zh_and_en_templates_have_the_same_settings() {
        let zh: toml::Value = toml::from_str(DEFAULT_TOML).unwrap();
        let en: toml::Value = toml::from_str(DEFAULT_TOML_EN).unwrap();
        // 除 Dock 名称外完全一致
        let strip = |mut v: toml::Value| {
            if let Some(arr) = v.get_mut("dock").and_then(|d| d.as_array_mut()) {
                for e in arr {
                    if let Some(t) = e.as_table_mut() {
                        t.remove("name");
                    }
                }
            }
            v
        };
        assert_eq!(strip(zh), strip(en));
    }
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn bare_system_program_names_resolve_to_files() {
        for n in ["notepad.exe", "explorer.exe", "calc.exe"] {
            let p = icon_source(n);
            println!("{} -> {}", n, p);
            assert!(std::path::Path::new(&p).is_file(), "{} did not resolve (got {})", n, p);
        }
    }
}
