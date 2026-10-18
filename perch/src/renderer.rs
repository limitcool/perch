use std::collections::HashMap;
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;

use crate::live_theme::{Deco, DecoKind};
use crate::theme::ThemeConfig;

pub struct D2dRenderer {
    pub d2d_factory: ID2D1Factory,
    pub dwrite_factory: IDWriteFactory,
    pub hwnd_target: Option<ID2D1HwndRenderTarget>,
    pub render_target: Option<ID2D1RenderTarget>,
    pub solid_brush: Option<ID2D1SolidColorBrush>,
    pub format_cache: HashMap<(String, u32, bool, u32, bool), IDWriteTextFormat>,
    pub font_num: String,
    pub wrap_next: Option<(f32, f32)>,
    emoji_face: Option<IDWriteFontFace>,
    dwrite4: Option<IDWriteFactory4>,
    width_cache: HashMap<String, f32>,
    pub image_cache: HashMap<String, Option<ID2D1Bitmap>>,
    wic: Option<IWICImagingFactory>,
    pub seg: Option<D2D1_COLOR_F>,
    pub hwnd: HWND,
    pub width: u32,
    pub height: u32,
}

impl D2dRenderer {
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
        unsafe {
            let options = D2D1_FACTORY_OPTIONS::default();
            let d2d_factory: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&options))?;

            let dwrite_factory: IDWriteFactory =
                DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let mut renderer = Self {
                d2d_factory,
                dwrite_factory,
                hwnd_target: None,
                render_target: None,
                solid_brush: None,
                format_cache: HashMap::new(),
                font_num: "Segoe UI Variable Display".to_string(),
                wrap_next: None,
                emoji_face: None,
                dwrite4: None,
                width_cache: HashMap::new(),
                image_cache: HashMap::new(),
                wic: None,
                seg: None,
                hwnd,
                width,
                height,
            };

            renderer.create_device_resources()?;
            Ok(renderer)
        }
    }

    /// 屏幕尺寸 / DPI 变化:丢弃渲染目标和所有设备资源,下一帧按新尺寸重建
    pub fn reset_for_display(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.hwnd_target = None;
        self.render_target = None;
        self.solid_brush = None;
        self.image_cache.clear();
        self.emoji_face = self.emoji_face.take();
    }

    #[allow(dead_code)]
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.width = width;
        self.height = height;
        if let Some(target) = &self.hwnd_target {
            unsafe {
                let size = D2D_SIZE_U { width, height };
                target.Resize(&size)?;
            }
        }
        Ok(())
    }

    pub fn create_device_resources(&mut self) -> Result<()> {
        if self.render_target.is_some() && self.solid_brush.is_some() {
            return Ok(());
        }

        unsafe {
            let render_props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };

            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: D2D_SIZE_U {
                    width: self.width,
                    height: self.height,
                },
                presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            };

            let hwnd_target = self
                .d2d_factory
                .CreateHwndRenderTarget(&render_props, &hwnd_props)?;
            // 绘制坐标一律是 DIP:把渲染目标 DPI 设成系统 DPI,物理像素大小保持不变
            let dpi = 96.0 * crate::window::scale();
            hwnd_target.SetDpi(dpi, dpi);
            hwnd_target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            hwnd_target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);

            let render_target: ID2D1RenderTarget = hwnd_target.cast()?;

            // 单实例画刷复用，消灭句柄泄漏
            let default_color = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
            let solid_brush = render_target.CreateSolidColorBrush(&default_color, None)?;

            self.hwnd_target = Some(hwnd_target);
            self.render_target = Some(render_target);
            self.solid_brush = Some(solid_brush);
            Ok(())
        }
    }

    pub fn begin_draw(&mut self) -> Result<()> {
        self.create_device_resources()?;
        if let Some(target) = &self.render_target {
            unsafe {
                target.BeginDraw();
                // 清空为完全透明
                target.Clear(Some(&D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                }));
            }
        }
        Ok(())
    }

    pub fn end_draw(&mut self) -> Result<()> {
        if let Some(target) = &self.render_target {
            unsafe {
                if let Err(err) = target.EndDraw(None, None) {
                    if err.code() == D2DERR_RECREATE_TARGET {
                        self.hwnd_target = None;
                        self.render_target = None;
                        self.solid_brush = None;
                        self.image_cache.clear();
                    } else {
                        return Err(err);
                    }
                }
            }
        }
        Ok(())
    }

    // 绘制战术圆角卡片底板 (零句柄开销)
    pub fn draw_card(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        fill_color: D2D1_COLOR_F,
        stroke_color: D2D1_COLOR_F,
        stroke_width: f32,
    ) -> Result<()> {
        let target = match &self.render_target {
            Some(t) => t,
            None => return Ok(()),
        };
        let brush = match &self.solid_brush {
            Some(b) => b,
            None => return Ok(()),
        };

        unsafe {
            let rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            let rounded = D2D1_ROUNDED_RECT {
                rect,
                radiusX: radius,
                radiusY: radius,
            };

            // 填充底色 (直接 SetColor 复用画刷)
            brush.SetColor(&fill_color);
            target.FillRoundedRectangle(&rounded, brush);

            // 描边
            if stroke_width > 0.0 && stroke_color.a > 0.0 {
                brush.SetColor(&stroke_color);
                target.DrawRoundedRectangle(&rounded, brush, stroke_width, None);
            }
        }
        Ok(())
    }

    // 绘制线段 (零句柄开销)
    pub fn draw_line(
        &self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: D2D1_COLOR_F,
        width: f32,
    ) -> Result<()> {
        let target = match &self.render_target {
            Some(t) => t,
            None => return Ok(()),
        };
        let brush = match &self.solid_brush {
            Some(b) => b,
            None => return Ok(()),
        };

        unsafe {
            brush.SetColor(&color);
            target.DrawLine(
                D2D_POINT_2F { x: x1, y: y1 },
                D2D_POINT_2F { x: x2, y: y2 },
                brush,
                width,
                None,
            );
        }
        Ok(())
    }

    // 绘制样式包装饰(theme.toml 的 ThemeShape2 / ThemeCorner);sx 为卡宽相对 260 的水平缩放
    pub fn draw_deco(&self, d: &Deco, ox: f32, oy: f32, sx: f32) -> Result<()> {
        let target = match &self.render_target {
            Some(t) => t,
            None => return Ok(()),
        };
        let brush = match &self.solid_brush {
            Some(b) => b,
            None => return Ok(()),
        };
        unsafe {
            match d.kind {
                DecoKind::Line { x1, y1, x2, y2 } => {
                    if let Some(c) = d.stroke {
                        brush.SetColor(&c.to_d2d());
                        target.DrawLine(
                            D2D_POINT_2F { x: ox + x1 * sx, y: oy + y1 },
                            D2D_POINT_2F { x: ox + x2 * sx, y: oy + y2 },
                            brush,
                            d.width,
                            None,
                        );
                    }
                }
                DecoKind::Rect { x, y, w, h } => {
                    let rect = D2D_RECT_F { left: ox + x * sx, top: oy + y, right: ox + (x + w) * sx, bottom: oy + y + h };
                    if let Some(c) = d.fill {
                        brush.SetColor(&c.to_d2d());
                        target.FillRectangle(&rect, brush);
                    }
                    if let Some(c) = d.stroke.filter(|c| c.a > 0.0 && d.width > 0.0) {
                        brush.SetColor(&c.to_d2d());
                        target.DrawRectangle(&rect, brush, d.width, None);
                    }
                }
                DecoKind::Ellipse { cx, cy, rx, ry } => {
                    let e = D2D1_ELLIPSE { point: D2D_POINT_2F { x: ox + cx * sx, y: oy + cy }, radiusX: rx, radiusY: ry };
                    if let Some(c) = d.fill {
                        brush.SetColor(&c.to_d2d());
                        target.FillEllipse(&e, brush);
                    }
                    if let Some(c) = d.stroke.filter(|c| c.a > 0.0 && d.width > 0.0) {
                        brush.SetColor(&c.to_d2d());
                        target.DrawEllipse(&e, brush, d.width, None);
                    }
                }
            }
        }
        Ok(())
    }

    // 文字:x,y 为左上角(右对齐=右边缘, 居中=中心线);字号单位 pt(×4/3 换算为像素)
    pub fn text_at(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        pt: f32,
        color: D2D1_COLOR_F,
        font: &str,
        bold: bool,
        alignment: DWRITE_TEXT_ALIGNMENT,
    ) -> Result<()> {
        let size = pt * 4.0 / 3.0;
        let bw = 460.0;
        let bx = if alignment == DWRITE_TEXT_ALIGNMENT_CENTER {
            x - bw / 2.0
        } else if alignment == DWRITE_TEXT_ALIGNMENT_TRAILING {
            x - bw
        } else {
            x
        };
        self.draw_text_aligned(text, bx, y, bw, size * 1.7, font, size, color, bold, alignment)
    }

    // 描边圆环
    pub fn draw_ring(&self, cx: f32, cy: f32, r: f32, color: D2D1_COLOR_F, width: f32) {
        if let (Some(t), Some(b)) = (&self.render_target, &self.solid_brush) {
            unsafe {
                b.SetColor(&color);
                let e = D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx, y: cy }, radiusX: r, radiusY: r };
                t.DrawEllipse(&e, b, width, None);
            }
        }
    }

    /// 从文件(exe / 文件夹 / 快捷方式)提取系统图标并绘制,按路径缓存
    pub fn draw_file_icon(&mut self, path: &str, x: f32, y: f32, size: f32) {
        let key = format!("icon:{}", path);
        if !self.image_cache.contains_key(&key) {
            let bmp = self.load_file_icon(path);
            self.image_cache.insert(key.clone(), bmp);
        }
        if let (Some(Some(bmp)), Some(t)) = (self.image_cache.get(&key), &self.render_target) {
            let dest = D2D_RECT_F { left: x, top: y, right: x + size, bottom: y + size };
            unsafe {
                t.DrawBitmap(bmp, Some(&dest), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            }
        }
    }

    fn load_file_icon(&mut self, path: &str) -> Option<ID2D1Bitmap> {
        use windows::Win32::Graphics::Gdi::*;
        use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
        use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
        use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL};
        unsafe {
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let mut info = SHFILEINFOW::default();
            let r = SHGetFileInfoW(PCWSTR(wide.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_LARGEICON);
            if r == 0 || info.hIcon.is_invalid() {
                return None;
            }
            // 画到 32 位 DIB(带 alpha)再交给 D2D
            let size = 32i32;
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(screen);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: size, biHeight: -size, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let hbm = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            let old = SelectObject(dc, hbm);
            std::ptr::write_bytes(bits as *mut u8, 0, (size * size * 4) as usize);
            let _ = DrawIconEx(dc, 0, 0, info.hIcon, size, size, 0, None, DI_NORMAL);
            // GDI 画图标时 alpha 通道可能为 0:若整张图 alpha 全 0,把非黑像素补成不透明
            let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
            if px.iter().all(|p| p >> 24 == 0) {
                for p in px.iter_mut() {
                    if *p & 0x00FF_FFFF != 0 {
                        *p |= 0xFF00_0000;
                    }
                }
            }
            let props = D2D1_BITMAP_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0 };
            let bmp = self.render_target.as_ref().and_then(|t| t.CreateBitmap(D2D_SIZE_U { width: size as u32, height: size as u32 }, Some(bits as *const _), (size * 4) as u32, &props).ok());
            SelectObject(dc, old);
            let _ = DeleteObject(hbm);
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            let _ = DestroyIcon(info.hIcon);
            bmp
        }
    }

    // 绘制图片(WIC 解码 + 缓存;路径带修改时间,封面更新会自动重载)
    pub fn draw_image(&mut self, path: &str, x: f32, y: f32, w: f32, h: f32) {
        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let key = format!("{}|{}", path, mtime);
        if !self.image_cache.contains_key(&key) {
            if self.image_cache.len() > 60 {
                self.image_cache.clear();
            }
            let bmp = self.load_bitmap(path);
            self.image_cache.insert(key.clone(), bmp);
        }
        if let (Some(Some(bmp)), Some(t)) = (self.image_cache.get(&key), &self.render_target) {
            let dest = D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h };
            unsafe {
                t.DrawBitmap(bmp, Some(&dest), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            }
        }
    }

    fn load_bitmap(&mut self, path: &str) -> Option<ID2D1Bitmap> {
        unsafe {
            if self.wic.is_none() {
                self.wic = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok();
            }
            let wic = self.wic.as_ref()?;
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let decoder = wic
                .CreateDecoderFromFilename(PCWSTR(wide.as_ptr()), None, windows::Win32::Foundation::GENERIC_READ, WICDecodeMetadataCacheOnDemand)
                .ok()?;
            let frame = decoder.GetFrame(0).ok()?;
            let conv = wic.CreateFormatConverter().ok()?;
            conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
            self.render_target.as_ref()?.CreateBitmapFromWicBitmap(&conv, None).ok()
        }
    }

    /// 带彩色 emoji 的文字:按 emoji / 普通文字分段,emoji 用 Segoe UI Emoji 的 COLR 图层上色
    pub fn draw_text_aligned(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        font_name: &str,
        font_size: f32,
        color: D2D1_COLOR_F,
        bold: bool,
        alignment: DWRITE_TEXT_ALIGNMENT,
    ) -> Result<()> {
        if self.wrap_next.is_some() || !text.chars().any(is_color_emoji) {
            return self.draw_plain(text, x, y, w, h, font_name, font_size, color, bold, alignment);
        }
        // 分段
        let mut segs: Vec<(bool, String)> = Vec::new();
        for ch in text.chars() {
            if ch == '\u{FE0F}' || ch == '\u{200D}' {
                continue;
            }
            let em = is_color_emoji(ch);
            match segs.last_mut() {
                Some((is_em, s)) if *is_em == em && !em => s.push(ch),
                _ => segs.push((em, ch.to_string())),
            }
        }
        let emoji_w = font_size * 1.22;
        let mut widths = Vec::with_capacity(segs.len());
        for (is_em, s) in &segs {
            widths.push(if *is_em { emoji_w } else { self.plain_width(s, font_name, font_size, bold) });
        }
        let total: f32 = widths.iter().sum();
        let mut cx = if alignment == DWRITE_TEXT_ALIGNMENT_CENTER {
            x + w / 2.0 - total / 2.0
        } else if alignment == DWRITE_TEXT_ALIGNMENT_TRAILING {
            x + w - total
        } else {
            x
        };
        for (i, (is_em, s)) in segs.iter().enumerate() {
            if *is_em {
                let ch = s.chars().next().unwrap();
                self.draw_emoji(ch, cx, y, font_size, color);
            } else {
                self.draw_plain(s, cx, y, widths[i] + 6.0, h, font_name, font_size, color, bold, DWRITE_TEXT_ALIGNMENT_LEADING)?;
            }
            cx += widths[i];
        }
        Ok(())
    }

    /// 文字宽度(与 draw_text_aligned 的分段/emoji 宽度规则一致)
    pub fn measure(&mut self, text: &str, font_name: &str, font_size: f32, bold: bool) -> f32 {
        let mut total = 0.0;
        let mut buf = String::new();
        for ch in text.chars() {
            if ch == '\u{FE0F}' || ch == '\u{200D}' {
                continue;
            }
            if is_color_emoji(ch) {
                if !buf.is_empty() {
                    total += self.plain_width(&buf, font_name, font_size, bold);
                    buf.clear();
                }
                total += font_size * 1.22;
            } else {
                buf.push(ch);
            }
        }
        if !buf.is_empty() {
            total += self.plain_width(&buf, font_name, font_size, bold);
        }
        total
    }

    fn plain_width(&mut self, s: &str, font_name: &str, size: f32, bold: bool) -> f32 {
        let key = format!("{}|{}|{}|{}", font_name, size, bold, s);
        if let Some(w) = self.width_cache.get(&key) {
            return *w;
        }
        if self.width_cache.len() > 800 {
            self.width_cache.clear();
        }
        let mut width = size * 0.6 * s.chars().count() as f32;
        if let Ok(fmt) = self.get_or_create_format(font_name, size, bold, DWRITE_TEXT_ALIGNMENT_LEADING) {
            let wide: Vec<u16> = s.encode_utf16().collect();
            unsafe {
                if let Ok(layout) = self.dwrite_factory.CreateTextLayout(&wide, &fmt, 4096.0, 200.0) {
                    let mut m = DWRITE_TEXT_METRICS::default();
                    if layout.GetMetrics(&mut m).is_ok() {
                        width = m.widthIncludingTrailingWhitespace;
                    }
                }
            }
        }
        self.width_cache.insert(key, width);
        width
    }

    fn draw_emoji(&mut self, ch: char, x: f32, y: f32, size: f32, fg: D2D1_COLOR_F) {
        unsafe {
            if self.emoji_face.is_none() {
                let mut coll: Option<IDWriteFontCollection> = None;
                if self.dwrite_factory.GetSystemFontCollection(&mut coll, false).is_ok() {
                    if let Some(coll) = coll {
                        let (mut idx, mut exists) = (0u32, windows::Win32::Foundation::BOOL(0));
                        if coll.FindFamilyName(w!("Segoe UI Emoji"), &mut idx, &mut exists).is_ok() && exists.as_bool() {
                            self.emoji_face = coll
                                .GetFontFamily(idx)
                                .and_then(|f| f.GetFirstMatchingFont(DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL))
                                .and_then(|f| f.CreateFontFace())
                                .ok();
                        }
                    }
                }
            }
            if self.dwrite4.is_none() {
                self.dwrite4 = self.dwrite_factory.cast::<IDWriteFactory4>().ok();
            }
            let (Some(face), Some(f4), Some(target), Some(brush)) = (&self.emoji_face, &self.dwrite4, &self.render_target, &self.solid_brush) else {
                return;
            };
            let cp = ch as u32;
            let mut glyph = 0u16;
            if face.GetGlyphIndices(&cp, 1, &mut glyph).is_err() || glyph == 0 {
                return;
            }
            let run = DWRITE_GLYPH_RUN {
                fontFace: ManuallyDrop::new(Some(face.clone())),
                fontEmSize: size,
                glyphCount: 1,
                glyphIndices: &glyph,
                glyphAdvances: std::ptr::null(),
                glyphOffsets: std::ptr::null(),
                isSideways: windows::Win32::Foundation::BOOL(0),
                bidiLevel: 0,
            };
            let (bx, by) = (x, y + size * 1.02);
            match f4.TranslateColorGlyphRun(D2D_POINT_2F { x: bx, y: by }, &run, None, DWRITE_GLYPH_IMAGE_FORMATS_COLR, DWRITE_MEASURING_MODE_NATURAL, None, 0) {
                Ok(en) => {
                    while en.MoveNext().map(|b| b.as_bool()).unwrap_or(false) {
                        if let Ok(cur) = en.GetCurrentRun() {
                            let cr = &*cur;
                            let b = &cr.Base;
                            let col = if b.paletteIndex == 0xFFFF { fg } else { D2D1_COLOR_F { r: b.runColor.r, g: b.runColor.g, b: b.runColor.b, a: b.runColor.a * fg.a } };
                            brush.SetColor(&col);
                            target.DrawGlyphRun(D2D_POINT_2F { x: b.baselineOriginX, y: b.baselineOriginY }, &b.glyphRun, brush, DWRITE_MEASURING_MODE_NATURAL);
                        }
                    }
                }
                Err(_) => {
                    brush.SetColor(&fg);
                    target.DrawGlyphRun(D2D_POINT_2F { x: bx, y: by }, &run, brush, DWRITE_MEASURING_MODE_NATURAL);
                }
            }
        }
    }

    // 绘制细线进度条
    pub fn draw_progress_bar(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        percent: f32,
        track_color: D2D1_COLOR_F,
        fill_color: D2D1_COLOR_F,
    ) -> Result<()> {
        let clamped = percent.clamp(0.0, 100.0) / 100.0;
        let radius = h / 2.0;

        // 底槽
        self.draw_card(x, y, w, h, radius, track_color, track_color, 0.0)?;

        // 填充条
        if clamped > 0.005 {
            let fill_w = (w * clamped).max(radius * 2.0);
            self.draw_card(x, y, fill_w, h, radius, fill_color, fill_color, 0.0)?;
        }
        if let Some(seg) = self.seg {
            let mut gx = x + 10.0;
            while gx < x + w {
                self.draw_card(gx, y - 0.5, 2.0, h + 1.0, 0.0, seg, seg, 0.0)?;
                gx += 12.0;
            }
        }
        Ok(())
    }

    // 绘制胶囊交互按钮 (Pill Button)
    pub fn draw_pill_button(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        text: &str,
        is_active: bool,
        is_hovered: bool,
        theme: &ThemeConfig,
    ) -> Result<()> {
        let radius = h / 2.0;
        let (bg, stroke, text_color) = if is_active {
            (theme.accent.to_d2d(), theme.accent.to_d2d(), theme.on_accent.to_d2d())
        } else if is_hovered {
            (theme.item_hover.to_d2d(), theme.accent.to_d2d(), theme.white.to_d2d())
        } else {
            (theme.item_bg.to_d2d(), theme.stroke.to_d2d(), theme.muted.to_d2d())
        };

        self.draw_card(x, y, w, h, radius, bg, stroke, 1.0)?;
        let font = self.font_num.clone();
        self.draw_text_aligned(
            text,
            x,
            y + (h - 14.0) / 2.0,
            w,
            14.0,
            &font,
            11.0,
            text_color,
            is_active,
            DWRITE_TEXT_ALIGNMENT_CENTER,
        )?;
        Ok(())
    }

    // 获取或创建字体格式 (LRU / 永久缓存，彻底消灭句柄泄漏)
    fn get_or_create_format(
        &mut self,
        font_name: &str,
        font_size: f32,
        bold: bool,
        alignment: DWRITE_TEXT_ALIGNMENT,
    ) -> Result<IDWriteTextFormat> {
        let wrap = self.wrap_next.is_some();
        let key = (
            font_name.to_string(),
            (font_size * 10.0) as u32,
            bold,
            alignment.0 as u32,
            wrap,
        );

        if let Some(format) = self.format_cache.get(&key) {
            return Ok(format.clone());
        }

        unsafe {
            let weight = if bold {
                DWRITE_FONT_WEIGHT_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            };

            let font_name_u16: Vec<u16> = font_name.encode_utf16().chain(Some(0)).collect();
            let locale_u16: Vec<u16> = "zh-CN".encode_utf16().chain(Some(0)).collect();

            let format = self.dwrite_factory.CreateTextFormat(
                PCWSTR::from_raw(font_name_u16.as_ptr()),
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size,
                PCWSTR::from_raw(locale_u16.as_ptr()),
            )?;

            format.SetTextAlignment(alignment)?;
            format.SetWordWrapping(if wrap { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP })?;

            self.format_cache.insert(key, format.clone());
            Ok(format)
        }
    }

    // DirectWrite 高清晰度文字排版
    pub fn draw_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        font_name: &str,
        font_size: f32,
        color: D2D1_COLOR_F,
        bold: bool,
    ) -> Result<()> {
        self.draw_text_aligned(
            text,
            x,
            y,
            w,
            h,
            font_name,
            font_size,
            color,
            bold,
            DWRITE_TEXT_ALIGNMENT_LEADING,
        )
    }

    // 进阶文字排版 (零句柄开销)
    fn draw_plain(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        font_name: &str,
        font_size: f32,
        color: D2D1_COLOR_F,
        bold: bool,
        alignment: DWRITE_TEXT_ALIGNMENT,
    ) -> Result<()> {
        let format = self.get_or_create_format(font_name, font_size, bold, alignment)?;

        let target = match &self.render_target {
            Some(t) => t,
            None => return Ok(()),
        };
        let brush = match &self.solid_brush {
            Some(b) => b,
            None => return Ok(()),
        };

        unsafe {
            brush.SetColor(&color);

            let (w, h, opts) = match self.wrap_next {
                Some((ww, hh)) => (ww, hh, D2D1_DRAW_TEXT_OPTIONS_CLIP),
                None => (w, h, D2D1_DRAW_TEXT_OPTIONS_NONE),
            };
            let rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };

            let text_u16: Vec<u16> = text.encode_utf16().collect();

            target.DrawText(
                &text_u16,
                &format,
                &rect,
                brush,
                opts,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
        Ok(())
    }
}

/// 以彩色 emoji 显示的字符(♪ ▶ ✓ ⏮ 等文字型符号不在此列)
fn is_color_emoji(c: char) -> bool {
    let u = c as u32;
    if (0x2660..=0x266F).contains(&u) {
        return false;
    }
    (0x1F000..=0x1FAFF).contains(&u) || (0x2600..=0x26FF).contains(&u) || matches!(u, 0x23F1 | 0x23F2 | 0x23F3 | 0x231A | 0x231B | 0x2705 | 0x2728 | 0x274C | 0x2B50 | 0x2B55)
}
