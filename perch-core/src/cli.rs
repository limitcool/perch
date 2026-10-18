//! perch-core::cli: 命令行参数解析与分发

use std::fs;

use crate::util::*;
use crate::todo::{handle_todo_command, load_todos, save_todos};
use crate::reminder::{handle_reminder_command, load_reminders, save_reminders, reminder_tick};
use crate::weather::update_weather;
use crate::media::*;
use crate::battery::update_battery;
use crate::wallpaper::*;
use crate::water::*;
use crate::theme::*;

pub fn reload_perch_system() {
    println!("[PERCH] Regenerating data files...");
    let items = load_todos();
    save_todos(&items);
    let reminders = load_reminders();
    save_reminders(&reminders);
    switch_theme(&current_theme_name());
    let _ = update_weather();
    let _ = update_battery();
    let _ = update_media();
    println!("[PERCH] Done");
}

pub fn run_cli(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 2 {
        let _ = update_media();
        return Ok(());
    }

    match args[1].as_str() {
        "notify" => {
            let title = if args.len() > 2 { args[2].clone() } else { perch_data::t("core2.notice_title") };
            let msg = if args.len() > 3 { args[3].clone() } else { perch_data::t("core2.notice_msg") };
            trigger_hud_notify(&title, &msg);
        }
        "quote" => {
            let next = args.get(2).map(|s| s.as_str()) == Some("next");
            let theme_arg = if next { None } else { args.get(2).map(|s| s.as_str()) };
            update_quote_text(theme_arg, next);
        }
        "todo" => {
            handle_todo_command(&args[2..]);
        }
        "reminder" => {
            handle_reminder_command(&args[2..]);
        }
        "reload" => {
            reload_perch_system();
        }
        "theme" => {
            if args.len() > 2 {
                switch_theme(&args[2]);
            }
        }
        "power" => {
            if args.len() > 2 {
                execute_power_action(&args[2]);
            }
        }
        "media" => {
            if args.len() > 2 {
                match args[2].as_str() {
                    "update" => {
                        probe_system_theme();
                        autotheme_tick();
                        water_tick();
                        reminder_tick();
                        token_check();
                        if update_media().unwrap_or(true) {
                        }
                    }
                    "list" => media_list(),
                    a => {
                        let _ = control_media(a);
                    }
                }
            } else {
                let _ = update_media();
            }
        }
        "wallpaper" => match args.get(2).map(|x| x.as_str()) {
            Some("mode") => {
                if let Some(m) = args.get(3) {
                    set_wallpaper_mode(m);
                }
            }
            Some("sync") => {
                probe_system_theme();
                sync_wallpaper();
            }
            Some("probe") => probe_system_theme(),
            Some("next") => wallpaper_next(),
            _ => {}
        },
        "ambient" => {
            if let Some(l) = args.get(2) {
                ambient_set_level(l);
            }
        }
        "water" => match args.get(2).map(|x| x.as_str()) {
            Some("add") => {
                if let Some(v) = args.get(3).and_then(|x| x.parse::<i64>().ok()) {
                    water_add(v);
                }
            }
            Some("undo") => water_undo(),
            Some("tick") => water_tick(),
            _ => water_save(&water_state(), &water_cfg()),
        },
        "autotheme" => match args.get(2).map(|x| x.as_str()) {
            Some("tick") => autotheme_tick(),
            Some("mode") => {
                if let Some(m) = args.get(3) {
                    set_auto_mode(m);
                }
            }
            _ => {}
        },
        "bgm" => match args.get(2).map(|x| x.as_str()) {
            Some("play") => {
                let theme = args.get(3).cloned().unwrap_or_else(current_theme_name);
                bgm_play(&theme);
            }
            Some("auto") => match args.get(3).map(|x| x.as_str()) {
                Some("on") => set_bgm_auto(true),
                Some("off") => set_bgm_auto(false),
                _ => set_bgm_auto(!bgm_auto_enabled()),
            },
            _ => {}
        },
        "volume" => {
            if args.len() > 2 {
                control_volume(&args[2]);
            }
        }
        "notify-history" => match args.get(2).map(|s| s.as_str()) {
            Some("clear") => {
                let _ = fs::write(notify_history_path(), "[]");
            }
            Some("delete") => {
                if let Some(ts) = args.get(3).and_then(|s| s.parse::<u64>().ok()) {
                    let mut list = notify_history_load();
                    if let Some(i) = list.iter().position(|v| v["ts"].as_u64() == Some(ts)) {
                        list.remove(i);
                    }
                    let _ = fs::write(notify_history_path(), serde_json::to_string(&list).unwrap_or_default());
                }
            }
            _ => {}
        },
        "token" => {
            match token_days_left() {
                Some(d) => println!("Music Assistant token: {} days left", d),
                None => println!("Music Assistant token: not found or not a JWT"),
            }
        }
        "weather" => {
            if args.get(2).map(|s| s.as_str()) == Some("city") {
                let name = args.get(3).cloned().unwrap_or_else(|| "auto".to_string());
                let _ = fs::write(perch_data::paths::weather_cfg_json(), serde_json::json!({ "city": name }).to_string());
            }
            let _ = update_weather();
        }
        "setup" => {
            reload_perch_system();
        }
        "battery" => match args.get(2).map(|s| s.as_str()) {
            // battery list:列出本机 HID 设备;battery dump <vid> <pid> [usage_page] [usage]:抓输入报告(找电量字节)
            Some("list") => crate::battery::list_hid_devices(),
            Some("dump") => {
                let num = |i: usize, d: u32| -> u32 {
                    args.get(i)
                        .and_then(|s| {
                            let s = s.trim();
                            if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) { u32::from_str_radix(h, 16).ok() } else { s.parse().ok() }
                        })
                        .unwrap_or(d)
                };
                crate::battery::dump_hid_reports(num(3, 0) as u16, num(4, 0) as u16, num(5, 0xFF00) as u16, num(6, 2) as u16);
            }
            _ => {
                let _ = update_battery();
            }
        },
        _ => {
            let _ = update_media();
        }
    }

    Ok(())
}
