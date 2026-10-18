//! perch-core::battery: 电量卡片的数据来源(笔记本电池 / 任意无线外设)
//!
//! config.toml 里可以写任意多个来源,按顺序取第一个读到数值的:
//!
//! ```toml
//! [[battery]]
//! kind = "hid"              # 读无线外设(鼠标/键盘…)通过 HID 上报的电量
//! name = "My Mouse"
//! vendor_id = 0x1234        # 用 `perch-core battery list` 找到
//! product_id = 0x5678
//! usage_page = 0xFF00       # 以下三项有默认值
//! usage = 2
//! battery_byte = 6          # 电量在输入报告里的第几个字节;不确定就用 `battery dump` 看
//!
//! [[battery]]
//! kind = "system"           # 本机电池(笔记本)
//! ```
//!
//! 旧的单设备写法 `[mouse]`(同样的键)继续支持。什么都不写:有本机电池就显示它,
//! 台式机没有电池则电量卡片自动隐藏。

use std::fs;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use crate::util::user_config;

struct HidCfg {
    vendor_id: u16,
    product_id: u16,
    usage_page: u16,
    usage: u16,
    battery_byte: usize,
}

enum Kind {
    Hid(HidCfg),
    System,
}

struct Source {
    name: String,
    kind: Kind,
}

fn parse_source(t: &toml::Table, default_kind: &str) -> Option<Source> {
    let int = |k: &str, d: i64| t.get(k).and_then(|v| v.as_integer()).unwrap_or(d);
    let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    match t.get("kind").and_then(|v| v.as_str()).unwrap_or(default_kind) {
        "system" => Some(Source { name, kind: Kind::System }),
        _ => {
            let (vendor_id, product_id) = (int("vendor_id", 0), int("product_id", 0));
            if vendor_id <= 0 || product_id <= 0 {
                return None;
            }
            Some(Source {
                name: if name.is_empty() { "Device".to_string() } else { name },
                kind: Kind::Hid(HidCfg {
                    vendor_id: vendor_id as u16,
                    product_id: product_id as u16,
                    usage_page: int("usage_page", 0xFF00) as u16,
                    usage: int("usage", 2) as u16,
                    battery_byte: int("battery_byte", 6).clamp(0, 63) as usize,
                }),
            })
        }
    }
}

/// 配置里的电量来源列表;什么都没配时默认只看本机电池
fn sources() -> Vec<Source> {
    let cfg = user_config();
    let mut v: Vec<Source> = Vec::new();
    if let Some(arr) = cfg.get("battery").and_then(|b| b.as_array()) {
        v.extend(arr.iter().filter_map(|e| e.as_table()).filter_map(|t| parse_source(t, "hid")));
    }
    if let Some(t) = cfg.get("mouse").and_then(|m| m.as_table()) {
        v.extend(parse_source(t, "hid")); // 旧写法
    }
    if v.is_empty() {
        v.push(Source { name: String::new(), kind: Kind::System });
    }
    v
}

