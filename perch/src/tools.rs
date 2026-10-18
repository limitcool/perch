//! 工具箱面板:剪贴板历史 + 收集箱。
//! 剪贴板历史只存在内存里(不落盘),跳过密码管理器标记为"不要记录"的内容。
//! 收集箱是待处理的暂存区(追加到一个 markdown 文件),每条可以一键"→ 待办"或"→ 日记"(追加到 Obsidian 今天的日记)。

use std::path::PathBuf;
use std::sync::atomic::AtomicI32;
use std::sync::Mutex;
use windows::core::Result;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

use crate::cards::{self, clip, View, C, L, P, R};
use crate::config;
use crate::data::{self, Hub};
use crate::state::AppState;
use crate::window::RectBox;

const W: f32 = 360.0;
const H: f32 = 400.0;
/// 快捷页(亮度 + 输出设备 + Clash)更高
const H_QUICK: f32 = 480.0;
static CUR_TAB: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn ph() -> f32 {
    if CUR_TAB.load(std::sync::atomic::Ordering::Relaxed) == 2 {
        H_QUICK
    } else {
        H
    }
}
const ROWS: usize = 7;
const CF_UNICODETEXT: u32 = 13;
const MAX_ITEMS: usize = 30;
const MAX_CHARS: usize = 4000;

static CLIPS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// (文件行号, 原始行内容 "YYYY-MM-DD HH:MM 文字")
static NOTES: Mutex<Vec<(usize, String)>> = Mutex::new(Vec::new());
/// 亮度滑块拖动中的预览值(-1 = 没有),松手时才真正写入
static PREVIEW: [AtomicI32; 2] = [AtomicI32::new(-1), AtomicI32::new(-1)];
/// 输出设备列表(快捷页);切换后需要让主循环重新绑定音量控制
static DEVICES: Mutex<Vec<crate::audio::OutputDevice>> = Mutex::new(Vec::new());
pub static AUDIO_REBIND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const DEV_ID: [&str; 4] = ["tl_o0", "tl_o1", "tl_o2", "tl_o3"];

fn refresh_devices() {
    if let Ok(mut d) = DEVICES.lock() {
        let hide: Vec<String> = crate::config::get().audio.hide.iter().map(|h| h.to_lowercase()).collect();
        *d = crate::audio::list_outputs().into_iter().filter(|x| !hide.iter().any(|h| x.name.to_lowercase().contains(h.as_str()))).collect();
    }
}

static SCROLL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

const ROW_ID: [&str; ROWS] = ["tl_r0", "tl_r1", "tl_r2", "tl_r3", "tl_r4", "tl_r5", "tl_r6"];
const DEL_ID: [&str; ROWS] = ["tl_x0", "tl_x1", "tl_x2", "tl_x3", "tl_x4", "tl_x5", "tl_x6"];
const TODO_ID: [&str; ROWS] = ["tl_t0", "tl_t1", "tl_t2", "tl_t3", "tl_t4", "tl_t5", "tl_t6"];
const DIARY_ID: [&str; ROWS] = ["tl_y0", "tl_y1", "tl_y2", "tl_y3", "tl_y4", "tl_y5", "tl_y6"];

pub fn rect() -> (f32, f32, f32, f32) {
    let c = cards::controls_rect();
    (c.0 + c.2 - W, c.1 - ph() - 12.0, W, ph())
}

pub fn region() -> (f32, f32, f32, f32, f32) {
    let r = rect();
    (r.0, r.1, r.2, r.3, 12.0)
}

fn abs(x: f32, y: f32, w: f32, h: f32, id: &'static str) -> RectBox {
    let r = rect();
    RectBox { id, x: r.0 + x, y: r.1 + y, w, h }
}

