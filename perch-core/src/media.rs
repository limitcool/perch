//! perch-core::media: Windows SMTC 系统媒体控制、专辑封面提取与 Music Assistant 协议集成

use std::env;
use std::fs;
use std::path::PathBuf;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
};
use windows::Storage::Streams::DataReader;

use crate::util::{epoch_secs, trigger_hud_notify, user_config};
use crate::wallpaper::{load_modes, save_modes, system_uses_light_theme};

// -------------------------------------------------------------
// 7. SMTC 媒体控制器与专辑封面提取
// -------------------------------------------------------------

// 应用图标: app_icons 目录下 map.txt 每行 `AppUserModelId 子串=图标文件名`,命中的会话在音乐卡上显示该应用图标
fn app_icon_for(app_id: &str) -> String {
    let dir = perch_data::paths::app_icons_dir();
    let idl = app_id.to_lowercase();
    if let Ok(t) = fs::read_to_string(dir.join("map.txt")) {
        for line in t.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, f)) = line.split_once('=') {
                if !k.trim().is_empty() && idl.contains(&k.trim().to_lowercase()) {
                    let p = dir.join(f.trim());
                    if p.exists() {
                        return p.to_string_lossy().to_string();
                    }
                }
            }
        }
    }
    String::new()
}

// 可选偏好文件: media_prefer.txt,每行一个 AppUserModelId 子串(不区分大小写),命中的会话优先
fn media_prefer_list() -> Vec<String> {
    let p = perch_data::paths::media_prefer_txt();
    fs::read_to_string(p)
        .map(|t| {
            t.lines()
                .map(|l| l.trim().to_lowercase())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect()
        })
        .unwrap_or_default()
}

fn session_status_code(s: &GlobalSystemMediaTransportControlsSession) -> i32 {
    s.GetPlaybackInfo()
        .ok()
        .and_then(|p| p.PlaybackStatus().ok())
        .map(|st| st.0)
        .unwrap_or(-1)
}

fn media_snapshot(s: &GlobalSystemMediaTransportControlsSession) -> String {
    let (mut t, mut a) = (String::new(), String::new());
    if let Ok(op) = s.TryGetMediaPropertiesAsync() {
        if let Ok(info) = op.get() {
            t = info.Title().map(|x| x.to_string()).unwrap_or_default();
            a = info.Artist().map(|x| x.to_string()).unwrap_or_default();
        }
    }
    format!("{}|{}|{}", t, a, session_status_code(s))
}

fn is_browser_id(id: &str) -> bool {
    ["chrome", "msedge", "firefox", "brave", "opera", "vivaldi"].iter().any(|b| id.contains(b))
}

fn session_score(
    s: &GlobalSystemMediaTransportControlsSession,
    prefer: &[String],
    current_id: &Option<String>,
) -> i32 {
    let id = s.SourceAppUserModelId().map(|x| x.to_string()).unwrap_or_default();
    let idl = id.to_lowercase();
    let mut score = 0;
    match session_status_code(s) {
        4 => score += 100, // Playing
        5 => score += 20,  // Paused
        _ => {}
    }
    if prefer.iter().any(|p| idl.contains(p.as_str())) {
        score += 200; // 偏好应用:只要会话还在(哪怕暂停),就压过浏览器里正在播放的标签页
    }
    if !is_browser_id(&idl) {
        score += 10;
    }
    if current_id.as_deref() == Some(id.as_str()) {
        score += 5;
    }
    score
}

// 同时开着浏览器和播放器时,GetCurrentSession 会在两者间来回跳:这里按“正在播放 > 偏好 > 非浏览器 > 系统当前”打分选一个稳定的会话
fn pick_media_session(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
) -> Option<GlobalSystemMediaTransportControlsSession> {
    let prefer = media_prefer_list();
    let current_id = manager
        .GetCurrentSession()
        .ok()
        .and_then(|s| s.SourceAppUserModelId().ok())
        .map(|s| s.to_string());
    let sessions = manager.GetSessions().ok()?;
    let mut best: Option<(i32, GlobalSystemMediaTransportControlsSession)> = None;
    for i in 0..sessions.Size().unwrap_or(0) {
        if let Ok(s) = sessions.GetAt(i) {
            let score = session_score(&s, &prefer, &current_id);
            if best.as_ref().map_or(true, |(b, _)| score > *b) {
                best = Some((score, s));
            }
        }
    }
    best.map(|(_, s)| s).or_else(|| manager.GetCurrentSession().ok())
}

