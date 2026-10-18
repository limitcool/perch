//! 壁纸动效:全屏、完全点击穿透的分层窗口,粒子随主题变化(樱花/光点/晶体/飘雪/心形/数据流/火星)。
//! 只在开启动效且没有全屏应用时绘制;关闭时窗口隐藏,零开销。

use std::collections::HashMap;
use std::time::Instant;

use windows::core::*;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Petal,
    Dot,
    Diamond,
    Heart,
    Streak,
}

struct Fx {
    kind: Kind,
    rgb: (f32, f32, f32),
    size: (f32, f32),
    speed: (f32, f32), // px/s
    up: bool,
    sway: f32,
    alpha: (f32, f32),
}

fn recipe(theme: &str) -> Fx {
    let pack = perch_data::theme_pack::load(theme);
    let fx = pack.fx;
    let kind = match fx.kind.as_str() {
        "petal" => Kind::Petal,
        "diamond" => Kind::Diamond,
        "heart" => Kind::Heart,
        "streak" => Kind::Streak,
        _ => Kind::Dot,
    };
    let rgb = if fx.rgb.len() >= 3 {
        (fx.rgb[0] as f32 / 255.0, fx.rgb[1] as f32 / 255.0, fx.rgb[2] as f32 / 255.0)
    } else {
        (150.0 / 255.0, 190.0 / 255.0, 255.0 / 255.0)
    };
    Fx {
        kind,
        rgb,
        size: (fx.size_min, fx.size_max),
        speed: (fx.speed_min, fx.speed_max),
        up: fx.up,
        sway: fx.sway,
        alpha: (fx.alpha_min, fx.alpha_max),
    }
}

struct Particle {
    x0: f32,
    y0: f32,
    size: f32,
    vy: f32,
    vx: f32,
    amp: f32,
    freq: f32,
    phase: f32,
    rot: f32,
    alpha: f32,
}

pub struct Ambient {
    pub hwnd: HWND,
    w: i32,
    h: i32,
    mem_dc: HDC,
    _bitmap: HBITMAP,
    rt: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    dwrite: IDWriteFactory,
    formats: HashMap<u32, IDWriteTextFormat>,
    fx: Fx,
    particles: Vec<Particle>,
    theme: String,
    level: String,
    start: Instant,
    last_frame: Instant,
    last_fs_check: Instant,
    fullscreen: bool,
    shown: bool,
}

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_ERASEBKGND => LRESULT(1),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

impl Ambient {
    pub fn new(factory: &ID2D1Factory, dwrite: &IDWriteFactory, w: i32, h: i32) -> Result<Self> {
        unsafe {
            let inst = GetModuleHandleW(None)?;
            let class = w!("PerchAmbientClass");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc),
                hInstance: inst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!("Perch Ambient"),
                WS_POPUP,
                crate::window::origin().0,
                crate::window::origin().1,
                w,
                h,
                None,
                None,
                inst,
                None,
            )?;

            // 32 位 DIB + 内存 DC,D2D 画进去后用 UpdateLayeredWindow 逐像素 alpha 合成
            let screen = GetDC(None);
            let mem_dc = CreateCompatibleDC(screen);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
            SelectObject(mem_dc, bitmap);
            ReleaseDC(None, screen);

            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = factory.CreateDCRenderTarget(&props)?;
            rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;

