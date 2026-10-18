//! 卡片绘制(坐标/字号/颜色沿用最初的设计稿)。
//! 卡片坐标为屏幕绝对坐标;卡内坐标与旧皮肤一致(卡宽 260)。

use chrono::{Datelike, Local, Timelike};
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::*;

use crate::audio::Audio;
use crate::config;
use crate::data::{self, Hub};
use crate::live_theme::{parse_color, ThemeStyle};
use crate::metrics::SystemMetrics;
use crate::renderer::D2dRenderer;
use crate::state::{AppState, TimerState};
use crate::theme::{ColorRgba, ThemeConfig};
use crate::window::RectBox;

// ---------------- 布局 ----------------
pub const LX: f32 = 110.0;
pub const CW: f32 = 260.0;
fn media_rect_x() -> f32 {
    LX
}

// ---------------- 屏幕尺寸(DIP)与锚点布局 ----------------
// 以 2560×1440 的旧布局为基准:左列固定,右列贴右边,底部元素贴底边/水平居中。
static SW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x4520_0000); // 2560.0
static SH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x44B4_0000); // 1440.0

pub fn set_screen(w: f32, h: f32) {
    SW.store(w.to_bits(), std::sync::atomic::Ordering::Relaxed);
    SH.store(h.to_bits(), std::sync::atomic::Ordering::Relaxed);
}
pub fn sw() -> f32 {
    f32::from_bits(SW.load(std::sync::atomic::Ordering::Relaxed))
}
pub fn sh() -> f32 {
    f32::from_bits(SH.load(std::sync::atomic::Ordering::Relaxed))
}
pub fn rx() -> f32 {
    sw() - 381.0
}
pub type Rect = (f32, f32, f32, f32);
pub fn clock_rect() -> Rect { off(6, (rx(), 40.0, CW, 84.0)) }
pub fn weather_rect() -> Rect { off(7, (rx(), 136.0, CW, 184.0)) }
pub fn system_rect() -> Rect { off(8, (rx(), 332.0, CW, 270.0)) }
pub fn controls_rect() -> Rect { off(9, (sw() - 601.0, sh() - 98.0, 480.0, 34.0)) }
/// 通知中心面板(在控制条上方,右对齐)
pub fn notif_rect() -> Rect {
    let c = controls_rect();
    (c.0 + c.2 - 320.0, c.1 - 372.0, 320.0, 360.0)
}
const NH_DEL: [&str; 8] = ["nh0", "nh1", "nh2", "nh3", "nh4", "nh5", "nh6", "nh7"];
pub fn dock_rect() -> Rect {
    let w = 52.0 * config::dock_count() as f32 + 34.0;
    // 右下角的控制条(宽 480、右边距 121)在窄屏上会压住 Dock 右端:Dock 向左让开,宽屏不受影响
    let x = ((sw() - w) / 2.0).min(sw() - 601.0 - 16.0 - w).max(8.0);
    off(10, (x, sh() - 120.0, w, 56.0))
}
pub fn quotes_rect() -> Rect { off(11, (LX, sh() - 172.0, 280.0, 96.0)) }
pub fn notify_rect() -> Rect { (sw() - 401.0, sh() - 172.0, 280.0, 56.0) }
pub const CARD_KEYS: [&str; 12] = ["battery", "timer", "remind", "todo", "media", "water", "clock", "weather", "system", "controls", "dock", "quotes"];
pub const CARD_IDS: [&str; 12] = ["card:battery", "card:timer", "card:remind", "card:todo", "card:media", "card:water", "card:clock", "card:weather", "card:system", "card:controls", "card:dock", "card:quotes"];

// 用户拖动后的偏移(DIP),存 %APPDATA%\perch\layout.json
static OFFSETS: std::sync::Mutex<[(f32, f32); 12]> = std::sync::Mutex::new([(0.0, 0.0); 12]);

fn off(i: usize, r: Rect) -> Rect {
    let o = OFFSETS.lock().map(|g| g[i]).unwrap_or((0.0, 0.0));
    (r.0 + o.0, r.1 + o.1, r.2, r.3)
}
pub fn battery_rect() -> Rect { off(0, (LX, 40.0, CW, 26.0)) }
pub fn timer_rect() -> Rect { off(1, (LX, 78.0, CW, 108.0)) }
pub fn remind_rect() -> Rect { off(2, (LX, 198.0, CW, 62.0)) }
pub fn todo_rect() -> Rect { off(3, (LX, 272.0, CW, 140.0)) }
pub fn media_rect() -> Rect { off(4, (LX, 424.0, CW, 151.0)) }
pub fn water_rect() -> Rect { off(5, (LX, 588.0, CW, 116.0)) }

fn layout_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join("perch").join("layout.json")
}

pub fn load_layout() {
    if let Ok(t) = std::fs::read_to_string(layout_path()) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
            if let Ok(mut g) = OFFSETS.lock() {
                for (i, key) in CARD_KEYS.iter().enumerate() {
                    if let Some(a) = v.get(*key).and_then(|x| x.as_array()) {
                        g[i] = (a.first().and_then(|x| x.as_f64()).unwrap_or(0.0) as f32, a.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32);
                    }
                }
            }
        }
    }
}

pub fn save_layout() {
    let g = OFFSETS.lock().map(|g| *g).unwrap_or([(0.0, 0.0); 12]);
    let mut m = serde_json::Map::new();
    for (i, key) in CARD_KEYS.iter().enumerate() {
        if g[i] != (0.0, 0.0) {
            m.insert(key.to_string(), serde_json::json!([g[i].0, g[i].1]));
        }
    }
    let p = layout_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, serde_json::Value::Object(m).to_string());
}

pub fn reset_layout() {
    if let Ok(mut g) = OFFSETS.lock() {
        *g = [(0.0, 0.0); 12];
    }
    let _ = std::fs::remove_file(layout_path());
}

pub fn offset_of(i: usize) -> (f32, f32) {
    OFFSETS.lock().map(|g| g[i]).unwrap_or((0.0, 0.0))
}

/// 拖动时设置某张卡片的偏移(限制在屏幕内)
pub fn set_offset(i: usize, dx: f32, dy: f32) {
    let base = [battery_rect, timer_rect, remind_rect, todo_rect, media_rect, water_rect, clock_rect, weather_rect, system_rect, controls_rect, dock_rect, quotes_rect][i]();
    let cur = offset_of(i);
    let (bx0, by0) = (base.0 - cur.0, base.1 - cur.1);
    let nx = (bx0 + dx).clamp(0.0, (sw() - base.2).max(0.0)) - bx0;
    let ny = (by0 + dy).clamp(0.0, (sh() - base.3).max(0.0)) - by0;
    if let Ok(mut g) = OFFSETS.lock() {
        g[i] = (nx, ny);
    }
}

/// 布局签名:偏移变化时主循环据此重新计算命中区域和窗口区域
pub fn layout_signature() -> i64 {
    OFFSETS.lock().map(|g| g.iter().enumerate().map(|(i, o)| ((o.0 as i64) * 31 + (o.1 as i64)) * (i as i64 + 7)).sum()).unwrap_or(0)
}

/// 通知中心打开时的可点击区域(优先级最高)
pub fn notif_boxes() -> Vec<RectBox> {
    let r = notif_rect();
    let mut v = vec![
        RectBox { id: "n_clear", x: r.0 + 246.0, y: r.1 + 10.0, w: 56.0, h: 20.0 },
        RectBox { id: "n_close", x: r.0 + 300.0, y: r.1 + 8.0, w: 16.0, h: 22.0 },
    ];
    for i in 0..8 {
        v.push(RectBox { id: NH_DEL[i], x: r.0 + 288.0, y: r.1 + 44.0 + 38.0 * i as f32, w: 26.0, h: 34.0 });
    }
    v.push(RectBox { id: "notif_body", x: r.0, y: r.1 + 40.0, w: r.2, h: r.3 - 40.0 });
    v.push(RectBox { id: "notif_panel", x: r.0, y: r.1, w: r.2, h: r.3 });
    v
}

pub fn notif_region() -> (f32, f32, f32, f32, f32) {
    let r = notif_rect();
    (r.0, r.1, r.2, r.3, 12.0)
}

/// 编辑模式下可拖动的整卡命中区域
pub fn edit_boxes() -> Vec<RectBox> {
    let rects = [battery_rect(), timer_rect(), remind_rect(), todo_rect(), media_rect(), water_rect(), clock_rect(), weather_rect(), system_rect(), controls_rect(), dock_rect(), quotes_rect()];
    let mask = hidden_mask();
    rects.iter().enumerate().filter(|(i, _)| mask & (1 << i) == 0).map(|(i, r)| RectBox { id: CARD_IDS[i], x: r.0, y: r.1, w: r.2, h: r.3 }).collect()
}

pub const CARD_NAMES: [&str; 12] = ["battery", "timer", "remind", "todo", "media", "water", "clock", "weather", "system", "controls", "dock", "quotes"];

pub fn card_label(i: usize) -> String {
    if let Some(name) = CARD_NAMES.get(i) {
        perch_data::t(&format!("cards.name_{}", name))
    } else {
        String::new()
    }
}

/// 被隐藏的卡片位掩码(位序同 CARD_NAMES)
pub fn hidden_mask() -> u32 {
    let hidden = config::get().widgets.hidden;
    let mut mask = CARD_NAMES.iter().enumerate().filter(|(_, n)| hidden.iter().any(|h| h == *n)).fold(0, |m, (i, _)| m | (1 << i));
    // 没有任何电量来源(台式机且没配设备)时,电量卡片自动隐藏
    if BATTERY_UNAVAILABLE.load(std::sync::atomic::Ordering::Relaxed) {
        mask |= 1 << 0; // CARD_NAMES[0] == "battery"
    }
    mask
}

static BATTERY_UNAVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_battery_available(available: bool) {
    BATTERY_UNAVAILABLE.store(!available, std::sync::atomic::Ordering::Relaxed);
}

/// 可点击区域 id 属于哪张卡片
fn card_of_hit(id: &str) -> Option<usize> {
    Some(match id {
        "battery" => 0,
        "t_main" | "t_reset" => 1,
        "r_cal" | "r_del" | "r_slot" | "r_tab_cal" | "r_tab_list" | "rem_body" | "r_panel" => 2,
        "td_add" | "td_clear" | "todo_body" => 3,
        "quotes" => 11,
        "sys_card" => 8,
        "m_seek" | "m_prev" | "m_play" | "m_next" | "m_mute" | "m_vdn" | "m_slider" | "m_vup" => 4,
        "w200" | "w300" | "w500" | "w_undo" => 5,
        _ if id.starts_with("c_") => 9,
        _ if id.starts_with("dk") => 10,
        _ if id.len() == 2 && id.starts_with('p') => 1,
        _ if id.len() == 3 && (id.starts_with("rx")) => 2,
        _ if id.len() == 3 && (id.starts_with("td") || id.starts_with("te") || id.starts_with("tx")) => 3,
        _ => return None,
    })
}

pub fn card_index(id: &str) -> Option<usize> {
    CARD_IDS.iter().position(|c| *c == id)
}

pub fn header_rect() -> Rect { (LX, 4.0, CW, 32.0) }
pub fn status_rect() -> Rect { (rx(), 4.0, CW, 32.0) }
pub fn selector_rect() -> Rect { ((sw() - 320.0) / 2.0, sh() - 580.0, 320.0, 556.0) }
/// 频谱条窗口(位于 Dock 正上方)
pub fn spectrum_rect() -> Rect {
    // 与 Dock 同宽(两端各留 13 的圆角余量),Dock 增减应用时随之伸缩
    let d = dock_rect();
    (d.0 + 13.0, d.1 - 56.0, (d.2 - 26.0).max(48.0), 56.0)
}