// 调试:列出所有 SMTC 会话与打分,写入 media_sessions.txt(本程序无控制台)
pub fn media_list() {
    let mut out = String::new();
    if let Ok(op) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
        if let Ok(m) = op.get() {
            let prefer = media_prefer_list();
            let current_id = m
                .GetCurrentSession()
                .ok()
                .and_then(|s| s.SourceAppUserModelId().ok())
                .map(|s| s.to_string());
            if let Ok(sessions) = m.GetSessions() {
                for i in 0..sessions.Size().unwrap_or(0) {
                    if let Ok(s) = sessions.GetAt(i) {
                        let id = s.SourceAppUserModelId().map(|x| x.to_string()).unwrap_or_default();
                        out.push_str(&format!(
                            "score={} | {} | {}\n",
                            session_score(&s, &prefer, &current_id),
                            id,
                            media_snapshot(&s)
                        ));
                    }
                }
            }
            let chosen = pick_media_session(&m)
                .and_then(|s| s.SourceAppUserModelId().ok())
                .map(|s| s.to_string())
                .unwrap_or_default();
            out.push_str(&format!("CHOSEN: {}\n", chosen));
        }
    }
    let _ = fs::write(perch_data::paths::media_sessions_txt(), out);
}

// 返回内容是否有变化(无变化时不写文件、不刷新皮肤,轮询几乎零开销)
pub fn update_media() -> Result<bool, Box<dyn std::error::Error>> {
    let cover_dst = perch_data::paths::cover_png();
    let default_cover = perch_data::paths::default_cover_png();
    let media_json_path = perch_data::paths::media_json();

    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;
    let session = pick_media_session(&manager);

    let prev = perch_data::read::<perch_data::MediaInfo>(&media_json_path).ok();

    let mut title = perch_data::t("core2.media_title");
    let mut artist = perch_data::t("core2.media_artist");
    let mut status = perch_data::t("core2.media_standby");
    let mut info_opt = None;

    if let Some(s) = &session {
        if let Ok(info) = s.TryGetMediaPropertiesAsync()?.get() {
            if let Ok(t) = info.Title() {
                let ts = t.to_string();
                if !ts.is_empty() {
                    title = ts;
                }
            }
            if let Ok(a) = info.Artist() {
                let as_str = a.to_string();
                if !as_str.is_empty() {
                    artist = as_str;
                }
            }
            info_opt = Some(info);
        }
        status = if session_status_code(s) == 4 {
            perch_data::t("core2.media_playing")
        } else {
            perch_data::t("core2.media_paused")
        };
    }

    let app_icon = session
        .as_ref()
        .and_then(|s| s.SourceAppUserModelId().ok())
        .map(|id| app_icon_for(&id.to_string()))
        .unwrap_or_default();
    let default_str = default_cover.to_string_lossy().to_string();
    let track_changed = prev.as_ref().map(|p| p.title != title || p.artist != artist).unwrap_or(true);
    let prev_cover_is_default = prev.as_ref().map(|p| p.cover_file.is_empty() || p.cover_file == default_str).unwrap_or(true);
    let mut cover_updated = false;
    let mut has_cover = cover_dst.exists() && !prev_cover_is_default;

    // 封面只在换歌(或此前没取到封面)时才读,避免每次轮询都读写大文件
    if track_changed || prev_cover_is_default {
        let mut fetched: Option<Vec<u8>> = None;
        if let Some(info) = &info_opt {
            if let Ok(thumb_ref) = info.Thumbnail() {
                if let Ok(stream) = thumb_ref.OpenReadAsync()?.get() {
                    let size = stream.Size().unwrap_or(0);
                    if size > 0 && size < 20 * 1024 * 1024 {
                        let reader = DataReader::CreateDataReader(&stream)?;
                        reader.LoadAsync(size as u32)?.get()?;
                        let mut bytes = vec![0u8; size as usize];
                        reader.ReadBytes(&mut bytes)?;
                        fetched = Some(bytes);
                    }
                }
            }
        }
        match fetched {
            Some(bytes) => {
                if fs::read(&cover_dst).unwrap_or_default() != bytes {
                    let _ = fs::write(&cover_dst, &bytes);
                    cover_updated = true;
                }
                has_cover = true;
            }
            None => has_cover = false,
        }
    }

    let cover_file_str = if has_cover && cover_dst.exists() {
        cover_dst.to_string_lossy().to_string()
    } else {
        default_str.clone()
    };

    // 播放进度:写 media_timeline.toml
    {
        let mut pos_out = 0u64;
        let mut dur_out = 0u64;
        let mut stamp_out = 0u64;
        let mut playing_out = 0u8;

        if let Some(s) = &session {
            if let Ok(tl) = s.GetTimelineProperties() {
                let start = tl.StartTime().map(|d| d.Duration / 10_000).unwrap_or(0);
                let end = tl.EndTime().map(|d| d.Duration / 10_000).unwrap_or(0);
                let pos = tl.Position().map(|d| d.Duration / 10_000).unwrap_or(0);
                // LastUpdatedTime 是 Windows FILETIME 刻度(1601 年起,100ns),换算成 Unix 毫秒
                let stamp = tl
                    .LastUpdatedTime()
                    .map(|t| t.UniversalTime / 10_000 - 11_644_473_600_000)
                    .unwrap_or_else(|_| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0));
                pos_out = (pos - start).max(0) as u64;
                dur_out = (end - start).max(0) as u64;
                stamp_out = stamp.max(0) as u64;
                playing_out = if status.starts_with('▶') { 1 } else { 0 };
            }
        }

        let tl_data = perch_data::MediaTimeline {
            v: 1,
            pos_ms: pos_out,
            dur_ms: dur_out,
            stamp_ms: stamp_out,
            playing: playing_out,
        };
        let _ = perch_data::write_atomic(perch_data::paths::media_timeline_toml(), &tl_data);
    }

    let changed = match &prev {
        Some(p) => p.title != title || p.artist != artist || p.status != status || p.cover_file != cover_file_str || cover_updated || p.app_icon != app_icon,
        None => true,
    };

    if changed {
        let play_icon = if status.starts_with('▶') { "⏸\u{FE0F}" } else { "▶\u{FE0F}" };
        let info = perch_data::MediaInfo {
            v: 1,
            title,
            artist,
            status,
            cover_file: cover_file_str,
            app_icon,
            play_icon: play_icon.to_string(),
        };
        let _ = perch_data::write_atomic(&media_json_path, &info);
    }
    Ok(changed)
}

