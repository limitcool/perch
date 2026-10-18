//! 音频频谱条(Dock 上方):WASAPI 环回采集 -> FFT -> 48 个对数频段。
//! 采集线程把频段写进原子量;绘制窗口是一个小的、完全点击穿透的分层窗口,静音时隐藏(零开销)。

use std::f32::consts::PI;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const BANDS: usize = 48;
/// 桌面被盖住时暂停 FFT(仍排空缓冲区,采集线程几乎不占 CPU)
pub static PAUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LEVELS: [AtomicU32; BANDS] = [const { AtomicU32::new(0) }; BANDS];

fn level(i: usize) -> f32 {
    f32::from_bits(LEVELS[i].load(Ordering::Relaxed))
}

// ---------------- FFT ----------------
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for i in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (re[i + k + len / 2] * cr - im[i + k + len / 2] * ci, re[i + k + len / 2] * ci + im[i + k + len / 2] * cr);
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let t = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = t;
            }
        }
        len <<= 1;
    }
}

// ---------------- 采集线程 ----------------
fn capture_loop() -> Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let fmt = client.GetMixFormat()?;
        let (channels, rate, bits) = ((*fmt).nChannels as usize, (*fmt).nSamplesPerSec as f32, (*fmt).wBitsPerSample);
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, fmt, None)?;
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;

        const N: usize = 2048;
        let mut ring: Vec<f32> = Vec::with_capacity(N * 2);
        let window: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / (N as f32 - 1.0)).cos()).collect();
        // 频段边界:50Hz ~ 16kHz 对数分布
        let edges: Vec<usize> = (0..=BANDS)
            .map(|i| {
                let f = 50.0 * (16000.0f32 / 50.0).powf(i as f32 / BANDS as f32);
                ((f * N as f32 / rate) as usize).clamp(1, N / 2 - 1)
            })
            .collect();
        let mut smooth = [0.0f32; BANDS];
        let mut last_ok = Instant::now();

        loop {
            let paused = PAUSED.load(Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(if paused { 250 } else { 16 }));
            let mut pkt = cap.GetNextPacketSize()?;
            while pkt > 0 {
                let mut data: *mut u8 = std::ptr::null_mut();
                let (mut frames, mut flags) = (0u32, 0u32);
                cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                for f in 0..frames as usize {
                    let mut s = 0.0f32;
                    if !silent && !data.is_null() {
                        for c in 0..channels {
                            s += if bits == 32 {
                                *(data as *const f32).add(f * channels + c)
                            } else {
                                *(data as *const i16).add(f * channels + c) as f32 / 32768.0
                            };
                        }
                        s /= channels as f32;
                    }
                    if !paused {
                        ring.push(s);
                    }
                }
                cap.ReleaseBuffer(frames)?;
                last_ok = Instant::now();
                pkt = cap.GetNextPacketSize()?;
            }
            if paused {
                ring.clear();
                continue;
            }
            // 没有数据包(没有声音)时让条形衰减到 0
            if last_ok.elapsed() > Duration::from_millis(200) {
                ring.clear();
                ring.resize(N, 0.0);
            }
            while ring.len() >= N {
                let mut re: Vec<f32> = ring[..N].iter().zip(&window).map(|(a, w)| a * w).collect();
                let mut im = vec![0.0f32; N];
                fft(&mut re, &mut im);
                for b in 0..BANDS {
                    let (lo, hi) = (edges[b], edges[b + 1].max(edges[b] + 1));
                    let mut m = 0.0f32;
                    for k in lo..hi.min(N / 2) {
                        m = m.max((re[k] * re[k] + im[k] * im[k]).sqrt());
                    }
                    let db = 20.0 * (m / (N as f32 / 4.0) + 1e-6).log10();
                    let target = ((db + 62.0) / 52.0).clamp(0.0, 1.0);
                    smooth[b] = if target > smooth[b] { smooth[b] + (target - smooth[b]) * 0.65 } else { smooth[b] * 0.88 };
                    LEVELS[b].store(smooth[b].to_bits(), Ordering::Relaxed);
                }
                ring.drain(..N / 2);
            }
        }
    }
}

pub fn start_capture() {
    std::thread::spawn(|| loop {
        let _ = capture_loop();
        // 默认输出设备切换/断开后重试
        for l in LEVELS.iter() {
            l.store(0, Ordering::Relaxed);
        }
        std::thread::sleep(Duration::from_secs(3));
    });
}

// ---------------- 绘制窗口 ----------------
/// 窗口位置/尺寸(物理像素)由 cards::spectrum_rect()(DIP)× 缩放得到
fn win() -> (i32, i32, i32, i32) {
    let (x, y, w, h) = crate::cards::spectrum_rect();
    let s = crate::window::scale();
    let (ox, oy) = crate::window::origin();
    (ox + (x * s) as i32, oy + (y * s) as i32, (w * s) as i32, (h * s) as i32)
}

