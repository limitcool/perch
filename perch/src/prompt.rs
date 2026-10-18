//! 主题风格的原生输入框(新增/编辑待办、新增日程)。
//! 无边框圆角窗口,按当前主题配色自绘背景/按钮;输入仍用系统 EDIT 控件,保证中文输入法可用。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_UNAWARE};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::theme::{ColorRgba, ThemeConfig};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: COLORREF,
    pub field: COLORREF,
    pub accent: COLORREF,
    pub stroke: COLORREF,
    pub text: COLORREF,
    pub muted: COLORREF,
    pub on_accent: COLORREF,
}

fn cr(c: &ColorRgba) -> COLORREF {
    COLORREF((c.r * 255.0) as u32 | (((c.g * 255.0) as u32) << 8) | (((c.b * 255.0) as u32) << 16))
}

fn lighten(c: &ColorRgba, amt: f32) -> COLORREF {
    let f = |v: f32| ((v + amt).clamp(0.0, 1.0) * 255.0) as u32;
    COLORREF(f(c.r) | (f(c.g) << 8) | (f(c.b) << 16))
}

impl Palette {
    pub fn from_theme(th: &ThemeConfig) -> Self {
        Self {
            bg: lighten(&th.bg, 0.02),
            field: lighten(&th.bg, 0.07),
            accent: cr(&th.accent),
            stroke: cr(&th.accent.with_alpha_color(150)),
            text: cr(&th.white),
            muted: cr(&th.muted),
            on_accent: cr(&th.on_accent),
        }
    }
}

struct Ctx {
    pal: Palette,
    title: String,
    label: String,
    font_title: HFONT,
    font_body: HFONT,
    bg_brush: HBRUSH,
    field_brush: HBRUSH,
    edit: HWND,
}

static DONE: AtomicBool = AtomicBool::new(false);
static RESULT: Mutex<Option<String>> = Mutex::new(None);
static CTX: Mutex<Option<usize>> = Mutex::new(None);

const ID_OK: isize = 1;
const ID_CANCEL: isize = 2;
const W: i32 = 460;
const H: i32 = 196;

fn ctx() -> Option<&'static Ctx> {
    CTX.lock().ok().and_then(|g| *g).map(|p| unsafe { &*(p as *const Ctx) })
}