pub fn control_media(action: &str) -> Result<(), Box<dyn std::error::Error>> {
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;
    if let Some(session) = pick_media_session(&manager) {
        // 跳转进度:media seek:<秒>
        if let Some(secs) = action.strip_prefix("seek:").and_then(|s| s.parse::<f64>().ok()) {
            let enabled = session
                .GetPlaybackInfo()
                .and_then(|i| i.Controls())
                .and_then(|c| c.IsPlaybackPositionEnabled())
                .unwrap_or(false);
            // 优先走 Music Assistant;失败再退回 SMTC
            let via_ma = ma_seek(secs);
            let ok = match &via_ma {
                Ok(_) => true,
                Err(_) => session.TryChangePlaybackPositionAsync((secs * 10_000_000.0) as i64)?.get().unwrap_or(false),
            };
            // 记录结果,方便排查"拖动没反应"(播放器不一定支持跳转)
            let _ = fs::write(
                perch_data::paths::media_seek_last_txt(),
                format!("secs={:.1} position_enabled={} accepted={} via={}
", secs, enabled, ok, match &via_ma { Ok(p) => format!("music-assistant({})", p), Err(e) => format!("smtc (ma failed: {})", e) }),
            );
            return Ok(());
        }
        let before = media_snapshot(&session);
        match action {
            "playpause" => {
                let _ = session.TryTogglePlayPauseAsync()?.get();
            }
            "next" => {
                let _ = session.TrySkipNextAsync()?.get();
            }
            "prev" => {
                let _ = session.TrySkipPreviousAsync()?.get();
            }
            _ => {}
        }
        // 网页播放器的曲目/状态通常晚 0.3~1s 才更新:轮询到真正变化(或超时)再刷新,切歌不再等下一次定时轮询
        let (max_ms, step): (u64, u64) = if action == "playpause" { (1200, 80) } else { (2500, 100) };
        let mut waited = 0;
        while waited < max_ms {
            std::thread::sleep(std::time::Duration::from_millis(step));
            waited += step;
            let cur = pick_media_session(&manager).unwrap_or_else(|| session.clone());
            if media_snapshot(&cur) != before {
                std::thread::sleep(std::time::Duration::from_millis(150)); // 等标题/艺术家/封面一起到位
                break;
            }
        }
    }
    if update_media().unwrap_or(true) {
    }
    Ok(())
}

fn bgm_dir() -> PathBuf {
    perch_data::paths::data_dir()
}

fn bgm_log(msg: &str) {
    let _ = fs::write(bgm_dir().join("bgm_last.txt"), msg);
}

pub fn bgm_auto_enabled() -> bool {
    let m = load_modes();
    m.bgm.trim() == "1"
}

pub fn set_bgm_auto(on: bool) {
    let mut m = load_modes();
    m.bgm = if on { "1".into() } else { "0".into() };
    save_modes(&m);
}

fn ma_call(base: &str, token: &str, command: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({ "command": command, "args": args }).to_string();
    let resp = ureq::post(&format!("{}/api", base.trim_end_matches('/')))
        .set("Authorization", &format!("Bearer {}", token))
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(8))
        .send_string(&body)
        .map_err(|e| format!("{} {}", command, e))?;
    let txt = resp.into_string().map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&txt).unwrap_or(serde_json::Value::Null))
}