/// 打开面板时的可点击区域(位置固定,与条目数量无关)
pub fn boxes(tab: usize) -> Vec<RectBox> {
    let mut v = vec![abs(W - 40.0, 8.0, 32.0, 28.0, "tl_close"), abs(16.0, 40.0, 84.0, 26.0, "tl_tab0"), abs(106.0, 40.0, 84.0, 26.0, "tl_tab1"), abs(196.0, 40.0, 84.0, 26.0, "tl_tab2")];
    if tab == 2 {
        v.push(abs(16.0, 100.0, W - 32.0, 30.0, "tl_b0"));
        v.push(abs(16.0, 176.0, W - 32.0, 30.0, "tl_b1"));
        for k in 0..4 {
            v.push(abs(16.0, 258.0 + 30.0 * k as f32, W - 32.0, 26.0, DEV_ID[k]));
        }
        v.push(abs(W - 74.0, 388.0, 58.0, 22.0, "tl_cm"));
        for k in 0..3 {
            v.push(abs(16.0 + 112.0 * k as f32, 418.0, 104.0, 26.0, ["tl_c0", "tl_c1", "tl_c2"][k]));
        }
        v.push(abs(0.0, 0.0, W, ph(), "tl_panel"));
        return v;
    }
    for k in 0..ROWS {
        let y = 76.0 + 38.0 * k as f32;
        v.push(abs(W - 40.0, y + 5.0, 26.0, 26.0, DEL_ID[k]));
        v.push(abs(210.0, y + 5.0, 50.0, 26.0, TODO_ID[k]));
        v.push(abs(264.0, y + 5.0, 50.0, 26.0, DIARY_ID[k]));
        v.push(abs(12.0, y, W - 24.0, 34.0, ROW_ID[k]));
    }
    if tab == 0 {
        v.push(abs(W - 100.0, H - 40.0, 84.0, 26.0, "tl_clear"));
    } else {
        v.push(abs(16.0, H - 40.0, 84.0, 26.0, "tl_add"));
        v.push(abs(108.0, H - 40.0, 100.0, 26.0, "tl_diary"));
        v.push(abs(216.0, H - 40.0, 100.0, 26.0, "tl_open"));
    }
    v.push(abs(0.0, 0.0, W, ph(), "tl_panel"));
    v
}

// ---------------- 剪贴板 ----------------

fn read_clipboard_text(hwnd: HWND) -> Option<String> {
    unsafe {
        // 密码管理器等会标记"不要进入监视/历史"
        let excl = RegisterClipboardFormatW(windows::core::w!("ExcludeClipboardContentFromMonitorProcessing"));
        if excl != 0 && IsClipboardFormatAvailable(excl).is_ok() {
            return None;
        }
        if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
            return None;
        }
        OpenClipboard(hwnd).ok()?;
        let mut out = None;
        if let Ok(h) = GetClipboardData(CF_UNICODETEXT) {
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if !p.is_null() {
                let mut n = 0usize;
                while *p.add(n) != 0 && n < 100_000 {
                    n += 1;
                }
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                let _ = GlobalUnlock(g);
            }
        }
        let _ = CloseClipboard();
        out
    }
}

/// WM_CLIPBOARDUPDATE:记一条(去重置顶)
pub fn on_clipboard_update(hwnd: HWND) {
    let Some(text) = read_clipboard_text(hwnd) else { return };
    let text = text.trim().to_string();
    if text.is_empty() || text.chars().count() > MAX_CHARS {
        return;
    }
    if let Ok(mut l) = CLIPS.lock() {
        l.retain(|t| *t != text);
        l.insert(0, text);
        l.truncate(MAX_ITEMS);
    }
}

pub(crate) fn set_clipboard(text: &str) {
    unsafe {
        let w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2) {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                let _ = GlobalUnlock(h);
                // 设置失败时内存仍归我们,但这里泄漏一小块可以接受;成功后归系统
                let _ = SetClipboardData(CF_UNICODETEXT, HANDLE(h.0));
            }
        }
        let _ = CloseClipboard();
    }
}

// ---------------- 收集箱 ----------------

fn notes_path() -> PathBuf {
    let custom = config::raw_str("notes", "file");
    if !custom.is_empty() {
        return PathBuf::from(custom);
    }
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("perch").join("notes.md")
}