            Ok(Self {
                hwnd,
                w,
                h,
                mem_dc,
                _bitmap: bitmap,
                rt,
                brush,
                dwrite: dwrite.clone(),
                formats: HashMap::new(),
                fx: recipe("default"),
                particles: Vec::new(),
                theme: String::new(),
                level: String::new(),
                start: Instant::now(),
                last_frame: Instant::now(),
                last_fs_check: crate::log::ago(10),
                fullscreen: false,
                shown: false,
            })
        }
    }

    /// 主题或档位变化时重建粒子
    pub fn configure(&mut self, theme: &str, level: &str) {
        if self.theme == theme && self.level == level {
            return;
        }
        self.theme = theme.to_string();
        self.level = level.to_string();
        self.fx = recipe(theme);
        self.particles.clear();
        if level == "off" {
            self.hide();
            return;
        }
        let n = if level == "standard" { 22 } else { 9 };
        let mut seed: u64 = 0x9E3779B97F4A7C15;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 10000) as f32 / 10000.0
        };
        let fx = &self.fx;
        for _ in 0..n {
            self.particles.push(Particle {
                x0: rnd() * (self.w as f32 + 80.0),
                y0: rnd() * (self.h as f32 + 80.0),
                size: (fx.size.0 + rnd() * (fx.size.1 - fx.size.0)) * crate::window::scale(),
                vy: fx.speed.0 + rnd() * (fx.speed.1 - fx.speed.0),
                vx: if fx.sway > 0.0 { 6.0 + rnd() * 10.0 } else { 0.0 },
                amp: (24.0 + rnd() * 40.0) * fx.sway,
                freq: 0.4 + rnd() * 0.6,
                phase: rnd() * 6.28,
                rot: rnd() * 360.0,
                alpha: fx.alpha.0 + rnd() * (fx.alpha.1 - fx.alpha.0),
            });
        }
    }

    pub fn active(&self) -> bool {
        self.level != "off" && !self.level.is_empty() && !self.fullscreen
    }

    pub fn frame_ms(&self) -> u64 {
        if self.level == "standard" {
            42
        } else {
            66
        }
    }

    pub fn hide(&mut self) {
        if self.shown {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shown = false;
        }
    }

    fn fmt(&mut self, size: f32) -> Option<IDWriteTextFormat> {
        let key = (size * 10.0) as u32;
        if let Some(f) = self.formats.get(&key) {
            return Some(f.clone());
        }
        unsafe {
            let f = self
                .dwrite
                .CreateTextFormat(w!("Segoe UI Symbol"), None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us"))
                .ok()?;
            self.formats.insert(key, f.clone());
            Some(f)
        }
    }

    /// 返回是否真的绘制了一帧
    pub fn tick(&mut self) -> bool {
        // 全屏应用 / 演示模式时自动暂停(每 2 秒检查一次)
        if self.last_fs_check.elapsed().as_secs() >= 2 {
            self.last_fs_check = Instant::now();
            let state = unsafe { SHQueryUserNotificationState() };
            self.fullscreen = matches!(state, Ok(s) if s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE);
        }
        if !self.active() {
            self.hide();
            return false;
        }
        if self.last_frame.elapsed().as_millis() < self.frame_ms() as u128 {
            return false;
        }
        self.last_frame = Instant::now();
        let t = self.start.elapsed().as_secs_f32();
        unsafe {
            let rc = RECT { left: 0, top: 0, right: self.w, bottom: self.h };
            if self.rt.BindDC(self.mem_dc, &rc).is_err() {
                return false;
            }
            self.rt.BeginDraw();
            self.rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            let (cr, cg, cb) = self.fx.rgb;
            let kind = self.fx.kind;
            let up = self.fx.up;
            let (ww, hh) = (self.w as f32 + 80.0, self.h as f32 + 80.0);
            for i in 0..self.particles.len() {
                let (x, y, size, rot, alpha) = {
                    let p = &self.particles[i];
                    let x = (p.x0 + t * p.vx + p.amp * (t * p.freq + p.phase).sin()).rem_euclid(ww) - 40.0;
                    let y = if up { (p.y0 - t * p.vy).rem_euclid(hh) - 40.0 } else { (p.y0 + t * p.vy).rem_euclid(hh) - 40.0 };
                    (x, y, p.size, p.rot, p.alpha)
                };
                self.brush.SetColor(&D2D1_COLOR_F { r: cr, g: cg, b: cb, a: alpha / 255.0 });
                match kind {
                    Kind::Dot => {
                        let e = D2D1_ELLIPSE { point: D2D_POINT_2F { x, y }, radiusX: size / 2.0, radiusY: size / 2.0 };
                        self.rt.FillEllipse(&e, &self.brush);
                    }
                    Kind::Petal => {
                        self.rt.SetTransform(&Matrix3x2::rotation(rot + t * 12.0, x, y));
                        let e = D2D1_ELLIPSE { point: D2D_POINT_2F { x, y }, radiusX: size / 2.0, radiusY: size * 0.31 };
                        self.rt.FillEllipse(&e, &self.brush);
                        self.rt.SetTransform(&Matrix3x2::identity());
                    }
                    Kind::Diamond => {
                        self.rt.SetTransform(&Matrix3x2::rotation(45.0, x, y));
                        let r = D2D_RECT_F { left: x - size * 0.275, top: y - size / 2.0, right: x + size * 0.275, bottom: y + size / 2.0 };
                        self.rt.FillRectangle(&r, &self.brush);
                        self.rt.SetTransform(&Matrix3x2::identity());
                    }
                    Kind::Streak => {
                        let r = D2D_RECT_F { left: x, top: y, right: x + 1.6, bottom: y + size };
                        self.rt.FillRectangle(&r, &self.brush);
                    }
                    Kind::Heart => {
                        if let Some(f) = self.fmt(size) {
                            let txt: Vec<u16> = "♥".encode_utf16().collect();
                            let r = D2D_RECT_F { left: x, top: y, right: x + size * 2.0, bottom: y + size * 2.0 };
                            self.rt.DrawText(&txt, &f, &r, &self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                        }
                    }
                }
            }
            if self.rt.EndDraw(None, None).is_err() {
                return false;
            }
            // 合成到屏幕
            let screen = GetDC(None);
            let pt_dst = POINT { x: crate::window::origin().0, y: crate::window::origin().1 };
            let pt_src = POINT { x: 0, y: 0 };
            let size = SIZE { cx: self.w, cy: self.h };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let _ = UpdateLayeredWindow(self.hwnd, screen, Some(&pt_dst), Some(&size), self.mem_dc, Some(&pt_src), COLORREF(0), Some(&blend), ULW_ALPHA);
            ReleaseDC(None, screen);
            if !self.shown {
                let _ = SetWindowPos(self.hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
                self.shown = true;
            }
        }
        true
    }
}

impl Drop for Ambient {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
