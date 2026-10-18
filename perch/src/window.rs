use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, HGDIOBJ, RGN_OR};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectBox {
    pub id: &'static str,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectBox {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= (self.x + self.w) && py >= self.y && py <= (self.y + self.h)
    }
}

// 全局交互区域与状态存储
pub static HIT_BOXES: Mutex<Vec<RectBox>> = Mutex::new(Vec::new());
pub static MOUSE_POS: Mutex<(f32, f32)> = Mutex::new((-1.0, -1.0));
pub static HOVERED_ID: Mutex<Option<&'static str>> = Mutex::new(None);
pub static CLICK_QUEUE: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
pub static SHOULD_EXIT: AtomicBool = AtomicBool::new(false);
static DRAG_SLIDER: AtomicBool = AtomicBool::new(false);
static DRAG_SEEK: AtomicBool = AtomicBool::new(false);
static DRAG_CARD: AtomicBool = AtomicBool::new(false);
/// 分辨率 / DPI / 显示器配置变化:主循环据此重建窗口尺寸、渲染目标和布局
pub static RELAYOUT: AtomicBool = AtomicBool::new(false);
/// HUD 所在显示器的左上角 / 尺寸(物理像素,虚拟桌面坐标)
static MON_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static MON_Y: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static MON_W: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static MON_H: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

pub fn origin() -> (i32, i32) {
    (MON_X.load(Ordering::Relaxed), MON_Y.load(Ordering::Relaxed))
}

pub fn monitor_size() -> (i32, i32) {
    (MON_W.load(Ordering::Relaxed), MON_H.load(Ordering::Relaxed))
}

/// 按 config.toml [display].monitor 选显示器:0 = 主显示器,1.. = 其余显示器(系统枚举顺序);越界回退主显示器
pub fn pick_monitor(index: u32) -> (i32, i32, i32, i32) {
    let list = enum_monitors();
    let primary = list.iter().find(|m| m.0).map(|m| m.1).unwrap_or(unsafe { (0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) });
    if index == 0 {
        return primary;
    }
    let others: Vec<(i32, i32, i32, i32)> = list.iter().filter(|m| !m.0).map(|m| m.1).collect();
    others.get(index as usize - 1).copied().unwrap_or(primary)
}
/// 系统 DPI 缩放(1.0 = 100%)。布局全部用 DIP(与旧皮肤一致),像素 = DIP × 缩放
static SCALE_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F80_0000);

pub fn scale() -> f32 {
    f32::from_bits(SCALE_BITS.load(Ordering::Relaxed))
}

pub fn set_scale(s: f32) {
    SCALE_BITS.store(s.max(0.5).to_bits(), Ordering::Relaxed);
}
/// 托盘菜单请求(主循环消费)
pub static TRAY_OPEN_SELECTOR: AtomicBool = AtomicBool::new(false);
pub static TRAY_RELOAD: AtomicBool = AtomicBool::new(false);
pub static TRAY_FONTS: AtomicBool = AtomicBool::new(false);
pub static TRAY_FLOAT: AtomicBool = AtomicBool::new(false);
pub static TRAY_SEARCH: AtomicBool = AtomicBool::new(false);
pub static TRAY_SETTINGS: AtomicBool = AtomicBool::new(false);
pub static TRAY_TOOLS: AtomicBool = AtomicBool::new(false);
/// 快捷页的亮度滑块拖动中(记录是哪一个)
pub static DRAG_QS: AtomicBool = AtomicBool::new(false);
pub static DRAG_QS_ID: Mutex<&'static str> = Mutex::new("");
/// 本次点击带了 Shift 或是中键:Dock 走"另一种"启动方式
pub static ALT_CLICK: AtomicBool = AtomicBool::new(false);
/// Dock 右键菜单选中的动作 (Dock 序号, 命令)
pub static DOCK_ACTIONS: Mutex<Vec<(usize, u8)>> = Mutex::new(Vec::new());
/// 拖到 HUD 上的文件(主循环消费:加入 Dock)
pub static DROPPED: Mutex<Vec<String>> = Mutex::new(Vec::new());
pub static FLOAT_ON: AtomicBool = AtomicBool::new(false);
pub static THEME_FONTS_ON: AtomicBool = AtomicBool::new(false);
static HUD_VISIBLE: AtomicBool = AtomicBool::new(true);
/// 桌面被展示(Win+D)时 HUD 暂时置顶,平时压在最底层
static LIFTED: AtomicBool = AtomicBool::new(false);
static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

