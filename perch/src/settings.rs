//! 设置面板:常规 / 番茄钟 / Dock / 系统 / 关于。
//! 所有改动写回 config.toml(toml_edit 保留注释),并即时生效。

use std::sync::atomic::Ordering;
use toml_edit::Value;
use windows::core::Result;

use crate::cards::{self, clip, sh, sw, View, C, L, P, R};
use crate::config;
use crate::data::{self, Hub};
use crate::state::AppState;
use crate::window::{RectBox, RELAYOUT};

const W: f32 = 520.0;
const H: f32 = 440.0;
/// 内容区原点(面板内坐标)
const CX: f32 = 132.0;
const CY: f32 = 56.0;
const DOCK_ROWS: usize = 7;

pub fn rect() -> (f32, f32, f32, f32) {
    ((sw() - W) / 2.0, (sh() - H) / 2.0 - 40.0, W, H)
}

pub fn region() -> (f32, f32, f32, f32, f32) {
    let r = rect();
    (r.0, r.1, r.2, r.3, 12.0)
}

const TAB_IDS: [&str; 7] = ["st_tab0", "st_tab1", "st_tab2", "st_tab3", "st_tab4", "st_tab5", "st_tab6"];

fn settings_tab_label(i: usize) -> String {
    match i {
        0 => perch_data::t("settings.tab_general"),
        1 => perch_data::t("settings.tab_cards"),
        2 => perch_data::t("settings.tab_pomodoro"),
        3 => perch_data::t("settings.tab_dock"),
        4 => perch_data::t("settings.tab_system"),
        5 => perch_data::t("settings.tab_hotkeys"),
        _ => perch_data::t("settings.tab_about"),
    }
}

const WG: [&str; 12] = ["st_w0", "st_w1", "st_w2", "st_w3", "st_w4", "st_w5", "st_w6", "st_w7", "st_w8", "st_w9", "st_w10", "st_w11"];
const HK: [&str; 4] = ["st_hk0", "st_hk1", "st_hk2", "st_hk3"];
const PM_DN: [&str; 8] = ["pm_d0", "pm_d1", "pm_d2", "pm_d3", "pm_d4", "pm_d5", "pm_d6", "pm_d7"];
const PM_UP: [&str; 8] = ["pm_u0", "pm_u1", "pm_u2", "pm_u3", "pm_u4", "pm_u5", "pm_u6", "pm_u7"];
const DK_UP: [&str; DOCK_ROWS] = ["st_du0", "st_du1", "st_du2", "st_du3", "st_du4", "st_du5", "st_du6"];
const DK_DN: [&str; DOCK_ROWS] = ["st_dd0", "st_dd1", "st_dd2", "st_dd3", "st_dd4", "st_dd5", "st_dd6"];
const DK_DEL: [&str; DOCK_ROWS] = ["st_dx0", "st_dx1", "st_dx2", "st_dx3", "st_dx4", "st_dx5", "st_dx6"];
const DK_MODE: [&str; DOCK_ROWS] = ["st_dm0", "st_dm1", "st_dm2", "st_dm3", "st_dm4", "st_dm5", "st_dm6"];

const GENERAL_BOXES: [(&str, f32, f32, f32, f32); 7] = [
    ("st_float", 0.0, 0.0, 182.0, 28.0),
    ("st_font", 194.0, 0.0, 182.0, 28.0),
    ("st_icons", 0.0, 36.0, 182.0, 28.0),
    ("st_edit", 194.0, 36.0, 182.0, 28.0),
    ("st_reset", 0.0, 72.0, 182.0, 28.0),
    ("st_wpnext", 194.0, 72.0, 182.0, 28.0),
    ("st_wpdir", 0.0, 108.0, 182.0, 28.0),
];
const MON_Y: f32 = 160.0;

const ABOUT_BOXES: [(&str, f32, f32, f32, f32); 6] = [
    ("st_cfg", 0.0, 150.0, 182.0, 28.0),
    ("st_log", 194.0, 150.0, 182.0, 28.0),
    ("st_reload", 0.0, 186.0, 182.0, 28.0),
    ("st_bak", 0.0, 230.0, 182.0, 28.0),
    ("st_rest", 194.0, 230.0, 182.0, 28.0),
    ("st_bakdir", 0.0, 266.0, 182.0, 28.0),
];

fn settings_tabs() -> Vec<(&'static str, String)> {
    TAB_IDS.iter().enumerate().map(|(i, &id)| (id, settings_tab_label(i))).collect()
}

