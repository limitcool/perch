//! 快速启动器(搜索):Win+Alt+S 或托盘菜单打开,输入名称过滤"开始菜单程序 + Dock 应用",↑↓ 选择,回车启动,Esc 关闭。
//! 无边框、跟随主题配色;输入仍用系统 EDIT 控件(中文输入法可用)。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_TAB, VK_UP};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::prompt::Palette;
use crate::theme::ThemeConfig;

const W: i32 = 520;
const ROW_H: i32 = 40;
const MAX_ROWS: usize = 8;
const TOP: i32 = 76;

static OPEN: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(false);

struct Ctx {
    pal: Palette,
    edit: HWND,
    font_body: HFONT,
    font_small: HFONT,
    field_brush: HBRUSH,
    results: Vec<(String, PathBuf)>,
    selected: usize,
    launch: Option<PathBuf>,
    last_q: String,
    /// 回车时按住了 Shift:打开所在文件夹
    reveal: bool,
    /// Tab 切换:0 程序 1 文件 2 窗口 3 问 AI 4 命令
    mode: u8,
    /// 路径 → HICON(0 = 取不到),只为可见行按需读取,关闭时统一销毁
    icons: std::collections::HashMap<String, isize>,
    font_emoji: HFONT,
}

/// 读取文件 / 快捷方式 / 文件夹的系统图标(32px)
fn icon_for(c: &mut Ctx, path: &Path) -> isize {
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    let key = path.to_string_lossy().to_string();
    if let Some(h) = c.icons.get(&key) {
        return *h;
    }
    let mut h = 0isize;
    // 窗口结果 "nx:win:<hwnd>|<exe 路径>" 用 exe 的图标
    let target = if let Some(rest) = key.strip_prefix("nx:win:") { rest.split_once('|').map(|x| x.1.to_string()).filter(|s| !s.is_empty()) } else if key.starts_with("nx:") { None } else { Some(key.clone()) };
    if let Some(target) = target {
        let wide: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        let mut sfi = SHFILEINFOW::default();
        unsafe {
            SHGetFileInfoW(PCWSTR(wide.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut sfi), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_LARGEICON);
        }
        h = sfi.hIcon.0 as isize;
    }
    c.icons.insert(key, h);
    h
}

/// 特殊结果(nx:)用符号代替图标
fn symbol_for(path: &Path) -> &'static str {
    let s = path.to_string_lossy();
    if s.starts_with("nx:calc") {
        "🧮"
    } else if s.starts_with("nx:ai") {
        "🤖"
    } else if s.starts_with("nx:web") {
        "🌐"
    } else if s.starts_with("nx:cmd") {
        "⌨"
    } else if s.starts_with("nx:sys") {
        "⚡"
    } else if s.starts_with("nx:win") {
        "🪟"
    } else {
        "ℹ"
    }
}

/// 模式条的点击区域:第 i 个模式
fn mode_at(x: i32, y: i32) -> Option<u8> {
    if !(48..=74).contains(&y) {
        return None;
    }
    (0..5).find(|i| x >= 24 + 64 * i && x < 24 + 64 * i + 62).map(|i| i as u8)
}

static CTX: Mutex<Option<usize>> = Mutex::new(None);

fn ctx() -> Option<&'static mut Ctx> {
    CTX.lock().ok().and_then(|g| *g).map(|p| unsafe { &mut *(p as *mut Ctx) })
}

// ---------------- 索引 ----------------
struct Index {
    built: Instant,
    items: Vec<(String, PathBuf)>,
}
static INDEX: Mutex<Option<Index>> = Mutex::new(None);

fn scan(dir: &Path, depth: u32, out: &mut Vec<(String, PathBuf)>) {
    if depth > 4 {
        return;
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                scan(&p, depth + 1, out);
            } else if let Some(ext) = p.extension().and_then(|x| x.to_str()) {
                if matches!(ext.to_ascii_lowercase().as_str(), "lnk" | "exe" | "url") {
                    if let Some(name) = p.file_stem().and_then(|s| s.to_str()) {
                        let low = name.to_lowercase();
                        // 去掉卸载 / 帮助之类的噪音
                        if !(low.contains("uninstall") || low.contains("卸载") || low.starts_with("readme")) {
                            out.push((name.to_string(), p.clone()));
                        }
                    }
                }
            }
        }
    }
}