const WM_TRAY: u32 = WM_APP + 1;
const CMD_TOGGLE: usize = 1;
const CMD_SELECTOR: usize = 2;
const CMD_RELOAD: usize = 3;
const CMD_CORE_RELOAD: usize = 4;
const CMD_ICONS: usize = 5;
const CMD_FONTS: usize = 6;
const CMD_FLOAT: usize = 7;
const CMD_SEARCH: usize = 8;
const CMD_EXIT: usize = 9;
const CMD_SETTINGS: usize = 10;
const CMD_TOOLS: usize = 11;
const CMD_OPEN_CFG: usize = 12;
const CMD_OPEN_LOG: usize = 13;

fn wide_tip(s: &str) -> [u16; 128] {
    let mut out = [0u16; 128];
    for (i, c) in s.encode_utf16().take(127).enumerate() {
        out[i] = c;
    }
    out
}

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let hinst = unsafe { GetModuleHandleW(None).unwrap_or_default() };
    // 资源 ID 1 = build.rs 嵌入的 perch.ico
    let icon = unsafe { LoadIconW(hinst, PCWSTR(1usize as *const u16)).unwrap_or_default() };
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        szTip: wide_tip("Perch"),
        ..Default::default()
    }
}

pub fn hud_visible() -> bool {
    HUD_VISIBLE.load(Ordering::Relaxed)
}