/// 重新读取收集箱(行号用于删除)
pub fn reload_notes() {
    let text = std::fs::read_to_string(notes_path()).unwrap_or_default();
    let mut list: Vec<(usize, String)> = text.lines().enumerate().filter(|(_, l)| l.starts_with("- ")).map(|(i, l)| (i, l[2..].to_string())).collect();
    list.reverse();
    if let Ok(mut n) = NOTES.lock() {
        *n = list;
    }
}

/// "2026-10-17 00:50 文字" → ("2026-10-17 00:50", "文字");不带时间戳的整行当文字
fn split_note(raw: &str) -> (&str, &str) {
    let b = raw.as_bytes();
    if raw.len() > 17 && b[4] == b'-' && b[7] == b'-' && b[10] == b' ' && b[13] == b':' && raw.is_char_boundary(17) {
        (&raw[..16], raw[17..].trim_start())
    } else {
        ("", raw)
    }
}

fn add_note(text: &str) {
    let text = text.replace(['\r', '\n'], " ");
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let p = notes_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let line = format!("- {} {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M"), text);
    let existing = std::fs::read_to_string(&p).unwrap_or_default();
    let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    let _ = std::fs::write(&p, format!("{}{}{}", existing, sep, line));
    reload_notes();
}

fn delete_note(line_no: usize) {
    let p = notes_path();
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    let kept: Vec<&str> = text.lines().enumerate().filter(|(i, _)| *i != line_no).map(|(_, l)| l).collect();
    let _ = std::fs::write(&p, kept.join("\n") + if kept.is_empty() { "" } else { "\n" });
    reload_notes();
}

// ---------------- Obsidian 日记 ----------------