// (id, 命令参数, 标签, x, 宽, 行 y, 模式键, 模式值)
const SEL_BUTTONS: [(&str, &str, &str, f32, f32, f32, &str); 14] = [
    ("sel_wp_auto", "wallpaper mode auto", "cards2.sel_wp_auto", 16.0, 94.0, 314.0, "WpMode=auto"),
    ("sel_wp_day", "wallpaper mode day", "cards2.sel_wp_day", 114.0, 94.0, 314.0, "WpMode=day"),
    ("sel_wp_night", "wallpaper mode night", "cards2.sel_wp_night", 212.0, 92.0, 314.0, "WpMode=night"),
    ("sel_at_fixed", "autotheme mode fixed", "cards2.sel_at_fixed", 16.0, 66.0, 402.0, "AutoMode=fixed"),
    ("sel_at_weekday", "autotheme mode weekday", "cards2.sel_at_weekday", 90.0, 66.0, 402.0, "AutoMode=weekday"),
    ("sel_at_random", "autotheme mode random", "cards2.sel_at_random", 164.0, 66.0, 402.0, "AutoMode=random"),
    ("sel_at_period", "autotheme mode period", "cards2.sel_at_period", 238.0, 66.0, 402.0, "AutoMode=period"),
    ("sel_am_off", "ambient off", "cards2.sel_am_off", 16.0, 94.0, 456.0, "ambient=off"),
    ("sel_am_low", "ambient low", "cards2.sel_am_low", 114.0, 94.0, 456.0, "ambient=low"),
    ("sel_am_std", "ambient standard", "cards2.sel_am_std", 212.0, 92.0, 456.0, "ambient=standard"),
    ("sel_bgm", "bgm auto toggle", "", 16.0, 288.0, 346.0, "BgmAuto=1"),
    ("sel_close", "", "", 284.0, 24.0, 8.0, ""),
    ("sel_settings", "", "", 16.0, 288.0, 498.0, ""),
    ("sel_dummy", "", "", 0.0, 0.0, 0.0, ""),
];

const REM_DEL: [&str; 5] = ["rx0", "rx1", "rx2", "rx3", "rx4"];
const TODO_IDS: [&str; 4] = ["td0", "td1", "td2", "td3"];
const TODO_EDIT: [&str; 4] = ["te0", "te1", "te2", "te3"];
const TODO_DEL: [&str; 4] = ["tx0", "tx1", "tx2", "tx3"];
const PRESET_IDS: [&str; 4] = ["p0", "p1", "p2", "p3"];
const DOCK_IDS: [&str; 24] = [
    "dk0", "dk1", "dk2", "dk3", "dk4", "dk5", "dk6", "dk7", "dk8", "dk9", "dk10", "dk11", "dk12", "dk13", "dk14", "dk15", "dk16", "dk17", "dk18", "dk19", "dk20", "dk21", "dk22", "dk23",
];

/// 番茄钟预设:(id, x, 分钟, 标签),取自 config.toml
fn presets() -> Vec<(&'static str, f32, u32, String)> {
    config::get().pomodoro.presets.iter().take(4).enumerate().map(|(i, m)| (PRESET_IDS[i], 122.0 + 32.0 * i as f32, *m, format!("{}m", m))).collect()
}

// ---------------- 悬停提示(稳定悬停 0.45 秒后显示) ----------------
static TIP: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

pub fn current_tip() -> Option<&'static str> {
    TIP.lock().ok().and_then(|g| *g)
}

pub fn set_tip(id: Option<&'static str>) {
    if let Ok(mut g) = TIP.lock() {
        *g = id.filter(|i| tip_text(i).is_some());
    }
}

pub fn tip_text(id: &str) -> Option<String> {
    if let Some(i) = DOCK_IDS.iter().position(|d| *d == id) {
        return config::get().dock.get(i).map(|d| {
            if !crate::running::is_running(d) {
                return d.name.clone();
            }
            let hint = if dock_is_multi(d) { perch_data::t("cards.hint_multi") } else { perch_data::t("cards.hint_single") };
            perch_data::t_fmt("cards.tip_dock_running", &[("name", &d.name), ("hint", &hint)])
        });
    }
    if let Some(i) = PRESET_IDS.iter().position(|d| *d == id) {
        return config::get().pomodoro.presets.get(i).map(|m| perch_data::t_fmt("cards.tip_pomo_preset", &[("mins", &m.to_string())]));
    }
    let t = match id {
        "battery" => perch_data::t("cards.tip_battery"),
        "t_main" => perch_data::t("cards.tip_t_main"),
        "t_reset" => perch_data::t("cards.tip_t_reset"),
        "td_add" => perch_data::t("cards.tip_td_add"),
        "td_clear" => perch_data::t("cards.tip_td_clear"),
        "r_cal" => perch_data::t("cards.tip_r_cal"),
        "r_slot" => perch_data::t("cards.tip_r_slot"),
        "r_del" => perch_data::t("cards.tip_r_del"),
        "r_tab_cal" => perch_data::t("cards.tip_r_tab_cal"),
        "r_tab_list" => perch_data::t("cards.tip_r_tab_list"),
        "m_prev" => perch_data::t("cards.tip_m_prev"),
        "m_play" => perch_data::t("cards.tip_m_play"),
        "m_next" => perch_data::t("cards.tip_m_next"),
        "m_mute" => perch_data::t("cards.tip_m_mute"),
        "m_vdn" => perch_data::t("cards.tip_m_vdn"),
        "m_vup" => perch_data::t("cards.tip_m_vup"),
        "m_slider" => perch_data::t("cards.tip_m_slider"),
        "m_seek" => perch_data::t("cards.tip_m_seek"),
        "w200" => perch_data::t("cards.tip_w200"),
        "w300" => perch_data::t("cards.tip_w300"),
        "w500" => perch_data::t("cards.tip_w500"),
        "w_undo" => perch_data::t("cards.tip_w_undo"),
        "quotes" => perch_data::t("cards.tip_quotes"),
        "c_net" => perch_data::t("cards.tip_c_net"),
        "c_bt" => perch_data::t("cards.tip_c_bt"),
        "c_lock" => perch_data::t("cards.tip_c_lock"),
        "c_clean" => perch_data::t("cards.tip_c_clean"),
        "c_bell" => perch_data::t("cards.tip_c_bell"),
        "c_theme" => perch_data::t("cards.tip_c_theme"),
        "sys_card" => perch_data::t("cards.tip_sys_card"),
        "c_tools" => perch_data::t("cards.tip_c_tools"),
        "c_set" => perch_data::t("cards.tip_c_set"),
        "c_search" => perch_data::t("cards.tip_c_search"),
        "notify_card" => perch_data::t("cards.tip_notify_card"),
        _ if id.starts_with("te") && id.len() == 3 => perch_data::t("cards.tip_edit_todo"),
        _ if id.starts_with("tx") && id.len() == 3 => perch_data::t("cards.tip_del_todo"),
        _ if id.starts_with("td") && id.len() == 3 => perch_data::t("cards.tip_toggle_todo"),
        _ => return None,
    };
    Some(t)
}

/// 提示框矩形(DIP),同时作为窗口区域的临时扩展
pub fn tooltip_rect() -> Option<(f32, f32, f32, f32, f32)> {
    let id = current_tip()?;
    let text = tip_text(id)?;
    let b = crate::window::HIT_BOXES.lock().ok()?.iter().find(|b| b.id == id).copied()?;
    let w = text.chars().map(|c| if c.is_ascii() { 6.6 } else { 12.0 }).sum::<f32>() + 20.0;
    let cx = (b.x + b.w / 2.0 - w / 2.0).clamp(4.0, (sw() - w - 4.0).max(4.0));
    let y = if b.y > 60.0 { b.y - 32.0 } else { b.y + b.h + 6.0 };
    Some((cx, y, w, 24.0, 8.0))
}

fn bx(id: &'static str, card: (f32, f32, f32, f32), x: f32, y: f32, w: f32, h: f32) -> RectBox {
    RectBox { id, x: card.0 + x, y: card.1 + y, w, h }
}

/// 所有可点击区域(绝对坐标)
pub fn hit_boxes(cal_open: bool) -> Vec<RectBox> {
    let mut v = Vec::new();
    // 日历抽屉展开时盖在待办卡片上,先登记以便优先命中
    if cal_open {
        v.push(bx("r_cal", remind_rect(), 176.0, 10.0, 72.0, 18.0));
        v.push(bx("r_del", remind_rect(), 226.0, 34.0, 22.0, 22.0));
        v.push(bx("r_slot", remind_rect(), 12.0, 34.0, 236.0, 22.0));
        v.push(bx("r_tab_cal", remind_rect(), 14.0, 62.0, 54.0, 16.0));
        v.push(bx("r_tab_list", remind_rect(), 72.0, 62.0, 54.0, 16.0));
        for i in 0..5 {
            v.push(bx(REM_DEL[i], remind_rect(), 224.0, 84.0 + 22.0 * i as f32, 22.0, 20.0));
        }
        v.push(bx("rem_body", remind_rect(), 0.0, 80.0, 260.0, 120.0));
        v.push(bx("r_panel", remind_rect(), 0.0, 0.0, 260.0, 200.0));
    }
    v.push(bx("battery", battery_rect(), 0.0, 0.0, 260.0, 26.0));
    if !cal_open {
        v.push(bx("r_cal", remind_rect(), 176.0, 10.0, 72.0, 18.0));
        v.push(bx("r_del", remind_rect(), 226.0, 34.0, 22.0, 22.0));
        v.push(bx("r_slot", remind_rect(), 12.0, 34.0, 236.0, 22.0));
    }
    v.push(bx("td_add", todo_rect(), 194.0, 8.0, 54.0, 20.0));
    for (id, x, _, _) in presets() {
        v.push(bx(id, timer_rect(), x, 34.0, 30.0, 20.0));
    }
    v.push(bx("t_main", timer_rect(), 122.0, 60.0, 62.0, 28.0));
    v.push(bx("t_reset", timer_rect(), 190.0, 60.0, 60.0, 28.0));
    v.push(bx("td_clear", todo_rect(), 160.0, 6.0, 30.0, 22.0));
    // 编辑/删除先登记,优先于整行的"切换完成"
    for i in 0..4 {
        let y = 34.0 + 26.0 * i as f32;
        v.push(bx(TODO_EDIT[i], todo_rect(), 204.0, y, 20.0, 22.0));
        v.push(bx(TODO_DEL[i], todo_rect(), 224.0, y, 22.0, 22.0));
    }
    for (i, id) in TODO_IDS.iter().enumerate() {
        v.push(bx(id, todo_rect(), 12.0, 34.0 + 26.0 * i as f32, 236.0, 22.0));
    }
    // 整张待办卡片的兜底区域(鼠标滚轮在卡片任意位置都能滚动列表)
    v.push(bx("todo_body", todo_rect(), 0.0, 30.0, 260.0, 110.0));
    v.push(bx("m_seek", media_rect(), 44.0, 82.0, 172.0, 16.0));
    v.push(bx("m_prev", media_rect(), 174.0, 48.0, 22.0, 26.0));
    v.push(bx("m_play", media_rect(), 200.0, 46.0, 28.0, 30.0));
    v.push(bx("m_next", media_rect(), 232.0, 48.0, 22.0, 26.0));
    v.push(bx("m_mute", media_rect(), 14.0, 110.0, 30.0, 26.0));
    v.push(bx("m_vdn", media_rect(), 50.0, 110.0, 26.0, 26.0));
    v.push(bx("m_slider", media_rect(), 84.0, 106.0, 104.0, 34.0));
    v.push(bx("m_vup", media_rect(), 194.0, 110.0, 26.0, 26.0));
    for (i, b) in [("w200", 68.0), ("w300", 118.0), ("w500", 168.0)].iter().enumerate() {
        let _ = i;
        v.push(bx(b.0, water_rect(), b.1, 88.0, 46.0, 20.0));
    }
    v.push(bx("w_undo", water_rect(), 218.0, 88.0, 30.0, 20.0));
    v.push(bx("quotes", quotes_rect(), 0.0, 0.0, 280.0, 96.0));
    v.push(bx("sys_card", system_rect(), 0.0, 0.0, 260.0, 270.0));
    for (i, (id, x, w)) in [("c_net", 8.0, 58.0), ("c_bt", 70.0, 58.0), ("c_lock", 132.0, 58.0), ("c_clean", 194.0, 58.0), ("c_bell", 256.0, 32.0), ("c_tools", 292.0, 32.0), ("c_set", 328.0, 32.0), ("c_search", 364.0, 32.0), ("c_theme", 400.0, 76.0)].iter().enumerate() {
        let _ = i;
        v.push(bx(id, controls_rect(), *x, 4.0, *w, 26.0));
    }
    for i in 0..config::get().dock.len() {
        v.push(bx(DOCK_IDS[i], dock_rect(), 20.0 + 52.0 * i as f32, 4.0, 46.0, 48.0));
    }
    let mask = hidden_mask();
    v.retain(|b| card_of_hit(b.id).map_or(true, |i| mask & (1 << i) == 0));
    v
}

/// 换装面板打开时额外的可点击区域(放在最前面优先命中)
pub fn selector_boxes() -> Vec<RectBox> {
    let mut v = Vec::new();
    let themes = perch_data::theme_pack::list();
    for (i, pack) in themes.iter().take(12).enumerate() {
        let col = (i % 2) as f32;
        let row = (i / 2) as f32;
        let sid: &'static str = Box::leak(format!("th_{}", pack.id).into_boxed_str());
        v.push(bx(sid, selector_rect(), 16.0 + 152.0 * col, 50.0 + 60.0 * row, 136.0, 52.0));
    }
    for (id, _, _, x, w, y, _) in SEL_BUTTONS.iter() {
        if *w > 0.0 {
            v.push(bx(id, selector_rect(), *x, *y, *w, if *id == "sel_close" { 22.0 } else { 24.0 }));
        }
    }
    v
}