fn general_buttons() -> Vec<(&'static str, String, f32, f32, f32, f32)> {
    vec![
        ("st_float", perch_data::t("settings2.btn_float"), 0.0, 0.0, 182.0, 28.0),
        ("st_font", perch_data::t("settings2.btn_font"), 194.0, 0.0, 182.0, 28.0),
        ("st_icons", perch_data::t("settings2.btn_icons"), 0.0, 36.0, 182.0, 28.0),
        ("st_edit", perch_data::t("settings.edit_layout"), 194.0, 36.0, 182.0, 28.0),
        ("st_reset", perch_data::t("settings.reset_layout"), 0.0, 72.0, 182.0, 28.0),
        ("st_wpnext", perch_data::t("settings.wp_next"), 194.0, 72.0, 182.0, 28.0),
        ("st_wpdir", perch_data::t("settings.wp_dir"), 0.0, 108.0, 182.0, 28.0),
    ]
}

fn about_buttons() -> Vec<(&'static str, String, f32, f32, f32, f32)> {
    vec![
        ("st_cfg", perch_data::t("settings.open_cfg"), 0.0, 150.0, 182.0, 28.0),
        ("st_log", perch_data::t("settings.open_log"), 194.0, 150.0, 182.0, 28.0),
        ("st_reload", perch_data::t("settings.reload_cfg"), 0.0, 186.0, 182.0, 28.0),
        ("st_bak", perch_data::t("settings.backup_data"), 0.0, 230.0, 182.0, 28.0),
        ("st_rest", perch_data::t("settings.restore_data"), 194.0, 230.0, 182.0, 28.0),
        ("st_bakdir", perch_data::t("settings.backup_dir"), 0.0, 266.0, 182.0, 28.0),
    ]
}

/// 备份 / 恢复的结果提示,显示在关于页
static ABOUT_MSG: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

fn set_msg(s: String) {
    if let Ok(mut m) = ABOUT_MSG.lock() {
        *m = s;
    }
}

fn monitor_count() -> u32 {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CMONITORS).max(1) as u32 }
}

fn fixed_drives() -> Vec<String> {
    let mask = unsafe { windows::Win32::Storage::FileSystem::GetLogicalDrives() };
    (0..26u32).filter(|i| mask & (1 << i) != 0).map(|i| format!("{}:\\", (b'A' + i as u8) as char)).collect()
}

fn weather_city() -> String {
    let text = std::fs::read_to_string(perch_data::paths::weather_cfg_json()).unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["city"].as_str().map(|s| s.to_string())).filter(|s| !s.is_empty()).unwrap_or_else(|| "auto".into())
}

fn abs(x: f32, y: f32, w: f32, h: f32, id: &'static str) -> RectBox {
    let r = rect();
    RectBox { id, x: r.0 + x, y: r.1 + y, w, h }
}

/// 面板打开时的可点击区域(优先级最高)
pub fn boxes(tab: usize, scroll: usize) -> Vec<RectBox> {
    let mut v = vec![abs(480.0, 8.0, 32.0, 28.0, "st_close")];
    for (i, id) in TAB_IDS.iter().enumerate() {
        v.push(abs(12.0, CY + 34.0 * i as f32, 104.0, 28.0, id));
    }
    let c = |x: f32, y: f32, w: f32, h: f32, id: &'static str| abs(CX + x, CY + y, w, h, id);
    match tab {
        0 => {
            for (id, x, y, w, h) in GENERAL_BOXES.iter() {
                v.push(c(*x, *y, *w, *h, id));
            }
            v.push(c(240.0, MON_Y, 28.0, 26.0, "st_mon_dn"));
            v.push(c(332.0, MON_Y, 28.0, 26.0, "st_mon_up"));
        }
        1 => {
            for k in 0..12 {
                v.push(c(194.0 * (k % 2) as f32, 36.0 * (k / 2) as f32, 182.0, 28.0, WG[k]));
            }
        }
        2 => {
            for k in 0..8 {
                v.push(c(240.0, 38.0 * k as f32, 28.0, 26.0, PM_DN[k]));
                v.push(c(332.0, 38.0 * k as f32, 28.0, 26.0, PM_UP[k]));
            }
        }
        3 => {
            v.push(c(262.0, 0.0, 114.0, 26.0, "st_dk_add"));
            let n = config::get().dock.len();
            for k in 0..DOCK_ROWS {
                if scroll + k >= n {
                    break;
                }
                let y = 36.0 + 40.0 * k as f32;
                v.push(c(232.0, y + 6.0, 60.0, 24.0, DK_MODE[k]));
                v.push(c(298.0, y + 6.0, 24.0, 24.0, DK_UP[k]));
                v.push(c(324.0, y + 6.0, 24.0, 24.0, DK_DN[k]));
                v.push(c(350.0, y + 6.0, 26.0, 24.0, DK_DEL[k]));
            }
        }
        4 => {
            v.push(c(240.0, 0.0, 28.0, 26.0, "st_d0_dn"));
            v.push(c(332.0, 0.0, 28.0, 26.0, "st_d0_up"));
            v.push(c(240.0, 38.0, 28.0, 26.0, "st_d1_dn"));
            v.push(c(332.0, 38.0, 28.0, 26.0, "st_d1_up"));
            v.push(c(296.0, 90.0, 80.0, 26.0, "st_city"));
            v.push(c(296.0, 128.0, 80.0, 26.0, "st_ma_url"));
            v.push(c(296.0, 166.0, 80.0, 26.0, "st_ma_player"));
            v.push(c(240.0, 214.0, 28.0, 26.0, "st_ga_dn"));
            v.push(c(332.0, 214.0, 28.0, 26.0, "st_ga_up"));
        }
        _ => {
            for (id, x, y, w, h) in ABOUT_BOXES.iter() {
                v.push(c(*x, *y, *w, *h, id));
            }
        }
    }
    if tab == 5 {
        for k in 0..4 {
            v.push(c(296.0, 38.0 * k as f32, 80.0, 26.0, HK[k]));
        }
        v.push(c(0.0, 190.0, 182.0, 28.0, "st_hkreset"));
    }
    // 兜底:整块面板(吞掉点击、接收滚轮)
    v.push(abs(0.0, 0.0, W, H, "st_panel"));
    v
}

