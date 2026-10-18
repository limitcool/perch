//! 进程运行状态:给 Dock 图标显示"是否在运行",并支持"切到已有窗口 / 新开实例"。
//! 事件驱动:SetWinEventHook 订阅窗口变化(回调只置标志),变化后才取一次快照(进程名 + pid + 可见顶层窗口,按 exe 文件名归并)。
//! 静默模式下主循环不调用 tick(),事件标志留着,回到桌面时补刷一次。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::{self, DockEntry};

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Entry {
    /// 同名进程个数
    pub procs: usize,
    /// 该应用的可见顶层窗口(按 Z 序,最上面的在前)
    pub windows: Vec<isize>,
    /// 前台窗口属于这个应用
    pub fg: bool,
}

static SNAP: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);
static TIMING: Mutex<Option<(Instant, Option<Instant>)>> = Mutex::new(None); // (上次刷新, 提前刷新的截止时间)
/// 窗口事件回调只置这个标志,真正的取快照在主循环里合并后做
static DIRTY: AtomicBool = AtomicBool::new(true);

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
    // 只关心顶层窗口本身(OBJID_WINDOW / CHILDID_SELF),子控件事件直接忽略
    if event != EVENT_SYSTEM_FOREGROUND && (id_object != 0 || id_child != 0) {
        return;
    }
    if hwnd.0.is_null() {
        return;
    }
    if event != EVENT_SYSTEM_FOREGROUND && GetParent(hwnd).map_or(false, |p| !p.0.is_null()) {
        return;
    }
    DIRTY.store(true, Ordering::Relaxed);
}

/// 订阅窗口事件(前台切换 / 窗口销毁、显示、隐藏)。回调经由主线程的消息循环投递,无需额外线程,也不轮询
pub fn install() {
    unsafe {
        let _ = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
        let _ = SetWinEventHook(EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
    }
}

/// 点击之后过一会儿再刷新一次,让状态条尽快反映新启动/关闭的应用(没有窗口的进程没有事件可订阅)
pub fn refresh_soon() {
    if let Ok(mut t) = TIMING.lock() {
        let last = t.map(|x| x.0).unwrap_or_else(Instant::now);
        *t = Some((last, Some(Instant::now() + Duration::from_millis(1200))));
    }
}

/// 主循环调用:有窗口事件(合并 300ms)、点击后的补刷、或 30 秒兜底(覆盖没有窗口的后台进程)才真正取快照;状态有变化返回 true
pub fn tick() -> bool {
    let due = {
        let mut t = TIMING.lock().unwrap();
        let now = Instant::now();
        let (last, soon) = t.unwrap_or((now.checked_sub(Duration::from_secs(60)).unwrap_or(now), None));
        let since = now.duration_since(last);
        let due = (DIRTY.load(Ordering::Relaxed) && since >= Duration::from_millis(300)) || since >= Duration::from_secs(30) || soon.map_or(false, |s| now >= s);
        if due {
            *t = Some((now, None));
            DIRTY.store(false, Ordering::Relaxed);
        } else {
            *t = Some((last, soon));
        }
        due
    };
    if !due {
        return false;
    }
    let new = snapshot();
    let mut g = SNAP.lock().unwrap();
    let changed = g.as_ref() != Some(&new);
    *g = Some(new);
    changed
}

struct Ctx<'a> {
    names: &'a HashMap<u32, String>,
    map: &'a mut HashMap<String, Entry>,
    fg_root: HWND,
    me: u32,
}

unsafe extern "system" fn on_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut Ctx);
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    if let Ok(owner) = GetWindow(hwnd, GW_OWNER) {
        if !owner.0.is_null() {
            return BOOL(1);
        }
    }
    if GetWindowTextLengthW(hwnd) == 0 {
        return BOOL(1);
    }
    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
        return BOOL(1);
    }
    let mut cloaked: u32 = 0;
    if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4).is_ok() && cloaked != 0 {
        return BOOL(1);
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == ctx.me {
        return BOOL(1);
    }
    if let Some(name) = ctx.names.get(&pid) {
        let e = ctx.map.entry(name.clone()).or_default();
        e.windows.push(hwnd.0 as isize);
        if hwnd == ctx.fg_root {
            e.fg = true;
        }
    }
    BOOL(1)
}

