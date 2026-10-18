//! perch-core::water: 每日饮水目标、打卡记录与健康补水提醒

use std::fs;
use crate::util::{auto_json, epoch_secs, local_date_weekday, timer_active, trigger_hud_notify};
use crate::theme::current_theme_name;

fn local_hm() -> (u32, u32) {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let st = unsafe { GetLocalTime() };
    (st.wHour as u32, st.wMinute as u32)
}

fn parse_hm(s: &str, default: u32) -> u32 {
    let mut it = s.split(':');
    match (it.next().and_then(|x| x.parse::<u32>().ok()), it.next().and_then(|x| x.parse::<u32>().ok())) {
        (Some(h), Some(m)) => h * 60 + m,
        _ => default,
    }
}

pub fn water_cfg() -> serde_json::Value {
    let c = auto_json("water_cfg.json");
    if c.is_null() {
        serde_json::json!({ "goal": 2000, "remind": true, "interval_min": 60, "start": "09:00", "end": "22:00" })
    } else {
        c
    }
}

pub fn water_state() -> serde_json::Value {
    let (today, _) = local_date_weekday();
    let toml_p = perch_data::paths::data_dir().join("water.toml");
    if let Ok(w) = perch_data::read::<perch_data::Water>(&toml_p) {
        if w.date == today {
            return serde_json::json!({
                "date": w.date,
                "ml": w.ml,
                "log": w.log,
                "last_ts": w.last_ts,
                "last_remind_ts": w.last_remind_ts,
            });
        }
    }
    let st = auto_json("water_state.json");
    if st["date"].as_str() == Some(today.as_str()) {
        st
    } else {
        let now = epoch_secs();
        serde_json::json!({ "date": today, "ml": 0, "log": [], "last_ts": now, "last_remind_ts": now })
    }
}

pub fn water_save(st: &serde_json::Value, cfg: &serde_json::Value) {
    let _ = fs::write(perch_data::paths::data_dir().join("water_state.json"), st.to_string());
    let goal = cfg["goal"].as_u64().unwrap_or(2000).max(1);
    let ml = st["ml"].as_u64().unwrap_or(0);
    let pct = (ml * 100 / goal).min(100);
    let log = st["log"].as_array().cloned().unwrap_or_default();
    let last = log.last().and_then(|x| x[0].as_str()).unwrap_or("--:--").to_string();

    let w = perch_data::Water {
        v: 1,
        date: st["date"].as_str().unwrap_or_default().to_string(),
        ml,
        goal,
        pct,
        last,
        cups_n: log.len(),
        done: if ml >= goal { 1 } else { 0 },
        last_ts: st["last_ts"].as_u64().unwrap_or(0),
        last_remind_ts: st["last_remind_ts"].as_u64().unwrap_or(0),
        log,
    };
    let _ = perch_data::write_atomic(perch_data::paths::data_dir().join("water.toml"), &w);
}

fn water_message(theme: &str) -> String {
    let pack = perch_data::theme_pack::load(theme);
    if !pack.messages.water.is_empty() {
        pack.messages.water
    } else {
        perch_data::t("core.water_fallback")
    }
}

pub fn water_add(ml: i64) {
    if ml == 0 {
        return;
    }
    let cfg = water_cfg();
    let mut st = water_state();
    let goal = cfg["goal"].as_i64().unwrap_or(2000);
    let before = st["ml"].as_i64().unwrap_or(0);
    let after = (before + ml).max(0);
    st["ml"] = serde_json::json!(after);
    if ml > 0 {
        let (h, m) = local_hm();
        let mut log = st["log"].as_array().cloned().unwrap_or_default();
        log.push(serde_json::json!([format!("{:02}:{:02}", h, m), ml]));
        st["log"] = serde_json::Value::Array(log);
        st["last_ts"] = serde_json::json!(epoch_secs());
    }
    water_save(&st, &cfg);
    if before < goal && after >= goal {
        let title = perch_data::t("core.notify_water_goal");
        let msg = perch_data::t_fmt("core.notify_water_goal_msg", &[("after", &after.to_string()), ("goal", &goal.to_string())]);
        trigger_hud_notify(&title, &msg);
    }
}

pub fn water_undo() {
    let cfg = water_cfg();
    let mut st = water_state();
    let mut log = st["log"].as_array().cloned().unwrap_or_default();
    if let Some(last) = log.pop() {
        let ml = last[1].as_i64().unwrap_or(0);
        st["ml"] = serde_json::json!((st["ml"].as_i64().unwrap_or(0) - ml).max(0));
        st["log"] = serde_json::Value::Array(log);
        water_save(&st, &cfg);
    }
}

/// 由媒体轮询每次顺带调用:跨天重置 + 到点提醒
pub fn water_tick() {
    let cfg = water_cfg();
    let mut st = water_state();
    let disk = auto_json("water_state.json");
    if disk["date"] != st["date"] || !perch_data::paths::data_dir().join("water.toml").exists() {
        water_save(&st, &cfg);
    }
    if !cfg["remind"].as_bool().unwrap_or(true) || timer_active() {
        return;
    }
    let goal = cfg["goal"].as_u64().unwrap_or(2000);
    if st["ml"].as_u64().unwrap_or(0) >= goal {
        return;
    }
    let (h, m) = local_hm();
    let now_min = h * 60 + m;
    if now_min < parse_hm(cfg["start"].as_str().unwrap_or("09:00"), 540) || now_min >= parse_hm(cfg["end"].as_str().unwrap_or("22:00"), 1320) {
        return;
    }
    let interval = cfg["interval_min"].as_u64().unwrap_or(60).max(5) * 60;
    let base = st["last_ts"].as_u64().unwrap_or(0).max(st["last_remind_ts"].as_u64().unwrap_or(0));
    if epoch_secs().saturating_sub(base) >= interval {
        st["last_remind_ts"] = serde_json::json!(epoch_secs());
        let _ = fs::write(perch_data::paths::data_dir().join("water_state.json"), st.to_string());
        let ml = st["ml"].as_u64().unwrap_or(0);
        trigger_hud_notify(
            &perch_data::t("core2.water_title"),
            &perch_data::t_fmt("core2.water_today", &[("msg", &water_message(&current_theme_name())), ("ml", &ml.to_string()), ("goal", &goal.to_string())]),
        );
    }
}
