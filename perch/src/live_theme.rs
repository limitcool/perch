//! 读取 perch-core 写出的 theme.toml,作为唯一的主题来源(配色 + 样式包)。
//! 换主题时 core 重写该文件,HUD 通过修改时间发现并热重载,不再在 HUD 里重复维护色板。

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::theme::{ColorRgba, ThemeConfig};

pub fn theme_toml() -> &'static str {
    static P: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    P.get_or_init(|| perch_data::paths::theme_toml().to_string_lossy().to_string())
}
pub fn core_exe() -> &'static str {
    static P: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    P.get_or_init(|| perch_data::paths::res().join(r"bin\perch-core.exe").to_string_lossy().to_string())
}

#[derive(Debug, Clone)]
pub enum DecoKind {
    Line { x1: f32, y1: f32, x2: f32, y2: f32 },
    Rect { x: f32, y: f32, w: f32, h: f32 },
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32 },
}

/// 卡片装饰(坐标以 260 宽卡片为基准,右侧元素按实际卡宽平移)
#[derive(Debug, Clone)]
pub struct Deco {
    pub kind: DecoKind,
    pub stroke: Option<ColorRgba>,
    pub fill: Option<ColorRgba>,
    pub width: f32,
}

#[derive(Debug, Clone)]
pub struct ThemeStyle {
    pub name: String,
    pub font_main: String,
    pub font_num: String,
    pub radius: f32,
    pub frame_stroke: Option<ColorRgba>,
    pub frame_w: f32,
    pub bar_h: f32,
    pub seg: Option<ColorRgba>,
    pub shape2: Option<Deco>,
    pub corner: Option<Deco>,
    tok: HashMap<String, String>,
}

impl ThemeStyle {
    /// 卡片描边色:样式包指定的边框色,否则用主题 stroke
    pub fn frame_color(&self, fallback: &ColorRgba) -> D2D1_COLOR_F {
        self.frame_stroke.as_ref().unwrap_or(fallback).to_d2d()
    }

    pub fn t<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        match self.tok.get(key) {
            Some(v) if !v.is_empty() => v.as_str(),
            _ => default,
        }
    }
}

impl Default for ThemeStyle {
    fn default() -> Self {
        Self {
            name: String::new(),
            font_main: "Microsoft YaHei UI".into(),
            font_num: "Segoe UI Variable Display".into(),
            radius: 8.0,
            frame_stroke: None,
            frame_w: 1.0,
            bar_h: 4.0,
            seg: None,
            shape2: None,
            corner: None,
            tok: HashMap::new(),
        }
    }
}

fn nums(s: &str) -> Vec<f32> {
    s.split(',').filter_map(|p| p.trim().parse::<f32>().ok()).collect()
}

pub fn parse_color(s: &str) -> Option<ColorRgba> {
    let s = s.trim();
    if s.starts_with('#') {
        let hex = &s[1..];
        if hex.len() == 8 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            return Some(ColorRgba::new(r, g, b, a));
        } else if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(ColorRgba::new(r, g, b, 255));
        }
    }
    let v = nums(s);
    match v.len() {
        3 => Some(ColorRgba::new(v[0] as u8, v[1] as u8, v[2] as u8, 255)),
        4 => Some(ColorRgba::new(v[0] as u8, v[1] as u8, v[2] as u8, v[3] as u8)),
        _ => None,
    }
}

fn parse_deco(s: &str) -> Option<Deco> {
    let mut parts = s.split('|').map(|p| p.trim());
    let head = parts.next()?;
    let (name, args) = head.split_once(' ')?;
    let a = nums(args);
    let kind = match (name, a.len()) {
        ("Line", 4) => DecoKind::Line { x1: a[0], y1: a[1], x2: a[2], y2: a[3] },
        ("Rectangle", n) if n >= 4 => DecoKind::Rect { x: a[0], y: a[1], w: a[2], h: a[3] },
        ("Ellipse", n) if n >= 4 => DecoKind::Ellipse { cx: a[0], cy: a[1], rx: a[2], ry: a[3] },
        _ => return None,
    };
    let (mut stroke, mut fill, mut width) = (None, None, 1.0);
    for p in parts {
        if let Some(c) = p.strip_prefix("Stroke Color ") {
            stroke = parse_color(c);
        } else if let Some(c) = p.strip_prefix("Fill Color ") {
            fill = parse_color(c);
        } else if let Some(w) = p.strip_prefix("StrokeWidth ") {
            width = w.trim().parse().unwrap_or(1.0);
        }
    }
    Some(Deco { kind, stroke, fill, width })
}