pub struct Spectrum {
    hwnd: HWND,
    mem_dc: HDC,
    bitmap: HBITMAP,
    /// 当前位图的像素尺寸;Dock 宽度变化(增减应用 / 换屏 / 缩放)时要重建
    bmp_size: (i32, i32),
    /// 最近一次有声音的时间:静音要持续一小会儿才隐藏,避免曲间停顿时频谱一闪而没
    last_loud: Instant,
    last_vis_check: Instant,
    rt: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
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

impl Spectrum {
    pub fn new(factory: &ID2D1Factory) -> Result<Self> {
        unsafe {
            let inst = GetModuleHandleW(None)?;
            let class = w!("PerchSpectrumClass");
            let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassExW(&wc);
            let (win_x, win_y, win_w, win_h) = win();
            let hwnd = CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, class, w!("Perch Spectrum"), WS_POPUP, win_x, win_y, win_w, win_h, None, None, inst, None)?;
            let screen = GetDC(None);
            let mem_dc = CreateCompatibleDC(screen);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: win_w, biHeight: -win_h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
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
            let dpi = 96.0 * crate::window::scale();
            rt.SetDpi(dpi, dpi);
            let brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
            Ok(Self { hwnd, mem_dc, bitmap, bmp_size: (win_w, win_h), last_loud: crate::log::ago(10), last_vis_check: Instant::now(), rt, brush, last_frame: Instant::now(), last_fs_check: crate::log::ago(10), fullscreen: false, shown: false })
        }
    }

    /// 窗口尺寸变了(Dock 宽度 / DPI)就换一张新位图,否则绘制会被旧尺寸裁掉
    fn ensure_bitmap(&mut self, w: i32, h: i32) -> bool {
        if (w, h) == self.bmp_size {
            return true;
        }
        if w < 1 || h < 1 {
            return false;
        }
        unsafe {
            let screen = GetDC(None);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let nb = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
            ReleaseDC(None, screen);
            let Ok(nb) = nb else { return false };
            SelectObject(self.mem_dc, nb);
            let _ = DeleteObject(self.bitmap);
            self.bitmap = nb;
            self.bmp_size = (w, h);
        }
        true
    }

    pub fn active(&self) -> bool {
        self.shown
    }

    pub fn hide(&mut self) {
        if self.shown {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shown = false;
        }
    }

    /// accent / sub 为主题色(0..1 RGB)。返回是否绘制了一帧
    pub fn tick(&mut self, accent: (f32, f32, f32), sub: (f32, f32, f32)) -> bool {
        if self.last_fs_check.elapsed().as_secs() >= 2 {
            self.last_fs_check = Instant::now();
            let st = unsafe { SHQueryUserNotificationState() };
            self.fullscreen = matches!(st, Ok(s) if s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE);
        }
        let peak = (0..BANDS).map(level).fold(0.0f32, f32::max);
        if peak >= 0.02 {
            self.last_loud = Instant::now();
        }
        // 全屏应用时隐藏;静音持续超过 1.5 秒才隐藏(曲间停顿 / 安静段落不让它消失)
        if self.fullscreen || self.last_loud.elapsed().as_millis() > 1500 {
            self.hide();
            return false;
        }
        // 窗口被系统/别的程序弄成不可见时(桌面切换等),下一帧重新显示
        if self.shown && self.last_vis_check.elapsed().as_millis() >= 1000 {
            self.last_vis_check = Instant::now();
            if !unsafe { IsWindowVisible(self.hwnd) }.as_bool() {
                self.shown = false;
            }
        }
        if self.last_frame.elapsed().as_millis() < 33 {
            return false;
        }
        self.last_frame = Instant::now();
        unsafe {
            let (win_x, win_y, win_w, win_h) = win();
            if !self.ensure_bitmap(win_w, win_h) {
                return false;
            }
            let rc = RECT { left: 0, top: 0, right: win_w, bottom: win_h };
            if self.rt.BindDC(self.mem_dc, &rc).is_err() {
                return false;
            }
            self.rt.BeginDraw();
            self.rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            // 48 根柱子均分整个窗口宽度(窗口宽度 = Dock 宽度),柱宽约占间距的 2/3
            let w_dip = crate::cards::spectrum_rect().2;
            let pitch = w_dip / BANDS as f32;
            let bar_w = (pitch * 0.68).max(2.0);
            for i in 0..BANDS {
                let v = level(i).clamp(0.0, 1.0);
                let h = (v * 48.0).max(1.0);
                let c = if i < 36 { accent } else { sub };
                self.brush.SetColor(&D2D1_COLOR_F { r: c.0, g: c.1, b: c.2, a: 0.25 + 0.65 * v });
                let x = pitch * i as f32 + (pitch - bar_w) / 2.0;
                let r = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x, top: 4.0 + 48.0 - h, right: x + bar_w, bottom: 52.0 }, radiusX: 1.5, radiusY: 1.5 };
                self.rt.FillRoundedRectangle(&r, &self.brush);
            }
            if self.rt.EndDraw(None, None).is_err() {
                return false;
            }
            let screen = GetDC(None);
            let pt_dst = POINT { x: win_x, y: win_y };
            let pt_src = POINT { x: 0, y: 0 };
            let size = SIZE { cx: win_w, cy: win_h };
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

impl Drop for Spectrum {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