fn build_index() -> Vec<(String, PathBuf)> {
    let mut items = Vec::new();
    for var in ["ProgramData", "APPDATA"] {
        if let Ok(base) = std::env::var(var) {
            scan(&Path::new(&base).join(r"Microsoft\Windows\Start Menu\Programs"), 0, &mut items);
        }
    }
    for d in crate::config::get().dock {
        items.push((d.name.clone(), PathBuf::from(crate::config::resolve_target(&d.target))));
    }
    items.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    items.dedup_by(|a, b| a.0.to_lowercase() == b.0.to_lowercase());
    items
}

fn with_index<R>(f: impl FnOnce(&[(String, PathBuf)]) -> R) -> R {
    let mut g = INDEX.lock().unwrap();
    let index_sec = crate::config::get().intervals.app_index_sec.max(10);
    let stale = g.as_ref().map(|i| i.built.elapsed() > Duration::from_secs(index_sec)).unwrap_or(true);
    if stale {
        *g = Some(Index { built: Instant::now(), items: build_index() });
    }
    f(&g.as_ref().unwrap().items)
}

/// 简单打分:前缀 > 子串 > 子序列(按拼音首字母不做,保持可预测)
fn score(name: &str, q: &str) -> Option<i32> {
    let n = name.to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    if n.starts_with(q) {
        return Some(300 - n.len() as i32);
    }
    if let Some(pos) = n.find(q) {
        return Some(200 - pos as i32 - n.len() as i32);
    }
    let mut it = n.chars();
    for qc in q.chars() {
        it.find(|c| *c == qc)?;
    }
    Some(50 - n.len() as i32)
}

fn search(q: &str, mode: u8) -> Vec<(String, PathBuf)> {
    let q = &crate::launcher_ext::normalize(q);
    if let Some(rows) = crate::launcher_ext::in_mode(q, mode) {
        return rows;
    }
    let raw = q.trim().to_string();
    let q = raw.to_lowercase();
    let freq = crate::launcher_ext::freq_load();
    let mut found: Vec<(String, PathBuf)> = with_index(|items| {
        let mut hit: Vec<(i32, &(String, PathBuf))> = items.iter().filter_map(|i| score(&i.0, &q).map(|s| (s + crate::launcher_ext::freq_bonus(&freq, &i.1.to_string_lossy()), i))).collect();
        hit.sort_by(|a, b| b.0.cmp(&a.0));
        hit.into_iter().take(MAX_ROWS).map(|(_, i)| i.clone()).collect()
    });
    // 系统命令(关机 / 锁屏 / 睡眠 …)
    for r in crate::launcher_ext::commands(&raw) {
        if found.len() < MAX_ROWS {
            found.push(r);
        }
    }
    // 没有匹配的程序:交给 AI(上次用的排第一)或传统网页搜索
    if found.is_empty() && !raw.is_empty() {
        found.extend(crate::launcher_ext::fallback(&raw));
    }
    found
}

// ---------------- 窗口 ----------------
fn height(rows: usize) -> i32 {
    TOP + (rows.max(1) as i32) * ROW_H + 14
}

fn draw_round(hdc: HDC, r: RECT, rad: i32, fill: COLORREF, stroke: COLORREF, width: i32) {
    unsafe {
        let brush = CreateSolidBrush(fill);
        let pen = CreatePen(PS_SOLID, width, stroke);
        let (ob, op) = (SelectObject(hdc, brush), SelectObject(hdc, pen));
        let _ = RoundRect(hdc, r.left, r.top, r.right, r.bottom, rad, rad);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        let _ = DeleteObject(brush);
        let _ = DeleteObject(pen);
    }
}