/// 窗口区域:所有卡片(外加通知条)
pub fn regions(cal_open: bool, r: f32) -> Vec<(f32, f32, f32, f32, f32)> {
    let all = vec![
        (battery_rect().0, battery_rect().1, battery_rect().2, battery_rect().3, 13.0),
        (timer_rect().0, timer_rect().1, timer_rect().2, timer_rect().3, r),
        (remind_rect().0, remind_rect().1, remind_rect().2, if cal_open { 200.0 } else { remind_rect().3 }, r),
        (todo_rect().0, todo_rect().1, todo_rect().2, todo_rect().3, r),
        (media_rect().0, media_rect().1, media_rect().2, media_rect().3, r),
        (water_rect().0, water_rect().1, water_rect().2, water_rect().3, r),
        (clock_rect().0, clock_rect().1, clock_rect().2, clock_rect().3, r),
        (weather_rect().0, weather_rect().1, weather_rect().2, weather_rect().3, r),
        (system_rect().0, system_rect().1, system_rect().2, system_rect().3, r),
        (controls_rect().0, controls_rect().1, controls_rect().2, controls_rect().3, 17.0),
        (dock_rect().0, dock_rect().1, dock_rect().2, dock_rect().3, 28.0),
        (quotes_rect().0, quotes_rect().1, quotes_rect().2, quotes_rect().3, 10.0),
    ];
    let mask = hidden_mask();
    all.into_iter().enumerate().filter(|(i, _)| mask & (1 << i) == 0).map(|(_, r)| r).collect()
}

pub fn selector_region() -> (f32, f32, f32, f32, f32) {
    (selector_rect().0, selector_rect().1, selector_rect().2, selector_rect().3, 12.0)
}

pub fn tactical_regions() -> Vec<(f32, f32, f32, f32, f32)> {
    vec![(header_rect().0, header_rect().1, header_rect().2, header_rect().3, 6.0), (status_rect().0, status_rect().1, status_rect().2, status_rect().3, 6.0)]
}

// ---------------- 绘制辅助 ----------------
pub(crate) struct P<'a> {
    pub(crate) r: &'a mut D2dRenderer,
    pub(crate) th: &'a ThemeConfig,
    pub(crate) st: &'a ThemeStyle,
    pub(crate) hover: Option<&'static str>,
    pub(crate) ox: f32,
    pub(crate) oy: f32,
}

pub(crate) const L: DWRITE_TEXT_ALIGNMENT = DWRITE_TEXT_ALIGNMENT_LEADING;
pub(crate) const C: DWRITE_TEXT_ALIGNMENT = DWRITE_TEXT_ALIGNMENT_CENTER;
pub(crate) const R: DWRITE_TEXT_ALIGNMENT = DWRITE_TEXT_ALIGNMENT_TRAILING;

impl<'a> P<'a> {
    pub(crate) fn at(&mut self, card: (f32, f32, f32, f32)) {
        self.ox = card.0;
        self.oy = card.1;
    }
    pub(crate) fn col(&self, c: &ColorRgba) -> D2D1_COLOR_F {
        c.to_d2d()
    }
    pub(crate) fn alpha(&self, c: &ColorRgba, a: u8) -> D2D1_COLOR_F {
        c.with_alpha(a)
    }
    pub(crate) fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, rad: f32, fill: D2D1_COLOR_F, stroke: D2D1_COLOR_F, sw: f32) -> Result<()> {
        self.r.draw_card(self.ox + x, self.oy + y, w, h, rad, fill, stroke, sw)
    }
    pub(crate) fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, c: D2D1_COLOR_F, w: f32) -> Result<()> {
        self.r.draw_line(self.ox + x1, self.oy + y1, self.ox + x2, self.oy + y2, c, w)
    }
    pub(crate) fn txt(&mut self, x: f32, y: f32, s: &str, pt: f32, c: D2D1_COLOR_F, al: DWRITE_TEXT_ALIGNMENT, num: bool) -> Result<()> {
        let font = if num { self.st.font_num.clone() } else { self.st.font_main.clone() };
        self.r.text_at(self.ox + x, self.oy + y, s, pt, c, &font, true, al)
    }
    pub(crate) fn hot(&self, id: &str) -> bool {
        self.hover == Some(id)
    }
    /// 卡片底板 + 样式包装饰
    fn card(&mut self, w: f32, h: f32) -> Result<()> {
        let (th, st) = (self.th, self.st);
        self.rect(0.0, 0.0, w, h, st.radius, th.bg.to_d2d(), st.frame_color(&th.stroke), st.frame_w)?;
        for d in [&st.shape2, &st.corner].into_iter().flatten() {
            self.r.draw_deco(d, self.ox, self.oy, w / 260.0)?;
        }
        Ok(())
    }
    /// 标题前的小竖条 + 标题(标题末尾的 tag 用副色、小号、数字字体)
    fn header(&mut self, tag_y: f32, tag_h: f32, title: &str, tag: &str, x: f32, y: f32, pt: f32, tag_pt: f32) -> Result<()> {
        let th = self.th;
        self.rect(12.0, tag_y, 3.0, tag_h, 1.5, self.col(&th.sub), self.alpha(&th.sub, 0), 0.0)?;
        self.title_tag(x, y, title, tag, pt, tag_pt)
    }

    fn title_tag(&mut self, x: f32, y: f32, title: &str, tag: &str, pt: f32, tag_pt: f32) -> Result<()> {
        let th = self.th;
        let (fm, fnum) = (self.st.font_main.clone(), self.st.font_num.clone());
        let split = if tag.is_empty() { None } else { title.rfind(tag) };
        let (head, tail) = match split {
            Some(i) => (&title[..i], &title[i..]),
            None => (title, ""),
        };
        let (ox, oy) = (self.ox, self.oy);
        self.r.text_at(ox + x, oy + y, head, pt, th.accent.to_d2d(), &fm, true, L)?;
        if !tail.is_empty() {
            let w = self.r.measure(head, &fm, pt * 4.0 / 3.0, true);
            self.r.text_at(ox + x + w, oy + y + (pt - tag_pt) * 1.1, tail, tag_pt, th.sub.to_d2d(), &fnum, true, L)?;
        }
        Ok(())
    }
    /// 通用按钮:悬停时填充 item_hover、描边 accent
    pub(crate) fn btn(&mut self, id: &'static str, x: f32, y: f32, w: f32, h: f32, rad: f32, label: &str, pt: f32, tc: D2D1_COLOR_F, fill: D2D1_COLOR_F, stroke: D2D1_COLOR_F, ty: f32) -> Result<()> {
        let th = self.th;
        let (f, s) = if self.hot(id) { (self.col(&th.item_hover), self.col(&th.accent)) } else { (fill, stroke) };
        self.rect(x, y, w, h, rad, f, s, 1.0)?;
        self.txt(x + w / 2.0, y + ty, label, pt, tc, C, true)
    }
    pub(crate) fn track(&self) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 28.0 / 255.0 }
    }
}

fn fmt_gb(v: f32) -> String {
    format!("{:.1} GB", v)
}

pub struct View<'a> {
    pub th: &'a ThemeConfig,
    pub st: &'a ThemeStyle,
    pub app: &'a AppState,
    pub hub: &'a Hub,
    pub sys: &'a SystemMetrics,
    pub disk_c: (f32, f32),
    pub disk_d: (f32, f32),
    pub vol: (f32, bool),
    pub gpu: Option<crate::gpu::GpuInfo>,
    pub cleaning: bool,
    pub hover: Option<&'static str>,
}

pub fn draw(r: &mut D2dRenderer, v: &View) -> Result<()> {
    let mut p = P { r, th: v.th, st: v.st, hover: v.hover, ox: 0.0, oy: 0.0 };
    let mask = hidden_mask();
    let on = |i: usize| mask & (1 << i) == 0;
    if on(0) {
        battery(&mut p, v)?;
    }
    if on(1) {
        timer(&mut p, v)?;
    }
    if !v.app.cal_open && on(2) {
        reminders(&mut p, v)?;
    }
    if on(3) {
        todo(&mut p, v)?;
    }
    if v.app.cal_open && on(2) {
        reminders(&mut p, v)?;
    }
    if on(4) {
        media(&mut p, v)?;
    }
    if on(5) {
        water(&mut p, v)?;
    }
    if on(6) {
        clock(&mut p, v)?;
    }
    if on(7) {
        weather(&mut p, v)?;
    }
    if on(8) {
        system(&mut p, v)?;
    }
    if on(11) {
        quotes(&mut p, v)?;
    }
    if on(9) {
        controls(&mut p, v)?;
    }
    if on(10) {
        dock(&mut p, v)?;
    }
    if v.app.chrome_tactical {
        tactical_bars(&mut p, v)?;
    }
    if v.app.edit_layout {
        edit_overlay(&mut p, v)?;
    }
    dock_tip(&mut p, v)?;
    crate::preview::draw(&mut p)?;
    notify(&mut p, v)?;
    if v.app.notif_open {
        notif_panel(&mut p, v)?;
    }
    if v.app.selector_open {
        selector(&mut p, v)?;
    }
    if v.app.settings_open {
        crate::settings::draw(&mut p, v)?;
    }
    if v.app.tools_open {
        crate::tools::draw(&mut p, v)?;
    }
    if v.app.procs_open {
        crate::procs::draw(&mut p, v)?;
    }
    Ok(())
}

fn tactical_bars(p: &mut P, v: &View) -> Result<()> {
    let th = p.th;
    let faint = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 35.0 / 255.0 };
    // Header
    p.at(header_rect());
    p.rect(0.0, 0.0, 260.0, 32.0, 6.0, p.col(&th.bg), p.col(&th.stroke), 1.0)?;
    p.rect(0.0, 0.0, 12.0, 12.0, 0.0, p.col(&th.accent), p.alpha(&th.accent, 0), 0.0)?;
    p.line(10.0, 1.0, 250.0, 1.0, faint, 1.0)?;
    p.txt(14.0, 8.0, &v.app.chrome_title, 9.0, p.col(&th.accent), L, false)?;
    p.txt(74.0, 10.0, &v.app.chrome_sub, 6.5, p.col(&th.white), L, true)?;
    p.rect(214.0, 8.0, 38.0, 15.0, 3.0, p.alpha(&th.accent, 40), p.col(&th.accent), 1.0)?;
    p.txt(233.0, 9.0, "ONLINE", 5.5, p.col(&th.accent), C, true)?;
    // status_rect()
    p.at(status_rect());
    p.rect(0.0, 0.0, 260.0, 32.0, 6.0, p.col(&th.bg), p.col(&th.stroke), 1.0)?;
    p.rect(248.0, 0.0, 12.0, 12.0, 0.0, p.col(&th.sub), p.alpha(&th.sub, 0), 0.0)?;
    p.line(10.0, 1.0, 250.0, 1.0, faint, 1.0)?;
    for (i, (x, c)) in [(8.0, th.accent), (92.0, th.sub), (176.0, th.white)].into_iter().enumerate() {
        let label = v.app.chrome_badges.get(i).map(|s| s.as_str()).unwrap_or("");
        let edge = if x == 92.0 { th.sub } else { th.accent };
        p.rect(x, 8.0, 76.0, 16.0, 3.0, p.col(&th.item_bg), edge.with_alpha(90), 1.0)?;
        p.txt(x + 38.0, 10.0, label, 5.5, p.col(&c), C, true)?;
    }
    Ok(())
}