/// 根据"桌面是否被展示"在置底 / 置顶之间切换
pub fn set_lifted(hwnd: HWND, lift: bool) {
    if LIFTED.swap(lift, Ordering::SeqCst) == lift {
        return;
    }
    unsafe {
        let _ = SetWindowPos(hwnd, if lift { HWND_TOPMOST } else { HWND_BOTTOM }, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

pub fn add_tray_icon(hwnd: HWND) {
    unsafe {
        let nid = tray_data(hwnd);
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        TASKBAR_CREATED.store(RegisterWindowMessageW(windows::core::w!("TaskbarCreated")), Ordering::Relaxed);
    }
}

pub fn remove_tray_icon(hwnd: HWND) {
    unsafe {
        let nid = tray_data(hwnd);
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

fn toggle_visible(hwnd: HWND) {
    let show = !HUD_VISIBLE.load(Ordering::Relaxed);
    HUD_VISIBLE.store(show, Ordering::Relaxed);
    unsafe {
        let _ = ShowWindow(hwnd, if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

/// 枚举所有显示器:(是否主显示器, (x, y, 宽, 高))
pub fn enum_monitors() -> Vec<(bool, (i32, i32, i32, i32))> {
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
    unsafe extern "system" fn cb(mon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<(bool, (i32, i32, i32, i32))>);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            let r = mi.rcMonitor;
            list.push((mi.dwFlags & 1 != 0, (r.left, r.top, r.right - r.left, r.bottom - r.top)));
        }
        BOOL(1)
    }
    let mut list: Vec<(bool, (i32, i32, i32, i32))> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

/// 托盘菜单里"显示在"子菜单的命令号起点(第 i 块屏幕 = 起点 + i,i 的含义同 config [display].monitor)
const CMD_MONITOR_BASE: usize = 100;

fn show_tray_menu(hwnd: HWND) {
    unsafe {
        let menu = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };
        // 动态文字的菜单项:Vec<u16> 以 0 结尾
        let add = |m: HMENU, id: usize, text: &str, checked: bool| {
            let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            let flags = if checked { MF_STRING | MF_CHECKED } else { MF_STRING };
            let _ = AppendMenuW(m, flags, id, PCWSTR(wide.as_ptr()));
        };
        let sep = |m: HMENU| {
            let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        };
        let cfg = crate::config::get();
        let hk = &cfg.hotkeys;
        let visible = HUD_VISIBLE.load(Ordering::Relaxed);
        let toggle_txt = perch_data::t_fmt(if visible { "tray.hide_hud" } else { "tray.show_hud" }, &[("hk", &hk.toggle)]);
        add(menu, CMD_TOGGLE, &toggle_txt, false);

        // 显示在 ▸ 屏幕列表:一眼看到每块屏的分辨率,当前的打勾
        let mons = enum_monitors();
        if mons.len() > 1 {
            if let Ok(sub) = CreatePopupMenu() {
                let others: Vec<&(bool, (i32, i32, i32, i32))> = mons.iter().filter(|m| !m.0).collect();
                let primary = mons.iter().find(|m| m.0).or_else(|| mons.first());
                let current = (cfg.display.monitor as usize).min(mons.len() - 1);
                if let Some(pm) = primary {
                    add(sub, CMD_MONITOR_BASE, &format!("{}  {}×{}", perch_data::t("tray.mon_primary"), pm.1 .2, pm.1 .3), current == 0);
                }
                for (i, m) in others.iter().enumerate() {
                    add(sub, CMD_MONITOR_BASE + i + 1, &format!("{} {}  {}×{}", perch_data::t("tray.mon_other"), i + 2, m.1 .2, m.1 .3), current == i + 1);
                }
                let title: Vec<u16> = perch_data::t("tray.display_on").encode_utf16().chain(Some(0)).collect();
                let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(title.as_ptr()));
            }
        }
        sep(menu);
        add(menu, CMD_SETTINGS, &perch_data::t("tray.settings_dots"), false);
        add(menu, CMD_TOOLS, &perch_data::t_fmt("tray.tools_entry", &[("hk", &hk.tools)]), false);
        add(menu, CMD_SEARCH, &perch_data::t_fmt("tray.search_entry", &[("hk", &hk.search)]), false);
        add(menu, CMD_SELECTOR, &perch_data::t("tray.selector_panel"), false);
        sep(menu);
        let float = FLOAT_ON.load(Ordering::Relaxed);
        let lvl = if float { perch_data::t("tray.level_float") } else { perch_data::t("tray.level_widget") };
        add(menu, CMD_FLOAT, &perch_data::t_fmt("tray.window_level", &[("level", &lvl), ("hk", &hk.float)]), false);
        add(menu, CMD_FONTS, &if THEME_FONTS_ON.load(Ordering::Relaxed) { perch_data::t("tray.font_on") } else { perch_data::t("tray.font_off") }, false);
        add(menu, CMD_ICONS, &if crate::desktop::currently_hidden() { perch_data::t("tray.desktop_icons_show") } else { perch_data::t("tray.desktop_icons_hide") }, false);
        sep(menu);
        add(menu, CMD_RELOAD, &perch_data::t("tray.reload_all"), false);
        add(menu, CMD_CORE_RELOAD, &perch_data::t("tray.regen_data"), false);
        add(menu, CMD_OPEN_CFG, &perch_data::t("tray.open_config"), false);
        add(menu, CMD_OPEN_LOG, &perch_data::t("tray.open_log"), false);
        sep(menu);
        add(menu, CMD_EXIT, &perch_data::t("tray.exit_hud"), false);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // 托盘菜单要求先把窗口置前,否则点别处菜单不会消失
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None);
        let _ = DestroyMenu(menu);
        let id = cmd.0 as usize;
        if (CMD_MONITOR_BASE..CMD_MONITOR_BASE + 16).contains(&id) {
            let idx = (id - CMD_MONITOR_BASE) as i64;
            crate::config::edit(|doc| crate::config::set_value(doc, "display", "monitor", toml_edit::Value::from(idx)));
            RELAYOUT.store(true, Ordering::SeqCst);
            return;
        }
        match id {
            CMD_TOGGLE => toggle_visible(hwnd),
            CMD_SELECTOR => TRAY_OPEN_SELECTOR.store(true, Ordering::SeqCst),
            CMD_SETTINGS => TRAY_SETTINGS.store(true, Ordering::SeqCst),
            CMD_TOOLS => TRAY_TOOLS.store(true, Ordering::SeqCst),
            CMD_RELOAD => TRAY_RELOAD.store(true, Ordering::SeqCst),
            CMD_CORE_RELOAD => crate::data::core(&["reload"]),
            CMD_ICONS => crate::desktop::toggle_user(),
            CMD_FONTS => TRAY_FONTS.store(true, Ordering::SeqCst),
            CMD_FLOAT => TRAY_FLOAT.store(true, Ordering::SeqCst),
            CMD_SEARCH => TRAY_SEARCH.store(true, Ordering::SeqCst),
            CMD_OPEN_CFG => crate::data::open_target(&crate::config::path().to_string_lossy(), ""),
            CMD_OPEN_LOG => crate::data::open_target(&format!(r"{}\perch\hud.log", std::env::var("APPDATA").unwrap_or_default()), ""),
            CMD_EXIT => {
                // 用户主动退出:临时停用看门狗计划任务,否则一分钟后又会被拉起来(下次启动 HUD 时自动重新启用)
                crate::desktop::set_watchdog(false);
                remove_tray_icon(hwnd);
                SHOULD_EXIT.store(true, Ordering::SeqCst);
                PostQuitMessage(0);
            }
            _ => {}
        }
    }
}

/// Dock 图标右键菜单,返回命令(0 = 取消):1 新开 / 启动,2 切到已有窗口,3 关闭,4 从 Dock 移除
fn show_dock_menu(hwnd: HWND, idx: usize) -> u8 {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return 0 };
        let running = crate::config::get().dock.get(idx).map_or(false, |d| crate::running::is_running(d));
        let add_item = |m: HMENU, id: usize, key: &str| {
            let wide: Vec<u16> = perch_data::t(key).encode_utf16().chain(Some(0)).collect();
            let _ = AppendMenuW(m, MF_STRING, id, PCWSTR(wide.as_ptr()));
        };
        if running {
            add_item(menu, 2, "dock_menu.switch_window");
            add_item(menu, 1, "dock_menu.new_instance");
            add_item(menu, 3, "dock_menu.close");
        } else {
            add_item(menu, 1, "dock_menu.launch");
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        add_item(menu, 4, "dock_menu.remove");
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None);
        let _ = DestroyMenu(menu);
        cmd.0 as u8
    }
}

/// 解析 "Win+Alt+S" 这样的写法 → (修饰键, 虚拟键码)
pub fn parse_combo(s: &str) -> Option<(u32, u32)> {
    let (mut mods, mut key) = (0u32, None);
    for tok in s.split('+').map(|t| t.trim()).filter(|t| !t.is_empty()) {
        match tok.to_lowercase().as_str() {
            "win" | "super" | "meta" => mods |= 0x8,
            "ctrl" | "control" => mods |= 0x2,
            "alt" => mods |= 0x1,
            "shift" => mods |= 0x4,
            "space" => key = Some(0x20),
            "enter" | "return" => key = Some(0x0D),
            "tab" => key = Some(0x09),
            "esc" | "escape" => key = Some(0x1B),
            k if k.len() >= 2 && k.starts_with('f') && k[1..].parse::<u32>().map_or(false, |n| (1..=24).contains(&n)) => key = Some(0x6F + k[1..].parse::<u32>().unwrap()),
            k if k.chars().count() == 1 && k.chars().next().map_or(false, |c| c.is_ascii_alphanumeric()) => key = Some(k.to_ascii_uppercase().as_bytes()[0] as u32),
            _ => return None,
        }
    }
    // 没有修饰键的全局热键会吃掉普通按键,不允许
    if mods == 0 {
        return None;
    }
    key.map(|k| (mods, k))
}

/// 热键注册结果(给设置面板显示)
pub static HOTKEY_STATUS: Mutex<String> = Mutex::new(String::new());
/// 设置面板改了热键:主循环重新注册
pub static REHOTKEY: AtomicBool = AtomicBool::new(false);
static HUD_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

pub fn hud_hwnd() -> isize {
    HUD_HWND.load(Ordering::Relaxed)
}

/// 按 config.toml [hotkeys] 重新注册全局热键(先注销旧的)
pub fn register_hotkeys(hwnd: HWND) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT};
    HUD_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
    let hk = crate::config::get().hotkeys;
    let list = [(1, perch_data::t("settings2.hkn_toggle"), hk.toggle), (2, perch_data::t("settings2.hkn_float"), hk.float), (3, perch_data::t("settings2.hkn_search"), hk.search), (4, perch_data::t("settings2.hkn_tools"), hk.tools)];
    let mut failed: Vec<String> = Vec::new();
    for (id, name, combo) in list {
        unsafe {
            let _ = UnregisterHotKey(hwnd, id);
            match parse_combo(&combo) {
                None => failed.push(perch_data::t_fmt("settings2.hk_bad", &[("name", &name), ("combo", &combo)])),
                Some((mods, key)) => {
                    if RegisterHotKey(hwnd, id, HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT, key).is_err() {
                        crate::log::log(&format!("hotkey {} unavailable", combo));
                        failed.push(perch_data::t_fmt("settings2.hk_taken", &[("name", &name), ("combo", &combo)]));
                    }
                }
            }
        }
    }
    if let Ok(mut s) = HOTKEY_STATUS.lock() {
        *s = if failed.is_empty() { perch_data::t("settings2.hk_all_ok") } else { perch_data::t_fmt("settings2.hk_failed", &[("list", &failed.join("; "))]) };
    }
}