fn text(hdc: HDC, font: HFONT, color: COLORREF, x: i32, y: i32, s: &str) {
    unsafe {
        let old = SelectObject(hdc, font);
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, color);
        let w: Vec<u16> = s.encode_utf16().collect();
        let _ = TextOutW(hdc, x, y, &w);
        SelectObject(hdc, old);
    }
}

/// 编辑框已提交文字 + 输入法正在组合的文字(拼音输入法下字母先处于"组合"状态,不拼进这里就搜不到)
fn current_query(edit: HWND) -> String {
    use windows::Win32::UI::Input::Ime::{ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext, GCS_COMPSTR};
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(edit, &mut buf) as usize;
        let mut q = String::from_utf16_lossy(&buf[..n]);
        let himc = ImmGetContext(edit);
        if !himc.is_invalid() {
            let bytes = ImmGetCompositionStringW(himc, GCS_COMPSTR, None, 0);
            if bytes > 0 {
                let mut comp = vec![0u16; bytes as usize / 2];
                ImmGetCompositionStringW(himc, GCS_COMPSTR, Some(comp.as_mut_ptr() as *mut _), bytes as u32);
                q.push_str(&String::from_utf16_lossy(&comp));
            }
            let _ = ImmReleaseContext(edit, himc);
        }
        q
    }
}

fn refresh(hwnd: HWND) {
    if let Some(c) = ctx() {
        let q = current_query(c.edit);
        c.last_q = q.clone();
        // 拼音输入法在组合过程中会插入分隔符(如 ch'r'o'm'e),搜索时去掉
        c.results = search(&q.replace('\'', ""), c.mode);
        c.selected = 0;
        let h = height(c.results.len());
        unsafe {
            let _ = SetWindowPos(hwnd, None, 0, 0, W, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            let rgn = CreateRoundRectRgn(0, 0, W + 1, h + 1, 18, 18);
            SetWindowRgn(hwnd, rgn, true);
            let _ = InvalidateRect(hwnd, None, true);
        }
    }
}

fn launch_selected(hwnd: HWND) {
    if let Some(c) = ctx() {
        if let Some((_, path)) = c.results.get(c.selected) {
            c.launch = Some(path.clone());
        }
        c.reveal = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) } < 0;
    }
    close(hwnd);
}

