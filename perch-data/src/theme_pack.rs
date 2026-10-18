//! perch-data::theme_pack: 数据驱动的主题包(Theme Pack)解析、内置加载与动态管理

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

use crate::paths;
use crate::ThemeToml;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemeMeta {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub preview: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemeColors {
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub sub: String,
    #[serde(default)]
    pub bg: String,
    #[serde(default)]
    pub dock_bg: String,
    #[serde(default)]
    pub stroke: String,
    #[serde(default)]
    pub item_bg: String,
    #[serde(default)]
    pub item_hover: String,
    #[serde(default)]
    pub pill_bg: String,
    #[serde(default)]
    pub white: String,
    #[serde(default)]
    pub muted: String,
    #[serde(default)]
    pub on_accent: String,
    #[serde(default)]
    pub on_sub: String,
    #[serde(default = "crate::default_danger")]
    pub danger: String,
    #[serde(default = "crate::default_warn")]
    pub warn: String,
    #[serde(default = "crate::default_ok")]
    pub ok: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemeStyle {
    #[serde(default)]
    pub font_main: String,
    #[serde(default)]
    pub font_num: String,
    #[serde(default)]
    pub radius: f32,
    #[serde(default)]
    pub frame_stroke: String,
    #[serde(default)]
    pub frame_width: f32,
    #[serde(default)]
    pub shape2: String,
    #[serde(default)]
    pub corner: String,
    #[serde(default)]
    pub bar_h: f32,
    #[serde(default)]
    pub seg_color: String,
    #[serde(default = "default_chrome")]
    pub chrome: String, // "none" | "tactical"
    /// 战术顶栏左侧标题 / 右侧副标题 / 三个状态徽章(chrome = "tactical" 时使用)
    #[serde(default = "default_chrome_title")]
    pub chrome_title: String,
    #[serde(default = "default_chrome_sub")]
    pub chrome_sub: String,
    #[serde(default = "default_chrome_badges")]
    pub chrome_badges: Vec<String>,
}

fn default_chrome_title() -> String {
    "⚡ OPS".to_string()
}

fn default_chrome_sub() -> String {
    "OPERATOR // 02".to_string()
}

fn default_chrome_badges() -> Vec<String> {
    vec!["1.ALPHA:OK".to_string(), "2.BETA:OK".to_string(), "3.GAMMA:AGREE".to_string()]
}

fn default_chrome() -> String {
    "none".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemeLabels {
    #[serde(default)]
    pub dock_title: String,
    #[serde(default)]
    pub field_name: String,
    #[serde(default)]
    pub unit_name: String,
    #[serde(default)]
    pub title_timer: String,
    #[serde(default)]
    pub sub_timer: String,
    #[serde(default)]
    pub title_events: String,
    #[serde(default)]
    pub sub_events: String,
    #[serde(default)]
    pub title_todo: String,
    #[serde(default)]
    pub sub_todo: String,
    #[serde(default)]
    pub title_media: String,
    #[serde(default)]
    pub sub_media: String,
    #[serde(default)]
    pub title_weather: String,
    #[serde(default)]
    pub title_water: String,
    #[serde(default)]
    pub sub_water: String,
    #[serde(default)]
    pub lbl_cpu: String,
    #[serde(default)]
    pub lbl_ram: String,
    #[serde(default)]
    pub lbl_c: String,
    #[serde(default)]
    pub lbl_d: String,
    #[serde(default)]
    pub lbl_net: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemeFx {
    #[serde(default = "default_fx_kind")]
    pub kind: String, // "dot" | "petal" | "diamond" | "heart" | "streak"
    #[serde(default)]
    pub rgb: Vec<u8>,
    #[serde(default)]
    pub size_min: f32,
    #[serde(default)]
    pub size_max: f32,
    #[serde(default)]
    pub speed_min: f32,
    #[serde(default)]
    pub speed_max: f32,
    #[serde(default)]
    pub up: bool,
    #[serde(default)]
    pub sway: f32,
    #[serde(default)]
    pub alpha_min: f32,
    #[serde(default)]
    pub alpha_max: f32,
}

fn default_fx_kind() -> String {
    "dot".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemeWallpaper {
    #[serde(default)]
    pub prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemeMessages {
    #[serde(default)]
    pub water: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemeQuotes {
    #[serde(default)]
    pub speaker: String,
    #[serde(default)]
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemePack {
    #[serde(default = "crate::default_v")]
    pub v: u32,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub order: u32,
    /// 声明自己取代某个内置主题(换装面板里隐藏被取代者);例如私有主题包 `replaces = "aurora"`
    #[serde(default)]
    pub replaces: String,
    #[serde(default)]
    pub meta: ThemeMeta,
    #[serde(default)]
    pub colors: ThemeColors,
    #[serde(default)]
    pub style: ThemeStyle,
    #[serde(default)]
    pub labels: ThemeLabels,
    /// 英文界面下的标签(可只填需要覆盖的字段)
    #[serde(default)]
    pub labels_en: ThemeLabels,
    #[serde(default)]
    pub fx: ThemeFx,
    #[serde(default)]
    pub wallpaper: ThemeWallpaper,
    #[serde(default)]
    pub messages: ThemeMessages,
    #[serde(default)]
    pub quotes: ThemeQuotes,
}

impl ThemePack {
    /// 转换为向后兼容的 ThemeToml (供写入 theme.toml 和 HUD 渲染使用)
    /// 当前界面语言下生效的标签:英文界面下,labels_en 里填了的字段覆盖 labels
    pub fn effective_labels(&self) -> ThemeLabels {
        let mut l = self.labels.clone();
        if crate::i18n::current_language() == "en" {
            let en = &self.labels_en;
            let pick = |dst: &mut String, src: &String| {
                if !src.is_empty() {
                    *dst = src.clone();
                }
            };
            pick(&mut l.dock_title, &en.dock_title);
            pick(&mut l.field_name, &en.field_name);
            pick(&mut l.unit_name, &en.unit_name);
            pick(&mut l.title_timer, &en.title_timer);
            pick(&mut l.sub_timer, &en.sub_timer);
            pick(&mut l.title_events, &en.title_events);
            pick(&mut l.sub_events, &en.sub_events);
            pick(&mut l.title_todo, &en.title_todo);
            pick(&mut l.sub_todo, &en.sub_todo);
            pick(&mut l.title_media, &en.title_media);
            pick(&mut l.sub_media, &en.sub_media);
            pick(&mut l.title_weather, &en.title_weather);
            pick(&mut l.title_water, &en.title_water);
            pick(&mut l.sub_water, &en.sub_water);
            pick(&mut l.lbl_cpu, &en.lbl_cpu);
            pick(&mut l.lbl_ram, &en.lbl_ram);
            pick(&mut l.lbl_c, &en.lbl_c);
            pick(&mut l.lbl_d, &en.lbl_d);
            pick(&mut l.lbl_net, &en.lbl_net);
        }
        l
    }

    pub fn to_theme_toml(&self) -> ThemeToml {
        let l = self.effective_labels();
        ThemeToml {
            v: 1,
            name: self.id.clone(),
            accent: self.colors.accent.clone(),
            sub: self.colors.sub.clone(),
            bg: self.colors.bg.clone(),
            dock_bg: self.colors.dock_bg.clone(),
            stroke: self.colors.stroke.clone(),
            item_bg: self.colors.item_bg.clone(),
            item_hover: self.colors.item_hover.clone(),
            pill_bg: self.colors.pill_bg.clone(),
            dock_title: l.dock_title.clone(),
            color_white: self.colors.white.clone(),
            color_muted: self.colors.muted.clone(),
            color_on_accent: self.colors.on_accent.clone(),
            color_on_sub: self.colors.on_sub.clone(),
            danger: self.colors.danger.clone(),
            warn: self.colors.warn.clone(),
            ok: self.colors.ok.clone(),
            field_name: l.field_name.clone(),
            unit_name: l.unit_name.clone(),
            font_main: self.style.font_main.clone(),
            font_num: self.style.font_num.clone(),
            radius: self.style.radius,
            frame_stroke: self.style.frame_stroke.clone(),
            frame_width: self.style.frame_width,
            shape2: self.style.shape2.clone(),
            corner: self.style.corner.clone(),
            bar_h: self.style.bar_h,
            seg_color: self.style.seg_color.clone(),
            title_timer: l.title_timer.clone(),
            sub_timer: l.sub_timer.clone(),
            title_events: l.title_events.clone(),
            sub_events: l.sub_events.clone(),
            title_todo: l.title_todo.clone(),
            sub_todo: l.sub_todo.clone(),
            title_media: l.title_media.clone(),
            sub_media: l.sub_media.clone(),
            title_weather: l.title_weather.clone(),
            title_water: l.title_water.clone(),
            sub_water: l.sub_water.clone(),
            lbl_cpu: l.lbl_cpu.clone(),
            lbl_ram: l.lbl_ram.clone(),
            lbl_c: l.lbl_c.clone(),
            lbl_d: l.lbl_d.clone(),
            lbl_net: l.lbl_net.clone(),
        }
    }
}

/// 内置主题嵌入
pub fn builtin(id: &str) -> Option<ThemePack> {
    let s = match id {
        "ember" => include_str!("../builtin/ember.toml"),
        "aurora" => include_str!("../builtin/aurora.toml"),
        "orchid" => include_str!("../builtin/orchid.toml"),
        "crimson" => include_str!("../builtin/crimson.toml"),
        "neon" => include_str!("../builtin/neon.toml"),
        "default" => include_str!("../builtin/default.toml"),
        "lavender" => include_str!("../builtin/lavender.toml"),
        "rose" => include_str!("../builtin/rose.toml"),
        _ => return None,
    };
    toml::from_str::<ThemePack>(s.trim_start_matches('\u{feff}')).ok()
}

/// 所有内置主题 ID 列表
pub const BUILTIN_IDS: [&str; 8] = [
    "ember", "aurora", "orchid", "crimson", "neon", "default", "lavender", "rose",
];

/// 加载单个主题包:
/// 1. 优先从 <root>/themes/<id>.toml 加载
/// 2. 其次回退内置 builtin(id)
/// 3. 若仍无，回退 builtin("default")
/// 4. 缺省字段用 default 主题补齐
pub fn load(id: &str) -> ThemePack {
    let fallback = builtin("default").unwrap_or_default();

    // 1. 尝试从用户/外部主题目录读取
    let themes_dir = paths::themes_dir();
    let ext_path = themes_dir.join(format!("{}.toml", id));
    if ext_path.is_file() {
        if let Ok(c) = fs::read_to_string(&ext_path) {
            if let Ok(mut pack) = toml::from_str::<ThemePack>(c.trim_start_matches('\u{feff}')) {
                if pack.v >= 2 {
                    if pack.id.is_empty() {
                        pack.id = id.to_string();
                    }
                    fill_missing(&mut pack, &fallback);
                    return pack;
                }
            }
        }
    }

    // 2. 尝试从内置读取
    if let Some(mut pack) = builtin(id) {
        fill_missing(&mut pack, &fallback);
        return pack;
    }

    fallback
}

/// 补齐缺失的核心字段
fn fill_missing(pack: &mut ThemePack, def: &ThemePack) {
    if pack.meta.name.is_empty() {
        pack.meta.name = pack.id.clone();
    }
    if pack.meta.preview.is_empty() {
        pack.meta.preview = vec![128, 128, 128];
    }
    if pack.colors.accent.is_empty() {
        pack.colors = def.colors.clone();
    }
    if pack.colors.danger.is_empty() {
        pack.colors.danger = def.colors.danger.clone();
    }
    if pack.colors.warn.is_empty() {
        pack.colors.warn = def.colors.warn.clone();
    }
    if pack.colors.ok.is_empty() {
        pack.colors.ok = def.colors.ok.clone();
    }
    if pack.style.font_main.is_empty() {
        pack.style.font_main = def.style.font_main.clone();
    }
    if pack.style.font_num.is_empty() {
        pack.style.font_num = def.style.font_num.clone();
    }
    if pack.wallpaper.prefix.is_empty() {
        pack.wallpaper.prefix = pack.id.clone();
    }
    if pack.messages.water.is_empty() {
        pack.messages.water = "该喝水了。".to_string();
    }
    if pack.quotes.speaker.is_empty() {
        pack.quotes.speaker = pack.meta.name.clone();
    }
    if pack.quotes.lines.is_empty() {
        pack.quotes.lines = vec!["系统状态就绪。".to_string()];
    }
}

/// 扫描并获取所有可用主题列表 (内置 ∪ 外部 themes/*.toml), 并按 order 升序排序
pub fn list() -> Vec<ThemePack> {
    let mut map: HashMap<String, ThemePack> = HashMap::new();

    // 1. 先加入内置主题
    for id in BUILTIN_IDS {
        if let Some(pack) = builtin(id) {
            map.insert(id.to_string(), pack);
        }
    }

    // 2. 扫描外部 themes 目录, 若有同名外部主题则覆盖内置
    let themes_dir = paths::themes_dir();
    if let Ok(entries) = fs::read_dir(&themes_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().map_or(false, |ext| ext == "toml") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(content) = fs::read_to_string(&p) {
                        match toml::from_str::<ThemePack>(content.trim_start_matches('\u{feff}')) {
                            Ok(mut pack) => {
                                if pack.v >= 2 {
                                    if pack.id.is_empty() {
                                        pack.id = stem.to_string();
                                    }
                                    map.insert(pack.id.clone(), pack);
                                }
                            }
                            Err(e) => {
                                eprintln!("[THEME_PACK] Failed to parse {}: {}", p.display(), e);
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. 主题包声明 replaces 的内置主题不再出现在列表里
    let replaced: Vec<String> = map.values().filter(|p| !p.replaces.is_empty()).map(|p| p.replaces.clone()).collect();
    for r in replaced {
        if let Some(p) = map.get(&r) {
            if p.replaces.is_empty() && BUILTIN_IDS.contains(&r.as_str()) {
                map.remove(&r);
            }
        }
    }

    let def = builtin("default").unwrap_or_default();
    let mut result: Vec<ThemePack> = map
        .into_values()
        .map(|mut p| {
            fill_missing(&mut p, &def);
            p
        })
        .collect();

    // 按 order 升序排序，order 相同按 id 排序
    result.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.id.cmp(&b.id)));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_themes_load() {
        for id in BUILTIN_IDS {
            let pack = builtin(id).unwrap_or_else(|| panic!("builtin {} missing or unparsable", id));
            assert_eq!(pack.id, id);
            assert!(!pack.quotes.lines.is_empty(), "quotes lines empty for {}", id);
            assert!(pack.quotes.lines.len() >= 5, "too few quotes for {}", id);
            assert!(!pack.messages.water.is_empty(), "water message empty for {}", id);
        }
    }

    #[test]
    fn test_unknown_id_falls_back_to_default() {
        assert_eq!(load("no_such_theme_xyz").id, "default");
    }

    #[test]
    fn test_replaces_hides_builtin_in_list() {
        // replaces 只作用于内置 id,且被取代者不出现在列表里(这里只验证内置自身互不取代)
        for id in BUILTIN_IDS {
            assert!(builtin(id).unwrap().replaces.is_empty(), "{} must not replace anything", id);
        }
    }

    #[test]
    fn test_to_theme_toml_conversion() {
        let aurora = load("aurora");
        let toml_obj = aurora.to_theme_toml();
        assert_eq!(toml_obj.name, "aurora");
        assert_eq!(toml_obj.accent, "#4285F4FF");
        assert_eq!(toml_obj.sub, "#FACC15FF");
    }

    #[test]
    fn test_fill_missing_and_fallback() {
        let non_exist = load("definitely_not_exist_theme_123");
        assert_eq!(non_exist.id, "default");
    }
}