/// 库路径:config.toml [notes].vault,否则取 Obsidian 自己记录的"当前打开的库"
pub(crate) fn vault() -> Option<PathBuf> {
    let custom = config::raw_str("notes", "vault");
    if !custom.is_empty() {
        return Some(PathBuf::from(custom));
    }
    let text = std::fs::read_to_string(PathBuf::from(std::env::var("APPDATA").ok()?).join("obsidian").join("obsidian.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let vaults = v.get("vaults")?.as_object()?;
    let pick = vaults.values().find(|x| x["open"].as_bool() == Some(true)).or_else(|| vaults.values().next())?;
    pick["path"].as_str().map(PathBuf::from)
}

/// moment.js 格式 → chrono(只支持常见的 YYYY MM DD)
fn moment_to_chrono(fmt: &str) -> String {
    fmt.replace("YYYY", "%Y").replace("MM", "%m").replace("DD", "%d")
}

pub(crate) fn daily_note_path(vault: &std::path::Path) -> (PathBuf, String) {
    let cfg: serde_json::Value = std::fs::read_to_string(vault.join(".obsidian").join("daily-notes.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    // [notes] daily_folder 优先(daily-notes.json 里的目录可能已经过期)
    let over = config::get().notes.daily_folder;
    let folder = if !over.trim().is_empty() { over.trim().to_string() } else { cfg["folder"].as_str().unwrap_or("").to_string() };
    let fmt = cfg["format"].as_str().filter(|s| !s.is_empty()).unwrap_or("YYYY-MM-DD");
    let template = cfg["template"].as_str().unwrap_or("").to_string();
    let name = chrono::Local::now().format(&moment_to_chrono(fmt)).to_string();
    (vault.join(&folder).join(format!("{}.md", name)), template)
}

/// 今天的日记不存在就按模板创建
pub(crate) fn ensure_daily(vault: &std::path::Path) -> PathBuf {
    let (path, template) = daily_note_path(vault);
    if path.exists() {
        return path;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let now = chrono::Local::now();
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut body = String::new();
    if !template.is_empty() {
        let t = if template.to_lowercase().ends_with(".md") { template.clone() } else { format!("{}.md", template) };
        body = std::fs::read_to_string(vault.join(t)).unwrap_or_default();
    }
    if body.is_empty() {
        body = "# {{date}}\n\n## 💭 随笔 & 闪念\n- \n".to_string();
    }
    let body = body.replace("{{date}}", &stem).replace("{{title}}", &stem).replace("{{time}}", &now.format("%H:%M").to_string());
    let _ = std::fs::write(&path, body);
    path
}

/// 追加一条到今天日记的"随笔 & 闪念"小节(没有就在文末新建)。成功返回 true
pub(crate) fn append_diary(hhmm: &str, text: &str) -> bool {
    let Some(vault) = vault() else { return false };
    let path = ensure_daily(&vault);
    let Ok(content) = std::fs::read_to_string(&path) else { return false };
    let crlf = content.contains("\r\n");
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let bullet = format!("- {} {}", hhmm, text);
    let head = lines.iter().position(|l| l.trim_start().starts_with("## ") && (l.contains("闪念") || l.contains("随笔")));
    match head {
        Some(h) => {
            let end = lines.iter().enumerate().skip(h + 1).find(|(_, l)| l.trim_start().starts_with("## ")).map(|(i, _)| i).unwrap_or(lines.len());
            let mut pos = end;
            while pos > h + 1 && lines[pos - 1].trim().is_empty() {
                pos -= 1;
            }
            if pos > h + 1 && lines[pos - 1].trim() == "-" {
                lines[pos - 1] = bullet;
            } else {
                lines.insert(pos, bullet);
            }
        }
        None => {
            lines.push(String::new());
            lines.push("## 💭 随笔 & 闪念".to_string());
            lines.push(bullet);
        }
    }
    let sep = if crlf { "\r\n" } else { "\n" };
    std::fs::write(&path, lines.join(sep) + sep).is_ok()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

pub(crate) fn open_daily_note() {
    open_daily()
}

fn open_daily() {
    use std::os::windows::process::CommandExt;
    match vault() {
        Some(v) => {
            let path = ensure_daily(&v);
            let uri = format!("obsidian://open?path={}", percent_encode(&path.to_string_lossy()));
            let _ = std::process::Command::new("explorer.exe").arg(uri).creation_flags(0x0800_0000).spawn();
        }
        None => data::open_target(&notes_path().to_string_lossy(), ""),
    }
}

// ---------------- 绘制与点击 ----------------

pub fn draw(p: &mut P, v: &View) -> Result<()> {
    let r = rect();
    p.at(r);
    let th = p.th;
    let (white, muted) = (p.col(&th.white), p.col(&th.muted));
    let faint = windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 35.0 / 255.0 };
    p.rect(0.0, 0.0, W, ph(), 12.0, th.bg.with_alpha(250), p.col(&th.stroke), 1.5)?;
    p.line(12.0, 1.0, W - 12.0, 1.0, faint, 1.0)?;
    p.txt(16.0, 12.0, &perch_data::t("tools.title"), 9.5, p.col(&th.accent), L, false)?;
    p.txt(W - 16.0, 12.0, "✕", 9.0, muted, R, false)?;

    let tab = v.app.tools_tab.min(2);
    let tab_labels = [
        ("tl_tab0", perch_data::t("tools.tab_clipboard"), 16.0),
        ("tl_tab1", perch_data::t("tools.tab_inbox"), 106.0),
        ("tl_tab2", perch_data::t("tools.tab_quick"), 196.0),
    ];
    for (i, (id, label, x)) in tab_labels.iter().enumerate() {
        let (f, s, sw_) = if tab == i { (p.col(&th.stroke), p.col(&th.accent), 1.5) } else { (p.col(&th.item_bg), p.col(&th.stroke), 1.0) };
        let (f, s) = if p.hot(id) { (p.col(&th.item_hover), p.col(&th.accent)) } else { (f, s) };
        p.rect(*x, 40.0, 84.0, 26.0, 6.0, f, s, sw_)?;
        p.txt(x + 42.0, 46.0, label, 8.0, white, C, false)?;
    }

    if tab == 2 {
        // 亮度滑块:标签 + 百分比 + 轨道;拖动时显示预览值
        let internal_lbl = perch_data::t("tools.slider_internal");
        let external_lbl = perch_data::t("tools.slider_external");
        let sliders = [(&internal_lbl, crate::brightness::internal(), 100.0), (&external_lbl, crate::brightness::external(), 176.0)];
        for (k, (label, known, y)) in sliders.iter().enumerate() {
            let prev = PREVIEW[k].load(std::sync::atomic::Ordering::Relaxed);
            let val = if prev >= 0 { prev } else { *known };
            p.txt(20.0, y - 22.0, label, 8.5, white, L, false)?;
            let shown = if val >= 0 { format!("{}%", val) } else { perch_data::t("tools.screen_unsupported") };
            p.txt(W - 20.0, y - 22.0, &shown, 8.5, p.col(&th.accent), R, true)?;
            let tr = p.track();
            p.rect(20.0, y + 8.0, W - 40.0, 8.0, 4.0, tr, tr, 0.0)?;
            if val >= 0 {
                let w = (W - 40.0) * val as f32 / 100.0;
                p.rect(20.0, y + 8.0, w.max(4.0), 8.0, 4.0, p.col(&th.accent), p.col(&th.accent), 0.0)?;
                let hot = p.hot(if k == 0 { "tl_b0" } else { "tl_b1" });
                p.rect(20.0 + w - 7.0, y + 5.0, 14.0, 14.0, 7.0, white, if hot { p.col(&th.accent) } else { white }, 1.0)?;
            }
        }
        p.txt(20.0, 228.0, &perch_data::t("tools.audio_devices_hint"), 8.5, white, L, false)?;
        let devs = DEVICES.lock().map(|d| d.clone()).unwrap_or_default();
        if devs.is_empty() {
            p.txt(20.0, 262.0, &perch_data::t("tools.no_audio_devices"), 8.0, muted, L, false)?;
        }
        for (k, d) in devs.iter().take(4).enumerate() {
            let y = 258.0 + 30.0 * k as f32;
            let hot = p.hot(DEV_ID[k]);
            let (f, s) = if d.is_default { (p.col(&th.stroke), p.col(&th.accent)) } else if hot { (p.col(&th.item_hover), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
            p.rect(16.0, y, W - 32.0, 26.0, 6.0, f, s, if d.is_default { 1.5 } else { 1.0 })?;
            p.txt(26.0, y + 6.0, &format!("{} {}", if d.is_default { "●" } else { "○" }, clip(&d.name, 34)), 8.0, white, L, false)?;
        }
        // ---- Clash:模式 / 当前节点 / 延迟 ----
        let cs = crate::clash::state();
        p.txt(20.0, 392.0, &perch_data::t("tools.clash_proxy"), 8.5, white, L, false)?;
        let (rf, rs) = if p.hot("tl_cm") { (p.col(&th.item_hover), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
        p.rect(W - 74.0, 388.0, 58.0, 22.0, 5.0, rf, rs, 1.0)?;
        let speed_btn_txt = if cs.busy { perch_data::t("tools.speed_testing") } else { perch_data::t("tools.speed_test_btn") };
        p.txt(W - 45.0, 392.0, &speed_btn_txt, 7.5, white, C, false)?;
        if !cs.ok {
            p.txt(20.0, 424.0, &perch_data::t("tools.clash_not_found"), 8.0, muted, L, false)?;
        } else {
            let rule_lbl = perch_data::t("tools.mode_rule");
            let global_lbl = perch_data::t("tools.mode_global");
            let direct_lbl = perch_data::t("tools.mode_direct");
            for (k, (id, label, key)) in [("tl_c0", &rule_lbl, "rule"), ("tl_c1", &global_lbl, "global"), ("tl_c2", &direct_lbl, "direct")].iter().enumerate() {
                let x = 16.0 + 112.0 * k as f32;
                let on = cs.mode == *key;
                let (f, s) = if on { (p.col(&th.stroke), p.col(&th.accent)) } else if p.hot(id) { (p.col(&th.item_hover), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
                p.rect(x, 418.0, 104.0, 26.0, 6.0, f, s, if on { 1.5 } else { 1.0 })?;
                p.txt(x + 52.0, 424.0, &format!("{}{}", if on { "● " } else { "" }, label), 8.0, white, C, false)?;
            }
            let delay = if cs.delay >= 0 { format!("  {} ms", cs.delay) } else { String::new() };
            p.txt(20.0, 452.0, &format!("{}{}{}", perch_data::t("tools.current_prefix"), clip(&cs.node, 26), delay), 7.5, p.col(&th.accent), L, true)?;
        }
        p.txt(W / 2.0, ph() - 14.0, &perch_data::t("tools.quick_footer_hint"), 6.5, muted, C, false)?;
        return Ok(());
    }
    let scroll = SCROLL.load(std::sync::atomic::Ordering::Relaxed);
    let items: Vec<String> = if tab == 0 {
        CLIPS.lock().map(|l| l.clone()).unwrap_or_default()
    } else {
        NOTES.lock().map(|l| l.iter().map(|n| n.1.clone()).collect()).unwrap_or_default()
    };
    if items.is_empty() {
        let hint = if tab == 0 { perch_data::t("tools.empty_clipboard") } else { perch_data::t("tools.empty_inbox") };
        for (i, line) in hint.lines().enumerate() {
            p.txt(W / 2.0, 170.0 + 18.0 * i as f32, line, 8.5, muted, C, false)?;
        }
    }
    let small_btn = |p: &mut P, id: &'static str, label: &str, x: f32, y: f32| -> Result<()> {
        let th = p.th;
        let (f, s) = if p.hot(id) { (p.col(&th.item_hover), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
        p.rect(x, y + 5.0, 50.0, 24.0, 5.0, f, s, 1.0)?;
        p.txt(x + 25.0, y + 9.0, label, 7.5, p.col(&th.white), C, false)
    };
    let btn_todo_txt = perch_data::t("tools.btn_to_todo");
    let btn_inbox_txt = perch_data::t("tools.btn_to_inbox");
    let btn_diary_txt = perch_data::t("tools.btn_to_diary");
    for k in 0..ROWS {
        let Some(text) = items.get(scroll + k) else { break };
        let y = 76.0 + 38.0 * k as f32;
        let hot = p.hot(ROW_ID[k]);
        p.rect(12.0, y, W - 24.0, 34.0, 6.0, if hot { p.col(&th.item_hover) } else { p.col(&th.item_bg) }, p.col(&th.stroke), 1.0)?;
        if tab == 0 {
            let one_line = text.replace(['\r', '\n', '\t'], " ");
            p.txt(22.0, y + 9.0, &clip(&one_line, 22), 8.5, white, L, false)?;
            small_btn(p, TODO_ID[k], &btn_todo_txt, 210.0, y)?;
            small_btn(p, DIARY_ID[k], &btn_inbox_txt, 264.0, y)?;
        } else {
            let (stamp, body) = split_note(text);
            p.txt(22.0, y + 4.0, &clip(body, 22), 8.5, white, L, false)?;
            p.txt(22.0, y + 20.0, stamp, 6.5, muted, L, true)?;
            small_btn(p, TODO_ID[k], &btn_todo_txt, 210.0, y)?;
            small_btn(p, DIARY_ID[k], &btn_diary_txt, 264.0, y)?;
        }
        let col = if p.hot(DEL_ID[k]) { crate::theme::ColorRgba::new(255, 120, 120, 255).to_d2d() } else { crate::theme::ColorRgba::new(239, 68, 68, 200).to_d2d() };
        p.txt(W - 27.0, y + 9.0, "✕", 8.5, col, C, false)?;
    }
    if items.len() > ROWS {
        let frac = ROWS as f32 / items.len() as f32;
        let pos = scroll as f32 / (items.len() - ROWS) as f32;
        let tr = p.track();
        let h = 38.0 * ROWS as f32 - 4.0;
        p.rect(W - 8.0, 76.0, 3.0, h, 1.5, tr, tr, 0.0)?;
        p.rect(W - 8.0, 76.0 + h * (1.0 - frac) * pos, 3.0, h * frac, 1.5, p.col(&th.accent), p.col(&th.accent), 0.0)?;
    }
    let btn = |p: &mut P, id: &'static str, label: &str, x: f32, w: f32| -> Result<()> {
        let th = p.th;
        let (f, s) = if p.hot(id) { (p.col(&th.item_hover), p.col(&th.accent)) } else { (p.col(&th.item_bg), p.col(&th.stroke)) };
        p.rect(x, H - 40.0, w, 26.0, 6.0, f, s, 1.0)?;
        p.txt(x + w / 2.0, H - 34.0, label, 8.0, p.col(&th.white), C, false)
    };
    if tab == 0 {
        btn(p, "tl_clear", &perch_data::t("tools.btn_clear_clips"), W - 100.0, 84.0)?;
        p.txt(16.0, H - 34.0, &perch_data::t("tools.click_copy_hint"), 7.5, muted, L, false)?;
    } else {
        btn(p, "tl_add", &perch_data::t("tools.btn_add_note"), 16.0, 84.0)?;
        btn(p, "tl_diary", &perch_data::t("tools.btn_today_diary"), 108.0, 100.0)?;
        btn(p, "tl_file", &perch_data::t("tools.btn_notes_file"), 216.0, 110.0)?;
        p.txt(16.0, H - 14.0, &perch_data::t("tools.click_open_hint"), 6.5, muted, L, false)?;
    }
    Ok(())
}

pub fn scroll(app: &AppState, dir: i32) {
    if app.tools_tab == 2 {
        return;
    }
    let len = if app.tools_tab == 0 { CLIPS.lock().map(|l| l.len()).unwrap_or(0) } else { NOTES.lock().map(|l| l.len()).unwrap_or(0) };
    let max = len.saturating_sub(ROWS) as i32;
    let cur = SCROLL.load(std::sync::atomic::Ordering::Relaxed) as i32;
    SCROLL.store((cur + dir).clamp(0, max) as usize, std::sync::atomic::Ordering::Relaxed);
}

pub fn open(app: &mut AppState) {
    CUR_TAB.store(app.tools_tab, std::sync::atomic::Ordering::Relaxed);
    if app.tools_tab == 2 {
        crate::clash::refresh_async();
    }
    app.tools_open = true;
    app.notif_open = false;
    reload_notes();
    SCROLL.store(0, std::sync::atomic::Ordering::Relaxed);
}

fn one_line(s: &str, max: usize) -> String {
    s.replace(['\r', '\n', '\t'], " ").trim().chars().take(max).collect()
}

pub fn on_click(id: &str, app: &mut AppState, hub: &mut Hub, mouse: (f32, f32)) {
    use std::sync::atomic::Ordering::Relaxed;
    // 亮度滑块:按下 / 拖动时更新预览,松手(tl_bcommit)才写入
    if id == "tl_b0" || id == "tl_b1" {
        let k = if id == "tl_b0" { 0 } else { 1 };
        let frac = ((mouse.0 - (rect().0 + 20.0)) / (W - 40.0)).clamp(0.0, 1.0);
        PREVIEW[k].store(((frac * 100.0).round() as i32).max(5), Relaxed);
        return;
    }
    if let Some(k) = DEV_ID.iter().position(|i| *i == id) {
        let dev = DEVICES.lock().ok().and_then(|d| d.get(k).cloned());
        if let Some(d) = dev {
            if crate::audio::set_default_output(&d.id) {
                AUDIO_REBIND.store(true, Relaxed);
            }
            refresh_devices();
        }
        return;
    }
    match id {
        "tl_c0" => return crate::clash::set_mode_async("rule"),
        "tl_c1" => return crate::clash::set_mode_async("global"),
        "tl_c2" => return crate::clash::set_mode_async("direct"),
        "tl_cm" => return crate::clash::measure_async(),
        _ => {}
    }
    if id == "tl_bcommit" {
        let (a, b) = (PREVIEW[0].swap(-1, Relaxed), PREVIEW[1].swap(-1, Relaxed));
        if a >= 0 {
            crate::brightness::set_internal_async(a as u8);
        }
        if b >= 0 {
            crate::brightness::set_external_async(b as u8);
        }
        return;
    }
    let scroll = SCROLL.load(std::sync::atomic::Ordering::Relaxed);
    let clip_item = |k: usize| CLIPS.lock().ok().and_then(|l| l.get(scroll + k).cloned());
    let note_item = |k: usize| NOTES.lock().ok().and_then(|l| l.get(scroll + k).cloned());
    for k in 0..ROWS {
        if id == ROW_ID[k] {
            if app.tools_tab == 0 {
                if let Some(t) = clip_item(k) {
                    set_clipboard(&t);
                }
            }
            return;
        }
        if id == TODO_ID[k] {
            if app.tools_tab == 0 {
                if let Some(t) = clip_item(k) {
                    let t = one_line(&t, 200);
                    if !t.is_empty() {
                        data::core(&["todo", "add", &t]);
                        hub.refresh_soon();
                    }
                }
            } else if let Some((line, raw)) = note_item(k) {
                let body = one_line(split_note(&raw).1, 200);
                if !body.is_empty() {
                    data::core(&["todo", "add", &body]);
                    delete_note(line);
                    hub.refresh_soon();
                }
            }
            return;
        }
        if id == DIARY_ID[k] {
            if app.tools_tab == 0 {
                // 剪贴板 → 收集箱
                if let Some(t) = clip_item(k) {
                    add_note(&one_line(&t, 500));
                }
            } else if let Some((line, raw)) = note_item(k) {
                let (stamp, body) = split_note(&raw);
                let hhmm = if stamp.len() >= 16 { stamp[11..16].to_string() } else { chrono::Local::now().format("%H:%M").to_string() };
                // 写进日记成功才从收集箱删掉,失败(找不到库)就保留
                if append_diary(&hhmm, body) {
                    delete_note(line);
                }
            }
            return;
        }
        if id == DEL_ID[k] {
            if app.tools_tab == 0 {
                if let Ok(mut l) = CLIPS.lock() {
                    if scroll + k < l.len() {
                        l.remove(scroll + k);
                    }
                }
            } else if let Some((line, _)) = note_item(k) {
                delete_note(line);
            }
            return;
        }
    }
    match id {
        "tl_close" => app.tools_open = false,
        "tl_tab0" | "tl_tab1" | "tl_tab2" => {
            app.tools_tab = (id.as_bytes()[6] - b'0') as usize;
            CUR_TAB.store(app.tools_tab, std::sync::atomic::Ordering::Relaxed);
            if app.tools_tab == 2 {
                crate::brightness::refresh_async();
                refresh_devices();
                crate::clash::refresh_async();
            }
            SCROLL.store(0, std::sync::atomic::Ordering::Relaxed);
            if app.tools_tab == 1 {
                reload_notes();
            }
        }
        "tl_clear" => {
            if let Ok(mut l) = CLIPS.lock() {
                l.clear();
            }
        }
        "tl_add" => {
            if let Some(text) = crate::prompt::ask(&perch_data::t("cards2.collect_title"), &perch_data::t("cards2.collect_hint"), "", &app.theme) {
                add_note(&text);
            }
        }
        "tl_diary" => open_daily(),
        "tl_open" => {
            let p = notes_path();
            if !p.exists() {
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&p, "");
            }
            data::open_target(&p.to_string_lossy(), "");
        }
        _ => {}
    }
}
