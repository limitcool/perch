//! Dock 窗口预览:悬停在有窗口的运行中应用上稍等片刻,图标上方弹出该应用所有窗口的 DWM 缩略图(系统合成,几乎不占 CPU)。
//! 点缩略图切到那个窗口,右上角 ✕ 关闭该窗口(WM_CLOSE);鼠标离开图标和预览 0.35 秒后收起。

use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::Result;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::cards::{self, clip, C, L, P, R};
use crate::config;
use crate::running;
use crate::window::{scale, RectBox};

const CELL_W: f32 = 176.0;
const CELL_H: f32 = 128.0;
const THUMB_W: f32 = 160.0;
const THUMB_H: f32 = 90.0;
const GAP: f32 = 8.0;
const MAX_WINDOWS: usize = 6;

struct Thumb {
    hwnd: isize,
    title: String,
    handle: isize,
}

struct State {
    dock_idx: usize,
    rect: (f32, f32, f32, f32),
    thumbs: Vec<Thumb>,
    leave_since: Option<Instant>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

const CELL_ID: [&str; MAX_WINDOWS] = ["pv0", "pv1", "pv2", "pv3", "pv4", "pv5"];
const CLOSE_ID: [&str; MAX_WINDOWS] = ["pv_x0", "pv_x1", "pv_x2", "pv_x3", "pv_x4", "pv_x5"];
const MIN_ID: [&str; MAX_WINDOWS] = ["pv_m0", "pv_m1", "pv_m2", "pv_m3", "pv_m4", "pv_m5"];
const MAX_ID: [&str; MAX_WINDOWS] = ["pv_z0", "pv_z1", "pv_z2", "pv_z3", "pv_z4", "pv_z5"];

/// 窗口控制按钮放在格子顶部的标题条里,缩略图在它下面(缩略图由 DWM 叠在最上层合成,画在它上面的东西会被盖住,所以标题条不能和缩略图重叠)
const BTN_W: f32 = 26.0;
const BTN_H: f32 = 20.0;
fn btn_x(k: usize) -> f32 {
    // k: 0 最小化,1 最大化/还原,2 关闭 → 相对格子左边的 x
    CELL_W - 8.0 - BTN_W * (3 - k) as f32 - 2.0 * (2 - k) as f32
}

pub fn active() -> bool {
    STATE.lock().map_or(false, |s| s.is_some())
}

/// 布局签名:预览出现 / 变化时重建点击区域和窗口区域
pub fn signature() -> i64 {
    STATE.lock().ok().and_then(|s| s.as_ref().map(|s| (s.dock_idx as i64 + 1) * 100 + s.thumbs.len() as i64)).unwrap_or(0)
}

pub fn region() -> Option<(f32, f32, f32, f32, f32)> {
    STATE.lock().ok()?.as_ref().map(|s| (s.rect.0, s.rect.1, s.rect.2, s.rect.3, 12.0))
}

fn cell_origin(r: (f32, f32, f32, f32), k: usize) -> (f32, f32) {
    (r.0 + GAP + k as f32 * (CELL_W + GAP), r.1 + GAP)
}

pub fn boxes() -> Vec<RectBox> {
    let g = STATE.lock().unwrap();
    let Some(s) = g.as_ref() else { return Vec::new() };
    let mut v = Vec::new();
    for k in 0..s.thumbs.len() {
        let (x, y) = cell_origin(s.rect, k);
        let by = y + 4.0;
        v.push(RectBox { id: CLOSE_ID[k], x: x + btn_x(2), y: by, w: BTN_W, h: BTN_H });
        v.push(RectBox { id: MAX_ID[k], x: x + btn_x(1), y: by, w: BTN_W, h: BTN_H });
        v.push(RectBox { id: MIN_ID[k], x: x + btn_x(0), y: by, w: BTN_W, h: BTN_H });
    }
    for k in 0..s.thumbs.len() {
        let (x, y) = cell_origin(s.rect, k);
        v.push(RectBox { id: CELL_ID[k], x, y, w: CELL_W, h: CELL_H });
    }
    v.push(RectBox { id: "pv_panel", x: s.rect.0, y: s.rect.1, w: s.rect.2, h: s.rect.3 });
    v
}

fn hud() -> HWND {
    HWND(crate::window::hud_hwnd() as *mut core::ffi::c_void)
}

fn title_of(h: isize) -> String {
    let mut buf = [0u16; 160];
    let n = unsafe { GetWindowTextW(HWND(h as *mut core::ffi::c_void), &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

/// 缩略图放进 THUMB_W×THUMB_H 的格子里,保持源窗口的宽高比并居中
fn place(handle: isize, x: f32, y: f32) {
    let s = scale();
    unsafe {
        let ht = handle;
        let size = DwmQueryThumbnailSourceSize(ht).unwrap_or_default();
        let (sw_, sh_) = (size.cx.max(1) as f32, size.cy.max(1) as f32);
        let k = (THUMB_W / sw_).min(THUMB_H / sh_);
        let (w, h) = (sw_ * k, sh_ * k);
        let (dx, dy) = (x + 8.0 + (THUMB_W - w) / 2.0, y + 30.0 + (THUMB_H - h) / 2.0);
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
            rcDestination: RECT { left: (dx * s) as i32, top: (dy * s) as i32, right: ((dx + w) * s) as i32, bottom: ((dy + h) * s) as i32 },
            opacity: 255,
            fVisible: BOOL(1),
            ..Default::default()
        };
        let _ = DwmUpdateThumbnailProperties(ht, &props);
    }
}

fn open(idx: usize) {
    close();
    let Some(entry) = config::get().dock.get(idx).cloned() else { return };
    let Some(e) = running::key_for(&entry).and_then(|k| running::entry(&k)) else { return };
    let wins: Vec<isize> = e.windows.iter().copied().take(MAX_WINDOWS).collect();
    if wins.is_empty() {
        return;
    }
    let mut thumbs = Vec::new();
    for w in wins {
        let src = HWND(w as *mut core::ffi::c_void);
        if let Ok(h) = unsafe { DwmRegisterThumbnail(hud(), src) } {
            thumbs.push(Thumb { hwnd: w, title: title_of(w), handle: h });
        }
    }
    if thumbs.is_empty() {
        return;
    }
    // 位置:Dock 图标正上方,水平居中,限制在屏幕内
    let d = cards::dock_rect();
    let icon_cx = d.0 + 20.0 + 52.0 * idx as f32 + 23.0;
    let w = GAP + thumbs.len() as f32 * (CELL_W + GAP);
    let h = CELL_H + 2.0 * GAP;
    let x = (icon_cx - w / 2.0).clamp(4.0, (cards::sw() - w - 4.0).max(4.0));
    let y = d.1 - h - 10.0;
    let rect = (x, y, w, h);
    for (k, t) in thumbs.iter().enumerate() {
        let (cx, cy) = cell_origin(rect, k);
        place(t.handle, cx, cy);
    }
    *STATE.lock().unwrap() = Some(State { dock_idx: idx, rect, thumbs, leave_since: None });
}

pub fn close() {
    let old = STATE.lock().unwrap().take();
    if let Some(s) = old {
        for t in s.thumbs {
            unsafe {
                let _ = DwmUnregisterThumbnail(t.handle);
            }
        }
    }
}

/// 主循环每轮调用:根据悬停对象决定打开 / 保持 / 收起。返回 true 表示需要重绘 / 重建布局
pub fn update(hover: Option<&'static str>, hover_for: Duration) -> bool {
    let on_dock = hover.and_then(|h| h.strip_prefix("dk")).and_then(|s| s.parse::<usize>().ok());
    let on_preview = hover.map_or(false, |h| h.starts_with("pv"));
    let cur = STATE.lock().unwrap().as_ref().map(|s| s.dock_idx);
    if on_preview || (on_dock.is_some() && on_dock == cur) {
        if let Some(s) = STATE.lock().unwrap().as_mut() {
            s.leave_since = None;
        }
        return false;
    }
    if let Some(idx) = on_dock {
        // 换到另一个图标,或停留够久:打开(没有窗口的应用 open 内部会直接放弃)
        if hover_for >= Duration::from_millis(550) || cur.is_some() {
            if on_dock != cur {
                open(idx);
                return active() || cur.is_some();
            }
        }
        return false;
    }
    // 既不在图标上也不在预览上:延迟收起
    let mut g = STATE.lock().unwrap();
    if let Some(s) = g.as_mut() {
        let since = *s.leave_since.get_or_insert_with(Instant::now);
        if since.elapsed() >= Duration::from_millis(350) {
            drop(g);
            close();
            return true;
        }
    }
    false
}

pub fn draw(p: &mut P) -> Result<()> {
    let g = STATE.lock().unwrap();
    let Some(s) = g.as_ref() else { return Ok(()) };
    let th = p.th;
    p.ox = 0.0;
    p.oy = 0.0;
    let r = s.rect;
    p.rect(r.0, r.1, r.2, r.3, 12.0, th.bg.with_alpha(245), p.col(&th.accent), 1.5)?;
    for (k, t) in s.thumbs.iter().enumerate() {
        let (x, y) = cell_origin(r, k);
        let hot = p.hot(CELL_ID[k]) || p.hot(CLOSE_ID[k]) || p.hot(MIN_ID[k]) || p.hot(MAX_ID[k]);
        p.rect(x, y, CELL_W, CELL_H, 8.0, if hot { p.col(&th.item_hover) } else { p.col(&th.item_bg) }, if hot { p.col(&th.accent) } else { p.col(&th.stroke) }, 1.0)?;
        // 标题条:标题(左) + 最小化 / 最大化(还原) / 关闭(右),始终可见
        p.txt(x + 8.0, y + 7.0, &clip(&t.title, 11), 7.5, p.col(&th.white), L, false)?;
        let zoomed = unsafe { IsZoomed(HWND(t.hwnd as *mut core::ffi::c_void)).as_bool() };
        for (idx, id, glyph) in [(0usize, MIN_ID[k], "—"), (1, MAX_ID[k], if zoomed { "❐" } else { "□" }), (2, CLOSE_ID[k], "✕")] {
            let bx = x + btn_x(idx);
            let on = p.hot(id);
            let fill = if idx == 2 && on { crate::theme::ColorRgba::new(220, 60, 60, 255).to_d2d() } else if on { p.col(&th.stroke) } else { p.col(&th.bg) };
            p.rect(bx, y + 4.0, BTN_W, BTN_H, 5.0, fill, if on { p.col(&th.accent) } else { p.col(&th.stroke) }, 1.0)?;
            p.txt(bx + BTN_W / 2.0, y + 7.0, glyph, 8.0, p.col(&th.white), C, false)?;
        }
    }
    let _ = (L, R);
    Ok(())
}

pub fn on_click(id: &str) {
    let hwnd_of = |k: usize| STATE.lock().ok().and_then(|s| s.as_ref().and_then(|s| s.thumbs.get(k).map(|t| t.hwnd)));
    // 系统菜单命令(对其他进程的窗口也有效):SC_MINIMIZE / SC_MAXIMIZE / SC_RESTORE
    let sys_cmd = |h: isize, cmd: usize| unsafe {
        let _ = PostMessageW(HWND(h as *mut core::ffi::c_void), WM_SYSCOMMAND, WPARAM(cmd), LPARAM(0));
    };
    if let Some(k) = MIN_ID.iter().position(|i| *i == id) {
        if let Some(h) = hwnd_of(k) {
            sys_cmd(h, 0xF020);
        }
        close();
        running::refresh_soon();
    } else if let Some(k) = MAX_ID.iter().position(|i| *i == id) {
        if let Some(h) = hwnd_of(k) {
            let zoomed = unsafe { IsZoomed(HWND(h as *mut core::ffi::c_void)).as_bool() };
            sys_cmd(h, if zoomed { 0xF120 } else { 0xF030 });
            if !zoomed {
                running::focus_window(h);
            }
        }
        close();
    } else if let Some(k) = CLOSE_ID.iter().position(|i| *i == id) {
        if let Some(h) = hwnd_of(k) {
            unsafe {
                let _ = PostMessageW(HWND(h as *mut core::ffi::c_void), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        close();
        running::refresh_soon();
    } else if let Some(k) = CELL_ID.iter().position(|i| *i == id) {
        if let Some(h) = hwnd_of(k) {
            running::focus_window(h);
        }
        close();
    }
}