/// 通过 Music Assistant 跳转播放位置(网页播放器常常忽略 SMTC 的跳转请求,这条路更可靠)
pub fn ma_seek(secs: f64) -> Result<String, String> {
    let cfg: serde_json::Value = serde_json::from_str(&fs::read_to_string(bgm_dir().join("bgm.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let token = fs::read_to_string(ma_token_path())
        .map_err(|_| "no token".to_string())?
        .trim()
        .to_string();
    let (base, want_player) = ma_settings(&cfg);
    let players = ma_call(&base, &token, "players/all", serde_json::json!({}))?;
    let want = want_player.clone();
    let pid = players
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|p| !want.is_empty() && p["display_name"].as_str().unwrap_or("").to_lowercase().contains(&want))
                .or_else(|| a.iter().find(|p| p["player_id"].as_str().unwrap_or("").starts_with("ma_companion_")))
                .and_then(|p| p["player_id"].as_str().map(|s| s.to_string()))
        })
        .ok_or_else(|| "player not found".to_string())?;
    ma_call(&base, &token, "player_queues/seek", serde_json::json!({ "queue_id": pid, "position": secs.round() as i64 }))?;
    Ok(pid)
}

/// 解码 base64url(JWT 的 payload),不依赖额外 crate
fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

/// Music Assistant 长期令牌(JWT)距离过期还有几天;读不到返回 None
pub fn token_days_left() -> Option<i64> {
    let token = fs::read_to_string(ma_token_path()).ok()?;
    let payload = token.trim().split('.').nth(1)?;
    let json: serde_json::Value = serde_json::from_slice(&b64url_decode(payload)?).ok()?;
    let exp = json["exp"].as_i64()?;
    Some((exp - epoch_secs() as i64) / 86400)
}