fn pm_value(k: usize) -> String {
    let p = config::get().pomodoro;
    match k {
        0..=3 => p.presets.get(k).map(|m| perch_data::t_fmt("settings2.minutes", &[("n", &m.to_string())])).unwrap_or_default(),
        4 => perch_data::t_fmt("settings2.minutes", &[("n", &p.short_break.to_string())]),
        5 => perch_data::t_fmt("settings2.minutes", &[("n", &p.long_break.to_string())]),
        6 => perch_data::t_fmt("settings2.count_unit", &[("n", &p.cycles_before_long.to_string())]),
        _ => match p.sound.as_str() {
            "off" => perch_data::t("settings2.sound_off"),
            "system" => perch_data::t("settings2.sound_system"),
            s => clip(s, 10),
        },
    }
}

pub fn draw(p: &mut P, v: &View) -> Result<()> {
    let r = rect();
    p.at(r);
    let (th, white, muted) = (p.th, p.col(&p.th.white), p.col(&p.th.muted));
    let faint = windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 35.0 / 255.0 };
    p.rect(0.0, 0.0, W, H, 12.0, th.bg.with_alpha(250), p.col(&th.stroke), 1.5)?;
    p.line(12.0, 1.0, W - 12.0, 1.0, faint, 1.0)?;
    p.line(W - 20.0, 1.0, W - 1.0, 20.0, p.col(&th.accent), 2.0)?;
    p.txt(16.0, 12.0, &perch_data::t("settings.title"), 10.0, p.col(&th.accent), L, false)?;
    p.txt(W - 16.0, 12.0, "✕", 9.0, muted, R, false)?;
    p.line(124.0, 48.0, 124.0, H - 14.0, faint, 1.0)?;

    let tab = v.app.settings_tab.min(6);
    let tabs = settings_tabs();
    for (i, (id, label)) in tabs.iter().enumerate() {
        let y = CY + 34.0 * i as f32;
        let (f, s, sw_) = if tab == i { (p.col(&th.stroke), p.col(&th.accent), 1.5) } else { (p.col(&th.item_bg), p.col(&th.stroke), 1.0) };
        let (f, s) = if p.hot(id) { (p.col(&th.item_hover), p.col(&th.accent)) } else { (f, s) };
        p.rect(12.0, y, 104.0, 28.0, 6.0, f, s, sw_)?;
        p.txt(64.0, y + 6.0, label, 8.5, white, C, false)?;
    }

    // 内容区:以 (CX, CY) 为原点
    p.at((r.0 + CX, r.1 + CY, 0.0, 0.0));
    let button = |p: &mut P, id: &'static str, label: &str, x: f32, y: f32, w: f32, h: f32, active: bool| -> Result<()> {
        let th = p.th;
        let (f, s, sw_) = if active { (p.col(&th.stroke), p.col(&th.accent), 1.5) } else { (p.col(&th.item_bg), p.col(&th.stroke), 1.0) };
        let (f, s) = if p.hot(id) { (p.col(&th.item_hover), p.col(&th.accent)) } else { (f, s) };
        p.rect(x, y, w, h, 6.0, f, s, sw_)?;
        p.txt(x + w / 2.0, y + (h - 14.0) / 2.0 + 1.0, label, 8.0, p.col(&th.white), C, false)
    };
    // 步进行:标签 + [−] 值 [+]
    let stepper = |p: &mut P, dn: &'static str, up: &'static str, label: &str, value: &str, y: f32| -> Result<()> {
        let th = p.th;
        p.txt(0.0, y + 5.0, label, 8.5, p.col(&th.white), L, false)?;
        button(p, dn, "−", 240.0, y, 28.0, 26.0, false)?;
        p.txt(300.0, y + 6.0, value, 8.5, p.col(&th.accent), C, false)?;
        button(p, up, "+", 332.0, y, 28.0, 26.0, false)
    };

    match tab {
        0 => {
            for (id, label, x, y, w, h) in general_buttons().iter() {
                let (text, active) = match *id {
                    "st_float" => (perch_data::t(if v.app.float_mode { "settings2.float_on" } else { "settings2.widget_on" }), v.app.float_mode),
                    "st_font" => (perch_data::t(if v.app.theme_fonts { "settings2.fonts_on" } else { "settings2.fonts_off" }), v.app.theme_fonts),
                    "st_icons" => (perch_data::t(if crate::desktop::currently_hidden() { "settings2.icons_hidden" } else { "settings2.icons_shown" }), crate::desktop::currently_hidden()),
                    "st_edit" => (label.clone(), v.app.edit_layout),
                    _ => (label.clone(), false),
                };
                button(p, id, &text, *x, *y, *w, *h, active)?;
            }
            let m = config::get().display.monitor;
            let name = if m == 0 { perch_data::t("settings2.mon_primary") } else { perch_data::t_fmt("settings2.mon_n", &[("n", &(m + 1).to_string())]) };
            stepper(p, "st_mon_dn", "st_mon_up", &perch_data::t("settings2.mon_label"), &name, MON_Y)?;
            p.txt(0.0, MON_Y + 40.0, &perch_data::t("settings2.general_hint"), 7.5, muted, L, false)?;
        }
        1 => {
            let mask = cards::hidden_mask();
            for k in 0..12 {
                let shown = mask & (1 << k) == 0;
                let label = format!("{} {}", if shown { "●" } else { "○" }, cards::card_label(k));
                button(p, WG[k], &label, 194.0 * (k % 2) as f32, 36.0 * (k / 2) as f32, 182.0, 28.0, shown)?;
            }
            p.txt(0.0, 224.0, &perch_data::t("settings2.cards_hint1"), 7.5, muted, L, false)?;
            p.txt(0.0, 240.0, &perch_data::t("settings2.cards_hint2"), 7.5, muted, L, false)?;
        }
        2 => {
            let labels: [String; 8] = [
                perch_data::t_fmt("settings2.pomo_preset", &[("n", "1")]),
                perch_data::t_fmt("settings2.pomo_preset", &[("n", "2")]),
                perch_data::t_fmt("settings2.pomo_preset", &[("n", "3")]),
                perch_data::t_fmt("settings2.pomo_preset", &[("n", "4")]),
                perch_data::t("settings2.pomo_short"),
                perch_data::t("settings2.pomo_long"),
                perch_data::t("settings2.pomo_cycles"),
                perch_data::t("settings2.pomo_sound"),
            ];
            let n = config::get().pomodoro.presets.len();
            for k in 0..8 {
                if k < 4 && k >= n {
                    continue;
                }
                let y = 38.0 * k as f32;
                stepper(p, PM_DN[k], PM_UP[k], &labels[k], &pm_value(k), y)?;
            }
            p.txt(0.0, 38.0 * 8.0 + 4.0, &perch_data::t("settings2.pomo_hint"), 7.5, muted, L, false)?;
        }
        3 => {
            let list = config::get().dock;
            p.txt(0.0, 6.0, &perch_data::t_fmt("settings2.dock_count", &[("n", &list.len().to_string())]), 9.0, p.col(&th.accent), L, false)?;
            button(p, "st_dk_add", &perch_data::t("settings2.dock_add"), 262.0, 0.0, 114.0, 26.0, false)?;
            for k in 0..DOCK_ROWS {
                let Some(e) = list.get(v.app.settings_scroll + k) else { break };
                let y = 36.0 + 40.0 * k as f32;
                p.rect(0.0, y, 376.0, 36.0, 6.0, p.col(&th.item_bg), p.col(&th.stroke), 1.0)?;
                p.txt(10.0, y + 4.0, &clip(&e.name, 16), 8.5, white, L, false)?;
                p.txt(10.0, y + 20.0, &clip(&e.target, 34), 6.5, muted, L, true)?;
                let mode_label = perch_data::t(match e.mode.as_str() {
                    "single" => "settings2.mode_single",
                    "multi" => "settings2.mode_multi",
                    _ => "settings2.mode_auto",
                });
                button(p, DK_MODE[k], &mode_label, 232.0, y + 6.0, 60.0, 24.0, false)?;
                for (id, label, x) in [(DK_UP[k], "↑", 298.0), (DK_DN[k], "↓", 324.0)] {
                    button(p, id, label, x, y + 6.0, 24.0, 24.0, false)?;
                }
                let hot = p.hot(DK_DEL[k]);
                let col = if hot { crate::theme::ColorRgba::new(255, 120, 120, 255).to_d2d() } else { crate::theme::ColorRgba::new(239, 68, 68, 210).to_d2d() };
                p.txt(363.0, y + 10.0, "✕", 9.0, col, C, false)?;
            }
            if list.len() > DOCK_ROWS {
                let frac = DOCK_ROWS as f32 / list.len() as f32;
                let pos = v.app.settings_scroll as f32 / (list.len() - DOCK_ROWS) as f32;
                let tr = p.track();
                p.rect(380.0, 36.0, 3.0, 276.0, 1.5, tr, tr, 0.0)?;
                p.rect(380.0, 36.0 + 276.0 * (1.0 - frac) * pos, 3.0, 276.0 * frac, 1.5, p.col(&th.accent), p.col(&th.accent), 0.0)?;
            }
            p.txt(0.0, 322.0, &perch_data::t("settings2.dock_hint"), 7.5, muted, L, false)?;
        }
        4 => {
            let disks = config::get().system.disks;
            for k in 0..2 {
                let d = disks.get(k).cloned().unwrap_or_default();
                let label = perch_data::t_fmt("settings2.disk_n", &[("n", if k == 0 { "1" } else { "2" })]);
                let (dn, up) = if k == 0 { ("st_d0_dn", "st_d0_up") } else { ("st_d1_dn", "st_d1_up") };
                stepper(p, dn, up, &label, &d, 38.0 * k as f32)?;
            }
            let rows: [(String, String, &'static str, f32); 3] = [
                (perch_data::t("settings2.city"), weather_city(), "st_city", 90.0),
                (perch_data::t("settings2.ma_url"), config::raw_str("music_assistant", "url"), "st_ma_url", 128.0),
                (perch_data::t("settings2.ma_player"), config::raw_str("music_assistant", "player"), "st_ma_player", 166.0),
            ];
            for (label, value, id, y) in rows {
                p.txt(0.0, y + 5.0, &label, 8.5, white, L, false)?;
                let shown = if value.is_empty() { perch_data::t("settings2.default_val") } else { clip(&value, 18) };
                p.txt(164.0, y + 6.0, &shown, 7.5, p.col(&th.accent), L, true)?;
                button(p, id, &perch_data::t("settings2.edit"), 296.0, y, 80.0, 26.0, false)?;
            }
            let g = config::get().alert.gpu_temp;
            stepper(p, "st_ga_dn", "st_ga_up", &perch_data::t("settings2.gpu_alert"), &if g == 0 { perch_data::t("settings2.off") } else { format!("{}°C", g) }, 214.0)?;
            p.txt(0.0, 252.0, &perch_data::t("settings2.sys_hint1"), 7.5, muted, L, false)?;
            p.txt(0.0, 268.0, &perch_data::t("settings2.sys_hint2"), 7.5, muted, L, false)?;
        }
        5 => {
            let hk = config::get().hotkeys;
            let rows = [(perch_data::t("settings2.hk_toggle"), hk.toggle), (perch_data::t("settings2.hk_float"), hk.float), (perch_data::t("settings2.hk_search"), hk.search), (perch_data::t("settings2.hk_tools"), hk.tools)];
            for (k, (label, combo)) in rows.iter().enumerate() {
                let y = 38.0 * k as f32;
                p.txt(0.0, y + 5.0, label, 8.5, white, L, false)?;
                p.txt(190.0, y + 6.0, combo, 8.5, p.col(&th.accent), L, true)?;
                button(p, HK[k], &perch_data::t("settings2.edit"), 296.0, y, 80.0, 26.0, false)?;
            }
            button(p, "st_hkreset", &perch_data::t("settings2.hk_reset"), 0.0, 190.0, 182.0, 28.0, false)?;
            let status = crate::window::HOTKEY_STATUS.lock().map(|s| s.clone()).unwrap_or_default();
            p.txt(0.0, 158.0, &clip(&status, 52), 7.5, p.col(&th.sub), L, false)?;
            p.txt(0.0, 236.0, &perch_data::t("settings2.hk_hint1"), 7.5, muted, L, false)?;
            p.txt(0.0, 252.0, &perch_data::t("settings2.hk_hint2"), 7.5, muted, L, false)?;
        }
        _ => {
            p.txt(0.0, 4.0, "Perch", 11.0, p.col(&th.accent), L, false)?;
            p.txt(0.0, 30.0, &perch_data::t("settings2.about_line"), 8.5, white, L, false)?;
            p.txt(0.0, 62.0, &perch_data::t("settings2.about_dir"), 7.5, muted, L, false)?;
            p.txt(0.0, 76.0, &clip(&config::root().to_string_lossy(), 52), 7.5, white, L, true)?;
            p.txt(0.0, 100.0, &perch_data::t("settings2.about_cfg"), 7.5, muted, L, false)?;
            p.txt(0.0, 114.0, &clip(&config::path().to_string_lossy(), 52), 7.5, white, L, true)?;
            p.txt(0.0, 322.0, &perch_data::t("settings2.about_hotkeys"), 7.5, muted, L, false)?;
            let msg = ABOUT_MSG.lock().map(|m| m.clone()).unwrap_or_default();
            if !msg.is_empty() {
                p.txt(0.0, 302.0, &clip(&msg, 52), 7.5, p.col(&th.accent), L, false)?;
            }
            for (id, label, x, y, w, h) in about_buttons().iter() {
                button(p, id, label.as_str(), *x, *y, *w, *h, false)?;
            }
        }
    }
    Ok(())
}

