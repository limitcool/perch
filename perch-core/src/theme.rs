//! perch-core::theme: 数据驱动主题管理、自动轮换与台词语录渲染

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::util::{auto_json, local_date_weekday, timer_active, user_config, set_windows_wallpaper};
use crate::wallpaper::{load_modes, save_modes, current_wallpaper_path, system_uses_light_theme};
use crate::media::{bgm_play, bgm_auto_enabled};
use perch_data::theme_pack::{self, ThemePack};

fn bgm_dir() -> PathBuf {
    perch_data::paths::data_dir()
}

pub fn current_theme_name() -> String {
    let toml_p = perch_data::paths::theme_toml();
    if let Ok(tt) = perch_data::read::<perch_data::ThemeToml>(&toml_p) {
        if !tt.name.is_empty() {
            return tt.name;
        }
    }
    "default".to_string()
}

pub fn get_theme(name: &str) -> ThemePack {
    theme_pack::load(name)
}

/// config.toml 里的 [theme.<name>] accent / sub / bg 覆盖(格式 "r,g,b" 或 "r,g,b,a")
pub fn apply_theme_override(pack: &mut ThemePack) {
    let all = user_config();
    let Some(t) = all.get("theme").and_then(|t| t.get(&pack.id)).and_then(|t| t.as_table()) else {
        return;
    };
    let parse = |key: &str| -> Option<(u8, u8, u8)> {
        let v: Vec<u8> = t.get(key)?.as_str()?.split(',').filter_map(|p| p.trim().parse::<u8>().ok()).collect();
        if v.len() >= 3 { Some((v[0], v[1], v[2])) } else { None }
    };
    if let Some((r, g, b)) = parse("accent") {
        pack.colors.accent = format!("#{:02X}{:02X}{:02X}FF", r, g, b);
        pack.colors.stroke = format!("#{:02X}{:02X}{:02X}46", r, g, b);
        pack.colors.item_hover = format!("#{:02X}{:02X}{:02X}91", r, g, b);
        pack.colors.pill_bg = format!("#{:02X}{:02X}{:02X}DC", r, g, b);
    }
    if let Some((r, g, b)) = parse("sub") {
        pack.colors.sub = format!("#{:02X}{:02X}{:02X}FF", r, g, b);
    }
    if let Some((r, g, b)) = parse("bg") {
        pack.colors.bg = format!("#{:02X}{:02X}{:02X}D4", r, g, b);
        pack.colors.dock_bg = format!("#{:02X}{:02X}{:02X}D2", r, g, b);
    }
}

fn pseudo_rand(n: usize) -> usize {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    ((t ^ (t >> 16)) as usize) % n.max(1)
}