fn notif_panel(p: &mut P, v: &View) -> Result<()> {
    p.at(notif_rect());
    let th = p.th;
    p.rect(0.0, 0.0, 320.0, 360.0, 12.0, th.bg.with_alpha(250), p.col(&th.stroke), 1.5)?;
    p.line(12.0, 1.0, 308.0, 1.0, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 35.0 / 255.0 }, 1.0)?;
    p.txt(16.0, 12.0, &perch_data::t("notif.title"), 9.5, p.col(&th.accent), L, false)?;
    let hot = p.hot("n_clear");
    p.rect(246.0, 10.0, 56.0, 20.0, 5.0, if hot { p.col(&th.item_hover) } else { p.col(&th.item_bg) }, p.col(&th.stroke), 1.0)?;
    p.txt(274.0, 13.0, &perch_data::t("notif.clear"), 8.0, p.col(&th.white), C, false)?;
    p.txt(311.0, 12.0, "✕", 9.0, p.col(&th.muted), R, false)?;
    let items = &v.hub.history;
    if items.is_empty() {
        p.txt(160.0, 170.0, &perch_data::t("notif.empty"), 9.0, p.col(&th.muted), C, false)?;
    }
    for (row, it) in items.iter().skip(v.hub.history_scroll).take(8).enumerate() {
        let y = 44.0 + 38.0 * row as f32;
        p.rect(12.0, y, 296.0, 34.0, 6.0, p.col(&th.item_bg), p.col(&th.stroke), 1.0)?;
        // 时间 HH:MM(本地时间)
        let secs = it.0 as i64;
        let tm = chrono::DateTime::from_timestamp(secs, 0).map(|d| d.with_timezone(&Local).format("%m-%d %H:%M").to_string()).unwrap_or_default();
        p.txt(20.0, y + 4.0, &clip(&it.1, 18), 8.0, p.col(&th.accent), L, true)?;
        p.txt(260.0, y + 5.0, &tm, 6.5, p.col(&th.sub), R, true)?;
        p.txt(20.0, y + 18.0, &clip(&it.2, 34), 7.5, p.col(&th.muted), L, false)?;
        let del_c = if p.hot(NH_DEL[row]) { ColorRgba::new(255, 120, 120, 255).to_d2d() } else { ColorRgba::new(239, 68, 68, 200).to_d2d() };
        p.txt(292.0, y + 9.0, "✕", 8.0, del_c, L, true)?;
    }
    if items.len() > 8 {
        let frac = 8.0 / items.len() as f32;
        let pos = v.hub.history_scroll as f32 / (items.len() - 8) as f32;
        let tr = p.track();
        p.rect(313.0, 44.0, 3.0, 304.0, 1.5, tr, tr, 0.0)?;
        p.rect(313.0, 44.0 + 304.0 * (1.0 - frac) * pos, 3.0, 304.0 * frac, 1.5, p.col(&th.accent), p.col(&th.accent), 0.0)?;
    }
    Ok(())
}