pub fn scroll(app: &mut AppState, dir: i32) {
    if app.settings_tab != 3 {
        return;
    }
    let max = config::get().dock.len().saturating_sub(DOCK_ROWS);
    app.settings_scroll = (app.settings_scroll as i32 + dir).clamp(0, max as i32) as usize;
}

fn int_array(items: &[u32]) -> Value {
    let mut a = toml_edit::Array::new();
    for i in items {
        a.push(*i as i64);
    }
    Value::Array(a)
}

fn pomodoro_step(k: usize, d: i32) {
    let p = config::get().pomodoro;
    let stepped = |v: u32, small: u32, big: u32, lo: u32, hi: u32| -> u32 {
        let s = if v <= big && (d < 0 || v < big) { small } else { big };
        (v as i32 + d * s as i32).clamp(lo as i32, hi as i32) as u32
    };
    match k {
        0..=3 => {
            if k >= p.presets.len() {
                return;
            }
            let mut list = p.presets.clone();
            list[k] = stepped(list[k], 1, 5, 1, 180);
            config::edit(|doc| config::set_value(doc, "pomodoro", "presets", int_array(&list)));
        }
        4 => config::edit(|doc| config::set_value(doc, "pomodoro", "short_break", Value::from(stepped(p.short_break, 1, 1, 1, 60) as i64))),
        5 => config::edit(|doc| config::set_value(doc, "pomodoro", "long_break", Value::from(stepped(p.long_break, 5, 5, 5, 120) as i64))),
        6 => config::edit(|doc| config::set_value(doc, "pomodoro", "cycles_before_long", Value::from(stepped(p.cycles_before_long, 1, 1, 1, 12) as i64))),
        _ => {
            let next = if p.sound == "system" { "off" } else { "system" };
            config::edit(|doc| config::set_value(doc, "pomodoro", "sound", Value::from(next)));
        }
    }
}

