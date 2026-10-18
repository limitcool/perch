//! perch-core::battery: 笔记本/外设/鼠标电池电量与电源驱动

use std::fs;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use crate::util::user_config;

struct MouseCfg {
    name: String,
    vendor_id: u16,
    product_id: u16,
    usage_page: u16,
    usage: u16,
    battery_byte: usize,
}

fn mouse_cfg() -> Option<MouseCfg> {
    let cfg = user_config();
    let m = cfg.get("mouse")?.as_table()?;
    let int = |k: &str, d: i64| m.get(k).and_then(|v| v.as_integer()).unwrap_or(d);
    let vendor_id = int("vendor_id", 0);
    let product_id = int("product_id", 0);
    if vendor_id <= 0 || product_id <= 0 {
        return None;
    }
    Some(MouseCfg {
        name: m.get("name").and_then(|v| v.as_str()).unwrap_or("Mouse").to_string(),
        vendor_id: vendor_id as u16,
        product_id: product_id as u16,
        usage_page: int("usage_page", 0xFF00) as u16,
        usage: int("usage", 2) as u16,
        battery_byte: int("battery_byte", 6).clamp(0, 63) as usize,
    })
}

fn read_mouse_battery(mc: &MouseCfg) -> Option<u8> {
    let (vendor_id, product_id) = (mc.vendor_id, mc.product_id);

    let api = match hidapi::HidApi::new() {
        Ok(a) => a,
        Err(_) => return None,
    };

    let mut candidates: Vec<_> = api
        .device_list()
        .filter(|d| {
            d.vendor_id() == vendor_id
                && d.product_id() == product_id
                && d.usage_page() == mc.usage_page
                && d.usage() == mc.usage
        })
        .collect();

    candidates.sort_by_key(|d| {
        !d.path()
            .to_string_lossy()
            .to_lowercase()
            .contains("col05")
    });

    for device_info in candidates {
        if let Ok(device) = device_info.open_device(&api) {
            for _ in 0..15 {
                let mut buf = [0u8; 64];
                if let Ok(size) = device.read_timeout(&mut buf, 80) {
                    if size > mc.battery_byte && buf.iter().any(|&b| b != 0) {
                        let level = buf[mc.battery_byte];
                        if (1..=100).contains(&level) {
                            return Some(level);
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn update_battery() -> Result<(), Box<dyn std::error::Error>> {
    let cache_file = perch_data::paths::last_battery_txt();

    // 配置了 [mouse] 时优先读取无线鼠标真实电量
    let mc = mouse_cfg();
    let mouse_name = mc.as_ref().map(|m| m.name.clone()).unwrap_or_default();
    let mut mouse_bat = mc.as_ref().and_then(read_mouse_battery);

    // 如果未立即读到(鼠标轻度休眠)，读取上次缓存的有效电量，避免电量跳变
    if mouse_bat.is_none() {
        if let Ok(content) = fs::read_to_string(&cache_file) {
            if let Ok(val) = content.trim().parse::<u8>() {
                if (1..=100).contains(&val) {
                    mouse_bat = Some(val);
                }
            }
        }
    } else if let Some(val) = mouse_bat {
        let _ = fs::write(&cache_file, val.to_string());
    }

    let low_thresh = user_config()
        .get("thresholds")
        .and_then(|t| t.get("battery_low"))
        .and_then(|v| v.as_integer())
        .unwrap_or(20) as u8;
    let warn_thresh = user_config()
        .get("thresholds")
        .and_then(|t| t.get("battery_warn"))
        .and_then(|v| v.as_integer())
        .unwrap_or(40) as u8;

    let (danger_color, warn_color) = if let Ok(c) = fs::read_to_string(perch_data::paths::theme_toml()) {
        if let Ok(t) = toml::from_str::<perch_data::ThemeToml>(&c) {
            (t.danger, t.warn)
        } else {
            (perch_data::default_danger(), perch_data::default_warn())
        }
    } else {
        (perch_data::default_danger(), perch_data::default_warn())
    };

    let (icon, percent_str, status_str, color_str) = match mouse_bat {
        Some(val) => {
            let color = if val <= low_thresh {
                &danger_color
            } else if val <= warn_thresh {
                &warn_color
            } else {
                "255,255,255,255" // 充足纯白
            };
            (
                "🖱️",
                format!("{}%", val),
                format!("{}: {}%", mouse_name, val),
                color,
            )
        }
        None => {
            // 如果连不上鼠标，读取笔记本系统电源作为兜底
            unsafe {
                let mut sps = SYSTEM_POWER_STATUS::default();
                if GetSystemPowerStatus(&mut sps).is_ok() && sps.BatteryLifePercent <= 100 {
                    let p = sps.BatteryLifePercent;
                    let st = perch_data::t(if sps.ACLineStatus == 1 { "core2.charging" } else { "core2.on_battery" });
                    ("⚡", format!("{}%", p), perch_data::t_fmt("core2.sys_power", &[("p", &p.to_string()), ("st", &st)]), "255,255,255,255")
                } else {
                    ("🖱️", "--%".to_string(), (if mouse_name.is_empty() { perch_data::t("core2.mouse_unset") } else { perch_data::t_fmt("core2.mouse_asleep", &[("name", &mouse_name)]) }), "160,174,192,200")
                }
            }
        }
    };

    let bat = perch_data::Battery {
        v: 1,
        mouse_icon: icon.to_string(),
        mouse_text: percent_str,
        mouse_status: status_str,
        mouse_color: color_str.to_string(),
    };
    let _ = perch_data::write_atomic(perch_data::paths::battery_toml(), &bat);
    Ok(())
}