/// 每天最多检查一次:不足 14 天时弹一条通知(令牌内容绝不记录)
pub fn token_check() {
    let stamp = bgm_dir().join("token_check.txt");
    let today = epoch_secs() / 86400;
    if fs::read_to_string(&stamp).ok().and_then(|s| s.trim().parse::<u64>().ok()) == Some(today) {
        return;
    }
    let _ = fs::write(&stamp, today.to_string());
    if let Some(days) = token_days_left() {
        if days <= 14 {
            let msg = if days < 0 { perch_data::t("core2.token_expired") } else { perch_data::t_fmt("core2.token_soon", &[("days", &days.max(0).to_string())]) };
            trigger_hud_notify(&perch_data::t("core.notify_ma_token"), &msg);
        }
    }
}


fn ma_token_path() -> PathBuf {
    let cfg = user_config();
    match cfg.get("music_assistant").and_then(|m| m.get("token_file")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(env::var("USERPROFILE").unwrap_or_default()).join(r".perch\ma_token.txt"),
    }
}

/// (服务器地址, 播放器名称片段):config.toml [music_assistant] 优先,其次 bgm.json
fn ma_settings(bgm_cfg: &serde_json::Value) -> (String, String) {
    let cfg = user_config();
    let ma = cfg.get("music_assistant");
    let url = ma.and_then(|m| m.get("url")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string()).or_else(|| bgm_cfg["api"].as_str().map(|s| s.to_string())).unwrap_or_else(|| "http://10.0.0.2:8095".to_string());
    let player = ma.and_then(|m| m.get("player")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_lowercase()).unwrap_or_else(|| bgm_cfg["player"].as_str().unwrap_or("").to_lowercase());
    (url, player)
}


pub fn bgm_play(theme: &str) {
    let cfg: serde_json::Value = match serde_json::from_str(&fs::read_to_string(bgm_dir().join("bgm.json")).unwrap_or_default()) {
        Ok(v) => v,
        Err(_) => {
            bgm_log("bgm.json missing or invalid");
            return;
        }
    };
    let token = match fs::read_to_string(ma_token_path()) {
        Ok(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => {
            bgm_log("no Music Assistant token file");
            return;
        }
    };
    let (base, want_player) = ma_settings(&cfg);
    let period = if system_uses_light_theme().unwrap_or(true) { "day" } else { "night" };
    let entry = &cfg["themes"][theme];
    let list_val = if entry[period].is_array() { &entry[period] } else { &entry["any"] };
    let uris: Vec<String> = list_val
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    if uris.is_empty() {
        bgm_log(&format!("no bgm configured for theme {}", theme));
        return;
    }
    let players = match ma_call(&base, &token, "players/all", serde_json::json!({})) {
        Ok(v) => v,
        Err(e) => {
            bgm_log(&e);
            return;
        }
    };
    let want = want_player.clone();
    let pid = players.as_array().and_then(|a| {
        a.iter()
            .find(|p| !want.is_empty() && p["display_name"].as_str().unwrap_or("").to_lowercase().contains(&want))
            .or_else(|| a.iter().find(|p| p["player_id"].as_str().unwrap_or("").starts_with("ma_companion_")))
            .and_then(|p| p["player_id"].as_str().map(|s| s.to_string()))
    });
    let pid = match pid {
        Some(p) => p,
        None => {
            bgm_log("player not found");
            return;
        }
    };
    if let Err(e) = ma_call(
        &base,
        &token,
        "player_queues/play_media",
        serde_json::json!({ "queue_id": pid, "media": uris, "option": "replace" }),
    ) {
        bgm_log(&e);
        return;
    }
    if cfg["shuffle"].as_bool().unwrap_or(true) {
        let _ = ma_call(&base, &token, "player_queues/shuffle", serde_json::json!({ "queue_id": pid, "shuffle_enabled": true }));
    }
    bgm_log(&format!("playing {} ({} items) on {}", theme, uris.len(), pid));
}