fn first_family(s: &str) -> String {
    s.split(',').next().unwrap_or("").trim().to_string()
}

pub fn theme_mtime() -> Option<SystemTime> {
    let toml_p = theme_toml();
    fs::metadata(toml_p).and_then(|m| m.modified()).ok()
}

/// 解析 theme.toml;成功时返回(覆盖了配色的 ThemeConfig, 样式, 主题名)。
pub fn load(base: ThemeConfig) -> Option<(ThemeConfig, ThemeStyle, String)> {
    let toml_path = Path::new(theme_toml());
    let tt = perch_data::read::<perch_data::ThemeToml>(toml_path).ok()?;
    let name = tt.name.clone();
    let col = |s: &str, d: ColorRgba| parse_color(s).unwrap_or(d);
    let leak = |s: String, d: &'static str| -> &'static str {
        if s.is_empty() { d } else { Box::leak(s.into_boxed_str()) }
    };

    let mut cfg = base;
    cfg.accent = col(&tt.accent, cfg.accent);
    cfg.sub = col(&tt.sub, cfg.sub);
    cfg.bg = col(&tt.bg, cfg.bg);
    cfg.dock_bg = col(&tt.dock_bg, cfg.dock_bg);
    cfg.stroke = col(&tt.stroke, cfg.stroke);
    cfg.item_bg = col(&tt.item_bg, cfg.item_bg);
    cfg.item_hover = col(&tt.item_hover, cfg.item_hover);
    cfg.pill = col(&tt.pill_bg, cfg.pill);
    cfg.white = col(&tt.color_white, cfg.white);
    cfg.muted = col(&tt.color_muted, cfg.muted);
    cfg.on_accent = col(&tt.color_on_accent, cfg.on_accent);
    cfg.on_sub = col(&tt.color_on_sub, cfg.on_sub);
    cfg.field_name = leak(tt.field_name, cfg.field_name);
    cfg.unit_name = leak(tt.unit_name, cfg.unit_name);
    cfg.dock_title = leak(tt.dock_title, cfg.dock_title);

    let mut tok = HashMap::new();
    tok.insert("ThemeTitleTimer".into(), tt.title_timer);
    tok.insert("ThemeSubTimer".into(), tt.sub_timer);
    tok.insert("ThemeTitleEvents".into(), tt.title_events);
    tok.insert("ThemeSubEvents".into(), tt.sub_events);
    tok.insert("ThemeTitleTodo".into(), tt.title_todo);
    tok.insert("ThemeSubTodo".into(), tt.sub_todo);
    tok.insert("ThemeTitleMedia".into(), tt.title_media);
    tok.insert("ThemeSubMedia".into(), tt.sub_media);
    tok.insert("ThemeTitleWeather".into(), tt.title_weather);
    tok.insert("ThemeTitleWater".into(), tt.title_water);
    tok.insert("ThemeSubWater".into(), tt.sub_water);
    tok.insert("ThemeLblCpu".into(), tt.lbl_cpu);
    tok.insert("ThemeLblRam".into(), tt.lbl_ram);
    tok.insert("ThemeLblC".into(), tt.lbl_c);
    tok.insert("ThemeLblD".into(), tt.lbl_d);
    tok.insert("ThemeLblNet".into(), tt.lbl_net);
    tok.insert("ThemeFieldName".into(), cfg.field_name.to_string());
    tok.insert("ThemeUnitName".into(), cfg.unit_name.to_string());

    let style = ThemeStyle {
        name: name.clone(),
        font_main: {
            let f = first_family(&tt.font_main);
            if f.is_empty() { "Microsoft YaHei UI".into() } else { f }
        },
        font_num: {
            let f = first_family(&tt.font_num);
            if f.is_empty() { "Segoe UI Variable Display".into() } else { f }
        },
        radius: tt.radius,
        frame_stroke: parse_color(&tt.frame_stroke),
        frame_w: tt.frame_width,
        bar_h: tt.bar_h,
        seg: parse_color(&tt.seg_color).filter(|c| c.a > 0.0),
        shape2: parse_deco(&tt.shape2),
        corner: parse_deco(&tt.corner),
        tok,
    };
    Some((cfg, style, name))
}