fn cycle_disk(k: usize, d: i32) {
    let drives = fixed_drives();
    if drives.is_empty() {
        return;
    }
    let mut disks = config::get().system.disks;
    while disks.len() < 2 {
        disks.push(if disks.is_empty() { r"C:\".into() } else { r"D:\".into() });
    }
    let cur = drives.iter().position(|x| x.eq_ignore_ascii_case(&disks[k])).unwrap_or(0) as i32;
    disks[k] = drives[(cur + d).rem_euclid(drives.len() as i32) as usize].clone();
    config::edit(|doc| {
        let mut a = toml_edit::Array::new();
        for s in &disks {
            a.push(s.as_str());
        }
        config::set_value(doc, "system", "disks", Value::Array(a));
    });
}

/// 系统文件选择框,选 exe 或快捷方式(快捷方式会被自动解析成目标)
pub fn pick_app() -> Option<String> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::UI::Controls::Dialogs::*;
    let mut buf = [0u16; 520];
    let filter: Vec<u16> = format!("{}\0*.exe;*.lnk;*.bat;*.cmd\0{}\0*.*\0\0", perch_data::t("settings2.filter_programs"), perch_data::t("settings2.filter_all")).encode_utf16().collect();
    let title: Vec<u16> = format!("{}\0", perch_data::t("settings2.pick_title")).encode_utf16().collect();
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFile: PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_EXPLORER | OFN_HIDEREADONLY,
        ..Default::default()
    };
    unsafe {
        if GetOpenFileNameW(&mut ofn).as_bool() {
            let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            return Some(String::from_utf16_lossy(&buf[..len]));
        }
    }
    None
}