fn selector(p: &mut P, v: &View) -> Result<()> {
    p.at(selector_rect());
    let (th, st) = (p.th, p.st);
    let faint = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 35.0 / 255.0 };
    p.rect(0.0, 0.0, 320.0, 556.0, 12.0, p.col(&th.bg), p.col(&th.stroke), 1.5)?;
    p.line(12.0, 1.0, 308.0, 1.0, faint, 1.0)?;
    p.line(300.0, 1.0, 319.0, 20.0, p.col(&th.accent), 2.0)?;
    p.txt(16.0, 12.0, &perch_data::t("theme_selector.hub_title"), 9.5, p.col(&th.accent), L, false)?;
    p.txt(16.0, 28.0, "THEME CUSTOMIZATION HUD", 7.5, p.col(&th.sub), L, false)?;
    p.txt(304.0, 12.0, "✕", 9.0, p.col(&th.muted), R, false)?;
    let white = p.col(&th.white);
    let themes = perch_data::theme_pack::list();
    let current_badge_txt = perch_data::t("theme_selector.current_badge");
    for (i, pack) in themes.iter().take(12).enumerate() {
        let col = (i % 2) as f32;
        let row = (i / 2) as f32;
        let (x, y) = (16.0 + 152.0 * col, 50.0 + 60.0 * row);
        let sid: &'static str = Box::leak(format!("th_{}", pack.id).into_boxed_str());
        let hot = p.hot(sid);
        let (r, g, b) = if pack.meta.preview.len() >= 3 {
            (pack.meta.preview[0], pack.meta.preview[1], pack.meta.preview[2])
        } else {
            (128, 128, 128)
        };
        let fill = ColorRgba::new(r, g, b, if hot { 70 } else { 30 });
        p.rect(x, y, 136.0, 52.0, 6.0, p.col(&fill), ColorRgba::new(r, g, b, if hot { 230 } else { 100 }).to_d2d(), 1.0)?;
        p.txt(x + 8.0, y + 8.0, &pack.meta.name, 8.5, ColorRgba::new(r, g, b, 255).to_d2d(), L, false)?;
        p.txt(x + 8.0, y + 28.0, &pack.meta.description, 7.5, white, L, false)?;
        if v.app.current_theme_name == pack.id {
            p.txt(x + 128.0, y + 8.0, &current_badge_txt, 8.0, ColorRgba::new(255, 255, 255, 235).to_d2d(), R, false)?;
        }
    }
    p.txt(16.0, 296.0, &perch_data::t("theme_selector.wp_mode"), 8.0, p.col(&th.muted), L, false)?;
    p.txt(16.0, 382.0, &perch_data::t("theme_selector.rotate_hint"), 8.0, p.col(&th.muted), L, false)?;
    p.txt(16.0, 436.0, &perch_data::t("theme_selector.fx_particles"), 8.0, p.col(&th.muted), L, false)?;
    for (id, _, label, x, w, y, key) in SEL_BUTTONS.iter() {
        if *id == "sel_close" || *id == "sel_dummy" {
            continue;
        }
        if *id == "sel_settings" {
            let hot = p.hot(id);
            let (f, s) = if hot { (p.col(&th.item_hover), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
            p.rect(*x, *y, *w, 26.0, 6.0, f, s, 1.0)?;
            p.txt(x + w / 2.0, y + 6.0, &perch_data::t("theme_selector.more_settings"), 8.0, white, C, false)?;
            continue;
        }
        let (k, val) = key.split_once('=').unwrap_or(("", ""));
        let active = if k == "ambient" { v.hub.ambient == val } else { Hub::v(&v.hub.modes, k) == val };
        let (f, s, sw) = if active { (p.col(&th.stroke), p.col(&th.accent), 1.5) } else { (p.col(&th.item_bg), p.col(&th.stroke), 1.0) };
        let (f, s) = if p.hot(id) { (p.col(&th.item_hover), p.col(&th.accent)) } else { (f, s) };
        p.rect(*x, *y, *w, 24.0, 6.0, f, s, sw)?;
        if *id == "sel_bgm" {
            let t = if active { perch_data::t("theme_selector.bgm_sync_on") } else { perch_data::t("theme_selector.bgm_sync_off") };
            let c = if active { white } else { p.col(&th.muted) };
            p.txt(160.0, 351.0, &t, 8.5, c, C, false)?;
        } else {
            p.txt(x + w / 2.0, y + 5.0, &perch_data::t(label), 8.5, white, C, false)?;
        }
    }
    let _ = st;
    Ok(())
}

fn battery(p: &mut P, v: &View) -> Result<()> {
    p.at(battery_rect());
    let th = p.th;
    p.rect(0.0, 0.0, 260.0, 26.0, 13.0, p.col(&th.bg), p.col(&th.stroke), 1.0)?;
    let icon = Hub::v(&v.hub.battery, "MouseBatteryIcon").to_string();
    p.txt(10.0, 5.0, &icon, 8.5, p.col(&th.sub), L, false)?;
    let val = Hub::v(&v.hub.battery, "MouseBatteryText").to_string();
    let c = parse_color(Hub::v(&v.hub.battery, "MouseBatteryColor")).unwrap_or(th.white);
    p.txt(30.0, 6.0, &val, 8.5, p.col(&c), L, true)?;
    let label = Hub::v(&v.hub.battery, "MouseBatteryStatus").split(':').next().unwrap_or("").trim().to_uppercase();
    p.txt(248.0, 7.0, &label, 8.0, p.col(&th.muted), R, true)
}

fn timer(p: &mut P, v: &View) -> Result<()> {
    p.at(timer_rect());
    let (th, st) = (p.th, p.st);
    p.card(260.0, 108.0)?;
    p.r.draw_ring(p.ox + 56.0, p.oy + 54.0, 38.0, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 15.0 / 255.0 }, 3.0);
    // 进度环:已走过的部分用 accent 画弧(以短线近似),空闲时整圈
    let total = v.app.timer_total_secs().max(1) as f32;
    let frac = if v.app.timer_state == TimerState::Idle { 1.0 } else { (v.app.timer_remaining_secs as f32 / total).clamp(0.0, 1.0) };
    let steps = 72;
    let n = (steps as f32 * frac).round() as i32;
    for i in 0..n {
        let a0 = -std::f32::consts::FRAC_PI_2 + (i as f32 / steps as f32) * std::f32::consts::TAU;
        let a1 = -std::f32::consts::FRAC_PI_2 + ((i + 1) as f32 / steps as f32) * std::f32::consts::TAU;
        p.line(56.0 + 38.0 * a0.cos(), 54.0 + 38.0 * a0.sin(), 56.0 + 38.0 * a1.cos(), 54.0 + 38.0 * a1.sin(), p.col(&th.accent), 3.0)?;
    }
    let time = format!("{:02}:{:02}", v.app.timer_remaining_secs / 60, v.app.timer_remaining_secs % 60);
    p.txt(56.0, 40.0, &time, 16.0, p.col(&th.white), C, true)?;
    let status = match (v.app.timer_state, v.app.timer_break) {
        (TimerState::Idle, _) => "STANDBY",
        (TimerState::Running, None) => "FOCUS",
        (TimerState::Running, Some(false)) => "BREAK",
        (TimerState::Running, Some(true)) => "LONG BREAK",
        (TimerState::Paused, _) => "PAUSED",
    };
    p.txt(56.0, 65.0, status, 7.5, p.col(&th.muted), C, false)?;
    p.rect(114.0, 14.0, 3.0, 14.0, 1.5, p.col(&th.sub), p.alpha(&th.sub, 0), 0.0)?;
    let title = st.t("ThemeTitleTimer", "⏱ 战术番茄钟  POMO").to_string();
    let tag = st.t("ThemeSubTimer", "POMO").to_string();
    p.title_tag(122.0, 13.0, &title, &tag, 9.0, 8.0)?;
    for (id, x, mins, label) in presets() {
        let on = v.app.timer_preset_mins == mins;
        let (f, s) = if on { (p.col(&th.stroke), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
        p.btn(id, x, 34.0, 30.0, 20.0, 4.0, &label, 8.0, p.col(&th.white), f, s, 4.0)?;
    }
    let (label_str, running) = match v.app.timer_state {
        TimerState::Idle => (perch_data::t("timer.btn_start"), false),
        TimerState::Running => (perch_data::t("timer.btn_pause"), true),
        TimerState::Paused => (perch_data::t("timer.btn_resume"), false),
    };
    let main_fill = if running { p.col(&th.stroke) } else { p.col(&th.accent) };
    let main_txt = if running { p.col(&th.white) } else { p.col(&th.on_accent) };
    p.btn("t_main", 122.0, 60.0, 62.0, 28.0, 5.0, &label_str, 8.0, main_txt, main_fill, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 50.0 / 255.0 }, 7.0)?;
    if v.app.pomo_count > 0 {
        let summary = perch_data::t_fmt("timer.today_summary", &[("count", &v.app.pomo_count.to_string()), ("mins", &v.app.pomo_minutes.to_string())]);
        p.txt(122.0, 92.0, &summary, 7.0, p.col(&th.muted), L, false)?;
    }
    let reset_lbl = perch_data::t("timer.btn_reset");
    p.btn("t_reset", 190.0, 60.0, 60.0, 28.0, 5.0, &reset_lbl, 8.0, p.col(&th.white), p.col(&th.item_bg), p.col(&th.stroke), 7.0)
}

fn reminders(p: &mut P, v: &View) -> Result<()> {
    p.at(remind_rect());
    let (th, st) = (p.th, p.st);
    let open = v.app.cal_open;
    if open {
        p.rect(0.0, 0.0, 260.0, 200.0, 10.0, th.bg.with_alpha(250), p.col(&th.accent), 1.5)?;
        for d in [&st.shape2, &st.corner].into_iter().flatten() {
            p.r.draw_deco(d, p.ox, p.oy, 1.0)?;
        }
    } else {
        p.card(260.0, 62.0)?;
    }
    p.header(12.0, 16.0, st.t("ThemeTitleEvents", "📅 战术日程  EVENTS"), st.t("ThemeSubEvents", "EVENTS"), 20.0, 12.0, 9.0, 8.0)?;
    let (f, s) = if p.hot("r_cal") { (p.col(&th.item_hover), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
    p.rect(176.0, 10.0, 72.0, 18.0, 4.0, f, s, 1.0)?;
    let cal_toggle_txt = if open { perch_data::t("remind.collapse_cal") } else { perch_data::t("remind.expand_cal") };
    p.txt(212.0, 13.0, &cal_toggle_txt, 7.5, p.col(&th.white), C, false)?;
    let (f, s) = if p.hot("r_slot") { (p.col(&th.item_hover), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
    p.rect(12.0, 34.0, 236.0, 22.0, 4.0, f, s, 1.0)?;
    let rt = Hub::v(&v.hub.reminders, "ReminderText").to_string();
    let time = Hub::v(&v.hub.reminders, "ReminderTime").to_string();
    let fired = Hub::v(&v.hub.reminders, "ReminderFired") == "1";
    let count: u32 = Hub::v(&v.hub.reminders, "ReminderCount").parse().unwrap_or(0);
    let has = !rt.is_empty();
    let text = if !has {
        perch_data::t("remind.add_prompt")
    } else if time.is_empty() {
        clip(&rt, 24)
    } else {
        format!("{}{}  {}", if fired { "✓ " } else { "" }, time, clip(&rt, 20))
    };
    let tc = if fired { p.col(&th.muted) } else { p.col(&th.white) };
    p.txt(20.0, 38.0, &text, 8.0, tc, L, false)?;
    if has {
        if count > 1 {
            p.txt(222.0, 38.0, &format!("+{}", count - 1), 7.5, p.col(&th.sub), R, true)?;
        }
        let del_c = if p.hot("r_del") { ColorRgba::new(255, 120, 120, 255).to_d2d() } else { ColorRgba::new(239, 68, 68, 220).to_d2d() };
        p.txt(232.0, 38.0, "✕", 8.0, del_c, L, true)?;
    }
    if open {
        p.line(14.0, 60.0, 246.0, 60.0, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 25.0 / 255.0 }, 1.0)?;
        let tab_cal_lbl = perch_data::t("remind.tab_cal");
        let tab_list_lbl = perch_data::t("remind.tab_list");
        for (id, x, label, tab) in [("r_tab_cal", 14.0, &tab_cal_lbl, 0u8), ("r_tab_list", 72.0, &tab_list_lbl, 1u8)] {
            let on = v.app.cal_tab == tab;
            let (f, s) = if on { (p.col(&th.stroke), p.col(&th.accent)) } else if p.hot(id) { (p.col(&th.item_hover), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
            p.rect(x, 62.0, 54.0, 16.0, 4.0, f, s, 1.0)?;
            p.txt(x + 27.0, 64.0, label, 7.0, p.col(&th.white), C, false)?;
        }
        if v.app.cal_tab == 0 {
            let pitch = 33.5;
            let now = Local::now();
            for (i, name) in perch_data::t("cards2.weekday_short").split(',').enumerate() {
                p.txt(30.0 + pitch * i as f32, 84.0, name, 6.8, p.col(&th.sub), C, false)?;
            }
            let first = chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap();
            let offset = first.weekday().num_days_from_monday() as i32;
            let days = {
                let (y, m) = if now.month() == 12 { (now.year() + 1, 1) } else { (now.year(), now.month() + 1) };
                (chrono::NaiveDate::from_ymd_opt(y, m, 1).unwrap() - first).num_days() as i32
            };
            let busy = crate::calendar::days_with_events(now.year(), now.month());
            for d in 1..=days {
                let idx = offset + d - 1;
                let (row, col) = (idx / 7, idx % 7);
                let (cx, y) = (30.0 + pitch * col as f32, 102.0 + 16.0 * row as f32);
                if busy.contains(&(d as u32)) {
                    // 日历订阅里这天有事件:数字下面一个小点
                    p.rect(cx - 1.5, y + 12.5, 3.0, 3.0, 1.5, p.col(&th.sub), p.col(&th.sub), 0.0)?;
                }
                if d as u32 == now.day() {
                    p.rect(cx - 13.0, y - 2.0, 26.0, 15.0, 4.0, p.alpha(&th.accent, 70), p.col(&th.accent), 1.0)?;
                    p.txt(cx, y, &format!("{:02}", d), 8.0, p.col(&th.white), C, true)?;
                } else {
                    p.txt(cx, y, &format!("{:02}", d), 8.0, p.col(&th.muted), C, false)?;
                }
            }
        } else {
            // 本地日程(可删除)+ 日历订阅里的事件(只读,📅 开头)
            let items = &v.hub.reminder_items;
            let cal = crate::calendar::upcoming();
            let total = items.len() + cal.len();
            if total == 0 {
                p.txt(130.0, 124.0, &perch_data::t("remind.empty"), 8.0, p.col(&th.muted), C, false)?;
            }
            for row in 0..5usize {
                let idx = v.hub.rem_scroll + row;
                if idx >= total {
                    break;
                }
                let y = 84.0 + 22.0 * row as f32;
                p.rect(12.0, y, 236.0, 20.0, 4.0, p.col(&th.item_bg), p.col(&th.stroke), 1.0)?;
                if let Some(it) = items.get(idx) {
                    let label = if it.1.is_empty() { clip(&it.2, 26) } else { format!("{}{}  {}", if it.3 { "✓ " } else { "" }, it.1, clip(&it.2, 20)) };
                    let tc = if it.3 { p.col(&th.muted) } else { p.col(&th.white) };
                    p.txt(20.0, y + 3.0, &label, 8.0, tc, L, false)?;
                    let del_c = if p.hot(REM_DEL[row]) { ColorRgba::new(255, 120, 120, 255).to_d2d() } else { ColorRgba::new(239, 68, 68, 220).to_d2d() };
                    p.txt(232.0, y + 3.0, "✕", 8.0, del_c, L, true)?;
                } else if let Some(e) = cal.get(idx - items.len()) {
                    let when = if e.all_day { format!("{} {}", e.start.format("%m-%d"), perch_data::t("remind.all_day")) } else { e.start.format("%m-%d %H:%M").to_string() };
                    p.txt(20.0, y + 3.0, &format!("📅 {}  {}", when, clip(&e.title, 14)), 8.0, p.col(&th.sub), L, false)?;
                }
            }
            if total > 5 {
                let frac = 5.0 / total as f32;
                let pos = v.hub.rem_scroll as f32 / (total - 5) as f32;
                let tr = p.track();
                p.rect(252.0, 84.0, 3.0, 108.0, 1.5, tr, tr, 0.0)?;
                p.rect(252.0, 84.0 + 108.0 * (1.0 - frac) * pos, 3.0, 108.0 * frac, 1.5, p.col(&th.accent), p.col(&th.accent), 0.0)?;
            }
        }
    }
    Ok(())
}

fn todo(p: &mut P, v: &View) -> Result<()> {
    p.at(todo_rect());
    let (th, st) = (p.th, p.st);
    p.card(260.0, 140.0)?;
    p.header(11.0, 15.0, st.t("ThemeTitleTodo", "📝 战术待办  COMMANDS"), st.t("ThemeSubTodo", "COMMANDS"), 20.0, 10.0, 9.0, 8.0)?;
    if v.hub.todo_any_done {
        let cc = if p.hot("td_clear") { p.col(&th.white) } else { p.col(&th.muted) };
        p.txt(166.0, 11.0, "🧹", 8.0, cc, L, false)?;
    }
    p.rect(194.0, 8.0, 54.0, 20.0, 4.0, p.col(&th.stroke), p.col(&th.accent), 1.0)?;
    p.txt(221.0, 11.0, &perch_data::t("todo.btn_add"), 8.0, p.col(&ColorRgba::new(255, 255, 255, 255)), C, false)?;
    for (i, id) in TODO_IDS.iter().enumerate() {
        let n = i + 1;
        if Hub::v(&v.hub.todos, &format!("TodoVisible{}", n)) != "1" {
            continue;
        }
        let y = 34.0 + 26.0 * i as f32;
        let (f, s) = if p.hot(id) { (p.col(&th.item_hover), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
        p.rect(12.0, y, 236.0, 22.0, 4.0, f, s, 1.0)?;
        let icon = Hub::v(&v.hub.todos, &format!("TodoIcon{}", n)).to_string();
        let ic = parse_color(Hub::v(&v.hub.todos, &format!("TodoIconColor{}", n))).unwrap_or(th.sub);
        p.txt(18.0, y + 4.0, &icon, 8.0, p.col(&ic), L, false)?;
        let text = Hub::v(&v.hub.todos, &format!("TodoText{}", n)).to_string();
        let tc = parse_color(Hub::v(&v.hub.todos, &format!("TodoColor{}", n))).unwrap_or(th.white);
        p.txt(36.0, y + 2.0, &clip(&text, 22), 8.0, p.col(&tc), L, false)?;
        let obs = Hub::v(&v.hub.todos, &format!("TodoId{}", n)).parse::<u32>().map_or(false, crate::obsidian_tasks::is_obsidian_id);
        let edit_c = if p.hot(TODO_EDIT[i]) { p.col(&th.white) } else { p.col(&th.sub) };
        // Obsidian 任务:✏ 变成"在 Obsidian 打开",不显示删除
        p.txt(212.0, y + 4.0, if obs { "↗" } else { "✏" }, 8.0, edit_c, L, false)?;
        if !obs {
            let del_c = if p.hot(TODO_DEL[i]) { ColorRgba::new(255, 120, 120, 255).to_d2d() } else { ColorRgba::new(239, 68, 68, 220).to_d2d() };
            p.txt(232.0, y + 4.0, "✕", 8.0, del_c, L, true)?;
        }
    }
    // 条目多于 4 条时显示滚动条(鼠标滚轮滚动)
    if v.hub.todo_total > 4 {
        let (top, h) = (34.0, 100.0);
        let frac = 4.0 / v.hub.todo_total as f32;
        let travel = h * (1.0 - frac);
        let pos = if v.hub.todo_total > 4 { v.hub.todo_scroll as f32 / (v.hub.todo_total - 4) as f32 } else { 0.0 };
        let tr = p.track();
        p.rect(252.0, top, 3.0, h, 1.5, tr, tr, 0.0)?;
        p.rect(252.0, top + travel * pos, 3.0, h * frac, 1.5, p.col(&th.accent), p.col(&th.accent), 0.0)?;
    }
    Ok(())
}

fn media(p: &mut P, v: &View) -> Result<()> {
    p.at(media_rect());
    let (th, st) = (p.th, p.st);
    let m = &v.hub.media;
    p.card(260.0, 151.0)?;
    p.header(12.0, 14.0, st.t("ThemeTitleMedia", "🎵 战术音频  MEDIA"), st.t("ThemeSubMedia", "MEDIA"), 20.0, 11.0, 9.0, 8.0)?;
    let status = Hub::v(m, "MediaStatus").to_string();
    p.txt(248.0, 13.0, &status, 8.0, p.col(&th.accent), R, false)?;
    let icon = Hub::v(m, "MediaAppIcon").to_string();
    if !icon.is_empty() {
        p.r.draw_image(&icon, p.ox + 172.0, p.oy + 10.0, 16.0, 16.0);
    }
    p.rect(142.0, 9.0, 24.0, 18.0, 5.0, p.col(&th.item_bg), p.col(&th.stroke), 1.0)?;
    p.txt(154.0, 10.0, "♪", 9.0, p.col(&th.accent), C, false)?;
    let cover = Hub::v(m, "MediaCoverFile").to_string();
    if !cover.is_empty() {
        p.r.draw_image(&cover, p.ox + 14.0, p.oy + 40.0, 44.0, 44.0);
    }
    let title = Hub::v(m, "MediaTitle").to_string();
    let artist = Hub::v(m, "MediaArtist").to_string();
    p.txt(66.0, 40.0, &clip(&title, 14), 9.0, p.col(&th.white), L, false)?;
    p.txt(66.0, 62.0, &clip(&artist, 16), 8.0, p.col(&th.muted), L, false)?;
    p.btn("m_prev", 174.0, 48.0, 22.0, 26.0, 4.0, "⏮", 8.0, p.col(&th.white), p.col(&th.item_bg), p.col(&th.stroke), 6.0)?;
    let play = Hub::v(m, "MediaPlayIcon").to_string();
    p.btn("m_play", 200.0, 46.0, 28.0, 30.0, 5.0, &play, 9.0, p.col(&ColorRgba::new(255, 255, 255, 255)), p.col(&th.stroke), p.col(&th.accent), 7.0)?;
    p.btn("m_next", 232.0, 48.0, 22.0, 26.0, 4.0, "⏭", 8.0, p.col(&th.white), p.col(&th.item_bg), p.col(&th.stroke), 6.0)?;
    let (mut pos_ms, dur_ms) = v.hub.media_progress();
    if let Some(f) = v.app.seek_display_frac() {
        pos_ms = (f as f64 * dur_ms as f64) as i64;
    }
    if dur_ms > 0 {
        let fmt = |ms: i64| format!("{}:{:02}", ms / 60000, (ms / 1000) % 60);
        p.txt(14.0, 86.0, &fmt(pos_ms), 7.0, p.col(&th.muted), L, true)?;
        p.txt(246.0, 86.0, &fmt(dur_ms), 7.0, p.col(&th.muted), R, true)?;
        let tr = p.track();
        let bar_h = if p.hot("m_seek") || v.app.seek_preview.is_some() { 5.0 } else { 3.0 };
        let y = 91.0 - (bar_h - 3.0) / 2.0;
        p.rect(46.0, y, 168.0, bar_h, bar_h / 2.0, tr, tr, 0.0)?;
        let w = 168.0 * pos_ms as f32 / dur_ms as f32;
        if w > 0.5 {
            p.rect(46.0, y, w.max(bar_h), bar_h, bar_h / 2.0, p.col(&th.sub), p.col(&th.sub), 0.0)?;
        }
    }
    p.line(12.0, 100.0, 248.0, 100.0, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 20.0 / 255.0 }, 1.0)?;
    let (pct, muted) = v.vol;
    p.btn("m_mute", 14.0, 110.0, 30.0, 26.0, 5.0, if muted || pct < 1.0 { "🔇" } else if pct < 50.0 { "🔉" } else { "🔊" }, 9.5, p.col(&th.accent), p.col(&th.item_bg), p.col(&th.stroke), 4.0)?;
    p.btn("m_vdn", 50.0, 110.0, 26.0, 26.0, 5.0, "−", 10.0, p.col(&th.white), p.col(&th.item_bg), p.col(&th.stroke), 3.0)?;
    let tr = p.track();
    p.rect(84.0, 121.0, 104.0, 4.0, 2.0, tr, tr, 0.0)?;
    let w = (104.0 * pct / 100.0).max(0.0);
    if w > 0.5 {
        p.rect(84.0, 121.0, w.max(4.0), 4.0, 2.0, p.col(&th.accent), p.col(&th.accent), 0.0)?;
    }
    p.btn("m_vup", 194.0, 110.0, 26.0, 26.0, 5.0, "+", 10.0, p.col(&th.white), p.col(&th.item_bg), p.col(&th.stroke), 3.0)?;
    p.txt(250.0, 114.0, &format!("{:.0}%", pct), 9.5, p.col(&th.white), R, true)
}

pub(crate) fn clip(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let t: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{}…", t)
    }
}

fn water(p: &mut P, v: &View) -> Result<()> {
    p.at(water_rect());
    let (th, st) = (p.th, p.st);
    let w = &v.hub.water;
    p.card(260.0, 116.0)?;
    p.header(12.0, 14.0, st.t("ThemeTitleWater", "💧 饮水  WATER"), st.t("ThemeSubWater", "WATER"), 20.0, 11.0, 8.5, 7.0)?;
    if Hub::v(w, "WaterDone") == "1" {
        p.txt(232.0, 12.0, &perch_data::t("water.done"), 8.0, p.col(&th.sub), R, false)?;
    }
    let pct: f32 = Hub::v(w, "WaterPct").parse().unwrap_or(0.0);
    p.rect(18.0, 34.0, 38.0, 50.0, 5.0, D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 70.0 / 255.0 }, p.col(&th.accent), 1.5)?;
    let lvl = 46.0 * pct.clamp(0.0, 100.0) / 100.0;
    if lvl > 0.5 {
        p.rect(20.0, 82.0 - lvl, 34.0, lvl, 3.0, p.col(&th.item_hover), p.alpha(&th.item_hover, 0), 0.0)?;
        p.line(21.0, 82.0 - lvl, 53.0, 82.0 - lvl, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 150.0 / 255.0 }, 1.5)?;
    }
    let ml = Hub::v(w, "WaterMl").to_string();
    p.txt(68.0, 30.0, &ml, 17.0, p.col(&th.white), L, true)?;
    let (fnum, ox, oy) = (st.font_num.clone(), p.ox, p.oy);
    let wnum = p.r.measure(&ml, &fnum, 17.0 * 4.0 / 3.0, true);
    p.r.text_at(ox + 68.0 + wnum + 3.0, oy + 30.0 + 10.5, "ml", 8.0, th.muted.to_d2d(), &fnum, true, L)?;
    let goal_s = perch_data::t_fmt("water.goal_pct", &[("goal", Hub::v(w, "WaterGoal")), ("pct", Hub::v(w, "WaterPct"))]);
    p.txt(68.0, 58.0, &goal_s, 8.0, p.col(&th.muted), L, false)?;
    let last_s = perch_data::t_fmt("water.last_cups", &[("last", Hub::v(w, "WaterLast")), ("cups", Hub::v(w, "WaterCupsN"))]);
    p.txt(68.0, 72.0, &last_s, 8.0, p.col(&th.muted), L, false)?;
    for (id, x, wd, label) in [("w200", 68.0, 46.0, "+200"), ("w300", 118.0, 46.0, "+300"), ("w500", 168.0, 46.0, "+500"), ("w_undo", 218.0, 30.0, "↶")] {
        p.btn(id, x, 88.0, wd, 20.0, 5.0, label, 8.5, p.col(&th.white), p.col(&th.item_bg), p.col(&th.stroke), 3.0)?;
    }
    Ok(())
}

fn clock(p: &mut P, _v: &View) -> Result<()> {
    p.at(clock_rect());
    let th = p.th;
    p.card(260.0, 84.0)?;
    let now = Local::now();
    p.txt(18.0, 8.0, &format!("{:02}:{:02}", now.hour(), now.minute()), 24.0, p.col(&th.white), L, true)?;
    p.txt(108.0, 22.0, &format!("{:02}", now.second()), 10.0, p.col(&th.accent), L, true)?;
    p.rect(18.0, 52.0, 224.0, 22.0, 4.0, p.col(&th.item_bg), p.col(&th.stroke), 1.0)?;
    let days = perch_data::t("cards2.weekday_full");
    let wd = days.split(',').nth(now.weekday().num_days_from_sunday() as usize).unwrap_or("").to_string();
    let date_line = perch_data::t_fmt("cards2.date_fmt", &[("y", &now.year().to_string()), ("m", &now.month().to_string()), ("d", &now.day().to_string()), ("wd", &wd)]);
    p.txt(130.0, 56.0, &date_line, 8.0, p.col(&ColorRgba::new(255, 255, 255, 255)), C, false)
}

fn weather(p: &mut P, v: &View) -> Result<()> {
    p.at(weather_rect());
    let (th, st) = (p.th, p.st);
    let w = &v.hub.weather;
    p.card(260.0, 184.0)?;
    let faint = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 20.0 / 255.0 };
    p.line(12.0, 68.0, 248.0, 68.0, faint, 1.0)?;
    p.line(12.0, 122.0, 248.0, 122.0, faint, 1.0)?;
    // 太阳图标
    let fill = parse_color(Hub::v(w, "WeatherGlowFill")).unwrap_or(th.sub);
    let stroke = parse_color(Hub::v(w, "WeatherGlowStroke")).unwrap_or(th.accent);
    let sc = p.col(&stroke);
    p.r.draw_ring(p.ox + 28.0, p.oy + 34.0, 15.0, p.col(&fill), 1.0);
    p.r.draw_card(p.ox + 17.0, p.oy + 23.0, 22.0, 22.0, 11.0, p.col(&fill), sc, 2.0)?;
    for (a, b, c2, d) in [(28.0, 14.0, 28.0, 18.0), (28.0, 50.0, 28.0, 54.0), (8.0, 34.0, 12.0, 34.0), (44.0, 34.0, 48.0, 34.0)] {
        p.line(a, b, c2, d, sc, 2.0)?;
    }
    p.txt(58.0, 8.0, &format!("{}°C", Hub::v(w, "WeatherTemp")), 20.0, p.col(&th.white), L, true)?;
    p.txt(58.0, 38.0, Hub::v(w, "WeatherStatus"), 10.0, p.col(&th.accent), L, false)?;
    p.txt(248.0, 12.0, st.t("ThemeTitleWeather", "🛰️ 实时气象"), 8.5, p.col(&th.sub), R, false)?;
    p.txt(248.0, 38.0, &format!("{}° ~ {}°", Hub::v(w, "WeatherLow"), Hub::v(w, "WeatherHigh")), 9.5, p.col(&th.white), R, true)?;
    let wc = p.col(&th.white);
    let rain_s = perch_data::t_fmt("weather.rain", &[("val", Hub::v(w, "WeatherRainMM"))]);
    p.txt(14.0, 74.0, &rain_s, 8.0, wc, L, false)?;
    let chance_s = perch_data::t_fmt("weather.rain_chance", &[("val", Hub::v(w, "WeatherRainChance"))]);
    p.txt(134.0, 74.0, &chance_s, 8.0, wc, L, false)?;
    let hum_s = perch_data::t_fmt("weather.humidity", &[("val", Hub::v(w, "WeatherHumidity"))]);
    p.txt(14.0, 96.0, &hum_s, 8.0, wc, L, false)?;
    let wind_s = perch_data::t_fmt("weather.wind", &[("val", Hub::v(w, "WeatherWind"))]);
    p.txt(134.0, 96.0, &wind_s, 8.0, wc, L, false)?;
    for (i, (bxx, y, h)) in [(12.0, 128.0, 48.0), (93.0, 130.0, 44.0), (174.0, 130.0, 44.0)].iter().enumerate() {
        let n = i + 1;
        p.rect(*bxx, *y, 74.0, *h, 6.0, p.col(&th.item_bg), p.col(&th.stroke), 1.0)?;
        let cx = bxx + 37.0;
        p.txt(cx, 134.0, Hub::v(w, &format!("WeatherD{}Date", n)), 8.0, p.col(&th.sub), C, false)?;
        let s = format!("{} {}", Hub::v(w, &format!("WeatherD{}Status", n)), Hub::v(w, &format!("WeatherD{}Range", n)));
        p.txt(cx, 152.0, &s, 7.5, wc, C, false)?;
    }
    Ok(())
}

fn bar(p: &mut P, y: f32, pct: f32, color: &ColorRgba) -> Result<()> {
    let h = p.st.bar_h;
    let tr = p.track();
    let w = 236.0;
    p.rect(12.0, y, w, h, h / 2.0, tr, tr, 0.0)?;
    let fw = w * pct.clamp(0.0, 100.0) / 100.0;
    if fw > 0.5 {
        p.rect(12.0, y, fw.max(h), h, h / 2.0, p.col(color), p.col(color), 0.0)?;
    }
    if let Some(seg) = p.st.seg {
        for i in 1..20 {
            let x = 12.0 + w * i as f32 / 20.0;
            p.line(x, y, x, y + h, p.col(&seg), 2.0)?;
        }
    }
    Ok(())
}

fn system(p: &mut P, v: &View) -> Result<()> {
    p.at(system_rect());
    let (th, st) = (p.th, p.st);
    p.card(260.0, 270.0)?;
    p.line(12.0, 206.0, 248.0, 206.0, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 20.0 / 255.0 }, 1.0)?;
    let (white, muted) = (p.col(&th.white), p.col(&th.muted));
    let sys = v.sys;
    // CPU
    p.txt(12.0, 8.0, st.t("ThemeLblCpu", "⚡ SYNC RATE (CPU)"), 8.0, p.col(&th.accent), L, false)?;
    p.txt(248.0, 7.0, &format!("{:.1}%", sys.cpu_usage), 8.5, p.col(&th.accent), R, true)?;
    bar(p, 24.0, sys.cpu_usage, &th.accent)?;
    // RAM
    p.txt(12.0, 38.0, st.t("ThemeLblRam", "🧠 MEMORY (RAM)"), 8.0, white, L, false)?;
    p.txt(248.0, 38.0, &format!("{} / {}", fmt_gb(sys.ram_used_gb), fmt_gb(sys.ram_total_gb)), 8.0, p.col(&th.sub), R, true)?;
    bar(p, 54.0, sys.ram_usage, &th.sub)?;
    // C:
    let (cf, ct) = v.disk_c;
    p.txt(12.0, 68.0, st.t("ThemeLblC", "🛡️ ARMOR (C:)"), 8.0, white, L, false)?;
    p.txt(248.0, 68.0, &perch_data::t_fmt("cards2.free_of", &[("free", &fmt_gb(cf)), ("total", &fmt_gb(ct))]), 8.0, white, R, true)?;
    bar(p, 84.0, if ct > 0.0 { (ct - cf) / ct * 100.0 } else { 0.0 }, &th.accent)?;
    // D:
    let (df, dt) = v.disk_d;
    p.txt(12.0, 98.0, st.t("ThemeLblD", "📦 AMMO (D:)"), 8.0, white, L, false)?;
    p.txt(248.0, 98.0, &perch_data::t_fmt("cards2.free_of", &[("free", &fmt_gb(df)), ("total", &fmt_gb(dt))]), 8.0, p.col(&th.sub), R, true)?;
    bar(p, 114.0, if dt > 0.0 { (dt - df) / dt * 100.0 } else { 0.0 }, &th.sub)?;
    // GPU(NVIDIA,经 nvml;没有 NVIDIA 显卡时显示 --)
    let gpu_pct = v.gpu.map(|g| g.util as f32).unwrap_or(0.0);
    p.txt(12.0, 128.0, "🎮 GPU", 8.0, white, L, false)?;
    let gpu_s = match v.gpu {
        Some(g) => format!("{}%  {}°C  {:.1}/{:.0} GB", g.util, g.temp, g.mem_used_mb as f32 / 1024.0, g.mem_total_mb as f32 / 1024.0),
        None => "--".to_string(),
    };
    let al = config::get().alert;
    let gpu_hot = al.gpu_temp > 0 && v.gpu.map_or(false, |g| g.temp >= al.gpu_temp);
    let gpu_col = if gpu_hot { ColorRgba::new(239, 68, 68, 255).to_d2d() } else { p.col(&th.accent) };
    p.txt(248.0, 128.0, &gpu_s, 8.0, gpu_col, R, true)?;
    bar(p, 144.0, gpu_pct, &th.accent)?;
    // 网络
    p.txt(12.0, 158.0, st.t("ThemeLblNet", "📡 TELEMETRY"), 8.0, p.col(&th.sub), L, false)?;
    let ping = crate::net::PING_MS.load(std::sync::atomic::Ordering::Relaxed);
    let ping_s = if ping == 0 { "📶 -- ms".to_string() } else { format!("📶 {} ms", ping) };
    p.txt(248.0, 158.0, &ping_s, 8.0, p.col(&th.accent), R, true)?;
    let down = crate::net::fmt_rate(crate::net::DOWN_BPS.load(std::sync::atomic::Ordering::Relaxed));
    let up = crate::net::fmt_rate(crate::net::UP_BPS.load(std::sync::atomic::Ordering::Relaxed));
    p.txt(14.0, 176.0, &format!("📥 {}", down), 8.0, white, L, true)?;
    p.txt(134.0, 176.0, &format!("📤 {}", up), 8.0, muted, L, true)?;
    // 底部徽章
    let acc_dim = th.accent.with_alpha(110);
    p.rect(12.0, 202.0, 112.0, 48.0, 6.0, p.col(&th.item_bg), acc_dim, 1.0)?;
    p.txt(48.0, 210.0, st.t("ThemeFieldName", "AT FIELD"), 7.5, p.col(&th.accent), L, false)?;
    p.txt(48.0, 226.0, "ACTIVE", 7.5, white, L, false)?;
    let sub_dim = th.sub.with_alpha(110);
    p.rect(136.0, 202.0, 112.0, 48.0, 6.0, p.col(&th.item_bg), sub_dim, 1.0)?;
    p.rect(146.0, 214.0, 8.0, 24.0, 2.0, th.sub.with_alpha(60), p.col(&th.sub), 1.0)?;
    p.txt(160.0, 210.0, st.t("ThemeUnitName", "UNIT"), 7.5, p.col(&th.sub), L, false)?;
    p.txt(160.0, 226.0, "STANDBY", 7.5, white, L, false)
}

fn quotes(p: &mut P, v: &View) -> Result<()> {
    p.at(quotes_rect());
    let th = p.th;
    let (rect_f, rect_s) = (if p.hot("quotes") { p.col(&th.item_hover) } else { p.col(&th.bg) }, p.col(&th.accent));
    p.rect(0.0, 0.0, 280.0, 96.0, 10.0, rect_f, rect_s, 1.5)?;
    p.line(10.0, 1.0, 270.0, 1.0, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 35.0 / 255.0 }, 1.0)?;
    p.line(265.0, 1.0, 279.0, 15.0, rect_s, 2.0)?;
    p.rect(12.0, 12.0, 3.0, 14.0, 1.5, p.col(&th.sub), p.alpha(&th.sub, 0), 0.0)?;
    let speaker = Hub::v(&v.hub.quotes, "QuoteSpeaker").to_string();
    let (head, tail) = match speaker.find("//") {
        Some(i) => (speaker[..i].to_string(), speaker[i..].to_string()),
        None => (speaker.clone(), String::new()),
    };
    p.txt(20.0, 11.0, &head, 8.0, p.col(&th.accent), L, false)?;
    if !tail.is_empty() {
        let fm = p.st.font_main.clone();
        let w = p.r.measure(&head, &fm, 8.0 * 4.0 / 3.0, true);
        p.txt(20.0 + w, 11.0, &tail, 8.0, p.col(&th.sub), L, false)?;
    }
    let text = Hub::v(&v.hub.quotes, "CurrentQuoteText").replace("#CRLF#", "\n");
    let font = p.st.font_main.clone();
    p.r.wrap_next = Some((248.0, 64.0));
    let (ox, oy) = (p.ox, p.oy);
    let white = p.col(&th.white);
    let res = p.r.text_at(ox + 20.0, oy + 28.0, &text, 8.5, white, &font, true, L);
    p.r.wrap_next = None;
    res
}

fn controls(p: &mut P, _v: &View) -> Result<()> {
    p.at(controls_rect());
    let th = p.th;
    p.rect(0.0, 0.0, 480.0, 34.0, 17.0, p.col(&th.bg), p.col(&th.stroke), 1.5)?;
    p.line(18.0, 1.0, 462.0, 1.0, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 35.0 / 255.0 }, 1.0)?;
    let net_lbl = perch_data::t("controls.net");
    let bt_lbl = perch_data::t("controls.bt");
    let lock_lbl = perch_data::t("controls.lock");
    for (id, x, w, label) in [("c_net", 8.0, 58.0, &net_lbl), ("c_bt", 70.0, 58.0, &bt_lbl), ("c_lock", 132.0, 58.0, &lock_lbl)] {
        p.btn(id, x, 4.0, w, 26.0, 13.0, label, 8.0, p.col(&th.white), p.col(&th.item_bg), p.col(&th.stroke), 5.0)?;
    }
    if _v.cleaning {
        // loading:旋转的弧形字符 + 呼吸的强调色底
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let spin = ["◐", "◓", "◑", "◒"][((ms / 120) % 4) as usize];
        let pulse = 0.5 + 0.5 * ((ms as f32) / 220.0).sin();
        let fill = th.accent.with_alpha((60.0 + 70.0 * pulse) as u8);
        p.rect(194.0, 4.0, 58.0, 26.0, 13.0, fill, p.col(&th.accent), 1.0)?;
        p.txt(223.0, 9.0, &format!("{} {}", spin, perch_data::t("controls.cleaning")), 8.0, p.col(&th.white), C, true)?;
    } else {
        let clean_lbl = perch_data::t("controls.clean");
        p.btn("c_clean", 194.0, 4.0, 58.0, 26.0, 13.0, &clean_lbl, 8.0, p.col(&th.white), p.col(&th.item_bg), p.col(&th.stroke), 5.0)?;
    }
    // 通知中心按钮(有未读时右上角红点)
    let unread = _v.hub.history.iter().filter(|h| h.0 > _v.app.notif_seen).count();
    let (bf, bs) = if _v.app.notif_open { (p.col(&th.stroke), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
    p.btn("c_bell", 256.0, 4.0, 32.0, 26.0, 13.0, "🔔", 8.0, p.col(&th.white), bf, bs, 5.0)?;
    if unread > 0 && !_v.app.notif_open {
        p.rect(278.0, 3.0, 9.0, 9.0, 4.5, ColorRgba::new(239, 68, 68, 255).to_d2d(), ColorRgba::new(255, 255, 255, 200).to_d2d(), 1.0)?;
    }
    for (id, x, label, open) in [("c_tools", 292.0, "🧰", _v.app.tools_open), ("c_set", 328.0, "⚙", _v.app.settings_open), ("c_search", 364.0, "🔍", false)] {
        let (f, s) = if open { (p.col(&th.stroke), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
        p.btn(id, x, 4.0, 32.0, 26.0, 13.0, label, 8.0, p.col(&th.white), f, s, 5.0)?;
    }
    let btn_switch_txt = perch_data::t("theme_selector.btn_switch");
    p.btn("c_theme", 400.0, 4.0, 76.0, 26.0, 13.0, &btn_switch_txt, 8.0, p.col(&th.on_accent), p.col(&th.accent), D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 70.0 / 255.0 }, 4.0)
}

/// 单击的默认动作是否为"新开"(条目的 mode 优先,auto 按应用类型)
fn dock_is_multi(it: &config::DockEntry) -> bool {
    match it.mode.as_str() {
        "multi" => true,
        "single" => false,
        _ => crate::running::default_multi(it),
    }
}

fn dock_launch(it: &config::DockEntry) {
    data::open_target(&config::resolve_target(&it.target), &it.args);
    crate::running::refresh_soon();
}

/// 点击 Dock 图标:alt = Shift/中键,走与默认动作相反的那种
pub fn dock_activate(i: usize, alt: bool) {
    let Some(it) = config::get().dock.get(i).cloned() else { return };
    // 默认多开且没按 Shift,或默认单例且按了 Shift → 新开;否则尽量切到已有窗口
    let want_existing = dock_is_multi(&it) == alt;
    if want_existing {
        if let Some(e) = crate::running::key_for(&it).and_then(|k| crate::running::entry(&k)) {
            if crate::running::focus(&e) {
                crate::running::refresh_soon();
                return;
            }
        }
    }
    dock_launch(&it);
}

/// Dock 右键菜单的动作:1 新开 / 启动,2 切到窗口,3 关闭,4 移除
pub fn handle_dock_action(i: usize, cmd: u8) {
    let Some(it) = config::get().dock.get(i).cloned() else { return };
    let entry = crate::running::key_for(&it).and_then(|k| crate::running::entry(&k));
    match cmd {
        1 => dock_launch(&it),
        2 => {
            if let Some(e) = entry {
                crate::running::focus(&e);
            }
        }
        3 => {
            if let Some(e) = entry {
                crate::running::close(&e);
                crate::running::refresh_soon();
            }
        }
        4 => {
            if crate::settings::confirm(&perch_data::t_fmt("cards2.dock_remove_confirm", &[("name", &it.name)])) {
                crate::settings::remove_dock_app(i);
            }
        }
        _ => {}
    }
}

fn dock(p: &mut P, _v: &View) -> Result<()> {
    p.at(dock_rect());
    let th = p.th;
    let items = config::get().dock;
    let w = 52.0 * items.len().max(1) as f32 + 34.0;
    p.rect(0.0, 0.0, w, 56.0, 28.0, p.col(&th.dock_bg), p.col(&th.stroke), 1.5)?;
    for (i, it) in items.iter().enumerate() {
        let id = DOCK_IDS[i];
        let x = 20.0 + 52.0 * i as f32;
        let hot = p.hot(id);
        let (f, s) = if hot { (p.col(&th.item_hover), p.col(&th.accent)) } else { (D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 10.0 / 255.0 }, D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }) };
        p.rect(x, 4.0, 46.0, 48.0, 8.0, f, s, if hot { 1.5 } else { 0.0 })?;
        // 图标:dock_icons\<icon>.png,或 icon 给的图片路径;都没有就从 target 的 exe 提取
        let named = perch_data::paths::dock_icons_dir().join(format!("{}.png", it.icon)).to_string_lossy().to_string();
        let icon_path = if it.icon.is_empty() {
            None
        } else if std::path::Path::new(&it.icon).is_file() {
            Some(it.icon.clone())
        } else if std::path::Path::new(&named).is_file() {
            Some(named)
        } else {
            None
        };
        match icon_path {
            Some(path) => p.r.draw_image(&path, p.ox + x + 6.0, p.oy + 8.0, 34.0, 34.0),
            None => p.r.draw_file_icon(&config::icon_source(&it.target), p.ox + x + 6.0, p.oy + 8.0, 34.0),
        }
        // 运行状态条:没运行=暗灰短条;运行=副色;前台=主色长条;多个窗口=两格;只有后台进程=细短条
        let run = crate::running::key_for(it).and_then(|k| crate::running::entry(&k)).filter(|e| e.procs > 0);
        let mut bar = |p: &mut P, bx_: f32, w: f32, c: D2D1_COLOR_F| p.rect(bx_, 47.0, w, 3.0, 1.5, c, c, 0.0);
        match &run {
            None => bar(p, x + 17.0, 12.0, D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 70.0 / 255.0 })?,
            Some(e) if e.windows.is_empty() => bar(p, x + 19.0, 8.0, p.alpha(&th.sub, 150))?,
            Some(e) => {
                let c = if e.fg { p.col(&th.accent) } else { p.col(&th.sub) };
                if e.windows.len() >= 2 {
                    bar(p, x + 11.0, 11.0, c)?;
                    bar(p, x + 24.0, 11.0, c)?;
                } else {
                    let w = if e.fg { 24.0 } else { 16.0 };
                    bar(p, x + (46.0 - w) / 2.0, w, c)?;
                }
            }
        }
    }
    Ok(())
}

