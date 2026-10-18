//! perch-core::weather: 纯 Rust 极速多日天气与降雨量解析引擎 (生动 Emoji 与全要素高光)

use std::fs;
use crate::util::{parse_ampm_minutes, user_config};

pub fn get_weather_glow(desc: &str) -> (String, String) {
    perch_data::weather_map::get_weather_glow(desc)
}

pub fn format_weather_emoji(desc: &str) -> String {
    perch_data::weather_map::format_weather_emoji(desc)
}

pub fn format_forecast_emoji(desc: &str) -> String {
    perch_data::weather_map::format_forecast_emoji(desc)
}


pub fn update_weather() -> Result<(), Box<dyn std::error::Error>> {
    let mut temp = "28".to_string();
    let mut raw_status = "Sunny".to_string();
    let mut high = "32".to_string();
    let mut low = "24".to_string();
    let mut rain_mm = "0.0".to_string();
    let mut rain_chance = "0%".to_string();
    let mut humidity = "62%".to_string();
    let mut wind = "12 km/h".to_string();

    let d1_date = perch_data::t("core2.wx_tomorrow");
    let mut d1_status = perch_data::weather_map::format_forecast_emoji("Sunny");
    let mut d1_range = "24°~33°".to_string();

    let d2_date = perch_data::t("core2.wx_day2");
    let mut d2_status = perch_data::weather_map::format_forecast_emoji("Partly cloudy");
    let mut d2_range = "23°~31°".to_string();

    let d3_date = perch_data::t("core2.wx_day3");
    let d3_status = perch_data::weather_map::format_forecast_emoji("Light rain");
    let d3_range = "22°~29°".to_string();

    // 城市可配置:data\weather_cfg.json {"city":"Hangzhou"};未配置时 wttr.in 按 IP 自动定位
    let city = serde_json::from_str::<serde_json::Value>(&fs::read_to_string(perch_data::paths::weather_cfg_json()).unwrap_or_default())
        .ok()
        .and_then(|v| v["city"].as_str().map(|s| s.trim().to_string()))
        .or_else(|| user_config().get("weather").and_then(|w| w.get("city")).and_then(|c| c.as_str()).map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty() && s != "auto")
        .unwrap_or_default();
    let city_enc: String = city
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' { (b as char).to_string() } else { format!("%{:02X}", b) })
        .collect();
    let weather_url = format!("https://wttr.in/{}?format=j1", city_enc);
    if let Ok(resp) = ureq::get(&weather_url).timeout(std::time::Duration::from_secs(3)).call() {
        if let Ok(json_str) = resp.into_string() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(cur) = v.get("current_condition").and_then(|c| c.get(0)) {
                    if let Some(t) = cur.get("temp_C").and_then(|t| t.as_str()) {
                        temp = t.to_string();
                    }
                    if let Some(desc) = cur.get("weatherDesc").and_then(|d| d.get(0)).and_then(|d| d.get("value")).and_then(|d| d.as_str()) {
                        raw_status = desc.to_string();
                    }
                    if let Some(p) = cur.get("precipMM").and_then(|p| p.as_str()) {
                        rain_mm = p.to_string();
                    }
                    if let Some(h) = cur.get("humidity").and_then(|h| h.as_str()) {
                        humidity = format!("{}%", h);
                    }
                    if let Some(w) = cur.get("windspeedKmph").and_then(|w| w.as_str()) {
                        wind = format!("{} km/h", w);
                    }
                }

                if let Some(astro) = v
                    .get("weather")
                    .and_then(|w| w.get(0))
                    .and_then(|d| d.get("astronomy"))
                    .and_then(|a| a.get(0))
                {
                    let rise = astro.get("sunrise").and_then(|x| x.as_str()).and_then(parse_ampm_minutes);
                    let set = astro.get("sunset").and_then(|x| x.as_str()).and_then(parse_ampm_minutes);
                    if let (Some(r), Some(sn)) = (rise, set) {
                        let _ = fs::write(
                            perch_data::paths::sun_times_txt(),
                            format!("{} {}", r, sn),
                        );
                    }
                }

                if let Some(days) = v.get("weather").and_then(|w| w.as_array()) {
                    if !days.is_empty() {
                        let today = &days[0];
                        if let Some(maxt) = today.get("maxtempC").and_then(|m| m.as_str()) {
                            high = maxt.to_string();
                        }
                        if let Some(mint) = today.get("mintempC").and_then(|m| m.as_str()) {
                            low = mint.to_string();
                        }
                        if let Some(hourly) = today.get("hourly").and_then(|h| h.as_array()).and_then(|h| h.get(0)) {
                            if let Some(chance) = hourly.get("chanceofrain").and_then(|c| c.as_str()) {
                                rain_chance = format!("{}%", chance);
                            }
                        }
                    }
                    if days.len() > 1 {
                        let d1 = &days[1];
                        let d1_max = d1.get("maxtempC").and_then(|m| m.as_str()).unwrap_or("32");
                        let d1_min = d1.get("mintempC").and_then(|m| m.as_str()).unwrap_or("24");
                        d1_range = format!("{}°~{}°", d1_min, d1_max);
                        if let Some(desc) = d1.get("hourly").and_then(|h| h.as_array()).and_then(|h| h.get(4)).and_then(|h| h.get("weatherDesc")).and_then(|d| d.get(0)).and_then(|d| d.get("value")).and_then(|d| d.as_str()) {
                            d1_status = format_forecast_emoji(desc);
                        }
                    }
                    if days.len() > 2 {
                        let d2 = &days[2];
                        let d2_max = d2.get("maxtempC").and_then(|m| m.as_str()).unwrap_or("31");
                        let d2_min = d2.get("mintempC").and_then(|m| m.as_str()).unwrap_or("23");
                        d2_range = format!("{}°~{}°", d2_min, d2_max);
                        if let Some(desc) = d2.get("hourly").and_then(|h| h.as_array()).and_then(|h| h.get(4)).and_then(|h| h.get("weatherDesc")).and_then(|d| d.get(0)).and_then(|d| d.get("value")).and_then(|d| d.as_str()) {
                            d2_status = format_forecast_emoji(desc);
                        }
                    }
                }
            }
        }
    }

    let status = format_weather_emoji(&raw_status);
    let (glow_fill, glow_stroke) = get_weather_glow(&raw_status);

    let w = perch_data::Weather {
        v: 1,
        temp: temp.parse::<i32>().unwrap_or(0),
        status: status.clone(),
        high: high.parse::<i32>().unwrap_or(0),
        low: low.parse::<i32>().unwrap_or(0),
        rain_mm: rain_mm.parse::<f64>().unwrap_or(0.0),
        rain_chance: rain_chance.clone(),
        humidity: humidity.clone(),
        wind: wind.clone(),
        glow_fill: glow_fill.to_string(),
        glow_stroke: glow_stroke.to_string(),
        d1: perch_data::DailyForecast {
            date: d1_date,
            status: d1_status,
            range: d1_range,
        },
        d2: perch_data::DailyForecast {
            date: d2_date,
            status: d2_status,
            range: d2_range,
        },
        d3: perch_data::DailyForecast {
            date: d3_date,
            status: d3_status,
            range: d3_range,
        },
    };
    let _ = perch_data::write_atomic(perch_data::paths::weather_json(), &w);
    Ok(())
}
