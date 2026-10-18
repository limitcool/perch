//! 天气映射规则: 关键词 -> 状态文字 / 预报文字 / 光晕色
//! 支持通过 <root>/weather_map.toml 外部覆盖

use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::OnceLock;

const BUILTIN_WEATHER_MAP: &str = include_str!("../builtin/weather_map.toml");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeatherDefaults {
    #[serde(default = "default_glow_fill")]
    pub glow_fill: String,
    #[serde(default = "default_glow_stroke")]
    pub glow_stroke: String,
    #[serde(default = "default_fallback_icon")]
    pub fallback_icon: String,
}

fn default_glow_fill() -> String {
    "#ThemeSub#".to_string()
}
fn default_glow_stroke() -> String {
    "#ThemeAccent#".to_string()
}
fn default_fallback_icon() -> String {
    "🌤️ ".to_string()
}

impl Default for WeatherDefaults {
    fn default() -> Self {
        Self {
            glow_fill: default_glow_fill(),
            glow_stroke: default_glow_stroke(),
            fallback_icon: default_fallback_icon(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlowRule {
    pub keywords: Vec<String>,
    pub fill: String,
    pub stroke: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextRule {
    pub keywords: Vec<String>,
    pub text: String,
    /// 英文界面下使用的文字;留空则沿用 text
    #[serde(default)]
    pub text_en: String,
}

impl TextRule {
    fn localized(&self) -> String {
        if !self.text_en.is_empty() && crate::i18n::current_language() == "en" {
            self.text_en.clone()
        } else {
            self.text.clone()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WeatherMap {
    #[serde(default)]
    pub defaults: WeatherDefaults,
    #[serde(default)]
    pub glow_rules: Vec<GlowRule>,
    #[serde(default)]
    pub status_rules: Vec<TextRule>,
    #[serde(default)]
    pub forecast_rules: Vec<TextRule>,
}

impl WeatherMap {
    pub fn parse(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }

    pub fn builtin() -> Self {
        Self::parse(BUILTIN_WEATHER_MAP).unwrap_or_default()
    }

    pub fn load() -> Self {
        let external_path = crate::paths::root().join("weather_map.toml");
        if external_path.exists() {
            if let Ok(content) = fs::read_to_string(&external_path) {
                if let Ok(map) = Self::parse(&content) {
                    return map;
                }
            }
        }
        Self::builtin()
    }

    pub fn get_glow(&self, desc: &str) -> (String, String) {
        let lower = desc.to_lowercase();
        for rule in &self.glow_rules {
            if rule.keywords.iter().any(|k| lower.contains(&k.to_lowercase())) {
                return (rule.fill.clone(), rule.stroke.clone());
            }
        }
        (self.defaults.glow_fill.clone(), self.defaults.glow_stroke.clone())
    }

    pub fn format_status(&self, desc: &str) -> String {
        let lower = desc.to_lowercase();
        for rule in &self.status_rules {
            if rule.keywords.iter().any(|k| lower.contains(&k.to_lowercase())) {
                return rule.localized();
            }
        }
        format!("{}{}", self.defaults.fallback_icon, desc)
    }

    pub fn format_forecast(&self, desc: &str) -> String {
        let lower = desc.to_lowercase();
        for rule in &self.forecast_rules {
            if rule.keywords.iter().any(|k| lower.contains(&k.to_lowercase())) {
                return rule.localized();
            }
        }
        format!("{}{}", self.defaults.fallback_icon, desc)
    }
}

static INSTANCE: OnceLock<WeatherMap> = OnceLock::new();

pub fn global_map() -> &'static WeatherMap {
    INSTANCE.get_or_init(WeatherMap::load)
}

pub fn get_weather_glow(desc: &str) -> (String, String) {
    global_map().get_glow(desc)
}

pub fn format_weather_emoji(desc: &str) -> String {
    global_map().format_status(desc)
}

pub fn format_forecast_emoji(desc: &str) -> String {
    global_map().format_forecast(desc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_weather_map() {
        crate::i18n::set_language("zh-CN");
        let map = WeatherMap::builtin();
        assert_eq!(map.get_glow("Sunny"), ("#ThemeSub#".into(), "#ThemeAccent#".into()));
        assert_eq!(map.get_glow("Light drizzle"), ("56,189,248,220".into(), "14,165,233,255".into()));
        assert_eq!(map.format_status("Sunny"), "☀️ 晴朗");
        assert_eq!(map.format_forecast("Sunny"), "☀️ 晴");
    }

    #[test]
    fn test_baseline_compatibility() {
        // 可选的回归基线:设置 PERCH_BASELINE_DIR 指向含 weather_cases.json 的目录才会运行
        let dir = match std::env::var("PERCH_BASELINE_DIR") {
            Ok(d) if !d.is_empty() => d,
            _ => return,
        };
        let baseline_path = std::path::Path::new(&dir).join("weather_cases.json");
        if !baseline_path.exists() {
            return;
        }
        let content = std::fs::read_to_string(baseline_path).unwrap();
        let cases: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        let map = WeatherMap::builtin();
        for c in cases {
            let raw = c["raw"].as_str().unwrap();
            let expected_status = c["status"].as_str().unwrap();
            let expected_forecast = c["forecast"].as_str().unwrap();
            let expected_glow_fill = c["glow_fill"].as_str().unwrap();
            let expected_glow_stroke = c["glow_stroke"].as_str().unwrap();

            assert_eq!(map.format_status(raw), expected_status, "status mismatch for {}", raw);
            assert_eq!(map.format_forecast(raw), expected_forecast, "forecast mismatch for {}", raw);
            let (fill, stroke) = map.get_glow(raw);
            assert_eq!(fill, expected_glow_fill, "glow_fill mismatch for {}", raw);
            assert_eq!(stroke, expected_glow_stroke, "glow_stroke mismatch for {}", raw);
        }
    }
}