/// 每日轮换检测:只在今天第一次检查时触发
pub fn autotheme_tick() {
    let modes = load_modes();
    let mode = modes.theme_auto.trim();
    if mode == "0" || mode == "fixed" {
        return;
    }
    let cfg = auto_json("theme_auto.json");
    if cfg.is_null() {
        return;
    }
    let (date, wd) = local_date_weekday();
    let period = if system_uses_light_theme().unwrap_or(true) { "day" } else { "night" };
    let key = if mode == "period" { format!("{}/{}", date, period) } else { date.clone() };
    let state_path = bgm_dir().join("theme_auto_state.json");
    let state = auto_json("theme_auto_state.json");
    if state["key"].as_str() == Some(key.as_str()) {
        return;
    }
    if timer_active() {
        return; // 番茄钟在跑:不打断,稍后再试
    }
    let cur = current_theme_name();
    let wd_names = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    let desired: Option<String> = match mode {
        "weekday" => cfg["weekday"][wd_names[wd]].as_str().map(|s| s.to_string()),
        "random" => {
            let pool: Vec<String> = cfg["pool"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).filter(|t| *t != cur).collect())
                .unwrap_or_default();
            if pool.is_empty() { None } else { Some(pool[pseudo_rand(pool.len())].clone()) }
        }
        "period" => {
            let list: Vec<String> = cfg["period"][period]
                .as_array()
                .map(|a: &Vec<serde_json::Value>| {
                    a.iter()
                        .filter_map(|x: &serde_json::Value| x.as_str().map(|s: &str| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            if list.is_empty() { None } else { Some(list[wd % list.len()].clone()) }
        }
        _ => None,
    };
    // 先落状态再切换,避免并发的轮询进程重复切换
    let _ = fs::write(&state_path, serde_json::json!({ "key": key, "theme": desired.clone().unwrap_or_default() }).to_string());
    if let Some(t) = desired {
        let all_themes = theme_pack::list();
        if all_themes.iter().any(|p| p.id == t) && t != cur {
            switch_theme(&t);
        }
    }
}

pub fn set_auto_mode(mode: &str) {
    if !["fixed", "weekday", "random", "period"].contains(&mode) {
        return;
    }
    let mut cfg = auto_json("theme_auto.json");
    if cfg.is_null() {
        return;
    }
    cfg["mode"] = serde_json::json!(mode);
    let _ = fs::write(bgm_dir().join("theme_auto.json"), cfg.to_string());
    let mut m = load_modes();
    m.theme_auto = mode.to_string();
    save_modes(&m);
    let _ = fs::remove_file(bgm_dir().join("theme_auto_state.json"));
    autotheme_tick();
}

#[allow(dead_code)]
pub fn ambient_level() -> String {
    let p = perch_data::paths::modes_toml();
    let Ok(t) = fs::read_to_string(p) else { return "standard".to_string() };
    for line in t.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("ambient") {
            if let Some((_, val)) = rest.split_once('=') {
                let v = val.trim().trim_matches('"').trim_matches('\'');
                if ["off", "low", "standard"].contains(&v) {
                    return v.to_string();
                }
            }
        }
    }
    "standard".to_string()
}

pub fn ambient_set_level(level: &str) {
    if !["off", "low", "standard"].contains(&level) {
        return;
    }
    let p = perch_data::paths::modes_toml();
    let cur = fs::read_to_string(&p).unwrap_or_default();
    let mut new_lines = Vec::new();
    let mut replaced = false;
    for line in cur.lines() {
        if line.trim().starts_with("ambient") {
            new_lines.push(format!("ambient = \"{}\"", level));
            replaced = true;
        } else {
            new_lines.push(line.to_string());
        }
    }
    if !replaced {
        new_lines.push(format!("ambient = \"{}\"", level));
    }
    let _ = fs::write(&p, new_lines.join("\n") + "\n");
}

#[allow(dead_code)]
pub fn export_all_themes_toml() {
    // 兼容空实现: 新规范下内置主题只从二进制读取，不再覆盖写旧目录
}

pub fn switch_theme(name: &str) {
    let mut pack = get_theme(name);
    apply_theme_override(&mut pack);

    // 壁纸同步
    {
        let wp_prefix = if !pack.wallpaper.prefix.is_empty() {
            &pack.wallpaper.prefix
        } else {
            &pack.id
        };
        let wp = current_wallpaper_path(wp_prefix)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                perch_data::paths::wallpapers_dir()
                    .join(format!("{}.png", wp_prefix))
                    .to_string_lossy()
                    .to_string()
            });
        // 没有壁纸文件时保持用户当前壁纸不变(开箱即用,壁纸是可选的)
        if Path::new(&wp).is_file() {
            set_windows_wallpaper(&wp);
        }
    }

    // 写当前生效的主题 TOML
    let theme_toml = pack.to_theme_toml();
    let _ = perch_data::write_atomic(perch_data::paths::theme_toml(), &theme_toml);

    generate_theme_quotes(&pack.id);
    if bgm_auto_enabled() {
        bgm_play(&pack.id);
    }
}

#[derive(Serialize, Deserialize, Default)]
struct QuotesState {
    theme: String,
    index: usize,
}

pub fn get_theme_quotes_data(theme: &str) -> (String, Vec<String>) {
    let pack = get_theme(theme);
    (pack.quotes.speaker, pack.quotes.lines)
}

pub fn update_quote_text(theme_opt: Option<&str>, next: bool) {
    let state_path = perch_data::paths::quotes_state_json();
    let mut state: QuotesState = fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(QuotesState {
            theme: "default".to_string(),
            index: 0,
        });

    if let Some(t) = theme_opt {
        if !t.is_empty() {
            state.theme = t.to_string();
            state.index = 0;
        }
    }

    let (speaker, quotes) = get_theme_quotes_data(&state.theme);
    if quotes.is_empty() {
        return;
    }

    if next {
        state.index = (state.index + 1) % quotes.len();
    } else if state.index >= quotes.len() {
        state.index = 0;
    }

    let current_quote = &quotes[state.index];

    // 保存状态
    let state_json = serde_json::to_string_pretty(&state).unwrap_or_default();
    let _ = fs::write(&state_path, &state_json);

    // 写入 quote.toml
    let q = perch_data::Quote {
        v: 1,
        speaker,
        text: localize_quote(current_quote),
    };
    let _ = perch_data::write_atomic(perch_data::paths::quote_toml(), &q);
}

/// 语录格式是 "原文#CRLF#译文"。英文界面下,如果原文本身是英文(不含 CJK),只显示原文,避免第二行冒出中文译文
fn localize_quote(q: &str) -> String {
    if perch_data::i18n::current_language() != "en" {
        return q.to_string();
    }
    match q.split_once("#CRLF#") {
        Some((first, _)) if !first.chars().any(|c| ('\u{3400}'..='\u{9FFF}').contains(&c) || ('\u{3040}'..='\u{30FF}').contains(&c)) => first.trim().to_string(),
        _ => q.to_string(),
    }
}

pub fn generate_theme_quotes(theme: &str) {
    update_quote_text(Some(theme), false);
}