fn snapshot() -> HashMap<String, Entry> {
    let mut names: HashMap<u32, String> = HashMap::new();
    let mut map: HashMap<String, Entry> = HashMap::new();
    unsafe {
        if let Ok(h) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut pe = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(h, &mut pe).is_ok();
            while ok {
                let len = pe.szExeFile.iter().position(|c| *c == 0).unwrap_or(pe.szExeFile.len());
                let name = String::from_utf16_lossy(&pe.szExeFile[..len]).to_lowercase();
                names.insert(pe.th32ProcessID, name.clone());
                map.entry(name).or_default().procs += 1;
                ok = Process32NextW(h, &mut pe).is_ok();
            }
            let _ = CloseHandle(h);
        }
        let fg = GetForegroundWindow();
        let fg_root = if fg.0.is_null() { fg } else { GetAncestor(fg, GA_ROOT) };
        let mut ctx = Ctx { names: &names, map: &mut map, fg_root, me: GetCurrentProcessId() };
        let _ = EnumWindows(Some(on_window), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    map
}

/// Dock 条目对应的进程名(小写)。target 不是 exe(文件夹 / 网址 / 快捷方式)且没写 match 时没有运行状态
pub fn key_for(e: &DockEntry) -> Option<String> {
    let raw = if !e.match_.trim().is_empty() { e.match_.trim().to_string() } else { config::resolve_target(&e.target) };
    let name = std::path::Path::new(&raw).file_name()?.to_string_lossy().to_lowercase();
    if !name.ends_with(".exe") {
        return None;
    }
    Some(match name.as_str() {
        "wt.exe" => "windowsterminal.exe".to_string(),
        _ => name,
    })
}

/// 单击的默认动作是否为"新开":资源管理器 / 终端 / 命令行 / 文件夹和网址默认多开,其余默认单例
pub fn default_multi(e: &DockEntry) -> bool {
    match key_for(e) {
        None => true,
        Some(k) => {
            let k_lower = k.to_lowercase();
            let cfg = crate::config::get();
            cfg.dock_settings.multi_apps.iter().any(|app| app.to_lowercase() == k_lower)
        }
    }
}

pub fn entry(key: &str) -> Option<Entry> {
    SNAP.lock().ok()?.as_ref()?.get(key).cloned()
}

/// 在运行(有进程)
pub fn is_running(e: &DockEntry) -> bool {
    key_for(e).and_then(|k| entry(&k)).map_or(false, |x| x.procs > 0)
}

/// 把应用的窗口切到前台:已经在前台且有多个窗口时,轮到最底下的那个
pub fn focus(e: &Entry) -> bool {
    if e.windows.is_empty() {
        return false;
    }
    let target = if e.fg && e.windows.len() > 1 { *e.windows.last().unwrap() } else { e.windows[0] };
    bring_to_front(HWND(target as *mut core::ffi::c_void));
    true
}

/// 把指定窗口切到前台(Dock 窗口预览用)
pub fn focus_window(h: isize) {
    bring_to_front(HWND(h as *mut core::ffi::c_void));
}

fn bring_to_front(h: HWND) {
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        // 前台限制:把自己的输入线程临时挂到当前前台线程上
        let fg = GetForegroundWindow();
        let fg_tid = if fg.0.is_null() { 0 } else { GetWindowThreadProcessId(fg, None) };
        let me = GetCurrentThreadId();
        if fg_tid != 0 && fg_tid != me {
            let _ = AttachThreadInput(me, fg_tid, true);
            let _ = SetForegroundWindow(h);
            let _ = BringWindowToTop(h);
            let _ = AttachThreadInput(me, fg_tid, false);
        } else {
            let _ = SetForegroundWindow(h);
        }
    }
}

/// 礼貌地关闭:给所有窗口发 WM_CLOSE(应用可以弹"是否保存")
pub fn close(e: &Entry) {
    for w in &e.windows {
        unsafe {
            let _ = PostMessageW(HWND(*w as *mut core::ffi::c_void), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}