pub fn pop_click_events() -> Vec<&'static str> {
    if let Ok(mut queue) = CLICK_QUEUE.lock() {
        std::mem::take(&mut *queue)
    } else {
        Vec::new()
    }
}

pub fn update_hit_boxes(boxes: Vec<RectBox>) {
    if let Ok(mut lock) = HIT_BOXES.lock() {
        *lock = boxes;
    }
}

/// 把窗口裁成"所有卡片矩形的并集":卡片之外根本不存在窗口,点击直接落到桌面/其他程序。
/// rects: (x, y, w, h, 圆角半径)
pub fn apply_window_region(hwnd: HWND, rects: &[(f32, f32, f32, f32, f32)]) {
    unsafe {
        let region = CreateRectRgn(0, 0, 0, 0);
        let s = scale();
        for &(x, y, w, h, r) in rects {
            let (x, y, w, h, r) = (x * s, y * s, w * s, h * s, r * s);
            let piece = CreateRoundRectRgn(x as i32, y as i32, (x + w) as i32 + 1, (y + h) as i32 + 1, (r * 2.0) as i32, (r * 2.0) as i32);
            CombineRgn(region, region, piece, RGN_OR);
            let _ = DeleteObject(HGDIOBJ(piece.0));
        }
        // 区域所有权转交给系统
        SetWindowRgn(hwnd, region, true);
    }
}

