use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

#[derive(Debug, Clone, Copy)]
pub struct ColorRgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl ColorRgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    pub fn to_d2d(&self) -> D2D1_COLOR_F {
        D2D1_COLOR_F {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
    }

    pub fn with_alpha_color(&self, alpha_255: u8) -> ColorRgba {
        ColorRgba { r: self.r, g: self.g, b: self.b, a: alpha_255 as f32 / 255.0 }
    }

    pub fn with_alpha(&self, alpha_255: u8) -> D2D1_COLOR_F {
        D2D1_COLOR_F {
            r: self.r,
            g: self.g,
            b: self.b,
            a: alpha_255 as f32 / 255.0,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ThemeConfig {
    pub name: &'static str,
    pub character_title: &'static str,
    pub accent: ColorRgba,
    pub sub: ColorRgba,
    pub bg: ColorRgba,
    pub dock_bg: ColorRgba,
    pub stroke: ColorRgba,
    pub item_bg: ColorRgba,
    pub item_hover: ColorRgba,
    pub pill: ColorRgba,
    pub white: ColorRgba,
    pub muted: ColorRgba,
    pub on_accent: ColorRgba,
    pub on_sub: ColorRgba,
    pub field_name: &'static str,
    pub unit_name: &'static str,
    pub dock_title: &'static str,
}

/// 基础配色(theme.toml 缺失/损坏时的兜底)。真正的主题内容来自主题包(perch-data::theme_pack),
/// 由 perch-core 写成 theme.toml,HUD 在 live_theme 里叠加到这份基础值之上。
pub fn get_theme(_name: &str) -> ThemeConfig {
    ThemeConfig {
        name: "default",
        character_title: "💠 PERCH // SYSTEM",
        accent: ColorRgba::new(56, 189, 248, 255),
        sub: ColorRgba::new(148, 163, 184, 255),
        bg: ColorRgba::new(16, 20, 28, 212),
        dock_bg: ColorRgba::new(12, 16, 24, 210),
        stroke: ColorRgba::new(56, 189, 248, 70),
        item_bg: ColorRgba::new(0, 0, 0, 65),
        item_hover: ColorRgba::new(56, 189, 248, 140),
        pill: ColorRgba::new(56, 189, 248, 220),
        white: ColorRgba::new(248, 250, 252, 255),
        muted: ColorRgba::new(190, 205, 224, 240),
        on_accent: ColorRgba::new(10, 15, 26, 255),
        on_sub: ColorRgba::new(10, 12, 20, 255),
        field_name: "SYSTEM",
        unit_name: "CORE",
        dock_title: "💠 极简科技应用坞",
    }
}