fn finish(hwnd: HWND, ok: bool) {
    unsafe {
        if ok {
            if let Some(c) = ctx() {
                let mut buf = [0u16; 512];
                let n = GetWindowTextW(c.edit, &mut buf) as usize;
                *RESULT.lock().unwrap() = Some(String::from_utf16_lossy(&buf[..n]).trim().to_string());
            }
        }
        DONE.store(true, Ordering::SeqCst);
        let _ = DestroyWindow(hwnd);
        let _ = PostMessageW(None, WM_NULL, WPARAM(0), LPARAM(0));
    }
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

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                if let Some(c) = ctx() {
                    // 背景 + 强调色边框
                    draw_round(hdc, RECT { left: 0, top: 0, right: W, bottom: H }, 18, c.pal.bg, c.pal.accent, 2);
                    // 标题前的小竖条
                    let bar = CreateSolidBrush(c.pal.accent);
                    FillRect(hdc, &RECT { left: 22, top: 20, right: 25, bottom: 38 }, bar);
                    let _ = DeleteObject(bar);
                    text(hdc, c.font_title, c.pal.accent, 34, 18, &c.title);
                    text(hdc, c.font_body, c.pal.muted, 24, 56, &c.label);
                    // 输入框底(圆角,描边)
                    draw_round(hdc, RECT { left: 22, top: 82, right: W - 22, bottom: 118 }, 10, c.pal.field, c.pal.stroke, 1);
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
            WM_DRAWITEM => {
                let di = &*(lparam.0 as *const DRAWITEMSTRUCT);
                if let Some(c) = ctx() {
                    let ok = di.CtlID as isize == ID_OK;
                    let pressed = di.itemState.0 & 0x0001 != 0; // ODS_SELECTED
                    let (fill, stroke, tc) = if ok {
                        (c.pal.accent, c.pal.accent, c.pal.on_accent)
                    } else {
                        (c.pal.field, c.pal.stroke, c.pal.text)
                    };
                    let fill = if pressed { c.pal.stroke } else { fill };
                    FillRect(di.hDC, &di.rcItem, c.bg_brush);
                    draw_round(di.hDC, di.rcItem, 14, fill, stroke, 1);
                    let label = perch_data::t(if ok { "settings.confirm" } else { "settings.cancel" });
                    let w: Vec<u16> = label.encode_utf16().collect();
                    let old = SelectObject(di.hDC, c.font_body);
                    SetBkMode(di.hDC, TRANSPARENT);
                    SetTextColor(di.hDC, tc);
                    let mut r = di.rcItem;
                    DrawTextW(di.hDC, &mut w.clone(), &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                    SelectObject(di.hDC, old);
                }
                LRESULT(1)
            }
            // 标题区域可拖动
            WM_NCHITTEST => {
                let r = DefWindowProcW(hwnd, msg, wparam, lparam);
                if r.0 == HTCLIENT as isize {
                    let mut pt = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
                    let _ = ScreenToClient(hwnd, &mut pt);
                    if pt.y < 50 {
                        return LRESULT(HTCAPTION as isize);
                    }
                }
                r
            }
            WM_COMMAND => {
                let id = (wparam.0 & 0xffff) as isize;
                if id == ID_OK {
                    finish(hwnd, true);
                } else if id == ID_CANCEL {
                    finish(hwnd, false);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                finish(hwnd, false);
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

/// 弹出输入框;确定返回 Some(文本),取消返回 None
pub fn ask(title: &str, label: &str, initial: &str, th: &ThemeConfig) -> Option<String> {
    unsafe {
        // 对话框按 100% 像素设计:本线程临时切成"不感知 DPI",由系统整体放大
        let old_ctx = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE);
        let out = ask_inner(title, label, initial, th);
        SetThreadDpiAwarenessContext(old_ctx);
        out
    }
}

fn ask_inner(title: &str, label: &str, initial: &str, th: &ThemeConfig) -> Option<String> {
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        let class = w!("PerchPromptClass2");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: inst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: LoadIconW(inst, PCWSTR(1usize as *const u16)).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let pal = Palette::from_theme(th);
        let context = Box::new(Ctx {
            pal,
            title: title.to_string(),
            label: label.to_string(),
            font_title: font(-18, 700),
            font_body: font(-15, 400),
            bg_brush: CreateSolidBrush(pal.bg),
            field_brush: CreateSolidBrush(pal.field),
            edit: HWND::default(),
        });
        let ptr = Box::into_raw(context);
        *CTX.lock().unwrap() = Some(ptr as usize);

        let (sx, sy) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class, w!("Perch"), WS_POPUP, (sx - W) / 2, (sy - H) / 2, W, H, None, None, inst, None).ok()?;
        // 圆角窗口
        let rgn = CreateRoundRectRgn(0, 0, W + 1, H + 1, 18, 18);
        SetWindowRgn(hwnd, rgn, true);

        let wide_init: Vec<u16> = initial.encode_utf16().chain(Some(0)).collect();
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            PCWSTR(wide_init.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(0x80 /* ES_AUTOHSCROLL */),
            34,
            90,
            W - 68,
            22,
            hwnd,
            HMENU(100 as *mut _),
            inst,
            None,
        )
        .ok()?;
        SendMessageW(edit, WM_SETFONT, WPARAM((*ptr).font_body.0 as usize), LPARAM(1));
        (*ptr).edit = edit;

        let btn = |id: isize, x: i32| {
            let b = CreateWindowExW(WINDOW_EX_STYLE(0), w!("BUTTON"), w!(""), WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(0xB /* BS_OWNERDRAW */), x, 136, 100, 34, hwnd, HMENU(id as *mut _), inst, None).unwrap_or_default();
            b
        };
        btn(ID_OK, W - 22 - 100 - 112);
        btn(ID_CANCEL, W - 22 - 100);
        SendMessageW(hwnd, 0x0401 /* DM_SETDEFID */, WPARAM(ID_OK as usize), LPARAM(0));

        DONE.store(false, Ordering::SeqCst);
        *RESULT.lock().unwrap() = None;
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(edit);
        SendMessageW(edit, 0x00B1 /* EM_SETSEL */, WPARAM(0), LPARAM(-1));

        let mut msg = MSG::default();
        while !DONE.load(Ordering::SeqCst) {
            if GetMessageW(&mut msg, None, 0, 0).0 <= 0 {
                break;
            }
            if !IsDialogMessageW(hwnd, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // 释放资源
        *CTX.lock().unwrap() = None;
        let c = Box::from_raw(ptr);
        let _ = DeleteObject(c.font_title);
        let _ = DeleteObject(c.font_body);
        let _ = DeleteObject(c.bg_brush);
        let _ = DeleteObject(c.field_brush);
        drop(c);
        let out = RESULT.lock().unwrap().take();
        out.filter(|s| !s.is_empty())
    }
}