fn close(hwnd: HWND) {
    DONE.store(true, Ordering::SeqCst);
    unsafe {
        let _ = DestroyWindow(hwnd);
        let _ = PostMessageW(None, WM_NULL, WPARAM(0), LPARAM(0));
    }
}

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                if let Some(c) = ctx() {
                    let h = height(c.results.len());
                    draw_round(hdc, RECT { left: 0, top: 0, right: W, bottom: h }, 18, c.pal.bg, c.pal.accent, 2);
                    draw_round(hdc, RECT { left: 18, top: 14, right: W - 18, bottom: 48 }, 10, c.pal.field, c.pal.stroke, 1);
                    // 模式条:当前模式高亮,Tab 切换(与输入法无关)
                    for (i, name) in crate::launcher_ext::mode_labels().iter().enumerate() {
                        let on = i == c.mode as usize % 5;
                        let x = 24 + 64 * i as i32;
                        if on {
                            draw_round(hdc, RECT { left: x, top: 50, right: x + 60, bottom: 72 }, 10, c.pal.stroke, c.pal.accent, 1);
                        }
                        text(hdc, c.font_small, if on { c.pal.text } else { c.pal.muted }, x + 10, 54, name);
                    }
                    text(hdc, c.font_small, c.pal.muted, W - 118, 54, &perch_data::t("launcher2.hint_keys"));
                    if c.results.is_empty() {
                        text(hdc, c.font_body, c.pal.muted, 28, TOP + 8, &perch_data::t("launcher2.no_matching_apps"));
                    }
                    let rows = c.results.clone();
                    for (i, (name, path)) in rows.iter().enumerate() {
                        let y = TOP + i as i32 * ROW_H;
                        if i == c.selected {
                            draw_round(hdc, RECT { left: 14, top: y, right: W - 14, bottom: y + ROW_H - 4 }, 10, c.pal.stroke, c.pal.accent, 1);
                        }
                        // 图标(系统图标;特殊结果用符号)
                        let ic = icon_for(c, path);
                        if ic != 0 {
                            let _ = DrawIconEx(hdc, 26, y + 5, HICON(ic as *mut core::ffi::c_void), 26, 26, 0, None, DI_NORMAL);
                        } else {
                            text(hdc, c.font_emoji, c.pal.text, 28, y + 6, symbol_for(path));
                        }
                        // 标题太长会盖住右边的路径 / 进程名
                        let shown: String = if name.chars().count() > 26 { format!("{}…", name.chars().take(25).collect::<String>()) } else { name.clone() };
                        text(hdc, c.font_body, c.pal.text, 64, y + 8, &shown);
                        let ps = path.to_string_lossy().to_string();
                        let dir = if let Some(rest) = ps.strip_prefix("nx:win:") { rest.split_once('|').map(|x| std::path::Path::new(x.1).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).unwrap_or_default() } else if ps.starts_with("nx:") { String::new() } else { path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default() };
                        let short: String = if dir.chars().count() > 38 { format!("…{}", dir.chars().rev().take(37).collect::<Vec<_>>().into_iter().rev().collect::<String>()) } else { dir };
                        text(hdc, c.font_small, c.pal.muted, W - 28 - 230, y + 11, &short);
                    }
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => {
                if let Some(c) = ctx() {
                    let hdc = HDC(wparam.0 as *mut _);
                    SetTextColor(hdc, c.pal.text);
                    SetBkColor(hdc, c.pal.field);
                    return LRESULT(c.field_brush.0 as isize);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_COMMAND => {
                // EN_CHANGE:输入变化 → 重新搜索
                if ((wparam.0 >> 16) & 0xffff) as u32 == 0x0300 {
                    refresh(hwnd);
                }
                LRESULT(0)
            }
            WM_TIMER => {
                // 输入法组合文字不会触发 EN_CHANGE,定时轮询
                if let Some(c) = ctx() {
                    if current_query(c.edit) != c.last_q {
                        refresh(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
                if y >= TOP {
                    let row = ((y - TOP) / ROW_H) as usize;
                    if let Some(c) = ctx() {
                        if row < c.results.len() && c.selected != row {
                            c.selected = row;
                            let _ = InvalidateRect(hwnd, None, true);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as i16;
                if let Some(c) = ctx() {
                    let n = c.results.len().max(1);
                    c.selected = if delta < 0 { (c.selected + 1) % n } else { (c.selected + n - 1) % n };
                    let _ = InvalidateRect(hwnd, None, true);
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let x = (lparam.0 & 0xffff) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
                if let Some(m) = mode_at(x, y) {
                    if let Some(c) = ctx() {
                        c.mode = m;
                        let edit = c.edit;
                        refresh(hwnd);
                        let _ = SetFocus(edit);
                    }
                    return LRESULT(0);
                }
                if y >= TOP {
                    let row = ((y - TOP) / ROW_H) as usize;
                    if let Some(c) = ctx() {
                        if row < c.results.len() {
                            c.selected = row;
                            launch_selected(hwnd);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_ACTIVATE => {
                // 失去焦点就关闭
                if (wparam.0 & 0xffff) == 0 {
                    close(hwnd);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                close(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn font(height: i32, weight: i32) -> HFONT {
    unsafe { CreateFontW(height, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, 0, w!("Microsoft YaHei UI")) }
}

pub fn is_open() -> bool {
    OPEN.load(Ordering::SeqCst)
}

/// 打开搜索框并运行自己的消息循环,直到关闭;选中项会被启动
pub fn run(th: &ThemeConfig) {
    if OPEN.swap(true, Ordering::SeqCst) {
        return;
    }
    unsafe {
        let old_ctx = windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_UNAWARE);
        run_inner(th);
        windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(old_ctx);
    }
    OPEN.store(false, Ordering::SeqCst);
}

fn run_inner(th: &ThemeConfig) {
    unsafe {
        let Ok(inst) = GetModuleHandleW(None) else { return };
        let class = w!("PerchSearchClass");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: inst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let pal = Palette::from_theme(th);
        let context = Box::new(Ctx {
            pal,
            edit: HWND::default(),
            font_body: font(-17, 500),
            font_small: font(-13, 400),
            field_brush: CreateSolidBrush(pal.field),
            results: Vec::new(),
            selected: 0,
            launch: None,
            last_q: String::new(),
            reveal: false,
            mode: 0,
            icons: std::collections::HashMap::new(),
            font_emoji: CreateFontW(-18, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, 0, w!("Segoe UI Emoji")),
        });
        let ptr = Box::into_raw(context);
        *CTX.lock().unwrap() = Some(ptr as usize);

        let (sx, sy) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let Ok(hwnd) = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class, w!("Perch Search"), WS_POPUP, (sx - W) / 2, sy / 4, W, height(MAX_ROWS), None, None, inst, None) else {
            *CTX.lock().unwrap() = None;
            drop(Box::from_raw(ptr));
            return;
        };
        let edit = CreateWindowExW(WINDOW_EX_STYLE(0), w!("EDIT"), w!(""), WS_CHILD | WS_VISIBLE | WINDOW_STYLE(0x80), 30, 21, W - 100, 22, hwnd, HMENU(100 as *mut _), inst, None).unwrap_or_default();
        SendMessageW(edit, WM_SETFONT, WPARAM((*ptr).font_body.0 as usize), LPARAM(1));
        (*ptr).edit = edit;
        refresh(hwnd);

        DONE.store(false, Ordering::SeqCst);
        SetTimer(hwnd, 1, 120, None);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(edit);

        let mut msg = MSG::default();
        while !DONE.load(Ordering::SeqCst) {
            if GetMessageW(&mut msg, None, 0, 0).0 <= 0 {
                break;
            }
            if msg.message == WM_KEYDOWN {
                let vk = msg.wParam.0 as u16;
                if vk == VK_ESCAPE.0 {
                    close(hwnd);
                    continue;
                } else if vk == VK_RETURN.0 {
                    launch_selected(hwnd);
                    continue;
                } else if vk == 0x41 && windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11) < 0 {
                    // Ctrl+A:单行 EDIT 默认不支持全选
                    if let Some(c) = ctx() {
                        SendMessageW(c.edit, 0x00B1, WPARAM(0), LPARAM(-1));
                    }
                    continue;
                } else if vk == VK_TAB.0 {
                    if let Some(c) = ctx() {
                        c.mode = (c.mode + 1) % 5;
                    }
                    refresh(hwnd);
                    continue;
                } else if vk == VK_DOWN.0 || vk == VK_UP.0 {
                    if let Some(c) = ctx() {
                        let n = c.results.len().max(1);
                        c.selected = if vk == VK_DOWN.0 { (c.selected + 1) % n } else { (c.selected + n - 1) % n };
                        let _ = InvalidateRect(hwnd, None, true);
                    }
                    continue;
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let launch = ctx().and_then(|c| c.launch.take());
        let reveal = ctx().map_or(false, |c| c.reveal);
        *CTX.lock().unwrap() = None;
        let c = Box::from_raw(ptr);
        for h in c.icons.values() {
            if *h != 0 {
                let _ = DestroyIcon(HICON(*h as *mut core::ffi::c_void));
            }
        }
        let _ = DeleteObject(c.font_emoji);
        let _ = DeleteObject(c.font_body);
        let _ = DeleteObject(c.font_small);
        let _ = DeleteObject(c.field_brush);
        drop(c);
        if let Some(path) = launch {
            let ps = path.to_string_lossy().to_string();
            if crate::launcher_ext::run_special(&ps) {
                return;
            }
            // 记录使用频率(下次排得更靠前)
            crate::launcher_ext::freq_bump(&ps);
            if reveal {
                crate::launcher_ext::reveal(&ps);
                return;
            }
            let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain(Some(0)).collect();
            ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
        }
    }
}