/// 往 Dock 末尾加一个应用(名称取文件名)
pub fn add_dock_app(path: &str) {
    if config::get().dock.len() >= 24 {
        return;
    }
    let name = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
    config::edit(|doc| config::dock_add(doc, &name, path));
    RELAYOUT.store(true, Ordering::SeqCst);
}

/// 系统是/否确认框
pub fn confirm(text: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO};
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("Perch"), MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND | MB_TOPMOST) == IDYES }
}

pub fn remove_dock_app(idx: usize) {
    config::edit(|doc| config::dock_remove(doc, idx));
    RELAYOUT.store(true, Ordering::SeqCst);
}

fn ask_setting(app: &AppState, title: &str, label: &str, current: &str, section: &str, key: &str) {
    if let Some(text) = crate::prompt::ask(title, label, current, &app.theme) {
        let text = text.trim().to_string();
        config::edit(|doc| config::set_value(doc, section, key, Value::from(text.as_str())));
    }
}

pub fn on_click(id: &str, app: &mut AppState, _hub: &mut Hub) {
    for k in 0..8 {
        if id == PM_DN[k] {
            return pomodoro_step(k, -1);
        }
        if id == PM_UP[k] {
            return pomodoro_step(k, 1);
        }
    }
    for k in 0..DOCK_ROWS {
        let idx = app.settings_scroll + k;
        if id == DK_UP[k] {
            config::edit(|doc| config::dock_move(doc, idx, -1));
            return RELAYOUT.store(true, Ordering::SeqCst);
        }
        if id == DK_DN[k] {
            config::edit(|doc| config::dock_move(doc, idx, 1));
            return RELAYOUT.store(true, Ordering::SeqCst);
        }
        if id == DK_MODE[k] {
            let cur = config::get().dock.get(idx).map(|d| d.mode.clone()).unwrap_or_default();
            let next = match cur.as_str() {
                "single" => "multi",
                "multi" => "auto",
                _ => "single",
            };
            config::edit(|doc| config::dock_set(doc, idx, "mode", Value::from(next)));
            return;
        }
        if id == DK_DEL[k] {
            remove_dock_app(idx);
            let max = config::get().dock.len().saturating_sub(DOCK_ROWS);
            app.settings_scroll = app.settings_scroll.min(max);
            return;
        }
    }
    if let Some(k) = id.strip_prefix("st_w").and_then(|s| s.parse::<usize>().ok()) {
        if k < 12 {
            let mut hidden = config::get().widgets.hidden;
            let name = cards::CARD_NAMES[k];
            if hidden.iter().any(|h| h == name) {
                hidden.retain(|h| h != name);
            } else {
                hidden.push(name.to_string());
            }
            config::edit(|doc| {
                let mut a = toml_edit::Array::new();
                for h in &hidden {
                    a.push(h.as_str());
                }
                config::set_value(doc, "widgets", "hidden", Value::Array(a));
            });
            return;
        }
    }
    match id {
        "st_close" => app.settings_open = false,
        "st_tab0" | "st_tab1" | "st_tab2" | "st_tab3" | "st_tab4" | "st_tab5" | "st_tab6" => {
            app.settings_tab = (id.as_bytes()[6] - b'0') as usize;
            app.settings_scroll = 0;
        }
        "st_float" => app.set_float(!app.float_mode),
        "st_font" => app.toggle_theme_fonts(),
        "st_icons" => crate::desktop::toggle_user(),
        "st_edit" => {
            app.edit_layout = !app.edit_layout;
            app.drag = None;
        }
        "st_reset" => {
            cards::reset_layout();
            RELAYOUT.store(true, Ordering::SeqCst);
        }
        "st_wpnext" => data::core(&["wallpaper", "next"]),
        "st_wpdir" => data::open_target(&perch_data::paths::wallpapers_dir().to_string_lossy(), ""),
        "st_mon_dn" | "st_mon_up" => {
            let cur = config::get().display.monitor as i32;
            let n = (cur + if id == "st_mon_up" { 1 } else { -1 }).clamp(0, monitor_count() as i32 - 1);
            if n != cur {
                config::edit(|doc| config::set_value(doc, "display", "monitor", Value::from(n as i64)));
                RELAYOUT.store(true, Ordering::SeqCst);
            }
        }
        "st_dk_add" => {
            if let Some(path) = pick_app() {
                add_dock_app(&path);
            }
        }
        "st_ga_dn" | "st_ga_up" => {
            let cur = config::get().alert.gpu_temp;
            let up = id == "st_ga_up";
            let mut n = if up { cur + 5 } else { cur.saturating_sub(5) };
            if n > 0 && n < 60 {
                n = if up { 60 } else { 0 };
            }
            let n = n.min(100);
            config::edit(|doc| config::set_value(doc, "alert", "gpu_temp", Value::from(n as i64)));
        }
        "st_d0_dn" => cycle_disk(0, -1),
        "st_d0_up" => cycle_disk(0, 1),
        "st_d1_dn" => cycle_disk(1, -1),
        "st_d1_up" => cycle_disk(1, 1),
        "st_city" => {
            if let Some(city) = crate::prompt::ask(&perch_data::t("settings2.city"), &perch_data::t("settings2.city_hint"), &weather_city(), &app.theme) {
                let city = city.trim();
                if !city.is_empty() {
                    data::core(&["weather", "city", city]);
                }
            }
        }
        "st_ma_url" => ask_setting(app, &perch_data::t("settings2.ma_url"), &perch_data::t("settings2.ma_url_hint"), &config::raw_str("music_assistant", "url"), "music_assistant", "url"),
        "st_ma_player" => ask_setting(app, &perch_data::t("settings2.ma_player"), &perch_data::t("settings2.ma_player_hint"), &config::raw_str("music_assistant", "player"), "music_assistant", "player"),
        "st_hkreset" => {
            let d = config::Hotkeys::default();
            config::edit(|doc| {
                config::set_value(doc, "hotkeys", "toggle", Value::from(d.toggle.as_str()));
                config::set_value(doc, "hotkeys", "float", Value::from(d.float.as_str()));
                config::set_value(doc, "hotkeys", "search", Value::from(d.search.as_str()));
                config::set_value(doc, "hotkeys", "tools", Value::from(d.tools.as_str()));
            });
            crate::window::REHOTKEY.store(true, Ordering::SeqCst);
        }
        "st_hk0" | "st_hk1" | "st_hk2" | "st_hk3" => {
            let k = (id.as_bytes()[5] - b'0') as usize;
            let (key, label) = [("toggle", perch_data::t("settings2.hk_toggle")), ("float", perch_data::t("settings2.hk_float")), ("search", perch_data::t("settings2.hk_search")), ("tools", perch_data::t("settings2.hk_tools"))][k].clone();
            let hk = config::get().hotkeys;
            let cur = [hk.toggle, hk.float, hk.search, hk.tools][k].clone();
            if let Some(text) = crate::prompt::ask(&perch_data::t_fmt("settings2.hk_prompt_title", &[("label", &label)]), &perch_data::t("settings2.hk_prompt_hint"), &cur, &app.theme) {
                let text = text.trim().to_string();
                if crate::window::parse_combo(&text).is_some() {
                    config::edit(|doc| config::set_value(doc, "hotkeys", key, Value::from(text.as_str())));
                    crate::window::REHOTKEY.store(true, Ordering::SeqCst);
                } else if let Ok(mut s) = crate::window::HOTKEY_STATUS.lock() {
                    *s = perch_data::t_fmt("settings2.hk_invalid", &[("text", &text)]);
                }
            }
        }
        "st_cfg" => data::open_target(&config::path().to_string_lossy(), ""),
        "st_log" => data::open_target(&format!(r"{}\perch\hud.log", std::env::var("APPDATA").unwrap_or_default()), ""),
        "st_bak" => match crate::backup::backup() {
            Ok(p) => set_msg(perch_data::t_fmt("settings2.backup_ok", &[("name", &p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())])),
            Err(e) => set_msg(perch_data::t_fmt("settings2.backup_fail", &[("err", &e.to_string())])),
        },
        "st_rest" => match crate::backup::latest() {
            None => set_msg(perch_data::t("settings2.backup_none")),
            Some(from) => {
                let name = from.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if confirm(&perch_data::t_fmt("settings2.restore_confirm", &[("name", &name)])) {
                    match crate::backup::restore(&from) {
                        Ok(n) => {
                            config::reload();
                            app.reload_theme();
                            data::core(&["reload"]);
                            RELAYOUT.store(true, Ordering::SeqCst);
                            set_msg(perch_data::t_fmt("settings2.restore_ok", &[("n", &n.to_string()), ("name", &name)]));
                        }
                        Err(e) => set_msg(perch_data::t_fmt("settings2.restore_fail", &[("err", &e.to_string())])),
                    }
                }
            }
        },
        "st_bakdir" => {
            let _ = std::fs::create_dir_all(crate::backup::dir());
            data::open_target(&crate::backup::dir().to_string_lossy(), "");
        }
        "st_reload" => {
            config::reload();
            app.reload_theme();
            RELAYOUT.store(true, Ordering::SeqCst);
        }
        _ => {}
    }
}
