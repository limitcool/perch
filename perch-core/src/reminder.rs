//! perch-core::reminder: 战术日程（Reminders）CRUD 引擎

use std::fs;
use std::path::PathBuf;
use perch_data::ReminderItem;
use crate::util::{epoch_secs, local_hm, trigger_hud_notify};

/// 日程:多条,按时间触发一次提醒(到点弹 HUD 通知)。time 为 "HH:MM";没有时间的是普通备注(不提醒)。
pub fn get_reminders_file_path() -> PathBuf {
    perch_data::paths::reminders_json()
}

pub fn load_reminders() -> Vec<ReminderItem> {
    let path = get_reminders_file_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(list) = serde_json::from_str::<Vec<ReminderItem>>(&content) {
            return list;
        }
        // 旧格式:单个对象 {time, text} → 迁移成一条没有提醒时间的备注
        if let Ok(old) = serde_json::from_str::<ReminderItem>(&content) {
            if !old.text.is_empty() {
                return vec![ReminderItem { id: 1, time: String::new(), text: old.text, due_ts: None, fired: false }];
            }
        }
    }
    Vec::new()
}

pub fn save_reminders(items: &[ReminderItem]) {
    let path1 = get_reminders_file_path();
    if let Some(parent) = path1.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(items) {
        let _ = fs::write(&path1, &json);
    }
}

/// 解析 "14:30" / "14：30" / "1430" → (时, 分)
pub fn parse_clock(s: &str) -> Option<(u32, u32)> {
    let t = s.trim().replace('：', ":");
    let (h, m) = if let Some((h, m)) = t.split_once(':') {
        (h.trim().parse::<u32>().ok()?, m.trim().parse::<u32>().ok()?)
    } else if t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) {
        (t[..2].parse().ok()?, t[2..].parse().ok()?)
    } else {
        return None;
    };
    if h < 24 && m < 60 {
        Some((h, m))
    } else {
        None
    }
}

pub fn handle_reminder_command(subargs: &[String]) {
    let mut items = load_reminders();
    match subargs.first().map(|s| s.as_str()) {
        Some("tick") => {
            reminder_tick();
            return;
        }
        Some("delete") => {
            if let Some(id) = subargs.get(1).and_then(|s| s.parse::<u32>().ok()) {
                items.retain(|i| i.id != id);
                save_reminders(&items);
            }
        }
        Some("clear") => {
            items.clear();
            save_reminders(&items);
        }
        Some(first) if subargs.len() > 1 => {
            let next_id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
            if let Some((h, m)) = parse_clock(first) {
                // 下一次出现的 HH:MM(已过则明天)
                let (nh, nm) = local_hm();
                let now_min = nh * 60 + nm;
                let delta_min = (h * 60 + m + 1440 - now_min) % 1440;
                let due = (epoch_secs() / 60) * 60 + delta_min as u64 * 60;
                items.push(ReminderItem { id: next_id, time: format!("{:02}:{:02}", h, m), text: subargs[1..].join(" "), due_ts: Some(due), fired: false });
            } else {
                items.push(ReminderItem { id: next_id, time: String::new(), text: subargs.join(" "), due_ts: None, fired: false });
            }
            save_reminders(&items);
        }
        Some(only) => {
            // 只有一段文字:当作无提醒时间的备注
            let next_id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
            items.push(ReminderItem { id: next_id, time: String::new(), text: only.to_string(), due_ts: None, fired: false });
            save_reminders(&items);
        }
        None => {}
    }
}

/// 到点提醒(由 media update 的心跳带动,桌面被盖住时每 30 秒一次)
pub fn reminder_tick() {
    let mut items = load_reminders();
    let now = epoch_secs();
    let mut changed = false;
    for it in items.iter_mut() {
        if !it.fired {
            if let Some(d) = it.due_ts {
                if now >= d {
                    trigger_hud_notify(&perch_data::t("core.notify_reminder"), &format!("{}  {}", it.time, it.text));
                    it.fired = true;
                    changed = true;
                }
            }
        }
    }
    // 已触发超过 12 小时的清掉
    let before = items.len();
    items.retain(|i| !(i.fired && i.due_ts.map(|d| now > d + 12 * 3600).unwrap_or(false)));
    if changed || items.len() != before {
        save_reminders(&items);
    }
}