fn edit_overlay(p: &mut P, _v: &View) -> Result<()> {
    let th = p.th;
    let rects = [battery_rect(), timer_rect(), remind_rect(), todo_rect(), media_rect(), water_rect(), clock_rect(), weather_rect(), system_rect(), controls_rect(), dock_rect(), quotes_rect()];
    for (i, r) in rects.iter().enumerate() {
        p.ox = 0.0;
        p.oy = 0.0;
        let hot = p.hot(CARD_IDS[i]);
        p.rect(r.0, r.1, r.2, r.3, 10.0, th.accent.with_alpha(if hot { 70 } else { 28 }), p.col(&th.accent), if hot { 2.0 } else { 1.2 })?;
        p.txt(r.0 + r.2 / 2.0, r.1 + (r.3 - 14.0) / 2.0, &perch_data::t("cards2.drag"), 8.0, p.col(&th.white), C, true)?;
    }
    Ok(())
}

fn dock_tip(p: &mut P, v: &View) -> Result<()> {
    let _ = v;
    if let (Some((x, y, w, h, r)), Some(id)) = (tooltip_rect(), current_tip()) {
        let th = p.th;
        p.ox = 0.0;
        p.oy = 0.0;
        p.rect(x, y, w, h, r, th.bg.with_alpha(245), p.col(&th.accent), 1.0)?;
        if let Some(text) = tip_text(id) {
            p.txt(x + w / 2.0, y + 4.0, &text, 8.5, p.col(&th.white), C, false)?;
        }
    }
    Ok(())
}

