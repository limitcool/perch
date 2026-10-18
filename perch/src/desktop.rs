//! 隐藏/恢复桌面图标(等同右键"查看 → 显示桌面图标";HUD 退出时恢复)

use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::WindowsAndMessaging::*;

/// 是否由我们隐藏的(退出时才恢复,不动用户原本就隐藏的状态)
static WE_HID: AtomicBool = AtomicBool::new(false);

fn find_defview() -> Option<HWND> {
    unsafe {
        if let Ok(progman) = FindWindowW(w!("Progman"), PCWSTR::null()) {
            if let Ok(v) = FindWindowExW(progman, None, w!("SHELLDLL_DefView"), PCWSTR::null()) {
                return Some(v);
            }
        }
        // 开启壁纸插件/幻灯片时 DefView 会被放到某个 WorkerW 下
        static mut FOUND: Option<HWND> = None;
        extern "system" fn cb(hwnd: HWND, _: LPARAM) -> BOOL {
            unsafe {
                if let Ok(v) = FindWindowExW(hwnd, None, w!("SHELLDLL_DefView"), PCWSTR::null()) {
                    FOUND = Some(v);
                    return BOOL(0);
                }
            }
            BOOL(1)
        }
        FOUND = None;
        let _ = EnumWindows(Some(cb), LPARAM(0));
        FOUND
    }
}

fn icons_hidden() -> bool {
    unsafe {
        let mut data = 0u32;
        let mut size = 4u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"),
            w!("HideIcons"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        );
        r.is_ok() && data == 1
    }
}

fn toggle() {
    if let Some(v) = find_defview() {
        unsafe {
            // 0x7402 = "显示桌面图标" 开关
            let _ = SendMessageW(v, WM_COMMAND, WPARAM(0x7402), LPARAM(0));
        }
    }
}

/// 标记文件:记录"图标是我们隐藏的"。HUD 被强杀/崩溃后下次启动能认出来,退出时仍会恢复。
fn flag_path() -> std::path::PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("perch").join("hid_icons.flag")
}

pub fn hide() {
    let flag = flag_path();
    if icons_hidden() {
        // 上次是我们隐藏的但没来得及恢复(崩溃/强杀)→ 继续认领,退出时恢复
        if flag.exists() {
            WE_HID.store(true, Ordering::SeqCst);
        }
    } else {
        // 标记文件已存在但图标是可见的:用户手动恢复过,清掉旧标记
        toggle();
        WE_HID.store(true, Ordering::SeqCst);
        if let Some(dir) = flag.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&flag, b"1");
    }
}

pub fn restore() {
    if WE_HID.swap(false, Ordering::SeqCst) && icons_hidden() {
        toggle();
    }
    let _ = std::fs::remove_file(flag_path());
}

pub fn currently_hidden() -> bool {
    icons_hidden()
}

pub fn toggle_user() {
    if icons_hidden() {
        toggle();
        WE_HID.store(false, Ordering::SeqCst);
        let _ = std::fs::remove_file(flag_path());
    } else {
        toggle();
        WE_HID.store(true, Ordering::SeqCst);
        if let Some(dir) = flag_path().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(flag_path(), b"1");
    }
}

/// 离开作用域(正常退出/panic 展开)时恢复图标
pub struct RestoreOnDrop;
impl Drop for RestoreOnDrop {
    fn drop(&mut self) {
        restore();
    }
}


/// (桌面是否被展示, 桌面是否被整屏窗口盖住)
/// - 展示:没有任何普通窗口(Win+D / 全部最小化),此时 HUD 要浮到被抬起的桌面之上;
/// - 被盖住:存在覆盖整个屏幕工作区的窗口(最大化 / 全屏),卡片根本看不见,可以完全停止绘制和采样。
pub fn desktop_state() -> (bool, bool) {
    static REAL: AtomicBool = AtomicBool::new(false);
    static COVER: AtomicBool = AtomicBool::new(false);
    extern "system" fn cb(hwnd: HWND, _: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                return BOOL(1);
            }
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            // 工具窗口 / 不激活窗口 / 点击穿透窗口(覆盖层、本程序自己的窗口)都不算"普通窗口"
            if ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_TRANSPARENT.0) != 0 {
                return BOOL(1);
            }
            if GetWindowTextLengthW(hwnd) == 0 {
                return BOOL(1);
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
            if cloaked != 0 {
                return BOOL(1);
            }
            let mut cls = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut cls) as usize;
            let name = String::from_utf16_lossy(&cls[..n]);
            if matches!(name.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "NotifyIconOverflowWindow" | "TopLevelWindowForOverflowXamlIsland") {
                return BOOL(1);
            }
            let mut r = RECT::default();
            if GetWindowRect(hwnd, &mut r).is_err() || r.right - r.left < 50 || r.bottom - r.top < 50 {
                return BOOL(1);
            }
            let (ox, oy) = crate::window::origin();
            let (mw, mh) = crate::window::monitor_size();
            // 覆盖整个工作区(任务栏高度以内的差距不算)
            let covers = r.left <= ox + 2 && r.top <= oy + 2 && r.right >= ox + mw - 2 && r.bottom >= oy + mh - 64;
            if covers {
                COVER.store(true, Ordering::SeqCst);
            }
            // 没有最小化按钮的窗口(属性对话框等)Win+D 本来就收不起来,不该让 HUD 认为"桌面还没显示";
            // 但覆盖整个屏幕的(无边框全屏游戏 / 视频)仍然算
            let has_minbox = GetWindowLongW(hwnd, GWL_STYLE) as u32 & 0x0002_0000 != 0;
            if covers || has_minbox {
                REAL.store(true, Ordering::SeqCst);
            }
            BOOL(1)
        }
    }
    REAL.store(false, Ordering::SeqCst);
    COVER.store(false, Ordering::SeqCst);
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(0));
        // Win+D / 点击桌面之后,系统把"桌面"本身(Progman / WorkerW)设为前台窗口。
        // 有的窗口(如某些 Electron 应用)不响应 Win+D 的最小化,单看"还有没有窗口"会误判,这个信号更直接
        let fg = GetForegroundWindow();
        if !fg.0.is_null() {
            let mut cls = [0u16; 32];
            let n = GetClassNameW(fg, &mut cls) as usize;
            if matches!(String::from_utf16_lossy(&cls[..n]).as_str(), "Progman" | "WorkerW") {
                return (true, false);
            }
        }
    }
    (!REAL.load(Ordering::SeqCst), COVER.load(Ordering::SeqCst))
}

pub fn desktop_is_shown() -> bool {
    desktop_state().0
}

/// 启用 / 停用"Perch"计划任务(看门狗)。没有这个任务时静默忽略。
pub fn set_watchdog(enabled: bool) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("schtasks")
        .args(["/Change", "/TN", "Perch", if enabled { "/ENABLE" } else { "/DISABLE" }])
        .creation_flags(0x0800_0000)
        .spawn();
}