fn read_hid_battery(c: &HidCfg) -> Option<u8> {
    let api = hidapi::HidApi::new().ok()?;

    let mut candidates: Vec<_> = api
        .device_list()
        .filter(|d| d.vendor_id() == c.vendor_id && d.product_id() == c.product_id && d.usage_page() == c.usage_page && d.usage() == c.usage)
        .collect();

    // 同一个设备常有多个接口:优先 col05 那个(厂商自定义报告通常在这里)
    candidates.sort_by_key(|d| !d.path().to_string_lossy().to_lowercase().contains("col05"));

    for device_info in candidates {
        if let Ok(device) = device_info.open_device(&api) {
            for _ in 0..15 {
                let mut buf = [0u8; 64];
                if let Ok(size) = device.read_timeout(&mut buf, 80) {
                    if size > c.battery_byte && buf.iter().any(|&b| b != 0) {
                        let level = buf[c.battery_byte];
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

/// 本机电池:(百分比, 是否接着电源);没有电池返回 None
fn read_system_battery() -> Option<(u8, bool)> {
    unsafe {
        let mut sps = SYSTEM_POWER_STATUS::default();
        if GetSystemPowerStatus(&mut sps).is_ok() && sps.BatteryLifePercent <= 100 {
            Some((sps.BatteryLifePercent, sps.ACLineStatus == 1))
        } else {
            None
        }
    }
}

pub fn update_battery() -> Result<(), Box<dyn std::error::Error>> {
    let cache_file = perch_data::paths::last_battery_txt();
    let srcs = sources();
    let has_hid = srcs.iter().any(|s| matches!(s.kind, Kind::Hid(_)));
    let first_hid_name = srcs.iter().find(|s| matches!(s.kind, Kind::Hid(_))).map(|s| s.name.clone()).unwrap_or_default();

    // (图标, 百分比, 状态文字, 是否低电量判断用的数值)
    let mut shown: Option<(&str, String, String, Option<u8>)> = None;
    for s in &srcs {
        match &s.kind {
            Kind::Hid(c) => {
                if let Some(val) = read_hid_battery(c) {
                    let _ = fs::write(&cache_file, val.to_string());
                    shown = Some(("🖱️", format!("{}%", val), format!("{}: {}%", s.name, val), Some(val)));
                    break;
                }
            }
            Kind::System => {
                if let Some((p, ac)) = read_system_battery() {
                    let st = perch_data::t(if ac { "core2.charging" } else { "core2.on_battery" });
                    shown = Some(("⚡", format!("{}%", p), perch_data::t_fmt("core2.sys_power", &[("p", &p.to_string()), ("st", &st)]), None));
                    break;
                }
            }
        }
    }

    // 设备暂时没读到(休眠):用上一次的有效数值,避免电量跳变
    if shown.is_none() && has_hid {
        if let Some(val) = fs::read_to_string(&cache_file).ok().and_then(|c| c.trim().parse::<u8>().ok()).filter(|v| (1..=100).contains(v)) {
            shown = Some(("🖱️", format!("{}%", val), format!("{}: {}%", first_hid_name, val), Some(val)));
        }
    }

    let low_thresh = user_config().get("thresholds").and_then(|t| t.get("battery_low")).and_then(|v| v.as_integer()).unwrap_or(20) as u8;
    let warn_thresh = user_config().get("thresholds").and_then(|t| t.get("battery_warn")).and_then(|v| v.as_integer()).unwrap_or(40) as u8;
    let (danger_color, warn_color) = fs::read_to_string(perch_data::paths::theme_toml())
        .ok()
        .and_then(|c| toml::from_str::<perch_data::ThemeToml>(&c).ok())
        .map(|t| (t.danger, t.warn))
        .unwrap_or_else(|| (perch_data::default_danger(), perch_data::default_warn()));

    // 没有任何可显示的:配了设备 → 显示"休眠/未连接";没配且没有本机电池(台式机) → 卡片隐藏
    let (icon, text, status, color, available) = match shown {
        Some((icon, text, status, level)) => {
            let color = match level {
                Some(v) if v <= low_thresh => danger_color,
                Some(v) if v <= warn_thresh => warn_color,
                _ => "255,255,255,255".to_string(),
            };
            (icon, text, status, color, true)
        }
        None if has_hid => (
            "🖱️",
            "--%".to_string(),
            perch_data::t_fmt("core2.mouse_asleep", &[("name", &first_hid_name)]),
            "160,174,192,200".to_string(),
            true,
        ),
        None => ("⚡", "--%".to_string(), perch_data::t("core2.mouse_unset"), "160,174,192,200".to_string(), false),
    };

    let bat = perch_data::Battery {
        v: 1,
        mouse_icon: icon.to_string(),
        mouse_text: text,
        mouse_status: status,
        mouse_color: color,
        available,
    };
    let _ = perch_data::write_atomic(perch_data::paths::battery_toml(), &bat);
    Ok(())
}

/// `perch-core battery list`:把本机所有 HID 设备写进 data\hid_devices.txt,方便找 vendor_id / product_id。
/// 厂商自定义用法页(>= 0xFF00)的接口最可能带电量,排在前面。
pub fn list_hid_devices() {
    let mut out = String::from("# HID devices (vendor-defined usage pages first). Candidates for [[battery]] kind = \"hid\".\n\n");
    if let Ok(api) = hidapi::HidApi::new() {
        let mut devs: Vec<_> = api.device_list().collect();
        devs.sort_by_key(|d| (d.usage_page() < 0xFF00, d.vendor_id(), d.product_id()));
        for d in devs {
            out.push_str(&format!(
                "{} {}\n    vendor_id = 0x{:04X}\n    product_id = 0x{:04X}\n    usage_page = 0x{:04X}\n    usage = {}\n\n",
                d.manufacturer_string().unwrap_or(""),
                d.product_string().unwrap_or("(unnamed)"),
                d.vendor_id(),
                d.product_id(),
                d.usage_page(),
                d.usage()
            ));
        }
    } else {
        out.push_str("(could not initialize the HID API)\n");
    }
    let p = perch_data::paths::data_dir().join("hid_devices.txt");
    let _ = fs::create_dir_all(perch_data::paths::data_dir());
    let _ = fs::write(&p, out);
    crate::util::open_path(&p);
}

/// `perch-core battery dump <vendor_id> <product_id> [usage_page] [usage]`:抓几份输入报告写进 data\hid_dump.txt,
/// 对着设备电量变化(或贴给别人)就能看出电量在第几个字节。
pub fn dump_hid_reports(vendor_id: u16, product_id: u16, usage_page: u16, usage: u16) {
    let mut out = format!("# reports from {:04X}:{:04X} usage_page=0x{:04X} usage={}\n# byte index -> value\n\n", vendor_id, product_id, usage_page, usage);
    if let Ok(api) = hidapi::HidApi::new() {
        for d in api.device_list().filter(|d| d.vendor_id() == vendor_id && d.product_id() == product_id && d.usage_page() == usage_page && d.usage() == usage) {
            out.push_str(&format!("interface {}\n", d.path().to_string_lossy()));
            if let Ok(dev) = d.open_device(&api) {
                for _ in 0..20 {
                    let mut buf = [0u8; 64];
                    if let Ok(n) = dev.read_timeout(&mut buf, 150) {
                        if n > 0 {
                            let hex: Vec<String> = buf[..n].iter().map(|b| format!("{:02X}", b)).collect();
                            out.push_str(&format!("  {}\n", hex.join(" ")));
                        }
                    }
                }
            } else {
                out.push_str("  (could not open this interface)\n");
            }
            out.push('\n');
        }
    }
    let p = perch_data::paths::data_dir().join("hid_dump.txt");
    let _ = fs::create_dir_all(perch_data::paths::data_dir());
    let _ = fs::write(&p, out);
    crate::util::open_path(&p);
}