fn fd(c: D2D1_COLOR_F, f: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * f, ..c }
}

fn notify(p: &mut P, v: &View) -> Result<()> {
    let fade = v.hub.notify_alpha();
    if fade <= 0.0 {
        return Ok(());
    }
    p.at(notify_rect());
    let th = p.th;
    p.rect(0.0, 0.0, 280.0, 56.0, 10.0, fd(p.col(&th.bg), fade), fd(p.col(&th.accent), fade), 1.5)?;
    p.line(10.0, 1.0, 270.0, 1.0, fd(D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 40.0 / 255.0 }, fade), 1.0)?;
    p.line(265.0, 1.0, 279.0, 15.0, fd(p.col(&th.accent), fade), 2.0)?;
    p.rect(12.0, 12.0, 3.0, 32.0, 1.5, fd(p.col(&th.sub), fade), fd(p.alpha(&th.sub, 0), fade), 0.0)?;
    let t = Hub::v(&v.hub.notify, "NotifyTitle").to_string();
    let m = Hub::v(&v.hub.notify, "NotifyMsg").to_string();
    p.txt(22.0, 11.0, &t, 8.5, fd(p.col(&th.accent), fade), L, false)?;
    p.txt(22.0, 31.0, &m, 7.5, fd(p.col(&th.white), fade), L, false)
}