pub struct HudWindow {
    pub hwnd: HWND,
    pub width: u32,
    pub height: u32,
}

impl HudWindow {
    /// 显示配置变化后重新取屏幕尺寸并调整窗口;返回是否发生了变化
    pub fn refresh_size(&mut self) -> bool {
        unsafe {
            let dpi = windows::Win32::UI::HiDpi::GetDpiForSystem();
            let dpi_window = windows::Win32::UI::HiDpi::GetDpiForWindow(self.hwnd);
            set_scale(if dpi_window > 0 { dpi_window } else { dpi } as f32 / 96.0);
            let (mx, my, mw, mh) = pick_monitor(crate::config::get().display.monitor);
            MON_X.store(mx, Ordering::Relaxed);
            MON_Y.store(my, Ordering::Relaxed);
            MON_W.store(mw, Ordering::Relaxed);
            MON_H.store(mh, Ordering::Relaxed);
            let (w, h) = (mw as u32, mh as u32);
            let changed = w != self.width || h != self.height;
            self.width = w;
            self.height = h;
            let _ = SetWindowPos(self.hwnd, None, mx, my, w as i32, h as i32, SWP_NOZORDER | SWP_NOACTIVATE);
            changed
        }
    }

    pub fn create() -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class_name = windows::core::w!("PerchHudWindowClass");
            let title_pcwstr = windows::core::w!("Perch Tactical HUD (Rust D2D Full Suite)");

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                hIcon: LoadIconW(instance, PCWSTR(1usize as *const u16)).unwrap_or_default(),
                lpszClassName: class_name,
                ..Default::default()
            };

            RegisterClassExW(&wc);

            let (mx, my, mw, mh) = pick_monitor(crate::config::get().display.monitor);
            MON_X.store(mx, Ordering::Relaxed);
            MON_Y.store(my, Ordering::Relaxed);
            MON_W.store(mw, Ordering::Relaxed);
            MON_H.store(mh, Ordering::Relaxed);
            let (width, height) = (mw as u32, mh as u32);

            // 仅使用 WS_EX_TOOLWINDOW 与 WS_EX_TOPMOST，配合 DwmExtendFrameIntoClientArea 达到 D2D 完美硬件级透明
            let ex_style = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
            let style = WS_POPUP | WS_VISIBLE;

            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                title_pcwstr,
                style,
                mx,
                my,
                width as i32,
                height as i32,
                None,
                None,
                instance,
                None,
            )?;

            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            DwmExtendFrameIntoClientArea(hwnd, &margins)?;

            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            windows::Win32::UI::Shell::DragAcceptFiles(hwnd, true);
            let _ = windows::Win32::System::DataExchange::AddClipboardFormatListener(hwnd);
            add_tray_icon(hwnd);
            let _ = windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification(hwnd, windows::Win32::System::RemoteDesktop::NOTIFY_FOR_THIS_SESSION);
            register_hotkeys(hwnd);

            // 置底或悬浮
            SetWindowPos(
                hwnd,
                HWND_BOTTOM,
                mx,
                my,
                width as i32,
                height as i32,
                SWP_SHOWWINDOW | SWP_NOACTIVATE,
            )?;

            Ok(Self {
                hwnd,
                width,
                height,
            })
        }
    }
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCHITTEST => {
                // 获取屏幕坐标
                let (ox, oy) = origin();
                let x = ((lparam.0 & 0xffff) as i16 as i32 - ox) as f32 / scale();
                let y = (((lparam.0 >> 16) & 0xffff) as i16 as i32 - oy) as f32 / scale();

                // 检查是否命中任何卡片或控件
                if let Ok(boxes) = HIT_BOXES.lock() {
                    for b in boxes.iter() {
                        if b.contains(x, y) {
                            return LRESULT(HTCLIENT as isize);
                        }
                    }
                }

                // 未命中任何 UI 元素：直接穿透到底层桌面或窗口！
                LRESULT(HTTRANSPARENT as isize)
            }
            // 鼠标离开 HUD 窗口(移到别的窗口 / 窗口区域之外):清掉悬停状态,否则高亮和提示会一直残留
            0x02A3 => {
                if let Ok(mut hovered) = HOVERED_ID.lock() {
                    *hovered = None;
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                {
                    use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                }
                let x = (lparam.0 & 0xffff) as i16 as f32 / scale();
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32 / scale();

                if let Ok(mut pos) = MOUSE_POS.lock() {
                    *pos = (x, y);
                }

                let mut current_hover = None;
                if let Ok(boxes) = HIT_BOXES.lock() {
                    for b in boxes.iter() {
                        if b.contains(x, y) {
                            current_hover = Some(b.id);
                            break;
                        }
                    }
                }

                if let Ok(mut hovered) = HOVERED_ID.lock() {
                    *hovered = current_hover;
                }
                // 编辑布局:拖动卡片
                if DRAG_CARD.load(Ordering::Relaxed) && (wparam.0 & 0x0001) != 0 {
                    if let Ok(mut queue) = CLICK_QUEUE.lock() {
                        if queue.last() != Some(&"card_drag") {
                            queue.push("card_drag");
                        }
                    }
                }
                // 拖动亮度滑块
                if DRAG_QS.load(Ordering::Relaxed) && (wparam.0 & 0x0001) != 0 {
                    let id = *DRAG_QS_ID.lock().unwrap();
                    if let Ok(mut queue) = CLICK_QUEUE.lock() {
                        if queue.last() != Some(&id) {
                            queue.push(id);
                        }
                    }
                }
                // 拖动播放进度条
                if DRAG_SEEK.load(Ordering::Relaxed) && (wparam.0 & 0x0001) != 0 {
                    if let Ok(mut queue) = CLICK_QUEUE.lock() {
                        if queue.last() != Some(&"m_seek_drag") {
                            queue.push("m_seek_drag");
                        }
                    }
                }
                // 拖动音量条:按住期间持续上报位置
                if DRAG_SLIDER.load(Ordering::Relaxed) && (wparam.0 & 0x0001) != 0 {
                    if let Ok(mut queue) = CLICK_QUEUE.lock() {
                        if queue.last() != Some(&"m_slider") {
                            queue.push("m_slider");
                        }
                    }
                }

                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                if let Some(id) = HOVERED_ID.lock().ok().and_then(|h| *h) {
                    if id.starts_with("card:") {
                        DRAG_CARD.store(true, Ordering::Relaxed);
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
                        if let Ok(mut queue) = CLICK_QUEUE.lock() {
                            queue.push(id);
                        }
                        return LRESULT(0);
                    }
                }
                if let Some(id) = HOVERED_ID.lock().ok().and_then(|h| *h).filter(|i| *i == "tl_b0" || *i == "tl_b1") {
                    DRAG_QS.store(true, Ordering::Relaxed);
                    *DRAG_QS_ID.lock().unwrap() = id;
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
                    if let Ok(mut queue) = CLICK_QUEUE.lock() {
                        queue.push(id);
                    }
                    return LRESULT(0);
                }
                if HOVERED_ID.lock().ok().and_then(|h| *h) == Some("m_seek") {
                    DRAG_SEEK.store(true, Ordering::Relaxed);
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
                    if let Ok(mut queue) = CLICK_QUEUE.lock() {
                        queue.push("m_seek_drag");
                    }
                    return LRESULT(0);
                }
                let on_slider = HOVERED_ID.lock().ok().and_then(|h| *h) == Some("m_slider");
                if on_slider {
                    DRAG_SLIDER.store(true, Ordering::Relaxed);
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
                    if let Ok(mut queue) = CLICK_QUEUE.lock() {
                        queue.push("m_slider");
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                ALT_CLICK.store(wparam.0 & 0x0004 != 0, Ordering::Relaxed);
                if DRAG_CARD.swap(false, Ordering::Relaxed) {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                    if let Ok(mut queue) = CLICK_QUEUE.lock() {
                        queue.push("card_drop");
                    }
                    return LRESULT(0);
                }
                if DRAG_QS.swap(false, Ordering::Relaxed) {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                    if let Ok(mut queue) = CLICK_QUEUE.lock() {
                        queue.push("tl_bcommit");
                    }
                    return LRESULT(0);
                }
                if DRAG_SEEK.swap(false, Ordering::Relaxed) {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                    if let Ok(mut queue) = CLICK_QUEUE.lock() {
                        queue.push("m_seek_commit");
                    }
                    return LRESULT(0);
                }
                if DRAG_SLIDER.swap(false, Ordering::Relaxed) {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                    return LRESULT(0);
                }
                if let Ok(hovered) = HOVERED_ID.lock() {
                    if let Some(id) = *hovered {
                        if let Ok(mut queue) = CLICK_QUEUE.lock() {
                            queue.push(id);
                        }
                    }
                }
                LRESULT(0)
            }
            // 中键:Dock 图标走"另一种"启动方式(同 Shift+单击)
            WM_MBUTTONUP => {
                if let Some(id) = HOVERED_ID.lock().ok().and_then(|h| *h) {
                    if id.starts_with("dk") {
                        ALT_CLICK.store(true, Ordering::Relaxed);
                        if let Ok(mut queue) = CLICK_QUEUE.lock() {
                            queue.push(id);
                        }
                    }
                }
                LRESULT(0)
            }
            // 右键 Dock 图标:弹出菜单
            WM_RBUTTONUP => {
                if let Some(id) = HOVERED_ID.lock().ok().and_then(|h| *h) {
                    if let Some(i) = id.strip_prefix("dk").and_then(|s| s.parse::<usize>().ok()) {
                        let cmd = show_dock_menu(hwnd, i);
                        if cmd != 0 {
                            if let Ok(mut q) = DOCK_ACTIONS.lock() {
                                q.push((i, cmd));
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            // 把 exe / 快捷方式拖到 HUD 上:加入 Dock
            WM_DROPFILES => {
                use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};
                let hdrop = HDROP(wparam.0 as *mut core::ffi::c_void);
                let n = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
                for i in 0..n {
                    let len = DragQueryFileW(hdrop, i, None) as usize;
                    let mut buf = vec![0u16; len + 1];
                    DragQueryFileW(hdrop, i, Some(&mut buf));
                    if let Ok(mut d) = DROPPED.lock() {
                        d.push(String::from_utf16_lossy(&buf[..len]));
                    }
                }
                DragFinish(hdrop);
                LRESULT(0)
            }
            // 剪贴板内容变化(WM_CLIPBOARDUPDATE)
            0x031D => {
                crate::tools::on_clipboard_update(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as i16;
                if let Ok(mut queue) = CLICK_QUEUE.lock() {
                    queue.push(if delta > 0 { "wheel_up" } else { "wheel_down" });
                }
                LRESULT(0)
            }
            WM_HOTKEY => {
                match wparam.0 {
                    1 => toggle_visible(hwnd),
                    2 => TRAY_FLOAT.store(true, Ordering::SeqCst),
                    3 => TRAY_SEARCH.store(true, Ordering::SeqCst),
                    4 => TRAY_TOOLS.store(true, Ordering::SeqCst),
                    _ => {}
                }
                LRESULT(0)
            }
            // 睡眠唤醒:重建渲染目标/覆盖层,避免设备丢失后画面停住
            WM_POWERBROADCAST => {
                if wparam.0 == 0x12 || wparam.0 == 0x07 {
                    // PBT_APMRESUMEAUTOMATIC / PBT_APMRESUMESUSPEND
                    RELAYOUT.store(true, Ordering::SeqCst);
                    crate::log::log("resume from sleep: rebuilding display resources");
                }
                LRESULT(1)
            }
            // 解锁 / 远程桌面重连
            0x02B1 => {
                // WM_WTSSESSION_CHANGE:1=控制台连接 5=登录 8=解锁
                if matches!(wparam.0, 1 | 5 | 8) {
                    RELAYOUT.store(true, Ordering::SeqCst);
                    crate::log::log("session unlock/reconnect: rebuilding display resources");
                }
                LRESULT(0)
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                RELAYOUT.store(true, Ordering::SeqCst);
                LRESULT(0)
            }
            // 点击卡片不激活窗口,否则窗口会被抬到普通窗口上面
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // 任何改变 Z 序的请求都压回最底层(桌面小部件)
            WM_WINDOWPOSCHANGING => {
                let pos = &mut *(lparam.0 as *mut WINDOWPOS);
                if (pos.flags & SWP_NOZORDER).0 == 0 {
                    pos.hwndInsertAfter = if LIFTED.load(Ordering::Relaxed) { HWND_TOPMOST } else { HWND_BOTTOM };
                }
                LRESULT(0)
            }
            WM_TRAY => {
                match (lparam.0 & 0xffff) as u32 {
                    WM_RBUTTONUP | WM_CONTEXTMENU => show_tray_menu(hwnd),
                    WM_LBUTTONDBLCLK => toggle_visible(hwnd),
                    _ => {}
                }
                LRESULT(0)
            }
            m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
                // 资源管理器重启后重新放回托盘图标
                add_tray_icon(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                remove_tray_icon(hwnd);
                SHOULD_EXIT.store(true, Ordering::SeqCst);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_combo;

    #[test]
    fn combos() {
        assert_eq!(parse_combo("Win+Alt+S"), Some((0x8 | 0x1, 'S' as u32)));
        assert_eq!(parse_combo("ctrl + shift + space"), Some((0x2 | 0x4, 0x20)));
        assert_eq!(parse_combo("Ctrl+F12"), Some((0x2, 0x7B)));
        assert_eq!(parse_combo("Alt+1"), Some((0x1, '1' as u32)));
        assert_eq!(parse_combo("F5"), None); // 没有修饰键
        assert_eq!(parse_combo("Win+Alt"), None); // 没有主键
        assert_eq!(parse_combo("Foo+S"), None);
        assert_eq!(parse_combo(""), None);
    }
}