// ---------------- 点击处理 ----------------
pub fn on_click(id: &str, app: &mut AppState, hub: &mut Hub, audio: &Audio, mouse: (f32, f32)) {
    let mouse_x = mouse.0;
    for (pid, _, mins, _) in presets() {
        if id == pid {
            app.set_preset(mins);
            return;
        }
    }
    match id {
        "battery" => data::core(&["battery"]),
        "t_main" => app.toggle_timer(),
        "t_reset" => app.reset_timer(),
        "td_clear" => data::core(&["todo", "clear-done"]),
        "wheel_up" | "wheel_down" => {
            let hov = crate::window::HOVERED_ID.lock().ok().and_then(|h| *h).unwrap_or("");
            let d = if id == "wheel_up" { -1 } else { 1 };
            if hov.starts_with("td") || hov.starts_with("te") || hov.starts_with("tx") || hov == "todo_body" {
                hub.scroll_todos(d);
            } else if hov.starts_with("rl") || hov.starts_with("rx") || hov == "rem_body" {
                hub.scroll_reminders(d);
            } else if hov.starts_with("nh") || hov == "notif_body" {
                hub.scroll_history(d);
            } else if hov.starts_with("st_") {
                crate::settings::scroll(app, d);
            } else if hov.starts_with("tl_") {
                crate::tools::scroll(app, d);
            }
        }
        _ if id.starts_with("rx") && id.len() == 3 => {
            if let Ok(row) = id[2..].parse::<usize>() {
                if let Some(it) = hub.reminder_items.get(hub.rem_scroll + row) {
                    data::core(&["reminder", "delete", &it.0.to_string()]);
                    hub.refresh_soon();
                }
            }
        }
        "r_tab_cal" => app.cal_tab = 0,
        "r_tab_list" => app.cal_tab = 1,
        "td_add" => {
            if let Some(text) = crate::prompt::ask(&perch_data::t("cards2.add_todo_title"), &perch_data::t("cards2.add_todo_hint"), "", &app.theme) {
                data::core(&["todo", "add", &text]);
            }
        }
        "r_cal" => app.cal_open = !app.cal_open,
        "r_del" => {
            let id = Hub::v(&hub.reminders, "ReminderId").to_string();
            if !id.is_empty() && id != "0" {
                data::core(&["reminder", "delete", &id]);
                hub.refresh_soon();
            }
        }
        "r_slot" => {
            if let Some(text) = crate::prompt::ask(&perch_data::t("cards2.add_event_title"), &perch_data::t("cards2.add_event_hint"), "", &app.theme) {
                let mut it = text.splitn(2, char::is_whitespace);
                let (time, what) = (it.next().unwrap_or(""), it.next().unwrap_or("").trim());
                if !time.is_empty() && !what.is_empty() {
                    data::core(&["reminder", time, what]);
                }
            }
        }
        "r_panel" => {}
        "m_prev" => data::core(&["media", "prev"]),
        "m_play" => data::core(&["media", "playpause"]),
        "m_next" => data::core(&["media", "next"]),
        "m_seek_drag" => {
            app.seek_preview = Some(((mouse_x - (media_rect_x() + 46.0)) / 168.0).clamp(0.0, 1.0));
        }
        "m_seek_commit" => {
            let (_, dur) = hub.media_progress();
            let frac = ((mouse_x - (media_rect_x() + 46.0)) / 168.0).clamp(0.0, 1.0);
            app.seek_preview = None;
            if dur > 0 {
                let secs = (frac as f64) * (dur as f64) / 1000.0;
                data::core(&["media", &format!("seek:{:.1}", secs)]);
                // 播放器的位置要约 1 秒后才更新:这期间进度条停在目标位置,避免回跳
                app.seek_hold = Some((std::time::Instant::now() + std::time::Duration::from_millis(1600), frac));
            }
        }
        "m_mute" => audio.toggle_mute(),
        "m_vdn" => audio.step(-5.0),
        "m_vup" => audio.step(5.0),
        "m_slider" => audio.set(((mouse_x - (media_rect().0 + 84.0)) / 104.0 * 100.0).clamp(0.0, 100.0)),
        "w200" => data::core(&["water", "add", "200"]),
        "w300" => data::core(&["water", "add", "300"]),
        "w500" => data::core(&["water", "add", "500"]),
        "w_undo" => data::core(&["water", "undo"]),
        "quotes" => data::core(&["quote", "next"]),
        "c_net" => data::open_target("ms-settings:network", ""),
        "c_bt" => data::open_target("ms-settings:bluetooth", ""),
        "c_lock" => data::core(&["power", "lock"]),
        "c_clean" => hub.start_clean(),
        "c_theme" => app.selector_open = !app.selector_open,
        "c_tools" => {
            if app.tools_open {
                app.tools_open = false;
            } else {
                crate::tools::open(app);
            }
        }
        "sys_card" => {
            app.procs_open = !app.procs_open;
            if !app.procs_open {
                crate::procs::reset();
            }
        }
        _ if id.starts_with("pr_") => crate::procs::on_click(id, app),
        _ if id.starts_with("pv") => crate::preview::on_click(id),
        "c_search" => crate::window::TRAY_SEARCH.store(true, std::sync::atomic::Ordering::SeqCst),
        "c_set" => {
            app.settings_open = !app.settings_open;
            if app.settings_open {
                app.selector_open = false;
            }
        }
        "c_bell" => {
            app.notif_open = !app.notif_open;
            if app.notif_open {
                app.tools_open = false;
            }
            if app.notif_open {
                app.notif_seen = hub.latest_history_ts();
            }
        }
        "n_close" => app.notif_open = false,
        "n_clear" => {
            data::core(&["notify-history", "clear"]);
            hub.refresh_soon();
        }
        "notify_card" => hub.dismiss_notify(),
        _ if id.starts_with("nh") && id.len() == 3 => {
            if let Ok(row) = id[2..].parse::<usize>() {
                if let Some(it) = hub.history.get(hub.history_scroll + row) {
                    data::core(&["notify-history", "delete", &it.0.to_string()]);
                    hub.refresh_soon();
                }
            }
        }
        "sel_close" => app.selector_open = false,
        "sel_settings" => {
            app.selector_open = false;
            app.settings_open = true;
        }
        _ if id.starts_with("st_") || id.starts_with("pm_") => crate::settings::on_click(id, app, hub),
        _ if id.starts_with("tl_") => crate::tools::on_click(id, app, hub, mouse),
        _ if id.starts_with("card:") => {
            // 编辑模式:按下卡片开始拖动
            if let Some(i) = card_index(id) {
                let o = offset_of(i);
                app.drag = Some((i, mouse.0, mouse.1, o.0, o.1));
            }
        }
        "card_drag" => {
            if let Some((i, mx0, my0, ox0, oy0)) = app.drag {
                set_offset(i, ox0 + mouse.0 - mx0, oy0 + mouse.1 - my0);
            }
        }
        "card_drop" => {
            if app.drag.take().is_some() {
                save_layout();
                crate::window::RELAYOUT.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        _ if id.starts_with("th_") => {
            data::core(&["theme", &id[3..]]);
            app.selector_open = false;
        }
        _ if id.starts_with("sel_") => {
            for (sid, args, _, _, _, _, _) in SEL_BUTTONS.iter() {
                if id == *sid && !args.is_empty() {
                    let parts: Vec<&str> = args.split(' ').collect();
                    data::core(&parts);
                }
            }
        }
        _ => {
            for i in 0..4 {
                let tid_val = Hub::v(&hub.todos, &format!("TodoId{}", i + 1)).to_string();
                if tid_val.is_empty() || tid_val == "0" {
                    continue;
                }
                let obs = tid_val.parse::<u32>().map_or(false, crate::obsidian_tasks::is_obsidian_id);
                if id == TODO_DEL[i] && obs {
                    // Obsidian 任务不在卡片上删除(避免误删日记内容)
                    return;
                }
                if id == TODO_EDIT[i] && obs {
                    // 编辑 = 在 Obsidian 里打开今天的日记
                    crate::tools::open_daily_note();
                    return;
                }
                if id == TODO_DEL[i] {
                    data::core(&["todo", "delete", &tid_val]);
                    hub.refresh_soon();
                    return;
                }
                if id == TODO_EDIT[i] {
                    let cur = Hub::v(&hub.todos, &format!("TodoText{}", i + 1)).to_string();
                    if let Some(text) = crate::prompt::ask(&perch_data::t("cards2.edit_todo_title"), &perch_data::t("cards2.edit_todo_hint"), &cur, &app.theme) {
                        data::core(&["todo", "edit", &tid_val, &text]);
                    }
                    hub.refresh_soon();
                    return;
                }
            }
            for (i, tid) in TODO_IDS.iter().enumerate() {
                if id == *tid {
                    let tid_val = Hub::v(&hub.todos, &format!("TodoId{}", i + 1)).to_string();
                    if let Ok(n) = tid_val.parse::<u32>() {
                        if crate::obsidian_tasks::is_obsidian_id(n) {
                            // Obsidian 任务:改写日记里那一行的复选框
                            crate::obsidian_tasks::toggle((n - crate::obsidian_tasks::ID_BASE) as usize);
                        } else if n != 0 {
                            data::core(&["todo", "toggle", &tid_val]);
                        }
                    }
                    break;
                }
            }
            for (i, did) in DOCK_IDS.iter().enumerate() {
                if id == *did {
                    dock_activate(i, crate::window::ALT_CLICK.swap(false, std::sync::atomic::Ordering::Relaxed));
                    break;
                }
            }
        }
    }
    hub.refresh_soon();
}
